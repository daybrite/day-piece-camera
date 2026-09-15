// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// The day-piece-camera crate's Android backend — a Java shim over CameraX. It is bundled with
// THIS crate and folded into the app's Gradle build via [package.metadata.day.android] (which
// also declares the CameraX dependencies), with ZERO edits to day-android. It uses only
// day-android's PUBLIC surface: DayBridge.ctx (the Context) and DayBridge.nativeOnEvent (the
// event trampoline). It is the Android twin of platform/ios/swift/DayCamera.swift.
package dev.daybrite.day.piece.camera;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Matrix;
import android.media.ExifInterface;
import android.os.Handler;
import android.os.Looper;
import android.view.View;

import androidx.annotation.NonNull;
import androidx.camera.core.CameraInfoUnavailableException;
import androidx.camera.core.CameraSelector;
import androidx.camera.core.ImageCapture;
import androidx.camera.core.ImageCaptureException;
import androidx.camera.core.Preview;
import androidx.camera.lifecycle.ProcessCameraProvider;
import androidx.camera.view.PreviewView;
import androidx.lifecycle.Lifecycle;
import androidx.lifecycle.LifecycleOwner;
import androidx.lifecycle.LifecycleRegistry;

import com.google.common.util.concurrent.ListenableFuture;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.Map;
import java.util.UUID;
import java.util.WeakHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

import dev.daybrite.day.bridge.DayBridge;

/**
 * Wraps a CameraX PreviewView bound to a Preview and an ImageCapture use case. Camera state and
 * captures are reported to the piece's Rust front-end through DayBridge.nativeOnEvent's Custom
 * kind (12) as the piece's own codes.
 */
public final class DayCamera {
    private DayCamera() {}

    private static final int K_CUSTOM = 12;
    private static final int STOPPED = 0, STARTING = 1, RUNNING = 2, UNAVAILABLE = 3, DENIED = 4,
            ERROR = 5, PHOTO = 6;
    private static final int CMD_START = 0, CMD_STOP = 1, CMD_CAPTURE = 2, CMD_FACING = 3;

    /**
     * Everything a live viewfinder carries. It is its OWN LifecycleOwner: CameraX binds use cases
     * to a lifecycle, and binding to the activity's would keep the camera open across pages —
     * this registry moves to RESUMED on start and DESTROYED on release, so the camera is freed the
     * moment Day releases the view.
     */
    private static final class Live implements LifecycleOwner {
        final long id;
        final LifecycleRegistry lifecycle = new LifecycleRegistry(this);
        int facing;
        ProcessCameraProvider provider;
        Preview preview;
        ImageCapture capture;
        boolean wanted;

        Live(long id, int facing) {
            this.id = id;
            this.facing = facing;
            lifecycle.setCurrentState(Lifecycle.State.CREATED);
        }

        @NonNull
        @Override
        public Lifecycle getLifecycle() {
            return lifecycle;
        }

        void report(int code, String text) {
            DayBridge.nativeOnEvent(id, K_CUSTOM, (double) code, text == null ? "" : text);
        }

        CameraSelector selector() {
            return facing == 1 ? CameraSelector.DEFAULT_FRONT_CAMERA : CameraSelector.DEFAULT_BACK_CAMERA;
        }

        /** Unbind THIS viewfinder's use cases only: `unbindAll` would take another page's with them. */
        void unbind() {
            if (provider != null) {
                if (preview != null) {
                    provider.unbind(preview);
                }
                if (capture != null) {
                    provider.unbind(capture);
                }
            }
            preview = null;
            capture = null;
        }
    }

    // Weak keys: a released view must not pin itself here.
    private static final Map<View, Live> LIVE = new WeakHashMap<>();
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    /** Where the JPEG is written and measured, off the UI thread. */
    private static final ExecutorService IO = Executors.newSingleThreadExecutor();

    /** Create the viewfinder for node `id`; `facing` 0 back, 1 front; `active` starts it now. */
    public static View makeCamera(long id, int facing, boolean active) {
        Context ctx = DayBridge.ctx;
        PreviewView view = new PreviewView(ctx);
        // A TextureView, not a SurfaceView: Day positions and clips views itself, and a
        // SurfaceView punches through scroll containers.
        view.setImplementationMode(PreviewView.ImplementationMode.COMPATIBLE);
        view.setScaleType(PreviewView.ScaleType.FILL_CENTER);
        Live live = new Live(id, facing);
        LIVE.put(view, live);
        if (active) {
            start(view, live);
        }
        return view;
    }

    /** A command from a `CameraPatch`: start, stop, capture, or switch cameras (`arg` = facing). */
    public static void cameraCommand(View view, int cmd, int arg) {
        Live live = LIVE.get(view);
        if (live == null || !(view instanceof PreviewView)) {
            return;
        }
        PreviewView preview = (PreviewView) view;
        switch (cmd) {
            case CMD_START:
                start(preview, live);
                break;
            case CMD_STOP:
                stop(live);
                break;
            case CMD_CAPTURE:
                capture(live);
                break;
            case CMD_FACING:
                if (live.facing != arg) {
                    live.facing = arg;
                    if (live.wanted) {
                        start(preview, live);
                    }
                }
                break;
            default:
                break;
        }
    }

    /** The renderer's release: unbind this viewfinder's use cases and end its lifecycle. */
    public static void releaseCamera(View view) {
        Live live = LIVE.remove(view);
        if (live == null) {
            return;
        }
        live.wanted = false;
        live.unbind();
        live.lifecycle.setCurrentState(Lifecycle.State.DESTROYED);
    }

    private static void start(PreviewView view, Live live) {
        live.wanted = true;
        live.report(STARTING, "");
        Context ctx = DayBridge.ctx;
        ListenableFuture<ProcessCameraProvider> future = ProcessCameraProvider.getInstance(ctx);
        future.addListener(() -> {
            if (!live.wanted) {
                return;
            }
            try {
                ProcessCameraProvider provider = future.get();
                live.provider = provider;
                CameraSelector selector = live.selector();
                if (!provider.hasCamera(selector)) {
                    // The other camera, if this one is missing (a tablet without a rear one).
                    CameraSelector other = live.facing == 1
                            ? CameraSelector.DEFAULT_BACK_CAMERA : CameraSelector.DEFAULT_FRONT_CAMERA;
                    if (!provider.hasCamera(other)) {
                        live.report(UNAVAILABLE, "");
                        return;
                    }
                    selector = other;
                }
                Preview preview = new Preview.Builder().build();
                preview.setSurfaceProvider(view.getSurfaceProvider());
                ImageCapture capture = new ImageCapture.Builder()
                        .setCaptureMode(ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY)
                        .build();
                live.unbind();
                if (live.lifecycle.getCurrentState() == Lifecycle.State.DESTROYED) {
                    return;
                }
                live.lifecycle.setCurrentState(Lifecycle.State.RESUMED);
                provider.bindToLifecycle(live, selector, preview, capture);
                live.preview = preview;
                live.capture = capture;
                live.report(RUNNING, "");
            } catch (ExecutionException | InterruptedException | CameraInfoUnavailableException
                     | IllegalArgumentException | IllegalStateException | SecurityException e) {
                live.report(ERROR, e.getMessage() == null ? e.getClass().getSimpleName() : e.getMessage());
            }
        }, MAIN::post);
    }

    private static void stop(Live live) {
        live.wanted = false;
        live.unbind();
        if (live.lifecycle.getCurrentState() != Lifecycle.State.DESTROYED) {
            live.lifecycle.setCurrentState(Lifecycle.State.CREATED);
        }
        live.report(STOPPED, "");
    }

    /** Rotate `file` in place by its EXIF orientation, if any. False when it could not be rewritten. */
    private static boolean upright(File file) {
        int degrees;
        try {
            ExifInterface exif = new ExifInterface(file.getAbsolutePath());
            switch (exif.getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)) {
                case ExifInterface.ORIENTATION_ROTATE_90:
                    degrees = 90;
                    break;
                case ExifInterface.ORIENTATION_ROTATE_180:
                    degrees = 180;
                    break;
                case ExifInterface.ORIENTATION_ROTATE_270:
                    degrees = 270;
                    break;
                default:
                    return true;
            }
        } catch (IOException e) {
            return true; // no EXIF to honour
        }
        Bitmap source = BitmapFactory.decodeFile(file.getAbsolutePath());
        if (source == null) {
            return false;
        }
        Matrix matrix = new Matrix();
        matrix.postRotate(degrees);
        Bitmap rotated = Bitmap.createBitmap(source, 0, 0, source.getWidth(), source.getHeight(), matrix, true);
        source.recycle();
        try (FileOutputStream out = new FileOutputStream(file)) {
            rotated.compress(Bitmap.CompressFormat.JPEG, 92, out);
        } catch (IOException e) {
            return false;
        } finally {
            rotated.recycle();
        }
        return true;
    }

    private static void capture(Live live) {
        ImageCapture capture = live.capture;
        if (capture == null) {
            live.report(ERROR, "the camera is not running");
            return;
        }
        File dir = new File(DayBridge.ctx.getCacheDir(), "day-camera");
        if (!dir.isDirectory() && !dir.mkdirs()) {
            live.report(ERROR, "the cache directory could not be created");
            return;
        }
        File file = new File(dir, UUID.randomUUID() + ".jpg");
        ImageCapture.OutputFileOptions options = new ImageCapture.OutputFileOptions.Builder(file).build();
        capture.takePicture(options, IO, new ImageCapture.OnImageSavedCallback() {
            @Override
            public void onImageSaved(@NonNull ImageCapture.OutputFileResults results) {
                // CameraX writes the sensor's pixels and records the rotation as EXIF. A photo
                // is handed over UPRIGHT, so a viewer that ignores EXIF shows it right too.
                if (!upright(file)) {
                    MAIN.post(() -> live.report(ERROR, "the photo could not be rotated"));
                    return;
                }
                // Bounds only: the JPEG is not decoded again, just measured.
                BitmapFactory.Options bounds = new BitmapFactory.Options();
                bounds.inJustDecodeBounds = true;
                BitmapFactory.decodeFile(file.getAbsolutePath(), bounds);
                final String text = file.getAbsolutePath() + '\u001F' + bounds.outWidth + '\u001F' + bounds.outHeight;
                MAIN.post(() -> live.report(PHOTO, text));
            }

            @Override
            public void onError(@NonNull ImageCaptureException e) {
                final String message = e.getMessage() == null ? "capture failed" : e.getMessage();
                MAIN.post(() -> live.report(ERROR, message));
            }
        });
    }
}

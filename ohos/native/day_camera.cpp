// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// The day-piece-camera crate's OWN HarmonyOS backend — a C shim over the NDK camera kit
// (ohcamera). Compiled by build.rs against the OpenHarmony sysroot and linked into the app's
// native library. C++ only because the camera headers pull in `rawfile/raw_file.h`, which the
// SDK writes with a C++ reference parameter; the shim itself is plain C with a C ABI. lib-arkui.rs calls the flat API below and receives reports through the
// callback it hands over at creation. The preview renders into the XComponent surface the
// crate's ArkTS half (ohos/ets/Index.ets) reports; each capture is the photo output's main
// image, written as a JPEG into the app's cache directory. It is the HarmonyOS twin of
// ios/swift/DayCamera.swift and android/java/DayCamera.java.

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <pthread.h>
#include <sys/stat.h>

#include <multimedia/image_framework/image/image_native.h>
#include <native_buffer/native_buffer.h>
#include <ohcamera/camera.h>
#include <ohcamera/camera_input.h>
#include <ohcamera/camera_manager.h>
#include <ohcamera/capture_session.h>
#include <ohcamera/photo_native.h>
#include <ohcamera/photo_output.h>
#include <ohcamera/preview_output.h>

// src/lib.rs `report::*`.
#define REPORT_STOPPED 0.0
#define REPORT_STARTING 1.0
#define REPORT_RUNNING 2.0
#define REPORT_UNAVAILABLE 3.0
#define REPORT_DENIED 4.0
#define REPORT_ERROR 5.0
#define REPORT_PHOTO 6.0

#define CMD_START 0
#define CMD_STOP 1
#define CMD_CAPTURE 2
#define CMD_FACING 3

typedef void (*day_camera_report)(uint64_t node, double num, const char* text);

typedef struct DayCamera {
    uint64_t node;
    day_camera_report report;
    int facing; // 0 back, 1 front
    int wanted; // the app wants the session running
    char surface[128];
    char cache_dir[1024];
    Camera_Manager* manager;
    Camera_Input* input;
    Camera_CaptureSession* session;
    Camera_PreviewOutput* preview;
    Camera_PhotoOutput* photo;
    int running;
} DayCamera;

// The photo-available callback carries no user pointer, so the live cameras are looked up by
// their photo output. A handful at most; a list is enough.
#define MAX_CAMERAS 8
static DayCamera* g_cameras[MAX_CAMERAS];
static pthread_mutex_t g_lock = PTHREAD_MUTEX_INITIALIZER;

static void track(DayCamera* cam) {
    pthread_mutex_lock(&g_lock);
    for (int i = 0; i < MAX_CAMERAS; i++) {
        if (!g_cameras[i]) {
            g_cameras[i] = cam;
            break;
        }
    }
    pthread_mutex_unlock(&g_lock);
}

static void untrack(DayCamera* cam) {
    pthread_mutex_lock(&g_lock);
    for (int i = 0; i < MAX_CAMERAS; i++) {
        if (g_cameras[i] == cam) g_cameras[i] = NULL;
    }
    pthread_mutex_unlock(&g_lock);
}

static DayCamera* by_photo_output(Camera_PhotoOutput* out) {
    DayCamera* found = NULL;
    pthread_mutex_lock(&g_lock);
    for (int i = 0; i < MAX_CAMERAS; i++) {
        if (g_cameras[i] && g_cameras[i]->photo == out) {
            found = g_cameras[i];
            break;
        }
    }
    pthread_mutex_unlock(&g_lock);
    return found;
}

static void send(DayCamera* cam, double code, const char* text) {
    if (cam && cam->report) cam->report(cam->node, code, text ? text : "");
}

static void fail(DayCamera* cam, const char* what, Camera_ErrorCode err) {
    char msg[160];
    snprintf(msg, sizeof msg, "%s failed (%d)", what, (int)err);
    send(cam, REPORT_ERROR, msg);
}

// Tear the session down, keeping the manager. Safe to call twice.
static void teardown(DayCamera* cam) {
    if (cam->session) {
        if (cam->running) OH_CaptureSession_Stop(cam->session);
        cam->running = 0;
        OH_CaptureSession_Release(cam->session);
        cam->session = NULL;
    }
    if (cam->photo) {
        OH_PhotoOutput_Release(cam->photo);
        cam->photo = NULL;
    }
    if (cam->preview) {
        OH_PreviewOutput_Release(cam->preview);
        cam->preview = NULL;
    }
    if (cam->input) {
        OH_CameraInput_Close(cam->input);
        OH_CameraInput_Release(cam->input);
        cam->input = NULL;
    }
}

// The preview profile: the largest 4:3 one no wider than 1280, else the first offered.
static const Camera_Profile* pick_preview(Camera_OutputCapability* cap) {
    const Camera_Profile* best = NULL;
    for (uint32_t i = 0; i < cap->previewProfilesSize; i++) {
        const Camera_Profile* p = cap->previewProfiles[i];
        if (!p) continue;
        if (!best) best = p;
        int four_three = p->size.width * 3 == p->size.height * 4;
        if (four_three && p->size.width <= 1280 &&
            (best->size.width * 3 != best->size.height * 4 || p->size.width > best->size.width)) {
            best = p;
        }
    }
    return best;
}

// The photo profile: the largest JPEG one, else the largest of any format.
static const Camera_Profile* pick_photo(Camera_OutputCapability* cap) {
    const Camera_Profile* best = NULL;
    for (uint32_t i = 0; i < cap->photoProfilesSize; i++) {
        const Camera_Profile* p = cap->photoProfiles[i];
        if (!p) continue;
        if (!best) {
            best = p;
            continue;
        }
        int best_jpeg = best->format == CAMERA_FORMAT_JPEG;
        int p_jpeg = p->format == CAMERA_FORMAT_JPEG;
        uint64_t best_area = (uint64_t)best->size.width * best->size.height;
        uint64_t p_area = (uint64_t)p->size.width * p->size.height;
        if ((p_jpeg && !best_jpeg) || (p_jpeg == best_jpeg && p_area > best_area)) best = p;
    }
    return best;
}

static void on_photo(Camera_PhotoOutput* out, OH_PhotoNative* photo);

static void start(DayCamera* cam) {
    cam->wanted = 1;
    if (!cam->surface[0]) {
        // The surface arrives from ArkTS once the node is on screen; start then.
        send(cam, REPORT_STARTING, "");
        return;
    }
    send(cam, REPORT_STARTING, "");
    teardown(cam);
    Camera_ErrorCode err;
    if (!cam->manager) {
        err = OH_Camera_GetCameraManager(&cam->manager);
        if (err != CAMERA_OK || !cam->manager) {
            cam->manager = NULL;
            send(cam, REPORT_UNAVAILABLE, "");
            return;
        }
    }
    Camera_Device* devices = NULL;
    uint32_t count = 0;
    err = OH_CameraManager_GetSupportedCameras(cam->manager, &devices, &count);
    if (err != CAMERA_OK || count == 0 || !devices) {
        send(cam, REPORT_UNAVAILABLE, "");
        return;
    }
    Camera_Position want = cam->facing == 1 ? CAMERA_POSITION_FRONT : CAMERA_POSITION_BACK;
    const Camera_Device* device = &devices[0];
    for (uint32_t i = 0; i < count; i++) {
        if (devices[i].cameraPosition == want) {
            device = &devices[i];
            break;
        }
    }
    Camera_OutputCapability* cap = NULL;
    err = OH_CameraManager_GetSupportedCameraOutputCapability(cam->manager, device, &cap);
    if (err != CAMERA_OK || !cap) {
        OH_CameraManager_DeleteSupportedCameras(cam->manager, devices, count);
        fail(cam, "output capability", err);
        return;
    }
    const Camera_Profile* preview = pick_preview(cap);
    const Camera_Profile* photo = pick_photo(cap);
    if (!preview || !photo) {
        OH_CameraManager_DeleteSupportedCameraOutputCapability(cam->manager, cap);
        OH_CameraManager_DeleteSupportedCameras(cam->manager, devices, count);
        send(cam, REPORT_UNAVAILABLE, "");
        return;
    }
    Camera_Profile preview_profile = *preview;
    Camera_Profile photo_profile = *photo;

    err = OH_CameraManager_CreateCameraInput(cam->manager, device, &cam->input);
    if (err == CAMERA_OK) err = OH_CameraInput_Open(cam->input);
    if (err == CAMERA_OK) err = OH_CameraManager_CreateCaptureSession(cam->manager, &cam->session);
    if (err == CAMERA_OK) {
        err = OH_CameraManager_CreatePreviewOutput(cam->manager, &preview_profile, cam->surface,
                                                   &cam->preview);
    }
    if (err == CAMERA_OK) {
        err = OH_CameraManager_CreatePhotoOutputWithoutSurface(cam->manager, &photo_profile,
                                                               &cam->photo);
    }
    if (err == CAMERA_OK) err = OH_PhotoOutput_RegisterPhotoAvailableCallback(cam->photo, on_photo);
    if (err == CAMERA_OK) err = OH_CaptureSession_BeginConfig(cam->session);
    if (err == CAMERA_OK) err = OH_CaptureSession_AddInput(cam->session, cam->input);
    if (err == CAMERA_OK) err = OH_CaptureSession_AddPreviewOutput(cam->session, cam->preview);
    if (err == CAMERA_OK) err = OH_CaptureSession_AddPhotoOutput(cam->session, cam->photo);
    if (err == CAMERA_OK) err = OH_CaptureSession_CommitConfig(cam->session);
    if (err == CAMERA_OK) err = OH_CaptureSession_Start(cam->session);
    OH_CameraManager_DeleteSupportedCameraOutputCapability(cam->manager, cap);
    OH_CameraManager_DeleteSupportedCameras(cam->manager, devices, count);
    if (err != CAMERA_OK) {
        teardown(cam);
        if (err == CAMERA_OPERATION_NOT_ALLOWED) {
            send(cam, REPORT_DENIED, "");
        } else {
            fail(cam, "session", err);
        }
        return;
    }
    cam->running = 1;
    send(cam, REPORT_RUNNING, "");
}

static void stop(DayCamera* cam) {
    cam->wanted = 0;
    teardown(cam);
    send(cam, REPORT_STOPPED, "");
}

// A capture landed: write its main image (a JPEG) into the cache directory and report the path.
static void on_photo(Camera_PhotoOutput* out, OH_PhotoNative* photo) {
    DayCamera* cam = by_photo_output(out);
    if (!cam) {
        OH_PhotoNative_Release(photo);
        return;
    }
    OH_ImageNative* image = NULL;
    if (OH_PhotoNative_GetMainImage(photo, &image) != CAMERA_OK || !image) {
        OH_PhotoNative_Release(photo);
        send(cam, REPORT_ERROR, "the photo has no main image");
        return;
    }
    uint32_t* types = NULL;
    size_t type_count = 0;
    OH_NativeBuffer* buffer = NULL;
    size_t size = 0;
    Image_Size dims = {0, 0};
    OH_ImageNative_GetImageSize(image, &dims);
    int ok = OH_ImageNative_GetComponentTypes(image, &types, &type_count) == IMAGE_SUCCESS &&
             type_count > 0 && types &&
             OH_ImageNative_GetByteBuffer(image, types[0], &buffer) == IMAGE_SUCCESS && buffer &&
             OH_ImageNative_GetBufferSize(image, types[0], &size) == IMAGE_SUCCESS && size > 0;
    void* bytes = NULL;
    if (ok && OH_NativeBuffer_Map(buffer, &bytes) != 0) ok = 0;
    char path[1200];
    if (ok) {
        snprintf(path, sizeof path, "%s/day-camera", cam->cache_dir);
        mkdir(path, 0700);
        struct timespec ts;
        clock_gettime(CLOCK_REALTIME, &ts);
        snprintf(path, sizeof path, "%s/day-camera/%lld-%ld.jpg", cam->cache_dir,
                 (long long)ts.tv_sec, ts.tv_nsec / 1000);
        FILE* f = fopen(path, "wb");
        if (!f || fwrite(bytes, 1, size, f) != size) ok = 0;
        if (f) fclose(f);
        OH_NativeBuffer_Unmap(buffer);
    }
    OH_ImageNative_Release(image);
    OH_PhotoNative_Release(photo);
    if (!ok) {
        send(cam, REPORT_ERROR, "the photo could not be written");
        return;
    }
    char text[1300];
    snprintf(text, sizeof text, "%s\x1f%u\x1f%u", path, dims.width, dims.height);
    send(cam, REPORT_PHOTO, text);
}

extern "C" DayCamera* day_camera_ohos_new(uint64_t node, int facing, int active,
                                          const char* cache_dir, day_camera_report report) {
    DayCamera* cam = static_cast<DayCamera*>(calloc(1, sizeof(DayCamera)));
    if (!cam) return NULL;
    cam->node = node;
    cam->report = report;
    cam->facing = facing;
    if (cache_dir) strncpy(cam->cache_dir, cache_dir, sizeof cam->cache_dir - 1);
    track(cam);
    if (active) start(cam);
    return cam;
}

// The ArkTS surface exists (or, with an empty id, went away). A wanted session starts now.
extern "C" void day_camera_ohos_surface(DayCamera* cam, const char* surface_id) {
    if (!cam) return;
    if (!surface_id || !surface_id[0]) {
        cam->surface[0] = 0;
        teardown(cam);
        return;
    }
    strncpy(cam->surface, surface_id, sizeof cam->surface - 1);
    if (cam->wanted) start(cam);
}

extern "C" void day_camera_ohos_command(DayCamera* cam, int cmd, int arg) {
    if (!cam) return;
    switch (cmd) {
        case CMD_START:
            start(cam);
            break;
        case CMD_STOP:
            stop(cam);
            break;
        case CMD_CAPTURE:
            if (!cam->photo || !cam->running) {
                send(cam, REPORT_ERROR, "the camera is not running");
            } else {
                Camera_ErrorCode err = OH_PhotoOutput_Capture(cam->photo);
                if (err != CAMERA_OK) fail(cam, "capture", err);
            }
            break;
        case CMD_FACING:
            if (cam->facing != arg) {
                cam->facing = arg;
                if (cam->wanted) start(cam);
            }
            break;
        default:
            break;
    }
}

extern "C" void day_camera_ohos_release(DayCamera* cam) {
    if (!cam) return;
    untrack(cam);
    teardown(cam);
    free(cam);
}

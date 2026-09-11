---
title: "Camera"
description: "A native camera viewfinder with photo capture: AVFoundation, CameraX and the HarmonyOS NDK camera kit behind one piece, wired to Day's permissions."
---

<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: CC-BY-SA-4.0
-->

> **Status: shipped** on ios-uikit and android-mdc; harmony-arkui compiles and links but is
> unverified on hardware.

`camera()` is a leaf piece: the platform's live preview, a command to take a photo, and the
photo back as a file. It fills the frame it is given, so constrain it with `.frame(w, h)`.

## The API

```rust
camera()
    .facing(facing)        // Facing::Back (default) or Front; a Signal switches live
    .active(active)        // bool; the session runs only while true AND the permission is held
    .auto_request()        // opt in: ask for the permission when the session is first wanted
    .capture(shutter)      // a Trigger; each fire takes one photo
    .state(state)          // Signal<CameraState>
    .photo(photo)          // Signal<Option<Photo>>
```

| State | Meaning |
|---|---|
| `Idle` | no session: never started, stopped, or the permission is undecided and nobody asked |
| `Starting` | the session is being configured |
| `Running` | the preview is live; a capture will succeed |
| `Unavailable` | no usable camera (the iOS Simulator) |
| `Denied` | the permission was refused or is restricted |
| `Error(text)` | the native side failed |

`CameraState::label()` is the locale-independent word a dayscript asserts on.

A `Photo` is `{ path, width, height }`: a JPEG under the app's cache directory
(`day-camera/<uuid>.jpg`) that the app may keep, move or delete. `bytes()` reads it as
`Arc<Vec<u8>>`, the shape `day-piece-remote-image` displays. The photo library is never touched.

## Permission

The piece declares `[package.metadata.day.permissions] uses = ["camera"]`, so `day build` adds
`android.permission.CAMERA`, `NSCameraUsageDescription` and `ohos.permission.CAMERA` to the app
it is built into. The reason is the app's (`permission_camera` in its catalogs or
`[permissions] camera = "…"` in `Day.toml`); on iOS and HarmonyOS a build without one fails
naming this crate.

Before every start the piece reads `day_part_permissions::status(Permission::Camera)`. It never
prompts unless the app opts in with `.auto_request()`, in which case an undecided permission is
requested the first time the session is wanted and the session starts on a grant. Prompting is
otherwise the app's: it owns the rationale and decides when a system dialog is welcome.

## Per platform

| Platform | Preview | Capture | Declared in |
|---|---|---|---|
| iOS | a `UIView` whose layer is `AVCaptureVideoPreviewLayer`, `resizeAspectFill` | `AVCapturePhotoOutput`, JPEG | `[package.metadata.day.ios] swift`, `frameworks = ["AVFoundation"]` |
| Android | CameraX `PreviewView`, `COMPATIBLE` mode | `ImageCapture.takePicture` to a file | `[package.metadata.day.android] java`, `gradle-dependencies` (camera-core, -camera2, -lifecycle, -view 1.5.1) |
| HarmonyOS | an ArkTS `XComponent` surface, drawn by the NDK camera kit | `OH_PhotoOutput_Capture`, the main image packed to JPEG | `[package.metadata.day.ohos] ets`, plus `build.rs` compiling `ohos/native/day_camera.cpp` |

**iOS.** `ios/swift/DayCamera.swift` runs in the generated DayPieces package. Rust calls three
`@_cdecl` functions (`day_camera_new`, `day_camera_command`, `day_camera_release`) and hands
over a C function pointer at creation; the shim reports through it on the main queue. Session
configuration and `startRunning()` run on a serial queue. iOS 17's `RotationCoordinator` keeps
the preview and the capture upright; below 17 the preview is fixed portrait, so the floor stays
at Day's 16.

**Android.** `DayCamera.java` is its own `LifecycleOwner`: CameraX binds use cases to a
lifecycle, and binding to the activity's would keep the camera open across pages. The
registry moves to `RESUMED` on start and `DESTROYED` on release, and only this viewfinder's
use cases are unbound, never `unbindAll`. Reports go through `DayBridge.nativeOnEvent` with
the node id the view was made with.

**HarmonyOS.** The ArkTS half builds an `XComponent` (type `SURFACE`) inside a `BuilderNode`
and reports its surface id through `pieceEvent`; Rust never sees the surface. The C half,
linked against `libohcamera.so`, `libohimage.so` and `libnative_window.so`, opens the camera,
creates the preview output on that surface and a photo output without one, and packs the
captured main image to a JPEG in the cache directory. Sizes are explicit, never percentages:
a `BuilderNode` builds detached, so `'100%'` would resolve against the whole window.

## The boundary

Commands cross as `CameraPatch::{Start, Stop, Capture, Facing}`; reports come back as
`Event::Custom { num, text }`, where `num` is the discriminator (`report::STOPPED` … `PHOTO`)
because a Custom event's `tag` is empty across JNI and the C ABI. A `PHOTO` report's text is
`path`, `width`, `height`, 0x1F-separated. Every report re-enters Rust on the main thread: the
Swift shim dispatches to the main queue, the Java class posts to the main looper, the ArkUI
bridge posts through its own seam.

## Testing

The demo's `dayscript/camera.yaml` asserts the page before the permission is granted and is
the CI script. `dayscript/camera-capture.yaml` takes a photo after `adb shell pm grant` (or
`xcrun simctl privacy … grant camera`) and is run by hand: an OS permission dialog is a system
modal a dayscript cannot dismiss. The Android emulator's virtual camera runs the full path; the
iOS Simulator has no capture device and reports `unavailable`.

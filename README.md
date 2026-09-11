<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# day-piece-camera

[![ci](https://github.com/daybrite/day-piece-camera/actions/workflows/ci.yml/badge.svg)](https://github.com/daybrite/day-piece-camera/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MPL--2.0-green.svg)](LICENSE)

## Overview and capabilities

`day-piece-camera` adds a live camera preview and JPEG photo capture to a Day app.

[Day](https://github.com/daybrite/day) is a Rust framework for building applications
from a shared codebase using each platform's native UI toolkit. A **piece** is a UI
component you place in a layout; a **part** provides a capability without drawing UI.
Day's `day` command builds and packages the Rust code, resources, and native platform
code together. `Cargo.toml` declares Rust dependencies; `Day.toml` configures the app
and its target platforms.

The `camera()` piece supports front/back camera switching, reactive session control,
a capture trigger, and signals reporting session state and the latest photo. A
**signal** is observable state: changing one updates the UI that reads it. A
**trigger** is an event you fire, such as a shutter press.

Photos are files in the app's cache directory, with a path, width, and height in
`Photo`. `Photo::bytes()` reads a file into `Arc<Vec<u8>>`, which can be displayed
with Day's `day-piece-remote-image`. The crate does not save to the photo library;
move or copy captures to persistent storage if the app needs to keep them.

## Platform support and limitations

| Day target | Implementation | Limitations |
|---|---|---|
| `ios-uikit` | [AVFoundation capture sessions](https://developer.apple.com/documentation/avfoundation/setting-up-a-capture-session), preview and photo output | Physical device required for capture; the iOS Simulator reports `Unavailable`. Below iOS 17, the preview is fixed portrait. |
| `android-mdc` | [CameraX (`androidx.camera`)](https://developer.android.com/jetpack/androidx/releases/camera), preview and image capture; [architecture guide](https://developer.android.com/media/camera/camerax/architecture) | Requires a usable camera and permission; the emulator's virtual camera supports the capture path. |
| `harmony-arkui` | ArkUI surface and [OpenHarmony native camera API (`OH_Camera`)](https://github.com/openharmony/docs/blob/master/en/application-dev/reference/apis-camera-kit/capi-oh-camera.md) | Compiles and links, but is unverified on hardware. |
| Desktop, web, mock | No camera renderer | The piece becomes a placeholder; `support()` returns `Unsupported`. |

`day_piece_camera::support()` reports whether a renderer was compiled in, not whether
hardware or permission is available. Read `CameraState` at runtime: `Idle`,
`Starting`, `Running`, `Unavailable`, `Denied`, or `Error(String)`. Torch, zoom, and
video recording are not exposed.

## Add it to a Day project

Start with a Day app (the [demo](demo/) shows its complete structure). Add this to
its `Cargo.toml`:

```toml
[dependencies]
day-piece-camera = { git = "https://github.com/daybrite/day-piece-camera.git" }
```

Add an app-specific camera explanation to `Day.toml`, merging it into any existing
`[permissions]` table:

```toml
[permissions]
camera = "Take photos to attach to your notes."
```

Alternatively, use `camera = true` and supply `permission_camera` in the app's
translation catalogs, as the demo does. The crate declares the permission it uses;
Day generates the Android, iOS, and HarmonyOS permission entries. iOS and HarmonyOS
builds require the app's explanation.

Return this piece from your app's UI-building function or place it in a layout:

```rust
use day::prelude::*;
use day_piece_camera::{camera, CameraState, Photo};

fn camera_panel() -> impl Piece {
    let shutter = Trigger::new();
    let state = Signal::new(CameraState::Idle);
    let photo: Signal<Option<Photo>> = Signal::new(None);

    column((
        camera()
            .auto_request()
            .capture(shutter)
            .state(state)
            .photo(photo)
            .frame(320.0, 240.0),
        button("Take photo")
            .enabled(move || state.get() == CameraState::Running)
            .action(move || shutter.notify()),
    ))
}
```

`.auto_request()` opts into the system permission prompt when the session is first
wanted. Without it, request permission yourself through `day-part-permissions`;
an undecided permission leaves the camera idle. `.active(signal)` starts/stops the
session, `.facing(signal)` switches cameras, and removing the piece stops its session.
Gate camera navigation with `support()` on apps that also target desktop or web.

With the chosen target configured in your app and its platform SDK installed, run
`day build -p android-mdc` or `day launch -p ios-uikit`. Day reads the crate's backend
metadata and enables its matching feature; you do not need to forward camera
features in your app. Use Day's build pipeline to include the native sources.

## Architecture and dependencies

Dependency links below lead to upstream source repositories or official API
documentation. Version requirements describe this checkout's [Cargo.toml](Cargo.toml),
not necessarily the newest upstream releases.

[src/lib.rs](src/lib.rs) defines the portable piece, properties, commands, permission
checks, and event decoding. Reactive changes become `CameraPatch` commands (`Start`,
`Stop`, `Capture`, `Facing`). Native callbacks become Day custom events that update
the app's state and photo signals on the UI thread. Platform renderers register at
link time through [linkme](https://github.com/dtolnay/linkme).

| Layer | Dependencies and responsibility |
|---|---|
| Shared Rust | [day-core](https://github.com/daybrite/day/tree/main/crates/day-core) for the piece/tree, [day-spec](https://github.com/daybrite/day/tree/main/crates/day-spec) for types/events, [day-pieces](https://github.com/daybrite/day/tree/main/crates/day-pieces) for builders, [day-reactive](https://github.com/daybrite/day/tree/main/crates/day-reactive) for bindings, [day-part-permissions](https://github.com/daybrite/day/tree/main/parts/day-part-permissions) for permission checks, [log](https://github.com/rust-lang/log) for diagnostics, [linkme](https://github.com/dtolnay/linkme) for registration. |
| iOS | Optional [day-uikit](https://github.com/daybrite/day/tree/main/toolkits/day-uikit), [objc2](https://github.com/madsmtm/objc2) 0.6 and [objc2-ui-kit](https://github.com/madsmtm/objc2) 0.3. A Swift shim calls system AVFoundation; Day stages it into the app's Swift build. |
| Android | Optional [day-android](https://github.com/daybrite/day/tree/main/toolkits/day-android) provides the JNI/backend boundary. Gradle adds CameraX `camera-core`, `camera-camera2`, `camera-lifecycle`, and `camera-view`, each at 1.5.1, plus their transitive dependencies. |
| HarmonyOS | Optional [day-arkui](https://github.com/daybrite/day/tree/main/toolkits/day-arkui) and build dependency [cc](https://github.com/rust-lang/cc-rs) 1. `build.rs` compiles the C++ camera shim; the native implementation links `ohcamera`, `ohimage`, and `native_window`. Day stages the ArkTS surface code. |
| Tests | [day-mock](https://github.com/daybrite/day/tree/main/crates/day-mock) supplies the headless test backend. |

The iOS shim configures the session on a serial queue and reports on the main queue.
Android owns a separate camera lifecycle so navigating away releases this
viewfinder's use cases. HarmonyOS connects an ArkTS `XComponent` surface to the NDK
camera and encodes captured images as JPEG. See [the design](docs/camera.md) and
[platform sources](platform/) for the command protocol and resource ownership.

## Compatibility and development

This checkout requires Rust 1.89 or newer and declares compatibility with Day 0.4 in
[Cargo.toml](Cargo.toml). The crate is consumed from Git, not crates.io. Its Day
dependencies use `https://github.com/daybrite/day.git` without a branch, tag, or
revision. Use the same source in your app and keep its `Cargo.lock` to record the
resolved revisions. Mixing Day source URLs or refs can introduce duplicate framework
crates and incompatible types.

For a local framework checkout, run `day patch --local ../day` from this repository
(adjust the path when running from `demo/`). The [demo](demo/) depends on this crate
by path and is a complete integration example.

Run `cargo test` for the host-side checks. From `demo/`, run
`day launch -p android-mdc --script dayscript/camera.yaml` for the walkthrough.
The separate `dayscript/camera-capture.yaml` requires permission to be granted first;
Day scripts cannot dismiss the OS permission dialog. Validate capture on a real
iPhone and on the intended HarmonyOS hardware before relying on those paths.

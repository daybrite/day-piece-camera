<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# day-piece-camera

[![ci](https://github.com/daybrite/day-piece-camera/actions/workflows/ci.yml/badge.svg)](https://github.com/daybrite/day-piece-camera/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MPL--2.0-green.svg)](LICENSE)

A camera viewfinder with photo capture for [Day](https://daybrite.dev) apps: AVFoundation on
iOS, CameraX on Android, the NDK camera kit on HarmonyOS, behind one Rust piece. The preview
is the platform's own view, a photo lands as a JPEG in the app's cache directory, and the
camera permission flows through Day's permissions system.

## Use it

```toml
[dependencies]
day-piece-camera = { git = "https://github.com/daybrite/day-piece-camera.git" }

[package.metadata.day.permissions]   # or `[permissions] camera = "…"` in Day.toml
```

The piece declares that it uses the `camera` permission; the app supplies the reason the OS
shows, as `permission_camera` in its catalogs or inline in `Day.toml`
([permissions guide](https://daybrite.dev/docs/guide-permissions)).

```rust
use day::prelude::*;
use day_piece_camera::{camera, CameraState, Photo};

let shutter = Trigger::new();
let state = Signal::new(CameraState::Idle);
let photo: Signal<Option<Photo>> = Signal::new(None);

column((
    camera()
        .auto_request()          // ask for the permission the first time the session is wanted
        .capture(shutter)        // fire it to take a photo
        .state(state)            // idle | starting | running | unavailable | denied | error
        .photo(photo)            // the last capture: a JPEG under the app's cache directory
        .frame(320.0, 240.0),
    button("Take photo")
        .enabled(move || state.get() == CameraState::Running)
        .action(move || shutter.notify()),
))
```

`.facing(signal)` switches between the rear and front cameras live; `.active(signal)` starts
and stops the session, which also stops when the piece leaves the tree. A `Photo` names its
file and size; `photo.bytes()` reads it, and
[`day-piece-remote-image`](https://github.com/daybrite/day/tree/main/pieces/day-piece-remote-image)
displays those bytes. The photo library is never touched.

Without `.auto_request()` the piece never prompts: it reads the permission's status before
every start and stays `idle` until the app asks through
[`day-part-permissions`](https://daybrite.dev/docs/permissions) itself, the way the demo does.

The front-end compiles on every backend and `day_piece_camera::support()` says whether this one
renders, so an app hides its camera page where the answer is `Unsupported`:

```rust
if day_piece_camera::support() != Support::Unsupported { /* list the page */ }
```

## Limits

- The iOS Simulator has no capture device: the state reads `unavailable` there. Capture is
  verified on an iPhone. The Android emulator's virtual camera runs the whole path.
- A dayscript cannot dismiss an OS permission dialog, so a walkthrough asserts the page before
  the permission is granted, and a capture script runs after `adb shell pm grant` or
  `xcrun simctl privacy … grant camera` (see [demo/](demo)).
- Torch and zoom are not exposed yet.

## Compatibility

| This crate | Tested against day | Toolkits |
|---|---|---|
| 0.1.x | main (0.4) | ios-uikit, android-mdc, harmony-arkui |

Every day dependency here names the bare canonical git URL with no ref, so an app's own
`Cargo.lock` picks one day revision for the whole graph; `[package.metadata.day] compat` records
the day minor this release was built against, and `day build` notes a mismatch. `day patch
--local ../day` (or `--git`) builds against a checkout.

## Develop it

```sh
cargo test                                                     # the host half
cargo clippy --target aarch64-apple-ios-sim --features uikit   # the iOS arm
cargo clippy --target aarch64-linux-android --features mdc     # the Android arm
cd demo && day launch -p ios-uikit --script dayscript/camera.yaml
```

[docs/camera.md](docs/camera.md) is the design: the API, each platform's mechanism, and the
event and command vocabulary that crosses the native boundary.

## Part of Day

Day is a Rust UI framework that renders each platform's own widgets from one codebase. This
crate is one of its external pieces: it lives in its own repository, is built by `day build`
from its `[package.metadata.day.*]` declarations, and registers itself into each backend at
link time. The framework is at [daybrite/day](https://github.com/daybrite/day); the guide to
writing a piece like this one is [Extending Day](https://daybrite.dev/docs/extending).

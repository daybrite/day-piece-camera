<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# Camera Demo

The demo and on-device test app for [`day-piece-camera`](..): one page with the camera
permission, the viewfinder, a shutter, a camera switch, and the last photo. It depends on the
piece by path (`day-piece-camera = { path = ".." }`), so a change to the piece and a change here
land in one pull request and one CI run.

## Run it

```sh
day doctor                                            # the iOS and Android toolchains
day launch -p ios-uikit --script dayscript/camera.yaml
day launch -p android-mdc --script dayscript/camera.yaml
```

The script asserts the page before the permission is granted and captures one screenshot under
`build/day/screenshots/<target>/`. CI runs exactly this on the iOS Simulator and the Android
emulator ([../.github/workflows/ci.yml](../.github/workflows/ci.yml)).

Taking a photo needs the permission, which only an OS dialog can grant. Grant it out of band,
then run the capture script:

```sh
adb shell pm grant dev.daybrite.camerademo android.permission.CAMERA
day launch -p android-mdc --script dayscript/camera-capture.yaml
```

The Android emulator's virtual camera makes the whole path real. The iOS Simulator has no
capture device, so the page reports `unavailable` there; run the capture script on an iPhone.

## Build against a local day

No `Cargo.lock` is committed: the first build resolves day at the tip of `main`, and `cargo
update` moves it there again. To build against a checkout of day instead:

```sh
day patch --local ../../day             # writes .cargo/config.toml, gitignored
day patch --check                       # every day crate now resolves from the checkout
```

Delete `.cargo/config.toml` to go back to the git dependency. The lock is gitignored, so a
patched build cannot leave the checkout's paths behind for anyone else.

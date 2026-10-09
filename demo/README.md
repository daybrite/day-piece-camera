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
`build/day/screenshots/<target>/`. CI runs exactly this on the iOS Simulator and the HarmonyOS
emulator ([../.github/workflows/ci.yml](../.github/workflows/ci.yml)); its Android leg launches
with the permission granted and runs the capture script below instead, so a photo is taken on
every push.

Taking a photo needs the permission, which only an OS dialog can grant. `--grant camera` marks
it on the device before the app starts, so the capture script never meets the dialog:

```sh
day launch -p android-mdc --grant camera --script dayscript/camera-capture.yaml
```

The Android emulator's virtual camera makes the whole path real. The iOS Simulator has no
camera at all, so the CLI refuses the grant there; on an iPhone, allow the permission once by
hand and run the capture script without the flag.

## Build against a local day

No `Cargo.lock` is committed: the first build resolves day at the tip of `main`, and `cargo
update` moves it there again. To build against a checkout of day instead:

```sh
day patch --local ../../day             # writes .cargo/config.toml, gitignored
day patch --check                       # every day crate now resolves from the checkout
```

Delete `.cargo/config.toml` to go back to the git dependency. The lock is gitignored, so a
patched build cannot leave the checkout's paths behind for anyone else.

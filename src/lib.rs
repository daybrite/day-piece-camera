// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

//! day-piece-camera — a camera viewfinder with photo capture for Day apps.
//!
//! An external Day Piece: one Rust API in front, the platform's camera behind it — AVFoundation
//! on iOS, CameraX on Android, the NDK camera kit on HarmonyOS — declared as
//! `[package.metadata.day.*]` in Cargo.toml and staged by `day build`. The renderers register
//! link-time into each backend's slice; nothing in day knows this crate exists. The desktops and
//! the web have no camera arm: there the kind realizes day's placeholder leaf, and [`support`]
//! answers `Unsupported` so an app can leave its camera page out.
//!
//! ```ignore
//! let shutter = Trigger::new();
//! let state = Signal::new(CameraState::Idle);
//! let photo: Signal<Option<Photo>> = Signal::new(None);
//! camera()
//!     .auto_request()      // ask for the permission the first time the session is wanted
//!     .capture(shutter)    // fire it to take a photo
//!     .state(state)        // what the camera is doing
//!     .photo(photo)        // the last capture, a JPEG in the app's cache directory
//!     .frame(320.0, 240.0)
//! ```
//!
//! The piece never starts a session without the camera permission: it reads
//! `day_part_permissions::status` first, and only [`Camera::auto_request`] makes it ask.
//! Prompting otherwise belongs to the app, which owns the rationale and decides when a system
//! dialog is welcome.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use day_core::{BuildCx, Flex, Piece, RNode, with_tree};
use day_part_permissions::{self as perms, Permission, Status};
use day_pieces::{IntoReactive, Reactive};
use day_reactive::{Signal, Trigger, on_main, watch};
use day_spec::Event;

pub const KIND: &str = "day.piece.camera";

/// Which camera the viewfinder shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facing {
    /// The rear camera, the one a photo is usually taken with.
    #[default]
    Back,
    /// The screen-side camera.
    Front,
}

impl Facing {
    /// The value that crosses the native boundary: 0 back, 1 front.
    pub fn code(self) -> i32 {
        match self {
            Facing::Back => 0,
            Facing::Front => 1,
        }
    }

    /// The other camera.
    pub fn flipped(self) -> Facing {
        match self {
            Facing::Back => Facing::Front,
            Facing::Front => Facing::Back,
        }
    }
}

/// Full props (realize). `active` is whether the session should run from the first frame; it
/// is only ever true when the permission is already held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraProps {
    pub facing: Facing,
    pub active: bool,
}

/// Sparse reconcile patch: the commands Rust sends the native camera after build.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraPatch {
    /// Run the session. Sent only once the permission is held.
    Start,
    /// Stop the session and release the camera.
    Stop,
    /// Take one photo; the native side answers with a [`report::PHOTO`] event.
    Capture,
    /// Switch cameras, live.
    Facing(Facing),
    /// HarmonyOS only: the ArkTS surface the preview renders into is ready; the text is its id.
    /// The front-end forwards the [`report::SURFACE`] event as this patch so the renderer can
    /// hand the surface to the NDK camera. Every other backend ignores it.
    Surface(String),
}

/// What the camera is doing, as the native side reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CameraState {
    /// No session: never started, stopped, or waiting for a permission the app has not asked for.
    #[default]
    Idle,
    /// The session is being configured and started.
    Starting,
    /// The preview is live and a capture will succeed.
    Running,
    /// This device has no usable camera — the iOS Simulator, a desktop without one.
    Unavailable,
    /// The permission was refused, or the OS restricts it, so the session cannot start.
    Denied,
    /// The native side failed; the text is its message.
    Error(String),
}

impl CameraState {
    /// The locale-independent label a script can assert on.
    pub fn label(&self) -> &'static str {
        match self {
            CameraState::Idle => "idle",
            CameraState::Starting => "starting",
            CameraState::Running => "running",
            CameraState::Unavailable => "unavailable",
            CameraState::Denied => "denied",
            CameraState::Error(_) => "error",
        }
    }

    /// The state a native report means. [`report::PHOTO`] is not a state and maps to `None`.
    pub fn from_report(num: f64, text: &str) -> Option<CameraState> {
        Some(match num as i32 {
            report::STOPPED => CameraState::Idle,
            report::STARTING => CameraState::Starting,
            report::RUNNING => CameraState::Running,
            report::UNAVAILABLE => CameraState::Unavailable,
            report::DENIED => CameraState::Denied,
            report::ERROR => CameraState::Error(text.to_string()),
            _ => return None,
        })
    }
}

/// The codes the native sides report through `Event::Custom { num, .. }`. `num` is the
/// discriminator: a Custom event's `tag` is empty across JNI and the C ABI, so it is never
/// matched on.
pub mod report {
    pub const STOPPED: i32 = 0;
    pub const STARTING: i32 = 1;
    pub const RUNNING: i32 = 2;
    pub const UNAVAILABLE: i32 = 3;
    pub const DENIED: i32 = 4;
    pub const ERROR: i32 = 5;
    /// A photo landed on disk. `text` is `path`, `width` and `height`, 0x1F-separated.
    pub const PHOTO: i32 = 6;
    /// HarmonyOS only: the ArkTS side's preview surface exists; `text` is its surface id.
    pub const SURFACE: i32 = 7;
}

/// A photo the camera took: a JPEG under the app's cache directory.
///
/// The file is the app's to keep, move, or delete; the piece never touches the photo library,
/// so no Photos permission is involved. [`Photo::bytes`] reads it on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Photo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

impl Photo {
    /// Parse a [`report::PHOTO`] payload: `path`, then the pixel size, 0x1F-separated. A
    /// payload without a size is still a photo; the size then reads 0 × 0.
    pub fn from_report(text: &str) -> Option<Photo> {
        let mut parts = text.split('\u{1f}');
        let path = parts.next().filter(|p| !p.is_empty())?;
        let width = parts
            .next()
            .and_then(|w| w.trim().parse().ok())
            .unwrap_or(0);
        let height = parts
            .next()
            .and_then(|h| h.trim().parse().ok())
            .unwrap_or(0);
        Some(Photo {
            path: PathBuf::from(path),
            width,
            height,
        })
    }

    /// The JPEG bytes, read from the cache file — shared, since a photo is large and a viewer
    /// keeps a reference to it.
    pub fn bytes(&self) -> io::Result<Arc<Vec<u8>>> {
        std::fs::read(&self.path).map(Arc::new)
    }

    /// The file's name, for a caption.
    pub fn file_name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What this backend realizes: `Native` where a camera arm is compiled in, `Unsupported`
/// elsewhere, where the kind renders day's placeholder leaf. `Native` is a promise about the
/// code, not the hardware — the iOS Simulator reports [`CameraState::Unavailable`] at run time.
pub fn support() -> day_spec::Support {
    if cfg!(any(
        all(feature = "uikit", target_os = "ios"),
        all(feature = "mdc", target_os = "android"),
        all(feature = "arkui", target_env = "ohos"),
    )) {
        day_spec::Support::Native
    } else {
        day_spec::Support::Unsupported
    }
}

/// A native camera viewfinder. Bind the session to `.active(signal)`, take photos with
/// `.capture(trigger)`, and read them back through `.photo(signal)`.
pub struct Camera {
    facing: Reactive<Facing>,
    active: Reactive<bool>,
    auto_request: bool,
    capture: Option<Trigger>,
    state: Option<Signal<CameraState>>,
    photo: Option<Signal<Option<Photo>>>,
}

/// `camera()` — the rear camera's live preview, running whenever the permission is held.
pub fn camera() -> Camera {
    Camera {
        facing: Reactive::Const(Facing::Back),
        active: Reactive::Const(true),
        auto_request: false,
        capture: None,
        state: None,
        photo: None,
    }
}

impl Camera {
    /// Which camera to show — a constant, a `Signal<Facing>`, or a `Fn() -> Facing`. When it is
    /// reactive the viewfinder switches live.
    pub fn facing<M>(mut self, facing: impl IntoReactive<Facing, M>) -> Self {
        self.facing = facing.into_reactive();
        self
    }

    /// Whether the session should run — a constant, a `Signal<bool>`, or a closure. Default
    /// true. The session runs only while this is true AND the permission is held; `false`
    /// stops it and releases the camera.
    pub fn active<M>(mut self, active: impl IntoReactive<bool, M>) -> Self {
        self.active = active.into_reactive();
        self
    }

    /// Ask for the camera permission the first time the session is wanted and the OS has not
    /// decided yet. Opt-in: an app that wants its own rationale screen first requests through
    /// `day_part_permissions` itself and leaves this off.
    pub fn auto_request(mut self) -> Self {
        self.auto_request = true;
        self
    }

    /// Take a photo whenever `trigger` fires.
    pub fn capture(mut self, trigger: Trigger) -> Self {
        self.capture = Some(trigger);
        self
    }

    /// Where the camera reports what it is doing.
    pub fn state(mut self, state: Signal<CameraState>) -> Self {
        self.state = Some(state);
        self
    }

    /// Where each capture lands.
    pub fn photo(mut self, photo: Signal<Option<Photo>>) -> Self {
        self.photo = Some(photo);
        self
    }
}

impl Piece for Camera {
    fn build(self, cx: &mut BuildCx) -> RNode {
        let Camera {
            facing,
            active,
            auto_request,
            capture,
            state,
            photo,
        } = self;
        let set_state = move |next: CameraState| {
            if let Some(state) = state
                && state.get_untracked() != next
            {
                state.set(next);
            }
        };
        // Whether the first frame may run: only with the permission already held. Anything else
        // starts idle and is resolved reactively below, after the node exists to patch.
        let wanted = active.get_untracked();
        let held = perms::status(Permission::Camera) == Status::Granted;
        let props = CameraProps {
            facing: facing.get_untracked(),
            active: wanted && held,
        };
        // A viewfinder fills the space it is offered (constrain via `.frame(w, h)`).
        let node = cx.leaf(
            KIND,
            &props,
            Flex {
                grow_w: true,
                grow_h: true,
                ..Default::default()
            },
        );
        let send = move |patch: CameraPatch| {
            with_tree(|t| t.patch(node, Box::new(patch), false));
        };

        // The session follows `active`, gated on the permission. Denied and restricted are
        // reported as such; an undecided permission is asked for only under `auto_request`,
        // and otherwise leaves the camera idle for the app to resolve.
        let start = move || match perms::status(Permission::Camera) {
            Status::Granted => send(CameraPatch::Start),
            Status::Prompt if auto_request => {
                set_state(CameraState::Starting);
                // The completion runs on an unspecified thread; the patch has to be issued on
                // the main thread, where the tree lives. A `Setter` is the door back for the
                // state signal, which itself never leaves that thread.
                let denied = state.map(|s| s.setter());
                perms::request(Permission::Camera, move |status| {
                    on_main(move || {
                        if status == Status::Granted {
                            with_tree(|t| t.patch(node, Box::new(CameraPatch::Start), false));
                        } else if let Some(denied) = denied {
                            denied.set(CameraState::Denied);
                        }
                    });
                });
            }
            Status::Prompt => set_state(CameraState::Idle),
            Status::Denied | Status::Restricted => set_state(CameraState::Denied),
            // Ungated platforms answer Granted; the rest have no camera arm to start.
            _ => set_state(CameraState::Unavailable),
        };
        if wanted && !held {
            start();
        }
        if let Reactive::Dyn(read) = active {
            // Only a CHANGE moves the session: the watch re-runs on any dependency of the
            // closure, and re-sending Start to a running camera would reconfigure it.
            let last = std::rc::Rc::new(std::cell::Cell::new(wanted));
            watch(
                move || read(),
                move |now, _| {
                    if *now == last.get() {
                        return;
                    }
                    last.set(*now);
                    if *now {
                        start();
                    } else {
                        send(CameraPatch::Stop);
                    }
                },
            );
        }
        if let Reactive::Dyn(read) = facing {
            let last = std::rc::Rc::new(std::cell::Cell::new(props.facing));
            watch(
                move || read(),
                move |f, _| {
                    if *f != last.get() {
                        last.set(*f);
                        send(CameraPatch::Facing(*f));
                    }
                },
            );
        }
        if let Some(capture) = capture {
            watch(
                move || capture.track(),
                move |_, _| send(CameraPatch::Capture),
            );
        }
        cx.on(node, move |ev| {
            if let Event::Custom { num, text, .. } = ev {
                if *num as i32 == report::SURFACE {
                    send(CameraPatch::Surface(text.clone()));
                    return;
                }
                if *num as i32 == report::PHOTO {
                    if let Some(photo) = photo
                        && let Some(shot) = Photo::from_report(text)
                    {
                        photo.set(Some(shot));
                    }
                    return;
                }
                if let Some(next) = CameraState::from_report(*num, text) {
                    set_state(next);
                }
            }
        });
        node
    }
}

// ---------------------------------------------------------------------------
// Per-toolkit native renderers — the three mobile toolkits. Each registers a `Renderer`
// link-time into its backend's `RENDERERS` slice; `#[cfg]` gates each to its feature + target.
// ---------------------------------------------------------------------------

day_pieces::glue_modules!(uikit, mdc, arkui);

// --- Typed builders, forwarded through `Decorated` (docs/api-style.md) ---

/// [`Camera`]'s own builders, reachable THROUGH a decoration (§5.2): `day_pieces::Decorated`
/// forwards them to the piece it wraps, so generic modifiers and typed ones chain in any order.
pub trait CameraBuilder: Sized {
    fn facing<M>(self, facing: impl IntoReactive<Facing, M>) -> Self;
    fn active<M>(self, active: impl IntoReactive<bool, M>) -> Self;
    fn auto_request(self) -> Self;
    fn capture(self, trigger: Trigger) -> Self;
    fn state(self, state: Signal<CameraState>) -> Self;
    fn photo(self, photo: Signal<Option<Photo>>) -> Self;
}

impl CameraBuilder for Camera {
    fn facing<M>(self, facing: impl IntoReactive<Facing, M>) -> Self {
        Camera::facing(self, facing)
    }
    fn active<M>(self, active: impl IntoReactive<bool, M>) -> Self {
        Camera::active(self, active)
    }
    fn auto_request(self) -> Self {
        Camera::auto_request(self)
    }
    fn capture(self, trigger: Trigger) -> Self {
        Camera::capture(self, trigger)
    }
    fn state(self, state: Signal<CameraState>) -> Self {
        Camera::state(self, state)
    }
    fn photo(self, photo: Signal<Option<Photo>>) -> Self {
        Camera::photo(self, photo)
    }
}

impl<Inner: CameraBuilder + day_pieces::prelude::Piece> CameraBuilder
    for day_pieces::Decorated<Inner>
{
    fn facing<M>(self, facing: impl IntoReactive<Facing, M>) -> Self {
        self.map_inner(|inner| inner.facing(facing))
    }
    fn active<M>(self, active: impl IntoReactive<bool, M>) -> Self {
        self.map_inner(|inner| inner.active(active))
    }
    fn auto_request(self) -> Self {
        self.map_inner(|inner| inner.auto_request())
    }
    fn capture(self, trigger: Trigger) -> Self {
        self.map_inner(|inner| inner.capture(trigger))
    }
    fn state(self, state: Signal<CameraState>) -> Self {
        self.map_inner(|inner| inner.state(state))
    }
    fn photo(self, photo: Signal<Option<Photo>>) -> Self {
        self.map_inner(|inner| inner.photo(photo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_report_parses_path_and_size() {
        let p = Photo::from_report("/tmp/day-camera/a.jpg\u{1f}4032\u{1f}3024").unwrap();
        assert_eq!(p.path, PathBuf::from("/tmp/day-camera/a.jpg"));
        assert_eq!((p.width, p.height), (4032, 3024));
        assert_eq!(p.file_name(), "a.jpg");
    }

    #[test]
    fn photo_report_without_size_is_still_a_photo() {
        let p = Photo::from_report("/tmp/x.jpg").unwrap();
        assert_eq!((p.width, p.height), (0, 0));
        assert!(Photo::from_report("").is_none());
    }

    #[test]
    fn state_reports_map_and_photo_is_not_a_state() {
        assert_eq!(
            CameraState::from_report(report::RUNNING as f64, ""),
            Some(CameraState::Running)
        );
        assert_eq!(
            CameraState::from_report(report::ERROR as f64, "boom"),
            Some(CameraState::Error("boom".into()))
        );
        assert_eq!(CameraState::from_report(report::PHOTO as f64, "x"), None);
        assert_eq!(CameraState::Unavailable.label(), "unavailable");
    }

    #[test]
    fn facing_codes_round_trip() {
        assert_eq!(Facing::Back.code(), 0);
        assert_eq!(Facing::Front.code(), 1);
        assert_eq!(Facing::Back.flipped(), Facing::Front);
    }
}

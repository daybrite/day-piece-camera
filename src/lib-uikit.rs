// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// ---------------------------------------------------------------------------
// UIKit: an AVFoundation capture session behind a preview view, created by this crate's Swift
// shim (platform/ios/swift/DayCamera.swift → the generated DayPieces SwiftPM package). AVFoundation is
// delegate-heavy, which is what makes Swift the right side for it; Rust owns the returned
// +1-retained UIView, sends the commands, and receives the reports through the C callback it
// hands the shim at creation.
// ---------------------------------------------------------------------------

use super::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use day_spec::NodeId;
use day_uikit::Uikit;
use objc2::rc::Retained;
use objc2_ui_kit::UIView;

/// The report callback the shim calls, always on the main queue: `num` is a [`report`] code,
/// `text` its payload.
type ReportFn = unsafe extern "C" fn(node: u64, num: f64, text: *const c_char);

unsafe extern "C" {
    fn day_camera_new(node: u64, facing: c_int, active: bool, report: ReportFn) -> *mut c_void;
    fn day_camera_command(view: *mut c_void, cmd: c_int, arg: c_int);
    fn day_camera_release(view: *mut c_void);
}

/// Commands, as the shim numbers them.
const CMD_START: c_int = 0;
const CMD_STOP: c_int = 1;
const CMD_CAPTURE: c_int = 2;
const CMD_FACING: c_int = 3;

/// The shim reports on the main queue, where the tree lives, so this emits directly.
unsafe extern "C" fn report(node: u64, num: f64, text: *const c_char) {
    day_spec::ffi_guard::contain((), || {
        let text = if text.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
        };
        day_uikit::emit(
            NodeId(node),
            Event::Custom {
                tag: "",
                num,
                text,
            },
        );
    });
}

fn make(_backend: &mut Uikit, p: &CameraProps, id: NodeId) -> Retained<UIView> {
    // The shim returns a +1-retained preview view (a UIView subclass); we take ownership.
    let ptr = unsafe { day_camera_new(id.0, p.facing.code(), p.active, report) };
    unsafe { Retained::from_raw(ptr.cast::<UIView>()) }.expect("DayCameraView")
}

fn update(_backend: &mut Uikit, h: &Retained<UIView>, patch: &CameraPatch) {
    let ptr = (&**h as *const UIView) as *mut c_void;
    let (cmd, arg) = match patch {
        CameraPatch::Start => (CMD_START, 0),
        CameraPatch::Stop => (CMD_STOP, 0),
        CameraPatch::Capture => (CMD_CAPTURE, 0),
        CameraPatch::Facing(f) => (CMD_FACING, f.code()),
        // The ArkTS surface handshake is HarmonyOS's alone.
        CameraPatch::Surface(_) => return,
    };
    unsafe { day_camera_command(ptr, cmd, arg) };
}

/// The renderer's release: stop the session so the camera is freed the moment the page goes
/// away, not when the view happens to deallocate.
fn release(_backend: &mut Uikit, h: &Retained<UIView>) {
    let ptr = (&**h as *const UIView) as *mut c_void;
    unsafe { day_camera_release(ptr) };
}

day_pieces::renderer!(day_uikit::RENDERERS, Uikit,
    kind: KIND, props: CameraProps, patch: CameraPatch, make: make, update: update,
    measure: day_pieces::fill_measure, release: release);

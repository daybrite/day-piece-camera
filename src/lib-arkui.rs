// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// ---------------------------------------------------------------------------
// HarmonyOS: the NDK camera kit (libohcamera) behind an ArkTS XComponent surface. The ArkUI C
// node API has no surface node kind, so this crate ships its OWN ArkTS (ohos/ets/Index.ets) that
// builds the XComponent in a BuilderNode and reports its surface id back through `pieceEvent`;
// the front-end forwards that report as `CameraPatch::Surface`, and from there this crate's C
// shim (ohos/native/day_camera.c, compiled by build.rs) owns the camera: it opens the device,
// renders the preview into that surface, and packs each capture to a JPEG in the cache
// directory. Reports come back through the C callback handed over at creation.
// ---------------------------------------------------------------------------

use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

use day_arkui::{AHandle, ArkUi, piece};
use day_spec::NodeId;

/// The report callback the shim calls from whatever thread the camera service uses.
type ReportFn = unsafe extern "C" fn(node: u64, num: f64, text: *const c_char);

unsafe extern "C" {
    fn day_camera_ohos_new(
        node: u64,
        facing: c_int,
        active: c_int,
        cache_dir: *const c_char,
        report: ReportFn,
    ) -> *mut c_void;
    fn day_camera_ohos_surface(cam: *mut c_void, surface_id: *const c_char);
    fn day_camera_ohos_command(cam: *mut c_void, cmd: c_int, arg: c_int);
    fn day_camera_ohos_release(cam: *mut c_void);
}

const CMD_START: c_int = 0;
const CMD_STOP: c_int = 1;
const CMD_CAPTURE: c_int = 2;
const CMD_FACING: c_int = 3;

day_core::tls_group! {
    // The C camera behind each ArkTS node, keyed by the handle's pointer.
    static CAMS: RefCell<HashMap<usize, usize>> = RefCell::new(HashMap::new());
}

/// The event kind day-arkui's trampoline maps to `Event::Custom`.
const K_CUSTOM: c_int = 12;

/// Reports arrive from the camera service's threads; the tree lives on the JS thread, so each
/// one is posted there before it becomes an event.
unsafe extern "C" fn report(node: u64, num: f64, text: *const c_char) {
    day_spec::ffi_guard::contain((), || {
        let text = if text.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
        };
        day_reactive::on_main(move || {
            let Ok(text) = CString::new(text) else {
                return;
            };
            day_arkui::day_arkui_on_event(node, K_CUSTOM, num, text.as_ptr());
        });
    });
}

fn make(_backend: &mut ArkUi, p: &CameraProps, id: NodeId) -> AHandle {
    // The ArkTS side needs nothing but the node id; it reports the surface as soon as it exists.
    let h = piece::make(KIND, id, "");
    let cache = day_spec::present::app_temp_dir();
    let cache = CString::new(cache.to_string_lossy().as_ref()).unwrap_or_default();
    let cam = unsafe {
        day_camera_ohos_new(
            id.0,
            p.facing.code(),
            p.active as c_int,
            cache.as_ptr(),
            report,
        )
    };
    if !cam.is_null() {
        CAMS.with(|m| m.borrow_mut().insert(h.0 as usize, cam as usize));
    }
    h
}

fn cam_of(h: &AHandle) -> Option<*mut c_void> {
    CAMS.with(|m| m.borrow().get(&(h.0 as usize)).map(|&p| p as *mut c_void))
}

fn update(_backend: &mut ArkUi, h: &AHandle, patch: &CameraPatch) {
    let Some(cam) = cam_of(h) else {
        return;
    };
    match patch {
        CameraPatch::Surface(id) => {
            if let Ok(id) = CString::new(id.as_str()) {
                unsafe { day_camera_ohos_surface(cam, id.as_ptr()) };
            }
        }
        CameraPatch::Start => unsafe { day_camera_ohos_command(cam, CMD_START, 0) },
        CameraPatch::Stop => unsafe { day_camera_ohos_command(cam, CMD_STOP, 0) },
        CameraPatch::Capture => unsafe { day_camera_ohos_command(cam, CMD_CAPTURE, 0) },
        CameraPatch::Facing(f) => unsafe { day_camera_ohos_command(cam, CMD_FACING, f.code()) },
    }
}

/// The renderer's release: stop the session and free the camera; the ArkTS node is disposed by
/// the bridge.
fn release(_backend: &mut ArkUi, h: &AHandle) {
    if let Some(cam) = CAMS.with(|m| m.borrow_mut().remove(&(h.0 as usize))) {
        unsafe { day_camera_ohos_release(cam as *mut c_void) };
    }
}

day_pieces::renderer!(day_arkui::RENDERERS, ArkUi,
    kind: KIND, props: CameraProps, patch: CameraPatch, make: make, update: update,
    measure: day_pieces::fill_measure, release: release);

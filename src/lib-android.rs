// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// ---------------------------------------------------------------------------
// Android: a CameraX PreviewView with an ImageCapture use case, created by this crate's OWN Java
// (android/java/…/DayCamera.java) — folded into the app's Gradle build via
// [package.metadata.day.android] (which also declares the CameraX dependencies), without
// touching day-android. Reports come back through DayBridge.nativeOnEvent's Custom kind, carrying
// the node id the view was made with.
// ---------------------------------------------------------------------------

use super::*;
use day_android::DayEnv;
use day_android::jni::objects::JValue;
use day_android::{AHandle, Android, with_env};
use day_spec::NodeId;

/// This piece's OWN Java class (in the crate's android/java, on the app classpath at build).
const CAMERA_CLASS: &str = "dev/daybrite/day/piece/camera/DayCamera";

/// Commands, as the Java side numbers them.
const CMD_START: i32 = 0;
const CMD_STOP: i32 = 1;
const CMD_CAPTURE: i32 = 2;
const CMD_FACING: i32 = 3;

fn make(_backend: &mut Android, p: &CameraProps, id: NodeId) -> AHandle {
    with_env(|env| {
        // A Java throw (the CameraX dependency missing from the app build, say) must not panic
        // inside realize — the panic unwinds the JNI up-call and aborts. Placeholder instead.
        let made = day_android::try_make_view_on(
            env,
            CAMERA_CLASS,
            "makeCamera",
            "(JIZ)Landroid/view/View;",
            &[
                JValue::Long(id.0 as i64),
                JValue::Int(p.facing.code()),
                JValue::Bool(p.active),
            ],
        )
        .ok();
        AHandle(made.unwrap_or_else(|| {
            log::warn!("day-piece-camera: DayCamera.makeCamera failed; substituting a placeholder");
            day_android::placeholder_view(env, "camera")
        }))
    })
}

fn update(_backend: &mut Android, h: &AHandle, patch: &CameraPatch) {
    let (cmd, arg) = match patch {
        CameraPatch::Start => (CMD_START, 0),
        CameraPatch::Stop => (CMD_STOP, 0),
        CameraPatch::Capture => (CMD_CAPTURE, 0),
        CameraPatch::Facing(f) => (CMD_FACING, f.code()),
        // The ArkTS surface handshake is HarmonyOS's alone.
        CameraPatch::Surface(_) => return,
    };
    with_env(|env| {
        let _ = env.dcall_static(
            CAMERA_CLASS,
            "cameraCommand",
            "(Landroid/view/View;II)V",
            &[
                JValue::Object(h.0.as_obj()),
                JValue::Int(cmd),
                JValue::Int(arg),
            ],
        );
    });
}

/// The renderer's release: unbind the use cases so the camera is freed the moment the page goes
/// away, not when the activity does.
fn release(_backend: &mut Android, h: &AHandle) {
    with_env(|env| {
        let _ = env.dcall_static(
            CAMERA_CLASS,
            "releaseCamera",
            "(Landroid/view/View;)V",
            &[JValue::Object(h.0.as_obj())],
        );
    });
}

day_pieces::renderer!(day_android::RENDERERS, Android,
    kind: KIND, props: CameraProps, patch: CameraPatch, make: make, update: update,
    measure: day_pieces::fill_measure, release: release);

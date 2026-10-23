// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// ---------------------------------------------------------------------------
// Android: a CameraX PreviewView with an ImageCapture use case, created by this crate's Java
// (src/DayCamera.java) and folded into the app's Gradle build via
// [package.metadata.day.android] (which also declares the CameraX dependencies), without
// touching day-mdc. Reports come back through DayBridge.nativeOnEvent's Custom kind, carrying
// the node id the view was made with.
// ---------------------------------------------------------------------------

use super::*;
use day_mdc::{AHandle, Android, java};
use day_spec::NodeId;

// This piece's Java class (src/DayCamera.java, on the app classpath at build).
day_mdc::java_class! {
    DayCamera => dev.daybrite.day.piece.camera.DayCamera,
    methods {
        static fn makeCamera(id: jlong, facing: jint, active: jboolean) -> android.view.View,
        static fn cameraCommand(view: android.view.View, cmd: jint, arg: jint),
        static fn releaseCamera(view: android.view.View),
    },
}

/// Commands, as the Java side numbers them.
const CMD_START: i32 = 0;
const CMD_STOP: i32 = 1;
const CMD_CAPTURE: i32 = 2;
const CMD_FACING: i32 = 3;

fn make(_backend: &mut Android, p: &CameraProps, id: NodeId) -> AHandle {
    // A Java throw (the CameraX dependency missing from the app build, say) must not panic
    // inside realize: the panic unwinds the JNI up-call and aborts. Placeholder instead.
    let (facing, active) = (p.facing.code(), p.active);
    day_mdc::realize("camera", |env| {
        DayCamera::makeCamera(env, id.0 as i64, facing, active)
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
    java(|env| DayCamera::cameraCommand(env, h.0.as_obj(), cmd, arg));
}

/// The renderer's release: unbind the use cases so the camera is freed the moment the page goes
/// away, not when the activity does.
fn release(_backend: &mut Android, h: &AHandle) {
    java(|env| DayCamera::releaseCamera(env, h.0.as_obj()));
}

day_pieces::renderer!(day_mdc::RENDERERS, Android,
    kind: KIND, props: CameraProps, patch: CameraPatch, make: make, update: update,
    measure: day_pieces::fill_measure, release: release);

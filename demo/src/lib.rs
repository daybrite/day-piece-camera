// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

//! Camera Demo: the demo and on-device test app for `day-piece-camera`.
//!
//! One page: the camera permission's status and the button that asks for it, the viewfinder,
//! a shutter, a camera switch, and the last photo. Every element carries a stable id, so
//! `dayscript/camera.yaml` can assert the page on the iOS Simulator and the Android emulator,
//! and `dayscript/camera-capture.yaml`, once the permission is granted out of band, can take a
//! photo and see it land.

use std::sync::Arc;

use day::prelude::*;
use day_part_permissions::{self as perms, Permission, Status};
use day_piece_camera::{CameraState, Facing, Photo, camera};
use day_piece_remote_image::remote_image;

// The mobile entry point; a plain cargo desktop build enters through src/main.rs.
day::day_start!(options: window(), root);

// Typed constants for everything under `resource/` (https://daybrite.dev/docs/resources).
day::resources!();

/// The window every entry point opens.
pub fn window() -> day::WindowOptions {
    day::WindowOptions {
        locales: Some((res::locales::DEFAULT, res::locales::CATALOG)),
        title_fn: Some(|| res::str::app_title().format()),
        size: day::prelude::Size::new(480.0, 800.0),
        ..Default::default()
    }
}

/// The whole app: permission, viewfinder, shutter, the last photo.
pub fn root() -> impl Piece {
    info!("Camera Demo starting");

    let state = Signal::new(CameraState::Idle);
    let photo: Signal<Option<Photo>> = Signal::new(None);
    let facing = Signal::new(Facing::Back);
    let shutter = Trigger::new();
    // The session runs while this is true: from the first frame when the permission was
    // already held, else from the moment the Allow button's request comes back granted.
    let active = Signal::new(perms::status(Permission::Camera) == Status::Granted);

    // Each capture's bytes, read once for the image below.
    let bytes: Signal<Option<Arc<Vec<u8>>>> = Signal::new(None);
    Effect::new(move || {
        let next = photo.get().and_then(|p| p.bytes().ok());
        bytes.set(next);
    });

    scroll(
        column((
            label(res::str::app_title())
                .font(Font::Title)
                .id("camera-title"),
            permission_row(active),
            column((camera()
                .facing(facing)
                .active(active)
                .capture(shutter)
                .state(state)
                .photo(photo)
                .frame(320.0, 240.0)
                .id("camera-view"),))
            .align(HAlign::Center)
            .grow_w(),
            row((
                button(res::str::take_photo())
                    .prominent()
                    .enabled(move || state.get() == CameraState::Running)
                    .action(move || shutter.notify())
                    .id("camera-shoot"),
                button(res::str::flip())
                    .bordered()
                    .enabled(move || state.get() == CameraState::Running)
                    .action(move || facing.update(|f| *f = f.flipped()))
                    .id("camera-flip"),
            ))
            .spacing(8.0),
            labeled(
                res::str::state(),
                // The locale-independent label, so the script can assert it.
                label(move || state.get().label().to_string()).id("camera-state"),
            ),
            when(
                move || bytes.get().is_some(),
                move || {
                    column((remote_image(bytes).frame(240.0, 180.0).id("camera-photo"),))
                        .align(HAlign::Center)
                        .grow_w()
                },
            ),
            labeled(
                res::str::photo(),
                label(move || match photo.get() {
                    // The generated accessor takes the message's variables in sorted order:
                    // height, name, width.
                    Some(p) => res::str::photo_size(p.height as i64, p.file_name(), p.width as i64)
                        .format(),
                    None => res::str::no_photo().format(),
                })
                .id("camera-photo-size"),
            ),
        ))
        .spacing(12.0)
        .padding(16.0),
    )
}

/// The camera permission's live status, and the button that asks for it. A grant flips
/// `active`, which is what starts the viewfinder.
fn permission_row(active: Signal<bool>) -> impl Piece {
    let perm = Permission::Camera;
    let status = Signal::new(perms::status(perm).label().to_string());
    let can_prompt = perms::can_prompt(perm);
    let gated = perms::gate(perm) != perms::Gate::Absent;
    let action_label = if can_prompt {
        res::str::allow()
    } else {
        res::str::open_settings()
    };
    labeled(
        res::str::permission(),
        row((
            label(move || status.get()).id("camera-perm"),
            when(
                move || gated && !active.get(),
                move || {
                    button(action_label.clone())
                        .bordered()
                        .action(move || {
                            if can_prompt {
                                // The completion runs on an unspecified thread; Setters cross
                                // back to the signals.
                                let set_status = status.setter();
                                let set_active = active.setter();
                                perms::request(perm, move |s| {
                                    set_status.set(s.label().to_string());
                                    if s == Status::Granted {
                                        set_active.set(true);
                                    }
                                });
                            } else {
                                perms::open_settings(perm);
                                status.set(perms::status(perm).label().to_string());
                            }
                        })
                        .id("camera-perm-action")
                },
            ),
        ))
        .spacing(8.0),
    )
}

<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# Changelog

## 0.1.0

New:

- `camera()`: a native viewfinder with `.facing`, `.active`, `.capture`, `.state`, `.photo`
  and the opt-in `.auto_request()`.
- iOS through AVFoundation (a Swift shim), Android through CameraX 1.5, HarmonyOS through the
  NDK camera kit behind an ArkTS `XComponent` surface.
- `[package.metadata.day.permissions] uses = ["camera"]`, so an app that depends on the piece
  gets the manifest entry, the usage description and the `requestPermissions` entry from
  `day build`.
- The demo app and its two scripts: the page before the permission, and the capture after it.

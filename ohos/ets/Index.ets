// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

// day-piece-camera — the HarmonyOS half of the piece (docs/camera.md).
//
// The ArkUI C node API has no surface node kind, so the preview surface is declared here in
// ArkTS: an XComponent built into a BuilderNode and handed to Day as a FrameNode the native tree
// mounts like any other node. Nothing else happens on this side. The surface id goes back
// through `pieceEvent` with the piece's SURFACE code; the crate's C shim then renders the NDK
// camera's preview into it and owns the session from there.
//
// NO percentage sizing: a BuilderNode is built detached, so '100%' resolves against the whole
// window. Day owns layout and sets this node's position + size through the ArkUI C API like
// every other leaf, which is what the piece's `fill_measure` asks for.

import { BuilderNode, FrameNode, UIContext } from '@kit.ArkUI';
import nativeEntry from 'libentry.so';
import { DayPieceModule } from '../DayPiece';

// src/lib.rs `report::SURFACE`.
const SURFACE: number = 7;

class DayCameraParams {
  id: number = 0;
  controller: XComponentController = new XComponentController();
}

@Builder
function dayCameraBuilder(params: DayCameraParams) {
  XComponent({
    id: `day-camera-${params.id}`,
    type: XComponentType.SURFACE,
    controller: params.controller
  })
    .onLoad(() => {
      nativeEntry.pieceEvent(params.id, params.controller.getXComponentSurfaceId(), SURFACE);
    })
    .onDestroy(() => {
      nativeEntry.pieceEvent(params.id, '', SURFACE);
    })
}

class DayCameraInstance {
  node: BuilderNode<[DayCameraParams]>;
  params: DayCameraParams;

  constructor(node: BuilderNode<[DayCameraParams]>, params: DayCameraParams) {
    this.node = node;
    this.params = params;
  }
}

const instances: Map<number, DayCameraInstance> = new Map();

function make(ui: UIContext, id: number, props: string): FrameNode | undefined {
  const params: DayCameraParams = new DayCameraParams();
  params.id = id;
  const node: BuilderNode<[DayCameraParams]> = new BuilderNode(ui);
  node.build(wrapBuilder<[DayCameraParams]>(dayCameraBuilder), params);
  const frame: FrameNode | null = node.getFrameNode();
  if (frame === null) {
    return undefined;
  }
  instances.set(id, new DayCameraInstance(node, params));
  return frame;
}

// The camera is driven from the crate's C shim, not from here: no command reaches this side.
function update(id: number, cmd: string, arg: string): void {
}

function dispose(id: number): void {
  const live: DayCameraInstance | undefined = instances.get(id);
  if (live !== undefined) {
    instances.delete(id);
    live.node.dispose();
  }
}

export const dayPiece: DayPieceModule = {
  kind: 'day.piece.camera',
  make: make,
  update: update,
  dispose: dispose
};

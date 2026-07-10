/// Mini-Map geometry (#66) — the pure world ↔ mini-map coordinate maths.
///
/// The Diagrams stage renders an SVG that occupies the world box
/// `[0, baseW] × [0, baseH]` (stage-local pixels at scale 1) and applies a
/// live `translate(tx, ty) scale(scale)` transform (see `viewport.ts`). The
/// Mini-Map draws a thumbnail of that whole world box, scaled to fit a small
/// card, and overlays a rectangle marking the region currently visible in the
/// stage.
///
/// All of the non-trivial logic lives here as pure functions so it can be
/// unit-tested without a DOM, and so `MiniMap.svelte` stays a thin renderer +
/// event forwarder. Screen ↔ world uses the same relation the viewport store
/// implements: `screen = world * scale + t`, i.e. `world = (screen - t) / scale`.

import type { Viewport } from './viewport';
import { MAX_SCALE, MIN_SCALE } from './viewport';

/// An axis-aligned rectangle. Reused for both the world-space viewport rect
/// and its projection into mini-map pixels.
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/// The affine fit of the world box into the mini-map card. `scale` is the
/// uniform world→mini factor; `offsetX/offsetY` centre the thumbnail inside
/// the card when the aspect ratios differ (letter-boxing). `contentW/H` are
/// the rendered thumbnail size (world box × scale) — handy for positioning
/// the `<svg>` inside the card.
export interface MiniMapFit {
  scale: number;
  offsetX: number;
  offsetY: number;
  contentW: number;
  contentH: number;
}

/// True when the viewport carries a usable world box. A zero base size means
/// the diagram hasn't been measured yet (or is a kind that doesn't pan/zoom,
/// e.g. the draw.io frame) — the caller hides the mini-map in that case.
export function hasWorldBox(v: Viewport): boolean {
  return v.baseW > 0 && v.baseH > 0;
}

/// Fit the world box (`baseW × baseH`) into a `mmW × mmH` card, preserving
/// aspect ratio and centring the result. Guards against a zero/negative base
/// size by returning a degenerate (scale 0) fit so callers can bail cleanly.
export function fitWorldToMiniMap(
  v: Viewport,
  mmW: number,
  mmH: number,
): MiniMapFit {
  if (!hasWorldBox(v) || mmW <= 0 || mmH <= 0) {
    return { scale: 0, offsetX: 0, offsetY: 0, contentW: 0, contentH: 0 };
  }
  const scale = Math.min(mmW / v.baseW, mmH / v.baseH);
  const contentW = v.baseW * scale;
  const contentH = v.baseH * scale;
  return {
    scale,
    offsetX: (mmW - contentW) / 2,
    offsetY: (mmH - contentH) / 2,
    contentW,
    contentH,
  };
}

/// The world-space rectangle currently visible in the stage. The stage's
/// top-left corner `(0, 0)` maps to world `(-tx/scale, -ty/scale)`; its size
/// in world units is `stage / scale`. This is what the viewport rectangle
/// covers before it's projected into the mini-map.
export function visibleWorldRect(
  v: Viewport,
  stageW: number,
  stageH: number,
): Rect {
  const s = v.scale || 1;
  return {
    x: -v.tx / s,
    y: -v.ty / s,
    w: stageW / s,
    h: stageH / s,
  };
}

/// Project a world-space rectangle into mini-map pixel space using a fit.
export function worldRectToMini(rect: Rect, fit: MiniMapFit): Rect {
  return {
    x: fit.offsetX + rect.x * fit.scale,
    y: fit.offsetY + rect.y * fit.scale,
    w: rect.w * fit.scale,
    h: rect.h * fit.scale,
  };
}

/// Convert a mini-map pixel point (relative to the card's top-left) back into
/// a world-space point. Inverse of the fit projection. Returns `(0, 0)` for a
/// degenerate fit rather than dividing by zero.
export function miniPointToWorld(
  mx: number,
  my: number,
  fit: MiniMapFit,
): { x: number; y: number } {
  if (fit.scale === 0) return { x: 0, y: 0 };
  return {
    x: (mx - fit.offsetX) / fit.scale,
    y: (my - fit.offsetY) / fit.scale,
  };
}

/// The viewport rectangle drawn on the mini-map, with a minimum grabbable
/// size clamp. At extreme zoom the true rectangle collapses to a sub-pixel
/// dot; we widen it to at least `minPx` (centred on its true middle) so the
/// user can still see and drag it. The clamp is display-only — panning still
/// uses the true centre, so widening never drifts the view.
export function clampedViewportRect(
  worldRect: Rect,
  fit: MiniMapFit,
  minPx: number,
): Rect {
  const r = worldRectToMini(worldRect, fit);
  let { x, y, w, h } = r;
  if (w < minPx) {
    x -= (minPx - w) / 2;
    w = minPx;
  }
  if (h < minPx) {
    y -= (minPx - h) / 2;
    h = minPx;
  }
  return { x, y, w, h };
}

/// Given a desired world-space centre point, compute the pan offset that puts
/// that point in the middle of the stage at the *current* scale. Used by
/// both click-to-centre and rectangle-drag: `screen = world * scale + t`, so
/// to land `world` at the stage centre we solve `t = stage/2 - world*scale`.
export function panToCenterWorld(
  v: Viewport,
  worldX: number,
  worldY: number,
  stageW: number,
  stageH: number,
): { tx: number; ty: number } {
  const s = v.scale || 1;
  return {
    tx: stageW / 2 - worldX * s,
    ty: stageH / 2 - worldY * s,
  };
}

/// Translate a click at mini-map pixel `(mx, my)` into the pan offset that
/// centres the stage on the corresponding world point. Composes
/// `miniPointToWorld` + `panToCenterWorld`.
export function clickToPan(
  v: Viewport,
  mx: number,
  my: number,
  fit: MiniMapFit,
  stageW: number,
  stageH: number,
): { tx: number; ty: number } {
  const world = miniPointToWorld(mx, my, fit);
  return panToCenterWorld(v, world.x, world.y, stageW, stageH);
}

/// Clamp a zoom factor so the resulting scale stays within the shared
/// [MIN_SCALE, MAX_SCALE] bounds. Mirrors the store's internal clamp so the
/// mini-map's wheel-zoom never asks for a scale the stage would reject —
/// keeping the "how much did we actually zoom" feedback honest.
export function clampZoomFactor(currentScale: number, factor: number): number {
  const target = currentScale * factor;
  const clamped = Math.min(MAX_SCALE, Math.max(MIN_SCALE, target));
  return clamped / currentScale;
}

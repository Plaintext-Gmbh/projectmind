/// Viewport (pan / zoom) store for the Diagrams stage — hoisted out of
/// `DiagramView.svelte` (Viz-Katalog V1.4, #66 Mini-Map prerequisite).
///
/// The diagram stage renders an SVG scaled to `baseW × baseH` at scale 1 and
/// applies live zoom via a CSS `translate(tx, ty) scale(scale)` transform.
/// Previously all of that lived as component-local `let`s, which meant the
/// Mini-Map (#66) had nowhere to read the current viewport rectangle from.
/// This module lifts that state into a small Svelte store whose transitions
/// are expressed as **pure reducers** — so the pan/zoom/reset maths can be
/// unit-tested without a DOM, and the Mini-Map can drive it by dispatching
/// the same reducers the main stage uses.

import { writable } from 'svelte/store';

/// The whole viewport state. `scale`/`tx`/`ty` are the live transform;
/// `baseW`/`baseH` are the fit-to-stage SVG size at scale 1, stamped once
/// per render. The Mini-Map derives its viewport rectangle from all five.
export interface Viewport {
  scale: number;
  tx: number;
  ty: number;
  baseW: number;
  baseH: number;
}

/// Zoom is clamped to this range everywhere (wheel, toolbar buttons,
/// mini-map). Kept as exported constants so the mini-map uses the same
/// bounds as the stage.
export const MIN_SCALE = 0.2;
export const MAX_SCALE = 8;

export function initialViewport(): Viewport {
  return { scale: 1, tx: 0, ty: 0, baseW: 0, baseH: 0 };
}

function clampScale(s: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, s));
}

/// Reset pan + zoom to the identity transform, preserving the current base
/// size (the SVG doesn't change size on a view reset).
export function reset(v: Viewport): Viewport {
  return { ...v, scale: 1, tx: 0, ty: 0 };
}

/// Pan by a screen-space delta (used while dragging).
export function panBy(v: Viewport, dx: number, dy: number): Viewport {
  return { ...v, tx: v.tx + dx, ty: v.ty + dy };
}

/// Set an absolute pan offset (used by drag, which tracks from a captured
/// start offset, and by the mini-map when it re-centres the view).
export function panTo(v: Viewport, tx: number, ty: number): Viewport {
  return { ...v, tx, ty };
}

/// Zoom by `factor` while keeping the world-point under the anchor point
/// `(ax, ay)` (in stage-local pixels) fixed. This is the shared kernel for
/// wheel-zoom (anchor = cursor), toolbar zoom (anchor = stage centre) and
/// mini-map wheel-zoom. Scale is clamped to [MIN_SCALE, MAX_SCALE]; when the
/// clamp bites, tx/ty are recomputed against the *clamped* scale so the
/// anchor stays put exactly.
export function zoomAround(v: Viewport, factor: number, ax: number, ay: number): Viewport {
  const nextScale = clampScale(v.scale * factor);
  const ratio = nextScale / v.scale;
  return {
    ...v,
    tx: ax - (ax - v.tx) * ratio,
    ty: ay - (ay - v.ty) * ratio,
    scale: nextScale,
  };
}

/// Set the fit-to-stage base size (stamped once per render). Leaves the live
/// transform untouched.
export function setBaseSize(v: Viewport, baseW: number, baseH: number): Viewport {
  return { ...v, baseW, baseH };
}

/// Store factory. Each `DiagramView` instance owns its own viewport store so
/// two stages (e.g. the compare view's side-by-side maps) don't fight over a
/// single global. The returned object exposes the Svelte-store contract plus
/// typed helpers that apply the pure reducers above.
export function createViewportStore() {
  const store = writable<Viewport>(initialViewport());
  const { subscribe, set, update } = store;
  return {
    subscribe,
    set,
    update,
    reset: () => update(reset),
    panBy: (dx: number, dy: number) => update((v) => panBy(v, dx, dy)),
    panTo: (tx: number, ty: number) => update((v) => panTo(v, tx, ty)),
    zoomAround: (factor: number, ax: number, ay: number) =>
      update((v) => zoomAround(v, factor, ax, ay)),
    setBaseSize: (baseW: number, baseH: number) =>
      update((v) => setBaseSize(v, baseW, baseH)),
  };
}

export type ViewportStore = ReturnType<typeof createViewportStore>;

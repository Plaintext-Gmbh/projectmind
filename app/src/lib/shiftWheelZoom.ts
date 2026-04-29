/// Shared shift-wheel zoom helper. Six different views (ClassViewer,
/// FileView, HtmlIndex, MarkdownIndex, WalkthroughView, DiffView) used to
/// each carry their own copy of the same readZoom / clampZoom / setZoom /
/// onWheel block, differing only in the localStorage key. This module
/// consolidates that logic into a single store + Svelte action pair.
///
/// Usage (the typical case — one wrapper element holds the zoomed content):
///
///   <script>
///     import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
///     const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.foo.zoom');
///   </script>
///
///   <section use:zoomAction style="font-size: {$zoom}em;">…</section>
///
/// Usage (the WalkthroughView shape — events live inside *two* elements
/// that are not a single ancestor): use `readZoom`, `writeZoom`,
/// `clampZoom`, `wheelDelta` directly to keep your bespoke handler.

import { writable, get, type Writable } from 'svelte/store';

export interface ShiftWheelZoomOpts {
  /// Lower bound. Default 0.6.
  min?: number;
  /// Upper bound. Default 2.0.
  max?: number;
  /// Per-tick increment. Default 0.1.
  step?: number;
}

export interface ShiftWheelZoom {
  /// Reactive zoom factor — 1.0 means 100%. Read via `$zoom` or
  /// `zoom.subscribe(...)`. Imperative writes via `zoom.set(...)` /
  /// `zoom.update(...)` are clamped and persisted automatically.
  zoom: Writable<number>;
  /// Svelte action — only zooms when wheel events originate inside the
  /// bound node. Attach with `use:action`.
  action: (node: HTMLElement) => { destroy(): void };
}

const DEFAULT_MIN = 0.6;
const DEFAULT_MAX = 2.0;
const DEFAULT_STEP = 0.1;

/// Read a persisted zoom from localStorage, clamped to [min, max].
/// Returns 1.0 when the key is missing, unparseable, or storage is
/// unavailable (Safari private mode etc.).
export function readZoom(key: string, min = DEFAULT_MIN, max = DEFAULT_MAX): number {
  try {
    const v = parseFloat(localStorage.getItem(key) ?? '');
    if (Number.isFinite(v) && v > 0) return clampZoom(v, min, max);
  } catch {
    // ignore
  }
  return 1.0;
}

/// Persist a zoom value. Silently swallows storage errors.
export function writeZoom(key: string, value: number) {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // ignore
  }
}

/// Clamp + round to 2 decimals.
export function clampZoom(z: number, min = DEFAULT_MIN, max = DEFAULT_MAX): number {
  return Math.min(max, Math.max(min, Math.round(z * 100) / 100));
}

/// macOS axis-swap formula: when Shift is held the OS may translate vertical
/// wheel motion into deltaX. Pick whichever axis carries the larger motion.
/// Returned `delta < 0` means "zoom in", `delta > 0` means "zoom out",
/// `delta === 0` means "ignore".
export function wheelDelta(ev: WheelEvent): number {
  return Math.abs(ev.deltaY) >= Math.abs(ev.deltaX) ? ev.deltaY : ev.deltaX;
}

export function createShiftWheelZoom(key: string, opts?: ShiftWheelZoomOpts): ShiftWheelZoom {
  const min = opts?.min ?? DEFAULT_MIN;
  const max = opts?.max ?? DEFAULT_MAX;
  const step = opts?.step ?? DEFAULT_STEP;

  // Underlying writable. We expose a wrapped facade so `set`/`update` from
  // callers (e.g. FileView's keyboard zoomIn / zoomOut / zoomReset) get
  // clamp + persist for free.
  const inner = writable(readZoom(key, min, max));

  const zoom: Writable<number> = {
    subscribe: inner.subscribe,
    set(value: number) {
      const clamped = clampZoom(value, min, max);
      inner.set(clamped);
      writeZoom(key, clamped);
    },
    update(updater) {
      inner.update((current) => {
        const next = clampZoom(updater(current), min, max);
        writeZoom(key, next);
        return next;
      });
    },
  };

  function action(node: HTMLElement) {
    function onWheel(ev: WheelEvent) {
      if (!ev.shiftKey) return;
      if (!node.isConnected) return;
      if (!(ev.target instanceof Node) || !node.contains(ev.target)) return;
      const delta = wheelDelta(ev);
      if (delta === 0) return;
      ev.preventDefault();
      const current = get(inner);
      if (delta < 0) zoom.set(current + step);
      else zoom.set(current - step);
    }

    window.addEventListener('wheel', onWheel, { passive: false });

    return {
      destroy() {
        window.removeEventListener('wheel', onWheel);
      },
    };
  }

  return { zoom, action };
}

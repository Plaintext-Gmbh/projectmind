/// Svelte action: turn a `<div>` into a vertical drag handle that resizes a
/// sibling column on the left.
///
/// Usage:
///
///   <div class="grid" style="--col1: {col1}px;">
///     <aside style="width: var(--col1);">…</aside>
///     <div use:resizable={{
///       storageKey: 'projectmind.layout.code.col1',
///       cssVar: '--col1',
///       min: 140,
///       max: 480,
///       initial: 220,
///     }} class="resizer-handle"></div>
///     <main>…</main>
///   </div>
///
/// The action reads the persisted width from localStorage on mount, applies it
/// to the host's parent element via the named CSS variable, and updates both
/// during drag (mouse-move) and on release (localStorage).

export interface ResizableOptions {
  /// localStorage key. Width persisted across reloads.
  storageKey: string;
  /// Name of the CSS variable to set on the parent (e.g. '--col1').
  cssVar: string;
  /// Lower bound in pixels. Default 100.
  min?: number;
  /// Upper bound in pixels. Default 800.
  max?: number;
  /// Width to apply when nothing is stored. Default 220.
  initial?: number;
}

export function resizable(node: HTMLElement, opts: ResizableOptions) {
  let { storageKey, cssVar } = opts;
  let min = opts.min ?? 100;
  let max = opts.max ?? 800;
  let initial = opts.initial ?? 220;

  const parent = node.parentElement;
  if (!parent) return {};

  function clamp(w: number): number {
    return Math.max(min, Math.min(max, Math.round(w)));
  }

  function read(): number {
    try {
      const raw = localStorage.getItem(storageKey);
      const v = raw ? parseFloat(raw) : NaN;
      if (Number.isFinite(v) && v > 0) return clamp(v);
    } catch {
      // ignore
    }
    return clamp(initial);
  }

  function write(w: number) {
    try {
      localStorage.setItem(storageKey, String(w));
    } catch {
      // ignore
    }
  }

  function apply(w: number) {
    parent!.style.setProperty(cssVar, `${w}px`);
  }

  apply(read());

  let dragging = false;
  let startX = 0;
  let startWidth = 0;

  function onPointerDown(ev: PointerEvent) {
    dragging = true;
    startX = ev.clientX;
    startWidth = read();
    node.setPointerCapture(ev.pointerId);
    node.classList.add('dragging');
    ev.preventDefault();
  }

  function onPointerMove(ev: PointerEvent) {
    if (!dragging) return;
    const next = clamp(startWidth + (ev.clientX - startX));
    apply(next);
  }

  function onPointerUp(ev: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try {
      node.releasePointerCapture(ev.pointerId);
    } catch {
      // ignore
    }
    node.classList.remove('dragging');
    // Read the current width back from the applied CSS var so we persist
    // exactly what the user sees.
    const cur = parent!.style.getPropertyValue(cssVar);
    const match = /([0-9.]+)px/.exec(cur);
    if (match) write(parseFloat(match[1]));
  }

  function onDoubleClick() {
    apply(clamp(initial));
    write(clamp(initial));
  }

  node.addEventListener('pointerdown', onPointerDown);
  node.addEventListener('pointermove', onPointerMove);
  node.addEventListener('pointerup', onPointerUp);
  node.addEventListener('pointercancel', onPointerUp);
  node.addEventListener('dblclick', onDoubleClick);

  return {
    update(next: ResizableOptions) {
      storageKey = next.storageKey;
      cssVar = next.cssVar;
      min = next.min ?? 100;
      max = next.max ?? 800;
      initial = next.initial ?? 220;
      apply(read());
    },
    destroy() {
      node.removeEventListener('pointerdown', onPointerDown);
      node.removeEventListener('pointermove', onPointerMove);
      node.removeEventListener('pointerup', onPointerUp);
      node.removeEventListener('pointercancel', onPointerUp);
      node.removeEventListener('dblclick', onDoubleClick);
    },
  };
}

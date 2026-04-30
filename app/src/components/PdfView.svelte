<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fileAssetUrl } from '../lib/api';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  /// Absolute filesystem path of the PDF to render. The viewer pulls bytes
  /// through the same `read_file_bytes` plumbing used by images, so the
  /// browser-mode token check applies.
  export let path: string;

  /// Shift + wheel zoom for the embedded PDF. Same `zoom:` CSS pattern as
  /// HtmlIndex's iframe — the native PDF viewer re-renders crisply at the
  /// scaled size, so it doesn't go pixelated like a transform: scale would.
  ///
  /// Wrinkle: native <embed type="application/pdf"> captures wheel and
  /// pointer events inside the plugin process and our window listener never
  /// sees them. We layer an always-on overlay on top that captures wheel +
  /// pointer events; pan and zoom both go through the overlay so the gesture
  /// works regardless of where the cursor sits inside the page.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.pdfview.zoom');

  let url = '';
  /// Set when `fileAssetUrl` returns an `URL.createObjectURL(...)` blob — we
  /// own that lifetime and must `revokeObjectURL` it when we tear down or
  /// switch documents. Tauri-mode just returns a `convertFileSrc` URL which
  /// has no per-instance cleanup.
  let ownedUrl: string | null = null;
  let error: string | null = null;
  let loading = true;
  let loadToken = 0;

  let wrapEl: HTMLDivElement | null = null;
  let dragging = false;
  let dragLastX = 0;
  let dragLastY = 0;

  $: if (path) void load(path);

  async function load(p: string) {
    const token = ++loadToken;
    loading = true;
    error = null;
    releaseUrl();
    url = '';
    try {
      const next = await fileAssetUrl(p);
      // Race guard: a later load() may have superseded us.
      if (token !== loadToken) {
        if (next.startsWith('blob:')) URL.revokeObjectURL(next);
        return;
      }
      url = next;
      if (next.startsWith('blob:')) ownedUrl = next;
    } catch (err) {
      if (token === loadToken) error = String(err);
    } finally {
      if (token === loadToken) loading = false;
    }
  }

  function releaseUrl() {
    if (ownedUrl) URL.revokeObjectURL(ownedUrl);
    ownedUrl = null;
  }

  /// Overlay wheel: shift+wheel is handled by the parent zoom action via
  /// the shared `createShiftWheelZoom` (it preventDefaults inside the same
  /// element tree). For non-shift wheel events we translate the deltas into
  /// container scroll, so vertical/horizontal panning works even though the
  /// underlying <embed> would normally swallow the event.
  function onOverlayWheel(ev: WheelEvent) {
    if (ev.shiftKey) return; // zoom action handles this
    if (!wrapEl) return;
    if (ev.deltaY === 0 && ev.deltaX === 0) return;
    ev.preventDefault();
    wrapEl.scrollTop += ev.deltaY;
    wrapEl.scrollLeft += ev.deltaX;
  }

  function onPointerDown(ev: PointerEvent) {
    if (ev.button !== 0) return; // left button only
    if (!wrapEl) return;
    dragging = true;
    dragLastX = ev.clientX;
    dragLastY = ev.clientY;
    (ev.currentTarget as HTMLElement).setPointerCapture(ev.pointerId);
    ev.preventDefault();
  }

  function onPointerMove(ev: PointerEvent) {
    if (!dragging || !wrapEl) return;
    const dx = ev.clientX - dragLastX;
    const dy = ev.clientY - dragLastY;
    dragLastX = ev.clientX;
    dragLastY = ev.clientY;
    wrapEl.scrollLeft -= dx;
    wrapEl.scrollTop -= dy;
  }

  function onPointerUp(ev: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try {
      (ev.currentTarget as HTMLElement).releasePointerCapture(ev.pointerId);
    } catch {
      // ignore
    }
  }

  function onKeyDown(ev: KeyboardEvent) {
    if (!wrapEl) return;
    // Ignore when the user is typing somewhere.
    const tag = (ev.target as HTMLElement | null)?.tagName?.toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;
    const step = ev.shiftKey ? 200 : 60;
    let used = true;
    switch (ev.key) {
      case 'ArrowUp':    wrapEl.scrollTop  -= step; break;
      case 'ArrowDown':  wrapEl.scrollTop  += step; break;
      case 'ArrowLeft':  wrapEl.scrollLeft -= step; break;
      case 'ArrowRight': wrapEl.scrollLeft += step; break;
      case 'PageUp':     wrapEl.scrollTop  -= wrapEl.clientHeight * 0.9; break;
      case 'PageDown':   wrapEl.scrollTop  += wrapEl.clientHeight * 0.9; break;
      case 'Home':       wrapEl.scrollTop = 0; break;
      case 'End':        wrapEl.scrollTop = wrapEl.scrollHeight; break;
      default: used = false;
    }
    if (used) ev.preventDefault();
  }

  /// Reset zoom to 100% — handy after the PDF gets unwieldy.
  function resetZoom() {
    zoom.set(1);
  }

  onMount(() => {
    if (path) void load(path);
    window.addEventListener('keydown', onKeyDown);
  });

  onDestroy(() => {
    releaseUrl();
    window.removeEventListener('keydown', onKeyDown);
  });
</script>

<section class="root" use:zoomAction>
  <header class="bar">
    <span class="kind">pdf</span>
    <code class="path" title={path}>{path}</code>
    <span class="zoom-readout">{Math.round($zoom * 100)}%</span>
    <button class="zoom-reset" type="button" on:click={resetZoom} title="Reset zoom">100%</button>
    <span class="zoom-hint">Shift+scroll = zoom · drag/arrows = pan</span>
  </header>
  {#if loading}
    <div class="status">Loading…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if url}
    <div class="pdf-stack">
      <div class="pdf-wrap" bind:this={wrapEl}>
        <embed type="application/pdf" src={url} class="pdf" style="zoom: {$zoom};" />
      </div>
      <div
        class="pan-overlay"
        class:dragging
        role="presentation"
        on:wheel|nonpassive={onOverlayWheel}
        on:pointerdown={onPointerDown}
        on:pointermove={onPointerMove}
        on:pointerup={onPointerUp}
        on:pointercancel={onPointerUp}
      ></div>
    </div>
  {/if}
</section>

<style>
  .root {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    background: var(--bg-2);
    border-radius: 3px;
    color: var(--fg-2);
  }

  .path {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pdf-stack {
    position: relative;
    flex: 1;
    overflow: hidden;
  }

  .pdf-wrap {
    position: absolute;
    inset: 0;
    overflow: auto;
    background: var(--bg-0);
  }

  .pan-overlay {
    position: absolute;
    inset: 0;
    background: transparent;
    cursor: grab;
    /* Stays anchored to the visible viewport — does not scroll with the
       PDF — so wheel/drag gestures are caught wherever the cursor sits. */
  }

  .pan-overlay.dragging {
    cursor: grabbing;
  }

  .pdf {
    display: block;
    width: 100%;
    height: 100%;
    min-height: 100%;
    border: 0;
    background: var(--bg-0);
  }

  .zoom-readout {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    margin-left: auto;
    font-variant-numeric: tabular-nums;
    min-width: 3.2em;
    text-align: right;
  }

  .zoom-reset {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-1);
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 3px;
    padding: 2px 6px;
    cursor: pointer;
  }
  .zoom-reset:hover {
    background: var(--bg-3);
    color: var(--fg-0);
  }

  .zoom-hint {
    font-size: 11px;
    color: var(--fg-2);
    opacity: 0.7;
  }

  .status,
  .error {
    padding: 24px;
    color: var(--fg-2);
  }

  .error {
    color: var(--error);
  }
</style>

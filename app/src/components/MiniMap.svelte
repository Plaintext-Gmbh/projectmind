<script lang="ts">
  /// Mini-Map overlay (#66) — a constantly-visible thumbnail of the active
  /// diagram in the bottom-right of the Diagrams stage, with a rectangle that
  /// marks the region currently visible at the live pan/zoom.
  ///
  /// Rendering trick (per docs/sketches/claude-minimap.md): rather than
  /// re-invoking the SVG renderers at a smaller scale, we *mirror the already
  /// rendered stage SVG* — the same `svg` string DiagramView injects — into a
  /// small, `pointer-events:none` box scaled by a fit transform. Zero extra
  /// render plumbing, always in sync with whatever colour-by / layout the
  /// stage shows, and no re-fetch.
  ///
  /// The viewport rectangle + all interaction maths come from the pure
  /// `lib/diagrams/minimap.ts` helpers; this component only wires DOM events
  /// to viewport-store dispatches (the same reducers the stage uses).

  import { onMount, onDestroy } from 'svelte';
  import { t } from '../lib/i18n';
  import type { ViewportStore } from '../lib/diagrams/viewport';
  import {
    clampZoomFactor,
    clampedViewportRect,
    clickToPan,
    fitWorldToMiniMap,
    hasWorldBox,
    miniPointToWorld,
    panToCenterWorld,
    visibleWorldRect,
    type Rect,
  } from '../lib/diagrams/minimap';

  /// The already-rendered stage SVG markup (mirrored, not re-rendered).
  export let svg: string;
  /// The per-instance viewport store the stage drives.
  export let viewport: ViewportStore;
  /// The stage element — read for its live pixel size (visible-rect maths) and
  /// tracked via a ResizeObserver so the mini-map follows stage resizes.
  export let stage: HTMLElement | null = null;

  // Card geometry. The inner map area is MAP_W × MAP_H; the header sits above.
  const MAP_W = 200;
  const MAP_H = 150;
  const MIN_RECT_PX = 10;

  const VISIBLE_KEY = 'projectmind.diagram.miniMap.visible';
  let collapsed = readCollapsedPref();

  function readCollapsedPref(): boolean {
    try {
      // Stored as the *visible* flag to match the sketch's localStorage key;
      // collapsed is its inverse. Default: visible (collapsed = false).
      return localStorage.getItem(VISIBLE_KEY) === 'false';
    } catch {
      return false;
    }
  }

  function writeCollapsedPref(next: boolean) {
    try {
      localStorage.setItem(VISIBLE_KEY, next ? 'false' : 'true');
    } catch {
      // ignore
    }
  }

  function toggleCollapsed() {
    collapsed = !collapsed;
    writeCollapsedPref(collapsed);
  }

  // Live stage size, kept in sync via ResizeObserver. Falls back to the last
  // known values so a transient 0 during layout doesn't blank the rectangle.
  let stageW = 0;
  let stageH = 0;
  let ro: ResizeObserver | null = null;

  function measureStage() {
    if (!stage) return;
    stageW = stage.clientWidth || stageW;
    stageH = stage.clientHeight || stageH;
  }

  onMount(() => {
    measureStage();
    if (typeof ResizeObserver !== 'undefined' && stage) {
      ro = new ResizeObserver(() => measureStage());
      ro.observe(stage);
    }
  });

  onDestroy(() => {
    ro?.disconnect();
    ro = null;
  });

  // Re-measure whenever the observed stage element changes (kind switch mounts
  // a fresh stage node).
  $: if (stage) reobserve(stage);
  function reobserve(node: HTMLElement) {
    measureStage();
    if (ro) {
      ro.disconnect();
      ro.observe(node);
    }
  }

  // The viewport store is the single source of truth for the transform. Read
  // it reactively so the rectangle tracks pan/zoom/reset live.
  $: v = $viewport;
  $: show = hasWorldBox(v) && stageW > 0 && stageH > 0;
  $: fit = fitWorldToMiniMap(v, MAP_W, MAP_H);
  $: worldRect = visibleWorldRect(v, stageW, stageH);
  $: rect = clampedViewportRect(worldRect, fit, MIN_RECT_PX) as Rect;

  // ----- interaction --------------------------------------------------------

  let mapEl: HTMLDivElement;
  let dragging = false;

  function localPoint(e: MouseEvent): { mx: number; my: number } {
    const r = mapEl.getBoundingClientRect();
    return { mx: e.clientX - r.left, my: e.clientY - r.top };
  }

  /// Centre the stage on the clicked mini-map point (dispatches panTo — same
  /// reducer the stage drag uses). Also the drag primitive: while dragging the
  /// rectangle we keep re-centring on the pointer.
  function centreOnPointer(e: MouseEvent) {
    if (!show) return;
    const { mx, my } = localPoint(e);
    const { tx, ty } = clickToPan(v, mx, my, fit, stageW, stageH);
    viewport.panTo(tx, ty);
  }

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0 || !show) return;
    e.preventDefault();
    e.stopPropagation();
    dragging = true;
    // Whether the user grabbed inside the rectangle or clicked empty space,
    // the intent is "look here" — centre immediately, then track the drag.
    centreOnPointer(e);
    window.addEventListener('mousemove', onWindowMouseMove);
    window.addEventListener('mouseup', onWindowMouseUp);
  }

  function onWindowMouseMove(e: MouseEvent) {
    if (!dragging) return;
    centreOnPointer(e);
  }

  function onWindowMouseUp() {
    dragging = false;
    window.removeEventListener('mousemove', onWindowMouseMove);
    window.removeEventListener('mouseup', onWindowMouseUp);
  }

  /// Wheel over the mini-map zooms the stage, keeping the world-point under
  /// the cursor fixed — same `zoomAround` kernel the stage wheel uses. The
  /// anchor is expressed in stage-local pixels: we map the cursor's world
  /// point through the current transform back to a stage coordinate.
  function onWheel(e: WheelEvent) {
    if (!show) return;
    e.preventDefault();
    e.stopPropagation();
    const { mx, my } = localPoint(e);
    const world = miniPointToWorld(mx, my, fit);
    // stage-local anchor = world * scale + t
    const ax = world.x * v.scale + v.tx;
    const ay = world.y * v.scale + v.ty;
    const raw = Math.exp(-e.deltaY * 0.0015);
    const factor = clampZoomFactor(v.scale, raw);
    viewport.zoomAround(factor, ax, ay);
  }

  onDestroy(() => {
    window.removeEventListener('mousemove', onWindowMouseMove);
    window.removeEventListener('mouseup', onWindowMouseUp);
  });

  // A neutral centre-pan for keyboard fallback (accessibility): pressing Enter
  // recentres on the middle of the diagram — harmless, and keeps the control
  // operable without a mouse.
  function onKeydown(e: KeyboardEvent) {
    if (!show) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      const { tx, ty } = panToCenterWorld(v, v.baseW / 2, v.baseH / 2, stageW, stageH);
      viewport.panTo(tx, ty);
    }
  }
</script>

{#if show}
  <div class="minimap" class:collapsed>
    {#if collapsed}
      <button
        class="pill"
        on:click={toggleCollapsed}
        title={$t('minimap.expand')}
        aria-label={$t('minimap.expand')}
      >▢ {$t('minimap.label')}</button>
    {:else}
      <div class="header">
        <span class="title">{$t('minimap.label')}</span>
        <button
          class="collapse"
          on:click={toggleCollapsed}
          title={$t('minimap.collapse')}
          aria-label={$t('minimap.collapse')}
        >×</button>
      </div>
      <!-- The map is a picture of the diagram (role="img") that also acts as a
           pan/zoom control. Svelte's a11y linter can't express "interactive
           image", so we opt out of the two rules the stage canvas in
           DiagramView already opts out of — keyboard support is still wired via
           on:keydown for the focus-visible fallback. -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="map"
        bind:this={mapEl}
        style="width: {MAP_W}px; height: {MAP_H}px;"
        role="img"
        tabindex="0"
        aria-label={$t('minimap.aria')}
        on:mousedown={onMouseDown}
        on:wheel|nonpassive={onWheel}
        on:keydown={onKeydown}
      >
        <div
          class="thumb"
          style="
            width: {fit.contentW}px;
            height: {fit.contentH}px;
            left: {fit.offsetX}px;
            top: {fit.offsetY}px;
          "
        >
          <div
            class="scaler"
            style="
              width: {v.baseW}px;
              height: {v.baseH}px;
              transform: scale({fit.scale});
              transform-origin: 0 0;
            "
          >
            {@html svg}
          </div>
        </div>
        <div
          class="viewport-rect"
          class:dragging
          style="
            left: {rect.x}px;
            top: {rect.y}px;
            width: {rect.w}px;
            height: {rect.h}px;
          "
        ></div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .minimap {
    position: absolute;
    right: 14px;
    bottom: 14px;
    z-index: 5;
    background: color-mix(in srgb, var(--bg-1) 92%, transparent);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.32);
    overflow: hidden;
    user-select: none;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px 4px 8px;
    border-bottom: 1px solid var(--bg-3);
  }

  .title {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
  }

  .collapse {
    width: 18px;
    height: 18px;
    padding: 0;
    line-height: 1;
    font-size: 13px;
    background: transparent;
    border: 0;
    color: var(--fg-2);
    cursor: pointer;
  }

  .collapse:hover {
    color: var(--fg-0);
  }

  .pill {
    padding: 5px 10px;
    font-size: 11px;
    background: transparent;
    border: 0;
    color: var(--fg-1);
    cursor: pointer;
  }

  .pill:hover {
    color: var(--accent-2);
  }

  .map {
    position: relative;
    overflow: hidden;
    cursor: pointer;
    background:
      radial-gradient(circle at 1px 1px, var(--bg-2) 1px, transparent 0) 0 0 / 12px 12px;
  }

  .map:focus-visible {
    outline: 2px solid var(--accent-2);
    outline-offset: -2px;
  }

  .thumb {
    position: absolute;
    /* The mirrored SVG must not intercept clicks — the .map owns interaction. */
    pointer-events: none;
  }

  .scaler {
    position: absolute;
    top: 0;
    left: 0;
  }

  .scaler :global(svg) {
    display: block;
    max-width: none;
  }

  .viewport-rect {
    position: absolute;
    pointer-events: none;
    border: 1.5px solid var(--accent-2, #6ea8fe);
    background: color-mix(in srgb, var(--accent-2, #6ea8fe) 16%, transparent);
    border-radius: 2px;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.35);
  }

  .viewport-rect.dragging {
    background: color-mix(in srgb, var(--accent-2, #6ea8fe) 26%, transparent);
  }
</style>

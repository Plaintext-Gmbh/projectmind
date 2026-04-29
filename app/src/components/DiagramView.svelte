<script lang="ts">
  import { onMount, tick } from 'svelte';
  import mermaid from 'mermaid';
  import { showDiagram } from '../lib/api';

  export let kind: 'bean-graph' | 'package-tree';

  let stage: HTMLDivElement;
  let mermaidSource = '';
  let svg = '';
  let loading = false;
  let error: string | null = null;

  // viewport state
  let scale = 1;
  let tx = 0;
  let ty = 0;
  let dragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartTx = 0;
  let dragStartTy = 0;

  // SVG size at scale=1 (after fit-to-stage). Zoom is applied by resizing the
  // SVG itself (so the vector re-rasterises crisply at the new resolution)
  // rather than by CSS `transform: scale()` which would blur a bitmap.
  let baseW = 0;
  let baseH = 0;

  $: applyScale(scale);

  $: if (kind) {
    void render(kind);
  }

  onMount(() => {
    mermaid.initialize({
      startOnLoad: false,
      theme: 'dark',
      securityLevel: 'loose',
      // Large repositories produce diagrams well past Mermaid's defaults
      // (50 000 chars / 500 edges). Allow up to ~1 MB and 10 000 edges.
      maxTextSize: 1_000_000,
      maxEdges: 10_000,
      // Render labels as SVG <text> instead of HTML inside <foreignObject>.
      // HTML labels rasterise once and scale as a bitmap when the SVG is
      // resized — SVG text re-renders crisply at any zoom level.
      flowchart: { htmlLabels: false, useMaxWidth: false },
      class: { htmlLabels: false, useMaxWidth: false },
    });
  });

  function resetView() {
    scale = 1;
    tx = 0;
    ty = 0;
  }

  async function render(k: 'bean-graph' | 'package-tree') {
    loading = true;
    error = null;
    try {
      mermaidSource = await showDiagram(k);
      const id = `mermaid-${Date.now()}`;
      const result = await mermaid.render(id, mermaidSource);
      svg = result.svg;
      resetView();
      await tick();
      const node = stage?.querySelector('svg') as SVGSVGElement | null;
      if (node) {
        // Drop Mermaid's inline width/maxWidth so we control sizing.
        node.removeAttribute('style');
        // Compute fit-to-stage at scale=1 from the SVG's viewBox aspect ratio.
        const vb = (node.getAttribute('viewBox') ?? '').split(/\s+/).map(Number);
        const [, , vbW = 0, vbH = 0] = vb;
        const sw = stage?.clientWidth ?? 0;
        const sh = stage?.clientHeight ?? 0;
        if (vbW > 0 && vbH > 0 && sw > 0 && sh > 0) {
          const fit = Math.min(sw / vbW, sh / vbH);
          baseW = vbW * fit;
          baseH = vbH * fit;
        } else {
          baseW = sw;
          baseH = sh;
        }
        node.style.display = 'block';
        applyScale(scale);
      }
    } catch (err) {
      error = String(err);
      svg = '';
    } finally {
      loading = false;
    }
  }

  function applyScale(s: number) {
    if (!stage || !baseW || !baseH) return;
    const node = stage.querySelector('svg');
    if (!node) return;
    // Resize the SVG so the renderer re-rasterises the vector at the new size.
    // `width`/`height` attributes (rather than CSS) keep `viewBox` scaling
    // crisp at any zoom level.
    node.setAttribute('width', String(baseW * s));
    node.setAttribute('height', String(baseH * s));
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const rect = stage.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = Math.exp(-e.deltaY * 0.0015);
    const nextScale = Math.min(8, Math.max(0.2, scale * factor));
    // Zoom toward cursor: keep the world-point under the cursor stable.
    tx = cx - (cx - tx) * (nextScale / scale);
    ty = cy - (cy - ty) * (nextScale / scale);
    scale = nextScale;
  }

  function onMouseDown(e: MouseEvent) {
    if (e.button !== 0) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartTx = tx;
    dragStartTy = ty;
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return;
    tx = dragStartTx + (e.clientX - dragStartX);
    ty = dragStartTy + (e.clientY - dragStartY);
  }

  function endDrag() {
    dragging = false;
  }

  function zoomBy(factor: number) {
    if (!stage) return;
    const rect = stage.getBoundingClientRect();
    const cx = rect.width / 2;
    const cy = rect.height / 2;
    const nextScale = Math.min(8, Math.max(0.2, scale * factor));
    tx = cx - (cx - tx) * (nextScale / scale);
    ty = cy - (cy - ty) * (nextScale / scale);
    scale = nextScale;
  }
</script>

<div class="root">
  <div class="toolbar">
    <button on:click={() => zoomBy(1.25)} title="Zoom in">＋</button>
    <button on:click={() => zoomBy(0.8)} title="Zoom out">－</button>
    <button on:click={resetView} title="Reset view">⌂</button>
    <span class="zoom-readout">{Math.round(scale * 100)}%</span>
    <span class="hint">Drag to pan • Wheel to zoom</span>
  </div>
  {#if loading}
    <div class="placeholder">Rendering diagram…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
    <pre>{mermaidSource}</pre>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="stage"
      class:dragging
      bind:this={stage}
      on:wheel|preventDefault={onWheel}
      on:mousedown={onMouseDown}
      on:mousemove={onMouseMove}
      on:mouseup={endDrag}
      on:mouseleave={endDrag}
      role="img"
      aria-label="Diagram canvas (drag to pan, wheel to zoom)"
    >
      <div
        class="diagram"
        style="transform: translate({tx}px, {ty}px); transform-origin: 0 0;"
      >
        {@html svg}
      </div>
    </div>
  {/if}
</div>

<style>
  .root {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-0);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .toolbar button {
    width: 28px;
    height: 28px;
    padding: 0;
    font-size: 14px;
    line-height: 1;
  }

  .zoom-readout {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    min-width: 44px;
    text-align: right;
  }

  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-2);
  }

  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    cursor: grab;
    /* let SVG fill the whole canvas */
    background:
      radial-gradient(circle at 1px 1px, var(--bg-2) 1px, transparent 0) 0 0 / 24px 24px;
  }

  .stage.dragging {
    cursor: grabbing;
  }

  .diagram {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    will-change: transform;
  }

  /* Width/height are set explicitly on the SVG by JS so zoom triggers a vector
     re-render. Make sure no UA stylesheet caps the size. */
  .diagram :global(svg) {
    max-width: none;
    display: block;
  }

  .placeholder {
    color: var(--fg-2);
    text-align: center;
    padding: 40px;
  }

  .error {
    color: var(--error);
    margin: 12px;
    padding: 12px;
    border: 1px solid var(--error);
    border-radius: var(--radius-sm);
  }

  pre {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    background: var(--bg-1);
    margin: 0 12px 12px;
    padding: 12px;
    border-radius: var(--radius-sm);
    overflow-x: auto;
  }
</style>

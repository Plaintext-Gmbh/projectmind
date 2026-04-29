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

  $: if (kind) {
    void render(kind);
  }

  onMount(() => {
    mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'loose' });
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
      // Defeat Mermaid's inline max-width that pins the SVG to its intrinsic
      // pixel width — we want it to grow to fill the stage.
      const node = stage?.querySelector('svg');
      if (node) {
        node.removeAttribute('style');
        node.setAttribute('width', '100%');
        node.setAttribute('height', '100%');
        node.style.maxWidth = 'none';
        node.style.height = '100%';
        node.style.width = '100%';
      }
    } catch (err) {
      error = String(err);
      svg = '';
    } finally {
      loading = false;
    }
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
        style="transform: translate({tx}px, {ty}px) scale({scale}); transform-origin: 0 0;"
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

  /* Override Mermaid's inline style="max-width: …px" that otherwise pins
     the SVG to its tiny intrinsic width. */
  .diagram :global(svg) {
    width: 100% !important;
    height: 100% !important;
    max-width: none !important;
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

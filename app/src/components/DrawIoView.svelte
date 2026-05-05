<script lang="ts">
  import DrawIoFrame from './DrawIoFrame.svelte';
  import { readFileText } from '../lib/api';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  export let path: string;

  let xml = '';
  let loading = false;
  let error: string | null = null;

  const { zoom, action: zoomAction } = createShiftWheelZoom(
    'projectmind.drawio.zoom',
  );

  $: void load(path);

  async function load(p: string) {
    if (!p) return;
    loading = true;
    error = null;
    try {
      xml = await readFileText(p);
    } catch (e) {
      error = String(e);
      xml = '';
    } finally {
      loading = false;
    }
  }
</script>

<section class="root" use:zoomAction style="font-size: {$zoom}em;">
  <header class="bar">
    <span class="kind">drawio</span>
    <span class="path">{path}</span>
  </header>
  <div class="body">
    {#if loading}
      <div class="empty">Loading…</div>
    {:else if error}
      <div class="error">⚠ {error}</div>
    {:else}
      <DrawIoFrame {xml} title="draw.io diagram" />
    {/if}
  </div>
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
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .empty,
  .error {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-2);
    font-size: 13px;
  }
  .error {
    color: var(--error);
  }
</style>

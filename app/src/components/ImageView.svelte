<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fileAssetUrl } from '../lib/api';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  /// Absolute filesystem path of the image to render. Same loading
  /// contract as PdfView — `fileAssetUrl` handles Tauri vs browser mode.
  export let path: string;

  let url = '';
  let ownedUrl: string | null = null;
  let error: string | null = null;
  let loading = true;
  let loadToken = 0;

  // Shift+wheel zoom, persisted in localStorage. The same helper backs every
  // other zoomable view in the app.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.imageview.zoom');

  $: if (path) void load(path);

  async function load(p: string) {
    const token = ++loadToken;
    loading = true;
    error = null;
    releaseUrl();
    url = '';
    try {
      const next = await fileAssetUrl(p);
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

  onMount(() => {
    if (path) void load(path);
  });

  onDestroy(() => {
    releaseUrl();
  });
</script>

<section class="root">
  <header class="bar">
    <span class="kind">image</span>
    <code class="path" title={path}>{path}</code>
  </header>
  {#if loading}
    <div class="status">Loading…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if url}
    <div class="canvas" use:zoomAction>
      <img src={url} alt={path} style="transform: scale({$zoom});" />
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

  .canvas {
    flex: 1;
    overflow: auto;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-0);
    padding: 24px;
  }

  .canvas img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    transform-origin: center center;
    transition: transform 80ms ease;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
    border-radius: var(--radius-sm);
    background: var(--bg-1);
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

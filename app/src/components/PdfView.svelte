<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fileAssetUrl } from '../lib/api';

  /// Absolute filesystem path of the PDF to render. The viewer pulls bytes
  /// through the same `read_file_bytes` plumbing used by images, so the
  /// browser-mode token check applies.
  export let path: string;

  let url = '';
  /// Set when `fileAssetUrl` returns an `URL.createObjectURL(...)` blob — we
  /// own that lifetime and must `revokeObjectURL` it when we tear down or
  /// switch documents. Tauri-mode just returns a `convertFileSrc` URL which
  /// has no per-instance cleanup.
  let ownedUrl: string | null = null;
  let error: string | null = null;
  let loading = true;
  let loadToken = 0;

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

  onMount(() => {
    if (path) void load(path);
  });

  onDestroy(() => {
    releaseUrl();
  });
</script>

<section class="root">
  <header class="bar">
    <span class="kind">pdf</span>
    <code class="path" title={path}>{path}</code>
  </header>
  {#if loading}
    <div class="status">Loading…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if url}
    <embed type="application/pdf" src={url} class="pdf" />
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

  .pdf {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--bg-0);
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

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { readFileText } from '../lib/api';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  export let path: string;

  // diagrams.net embed protocol — `proto=json` lets us postMessage the XML
  // payload after the iframe loads, so we don't have to encode it into the
  // URL (which has a hard size limit on every browser).
  //
  // Privacy note: the .drawio XML is sent into an iframe pointed at
  // embed.diagrams.net (a third-party service). For repos with sensitive
  // diagrams, build the Tauri shell with a self-hosted draw.io viewer or
  // run the .drawio file through a local converter first.
  const EMBED_ORIGIN = 'https://embed.diagrams.net';
  const EMBED_URL = `${EMBED_ORIGIN}/?embed=1&ui=atlas&proto=json&splash=0&toolbar=0&libraries=0&dark=auto`;

  let frame: HTMLIFrameElement;
  let xml = '';
  let loading = false;
  let error: string | null = null;
  let initialised = false;

  const { zoom, action: zoomAction } = createShiftWheelZoom(
    'projectmind.drawio.zoom',
  );

  $: void load(path);

  async function load(p: string) {
    if (!p) return;
    loading = true;
    error = null;
    initialised = false;
    try {
      xml = await readFileText(p);
    } catch (e) {
      error = String(e);
      xml = '';
    } finally {
      loading = false;
    }
  }

  function onMessage(ev: MessageEvent) {
    // Defence-in-depth: only react to messages from the iframe we mounted
    // AND from the diagrams.net origin we expect. Either check on its own
    // is enough for the happy path; combining them rules out a redirected
    // iframe smuggling messages back at us.
    if (ev.source !== frame?.contentWindow) return;
    if (ev.origin !== EMBED_ORIGIN) return;
    let data: { event?: string };
    try {
      data = typeof ev.data === 'string' ? JSON.parse(ev.data) : ev.data;
    } catch {
      return;
    }
    if (data?.event === 'init' && !initialised && xml) {
      initialised = true;
      // Pin the postMessage target origin to embed.diagrams.net so a
      // navigated-away iframe doesn't end up receiving the diagram XML.
      frame.contentWindow?.postMessage(
        JSON.stringify({ action: 'load', xml, autosave: 0 }),
        EMBED_ORIGIN,
      );
    }
  }

  // Once xml lands AND the iframe is ready, we dispatch the load action.
  // The iframe announces "init" via postMessage; we react to that above.
  $: if (xml && frame && !initialised) {
    // Trigger a no-op postMessage so the iframe's `init` re-fires if it's
    // already past that point. Cheap and safe.
    try {
      frame.contentWindow?.postMessage(
        JSON.stringify({ action: 'status' }),
        EMBED_ORIGIN,
      );
    } catch {
      // ignore
    }
  }

  onMount(() => window.addEventListener('message', onMessage));
  onDestroy(() => window.removeEventListener('message', onMessage));
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
      <iframe
        bind:this={frame}
        class="frame"
        title="draw.io diagram"
        src={EMBED_URL}
        allow="clipboard-read; clipboard-write"
      ></iframe>
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
  .frame {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--bg-0);
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

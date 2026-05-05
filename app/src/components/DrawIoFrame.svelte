<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  export let xml: string;
  export let title = 'draw.io diagram';

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
  let initialised = false;

  $: if (xml) initialised = false;

  function sendLoad() {
    if (!xml || !frame?.contentWindow) return;
    initialised = true;
    frame.contentWindow.postMessage(
      JSON.stringify({ action: 'load', xml, autosave: 0 }),
      EMBED_ORIGIN,
    );
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
    if (data?.event === 'init' && !initialised) sendLoad();
  }

  // Once xml lands AND the iframe is ready, we dispatch the load action.
  // The iframe announces "init" via postMessage; we react to that above.
  $: if (xml && frame && !initialised) {
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

<iframe
  bind:this={frame}
  class="frame"
  {title}
  src={EMBED_URL}
  allow="clipboard-read; clipboard-write"
></iframe>

<style>
  .frame {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--bg-0);
  }
</style>

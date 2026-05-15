<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { wheelDelta } from '../lib/shiftWheelZoom';

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
  // URL parameter cheatsheet for the diagrams.net embed:
  //   embed=1       — embed mode (required for postMessage protocol)
  //   ui=atlas      — UI variant (left over from the menu/toolbar layout)
  //   proto=json    — load XML via postMessage(json) instead of URL
  //   splash=0      — no welcome splash
  //   toolbar=0     — no top toolbar
  //   libraries=0   — no shape library panel
  //   chrome=0      — kill the rest of the chrome (menubar, status bar,
  //                   format/outline sidebars) so only the canvas shows.
  //                   Pan/zoom inside the iframe is now disabled because
  //                   `pointer-events: none` on the frame routes the mouse
  //                   to our wrapper instead — see the .stage block below.
  //   dark=auto     — follow OS dark mode
  const EMBED_URL = `${EMBED_ORIGIN}/?embed=1&ui=atlas&proto=json&splash=0&toolbar=0&libraries=0&chrome=0&dark=auto`;

  let frame: HTMLIFrameElement;
  let stage: HTMLDivElement;
  let initialised = false;

  // Pan + zoom state — identical model to DiagramView so the gesture feels
  // the same everywhere: plain wheel and Shift+wheel both zoom (Shift parity
  // matches the text/code viewers), drag-anywhere pans. We apply a single
  // `translate(...) scale(...)` transform to the iframe; `pointer-events:
  // none` on the iframe forwards the mouse to this wrapper so the iframe's
  // own draw.io scroll/pan can't fight us.
  let scale = 1;
  let tx = 0;
  let ty = 0;
  let dragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartTx = 0;
  let dragStartTy = 0;

  $: frameTransform = `translate(${tx}px, ${ty}px) scale(${scale})`;

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

  function onWheel(e: WheelEvent) {
    if (e.cancelable) e.preventDefault();
    e.stopPropagation();
    const delta = wheelDelta(e);
    if (delta === 0) return;
    const rect = stage.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = Math.exp(-delta * 0.0015);
    const nextScale = Math.min(8, Math.max(0.2, scale * factor));
    // Zoom toward cursor: keep the world-point under the cursor stable.
    tx = cx - (cx - tx) * (nextScale / scale);
    ty = cy - (cy - ty) * (nextScale / scale);
    scale = nextScale;
  }

  // Svelte's `on:wheel` registers a passive listener so `preventDefault`
  // wouldn't take effect — explicit non-passive registration here.
  function nonPassiveWheel(node: HTMLDivElement, handler: (e: WheelEvent) => void) {
    let current = handler;
    const fn = (e: WheelEvent) => current(e);
    node.addEventListener('wheel', fn, { passive: false });
    return {
      update(next: (e: WheelEvent) => void) {
        current = next;
      },
      destroy() {
        node.removeEventListener('wheel', fn);
      },
    };
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

  onMount(() => window.addEventListener('message', onMessage));
  onDestroy(() => window.removeEventListener('message', onMessage));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="stage"
  class:dragging
  bind:this={stage}
  use:nonPassiveWheel={onWheel}
  on:mousedown={onMouseDown}
  on:mousemove={onMouseMove}
  on:mouseup={endDrag}
  on:mouseleave={endDrag}
>
  <iframe
    bind:this={frame}
    class="frame"
    style="transform: {frameTransform}; transform-origin: 0 0;"
    {title}
    src={EMBED_URL}
    allow="clipboard-read; clipboard-write"
  ></iframe>
</div>

<style>
  .stage {
    flex: 1;
    width: 100%;
    height: 100%;
    position: relative;
    overflow: hidden;
    cursor: grab;
    background: var(--bg-0);
  }
  .stage.dragging {
    cursor: grabbing;
  }
  .frame {
    /* Block the iframe from receiving pointer events so plain mouse
       interactions (wheel, drag) hit the wrapper above and feed our pan/
       zoom handlers. The user couldn't interact with the embed's UI
       anyway — chrome=0 hid the toolbar / menus. Selectable text and
       hyperlinks inside the diagram are also disabled as a side effect,
       which matches the "view-only" intent. */
    pointer-events: none;
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--bg-0);
  }
</style>

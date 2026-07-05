<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { marked } from 'marked';
  import mermaid from 'mermaid';
  import { currentArtifact } from '../lib/api';
  import type { Artifact } from '../lib/api';
  import { t } from '../lib/i18n';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';
  import { sandboxedHtmlDataUrl } from '../lib/htmlSandbox';

  /// Artifact id to render. Comes from the `{kind:'artifact'}` view intent.
  export let artifactId: string;
  /// Bumped on every (re)issued intent so a same-id replacement re-fetches.
  export let nonce: number = 0;

  let artifact: Artifact | null = null;
  let loading = true;
  let error: string | null = null;
  let notFound = false;
  let markdownHtml = '';
  let host: HTMLDivElement;
  let lastNonce = -1;

  const ZOOM_STEP = 0.1;
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.artifact.zoom', {
    step: ZOOM_STEP,
  });

  $: void load(artifactId, nonce);

  // For HTML artifacts, the body is rendered inside a sandboxed iframe with a
  // strict CSP — never injected into the app DOM. This is the hard security
  // boundary: AI-authored <script> stays inert.
  $: iframeSrc =
    artifact && artifact.format === 'html' ? sandboxedHtmlDataUrl(artifact.content) : '';

  async function load(id: string, n: number) {
    if (n === lastNonce && artifact?.id === id) return;
    lastNonce = n;
    loading = true;
    error = null;
    notFound = false;
    markdownHtml = '';
    try {
      artifact = await currentArtifact(id);
      if (!artifact) {
        notFound = true;
        return;
      }
      if (artifact.format === 'markdown') {
        markdownHtml = await renderMarkdown(artifact.content);
        await tick();
        await renderMermaidBlocks();
      }
    } catch (err) {
      error = String(err);
      artifact = null;
    } finally {
      loading = false;
    }
  }

  async function renderMarkdown(src: string): Promise<string> {
    marked.setOptions({ gfm: true, breaks: false });
    return marked.parse(src) as string;
  }

  /// Render fenced ```mermaid blocks to inline SVG, exactly like the file
  /// viewer. Failures render a small error note instead of blowing up.
  async function renderMermaidBlocks() {
    if (!host) return;
    const blocks = host.querySelectorAll<HTMLElement>('pre code.language-mermaid');
    for (const code of Array.from(blocks)) {
      const source = code.textContent ?? '';
      const id = `mmd-art-${Math.random().toString(36).slice(2, 10)}`;
      try {
        const result = await mermaid.render(id, source);
        const wrapper = document.createElement('div');
        wrapper.className = 'mermaid-rendered';
        wrapper.innerHTML = result.svg;
        code.parentElement?.replaceWith(wrapper);
      } catch (err) {
        const note = document.createElement('div');
        note.className = 'mermaid-error';
        note.textContent = `mermaid: ${String(err)}`;
        code.parentElement?.replaceWith(note);
      }
    }
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  onMount(() => {
    void load(artifactId, nonce);
  });
</script>

<section class="root">
  <header class="bar">
    <span class="kind">{$t('artifact.badge')}</span>
    {#if artifact}
      <span class="title" title={artifact.title}>{artifact.title}</span>
      <span class="meta">{artifact.format} · {fmtSize(artifact.size)}</span>
    {/if}
    <div class="spacer"></div>
    <div class="zoom" title="Zoom: Shift + Wheel">
      <button class="zoom-btn" on:click={() => zoom.update((z) => z - ZOOM_STEP)} aria-label="Zoom out">−</button>
      <button class="zoom-pct" on:click={() => zoom.set(1)} aria-label="Reset zoom">{Math.round($zoom * 100)}%</button>
      <button class="zoom-btn" on:click={() => zoom.update((z) => z + ZOOM_STEP)} aria-label="Zoom in">+</button>
    </div>
  </header>

  {#if loading}
    <div class="status">{$t('artifact.loading')}</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else if notFound}
    <div class="status">{$t('artifact.notFound')}</div>
  {:else if artifact && artifact.format === 'html'}
    <iframe class="render-frame" title={$t('artifact.previewTitle')} sandbox="" src={iframeSrc} style="zoom: {$zoom};"></iframe>
  {:else if artifact}
    <div class="scroller" use:zoomAction>
      <div class="content" bind:this={host} style="font-size: {$zoom}em;">
        {@html markdownHtml}
      </div>
    </div>
  {/if}
</section>

<style>
  .root {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--bg-0);
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
    background: color-mix(in srgb, var(--accent-2) 20%, var(--bg-2));
    color: var(--accent-2);
    border-radius: 3px;
    font-weight: 600;
  }
  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 11px;
    color: var(--fg-2);
    font-family: var(--mono);
  }
  .spacer {
    flex: 1;
  }

  .zoom {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    padding: 1px;
  }
  .zoom-btn,
  .zoom-pct {
    background: transparent;
    color: var(--fg-1);
    border: 0;
    padding: 2px 8px;
    cursor: pointer;
    font: inherit;
    line-height: 1;
  }
  .zoom-btn {
    width: 24px;
    font-size: 14px;
  }
  .zoom-pct {
    min-width: 44px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--fg-2);
  }
  .zoom-btn:hover,
  .zoom-pct:hover {
    background: var(--bg-3);
    color: var(--fg-0);
  }

  .status,
  .error {
    padding: 24px;
    color: var(--fg-2);
  }
  .error {
    color: var(--error);
  }

  .render-frame {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: white;
  }

  .scroller {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .content {
    padding: 24px 32px 64px;
    line-height: 1.55;
    color: var(--fg-1);
    max-width: 920px;
    margin: 0 auto;
    transition: font-size 80ms ease;
  }

  /* Markdown styling — mirrors the file viewer so artifacts read the same. */
  .content :global(h1),
  .content :global(h2),
  .content :global(h3),
  .content :global(h4) {
    color: var(--fg-0);
    margin-top: 1.4em;
    margin-bottom: 0.5em;
    font-weight: 600;
  }
  .content :global(h1) { font-size: 1.7em; border-bottom: 1px solid var(--bg-3); padding-bottom: 6px; }
  .content :global(h2) { font-size: 1.35em; border-bottom: 1px solid var(--bg-3); padding-bottom: 4px; }
  .content :global(h3) { font-size: 1.15em; }
  .content :global(p) { margin: 0.6em 0; }
  .content :global(ul),
  .content :global(ol) { padding-left: 1.5em; }
  .content :global(code) {
    background: var(--bg-2);
    padding: 1px 5px;
    border-radius: 3px;
    font-family: var(--mono);
    font-size: 0.92em;
  }
  .content :global(pre) {
    background: var(--bg-1);
    padding: 12px 16px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--bg-3);
    overflow-x: auto;
  }
  .content :global(pre code) {
    background: none;
    padding: 0;
  }
  .content :global(blockquote) {
    border-left: 3px solid var(--accent-2);
    margin: 0.6em 0;
    padding: 0.2em 1em;
    color: var(--fg-2);
    background: color-mix(in srgb, var(--accent-2) 6%, transparent);
  }
  .content :global(a) { color: var(--accent-2); }
  .content :global(a:hover) { text-decoration: underline; }
  .content :global(table) {
    border-collapse: collapse;
    margin: 1em 0;
  }
  .content :global(th),
  .content :global(td) {
    border: 1px solid var(--bg-3);
    padding: 6px 10px;
    text-align: left;
  }
  .content :global(th) {
    background: var(--bg-1);
    font-weight: 600;
  }
  .content :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-sm);
    margin: 0.6em 0;
  }
  .content :global(.mermaid-rendered) {
    margin: 1em 0;
    padding: 12px;
    background: var(--bg-1);
    border-radius: var(--radius-sm);
    border: 1px solid var(--bg-3);
    overflow-x: auto;
  }
  .content :global(.mermaid-rendered svg) {
    max-width: 100%;
    height: auto;
  }
  .content :global(.mermaid-error) {
    color: var(--error);
    font-family: var(--mono);
    font-size: 12px;
    padding: 12px;
    border: 1px dashed var(--error);
    border-radius: var(--radius-sm);
  }
</style>

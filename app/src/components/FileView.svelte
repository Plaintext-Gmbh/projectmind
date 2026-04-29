<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { marked } from 'marked';
  import mermaid from 'mermaid';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { readFileText } from '../lib/api';

  export let path: string;

  let content = '';
  let html = '';
  let error: string | null = null;
  let loading = false;
  let host: HTMLDivElement;

  $: if (path) void load(path);

  async function load(p: string) {
    loading = true;
    error = null;
    html = '';
    content = '';
    try {
      content = await readFileText(p);
      if (isMarkdown(p)) {
        html = await renderMarkdown(content, p);
        await tick();
        await rewriteImages(p);
        await renderMermaidBlocks();
      } else {
        // Plain text fallback. Escape HTML and wrap in <pre>.
        html = `<pre class="plain">${escapeHtml(content)}</pre>`;
      }
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function isMarkdown(p: string): boolean {
    return /\.(md|markdown|mdx)$/i.test(p);
  }

  async function renderMarkdown(src: string, _filePath: string): Promise<string> {
    marked.setOptions({ gfm: true, breaks: false });
    return await marked.parse(src);
  }

  async function rewriteImages(filePath: string) {
    if (!host) return;
    const dir = parentDir(filePath);
    const imgs = host.querySelectorAll<HTMLImageElement>('img');
    for (const img of Array.from(imgs)) {
      const raw = img.getAttribute('src') ?? '';
      if (!raw || /^(?:[a-z]+:)?\/\//i.test(raw) || raw.startsWith('data:')) {
        continue; // already absolute / external / data URL
      }
      const abs = raw.startsWith('/') ? raw : `${dir}/${raw}`;
      img.setAttribute('src', convertFileSrc(normalizePath(abs)));
    }
  }

  async function renderMermaidBlocks() {
    if (!host) return;
    const blocks = host.querySelectorAll<HTMLElement>('pre code.language-mermaid');
    for (const code of Array.from(blocks)) {
      const source = code.textContent ?? '';
      const id = `mmd-${Math.random().toString(36).slice(2, 10)}`;
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

  function parentDir(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? '' : p.slice(0, idx);
  }

  function normalizePath(p: string): string {
    // Collapse `a/./b` and `a/b/../c` segments (not full POSIX, but good enough
    // for relative paths inside a markdown file).
    const parts: string[] = [];
    for (const seg of p.split('/')) {
      if (seg === '' || seg === '.') continue;
      if (seg === '..') parts.pop();
      else parts.push(seg);
    }
    return (p.startsWith('/') ? '/' : '') + parts.join('/');
  }

  function escapeHtml(s: string): string {
    return s
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  onMount(() => {
    if (path) void load(path);
  });
</script>

<section class="root">
  <header class="bar">
    <span class="kind">{isMarkdown(path) ? 'markdown' : 'file'}</span>
    <code class="path" title={path}>{path}</code>
  </header>
  {#if loading}
    <div class="status">Loading…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else}
    <div class="content" bind:this={host}>{@html html}</div>
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

  .status,
  .error {
    padding: 24px;
    color: var(--fg-2);
  }

  .error {
    color: var(--error);
  }

  .content {
    overflow-y: auto;
    padding: 24px 32px;
    line-height: 1.55;
    color: var(--fg-1);
  }

  /* Markdown styling */
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

  .content :global(a) {
    color: var(--accent-2);
  }
  .content :global(a:hover) {
    text-decoration: underline;
  }

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

  .content :global(.plain) {
    white-space: pre;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    background: var(--bg-1);
    padding: 16px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--bg-3);
  }
</style>

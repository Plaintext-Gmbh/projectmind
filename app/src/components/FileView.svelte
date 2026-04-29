<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { marked } from 'marked';
  import mermaid from 'mermaid';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { readFileText, listMarkdownFiles } from '../lib/api';
  import type { MarkdownFile } from '../lib/api';
  import { repo, fileView } from '../lib/store';

  export let path: string;
  /// Optional heading slug to scroll to after rendering. If a slug doesn't
  /// match exactly we fall back to a case-insensitive substring match.
  export let anchor: string | null = null;
  /// Bumped on every (re)issued intent — re-runs the scroll even if path/anchor
  /// is unchanged. Optional so manual GUI navigation still works.
  export let nonce: number = 0;

  interface TocEntry {
    id: string;
    text: string;
    level: number;
  }

  let content = '';
  let html = '';
  let error: string | null = null;
  let loading = false;
  let host: HTMLDivElement;
  let scroller: HTMLDivElement;
  let toc: TocEntry[] = [];
  let activeHeadingId: string | null = null;

  /// Zoom factor for content text. Persisted via localStorage so reopens keep it.
  let zoom = readZoom();
  const ZOOM_MIN = 0.6;
  const ZOOM_MAX = 2.0;
  const ZOOM_STEP = 0.1;

  // ----- Markdown picker (project-wide .md files) ---------------------------
  let mdFiles: MarkdownFile[] = [];
  let mdFilesLoadedFor: string | null = null;
  let pickerOpen = false;
  let pickerQuery = '';
  let pickerInput: HTMLInputElement | null = null;
  let pickerHighlight = 0;

  $: pickerFiltered = filterFiles(mdFiles, pickerQuery);

  function filterFiles(files: MarkdownFile[], q: string): MarkdownFile[] {
    if (!q.trim()) return files;
    const needle = q.toLowerCase();
    return files.filter(
      (f) =>
        f.rel.toLowerCase().includes(needle) ||
        f.title.toLowerCase().includes(needle),
    );
  }

  async function ensureMdFilesLoaded() {
    const root = $repo?.root;
    if (!root) {
      mdFiles = [];
      mdFilesLoadedFor = null;
      return;
    }
    if (mdFilesLoadedFor === root) return;
    try {
      mdFiles = await listMarkdownFiles(root);
      mdFilesLoadedFor = root;
    } catch (err) {
      // Don't blow up the viewer just because the picker can't load — log via
      // the inline error slot would be too noisy. Silent failure: button shows 0.
      console.warn('list_markdown_files failed:', err);
      mdFiles = [];
      mdFilesLoadedFor = root;
    }
  }

  async function togglePicker() {
    if (pickerOpen) {
      pickerOpen = false;
      return;
    }
    await ensureMdFilesLoaded();
    pickerOpen = true;
    pickerQuery = '';
    pickerHighlight = 0;
    await tick();
    pickerInput?.focus();
  }

  function pickFile(f: MarkdownFile) {
    pickerOpen = false;
    fileView.update((cur) => ({
      path: f.abs,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
  }

  function onPickerKeydown(ev: KeyboardEvent) {
    if (ev.key === 'Escape') {
      pickerOpen = false;
      ev.preventDefault();
      return;
    }
    if (ev.key === 'ArrowDown') {
      pickerHighlight = Math.min(pickerHighlight + 1, pickerFiltered.length - 1);
      ev.preventDefault();
    } else if (ev.key === 'ArrowUp') {
      pickerHighlight = Math.max(pickerHighlight - 1, 0);
      ev.preventDefault();
    } else if (ev.key === 'Enter') {
      const target = pickerFiltered[pickerHighlight];
      if (target) {
        pickFile(target);
        ev.preventDefault();
      }
    }
  }

  // Reset highlight whenever the filtered list changes shape.
  $: if (pickerHighlight >= pickerFiltered.length) pickerHighlight = 0;

  function onDocClick(ev: MouseEvent) {
    if (!pickerOpen) return;
    const target = ev.target as Node | null;
    if (!target) return;
    const popover = document.getElementById('md-picker-popover');
    const trigger = document.getElementById('md-picker-trigger');
    if (popover?.contains(target) || trigger?.contains(target)) return;
    pickerOpen = false;
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  $: if (path) void load(path);
  $: if (host && anchor !== null && nonce >= 0) void scrollToAnchor(anchor);

  async function load(p: string) {
    loading = true;
    error = null;
    html = '';
    content = '';
    toc = [];
    activeHeadingId = null;
    try {
      content = await readFileText(p);
      if (isMarkdown(p)) {
        html = await renderMarkdown(content);
        await tick();
        injectHeadingIds();
        toc = extractToc();
        await rewriteImages(p);
        await renderMermaidBlocks();
        // After DOM is settled, scroll to anchor if provided.
        await tick();
        if (anchor) await scrollToAnchor(anchor);
        else if (scroller) scroller.scrollTop = 0;
      } else {
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

  async function renderMarkdown(src: string): Promise<string> {
    marked.setOptions({ gfm: true, breaks: false });
    return await marked.parse(src);
  }

  /// Walk h1–h4 in render order and assign stable, unique slugs as IDs.
  function injectHeadingIds() {
    if (!host) return;
    const seen = new Map<string, number>();
    const headings = host.querySelectorAll<HTMLHeadingElement>('h1, h2, h3, h4');
    for (const h of Array.from(headings)) {
      const base = slugify(h.textContent ?? '');
      if (!base) continue;
      const n = seen.get(base) ?? 0;
      const id = n === 0 ? base : `${base}-${n}`;
      seen.set(base, n + 1);
      h.id = id;
    }
  }

  function extractToc(): TocEntry[] {
    if (!host) return [];
    const out: TocEntry[] = [];
    const headings = host.querySelectorAll<HTMLHeadingElement>('h1, h2, h3, h4');
    for (const h of Array.from(headings)) {
      if (!h.id) continue;
      out.push({
        id: h.id,
        text: h.textContent ?? h.id,
        level: Number(h.tagName.slice(1)),
      });
    }
    return out;
  }

  function slugify(s: string): string {
    return s
      .toLowerCase()
      .trim()
      .replace(/[^\p{Letter}\p{Number}\s-]/gu, '')
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

  async function scrollToAnchor(slug: string) {
    if (!host) return;
    const id = resolveAnchor(slug);
    if (!id) return;
    const el = host.querySelector<HTMLElement>(`#${cssEscape(id)}`);
    if (!el) return;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    activeHeadingId = id;
    // Brief highlight pulse so the user notices where we landed.
    el.classList.add('flash');
    setTimeout(() => el.classList.remove('flash'), 1200);
  }

  /// Match exact slug first, otherwise the first heading whose text contains
  /// the requested term (case-insensitive).
  function resolveAnchor(needle: string): string | null {
    const exact = toc.find((e) => e.id === needle);
    if (exact) return exact.id;
    const lower = needle.toLowerCase();
    const fuzzy = toc.find((e) => e.text.toLowerCase().includes(lower));
    return fuzzy?.id ?? null;
  }

  function cssEscape(id: string): string {
    if (typeof CSS !== 'undefined' && CSS.escape) return CSS.escape(id);
    return id.replace(/([^A-Za-z0-9_-])/g, '\\$1');
  }

  function onTocClick(id: string) {
    const el = host.querySelector<HTMLElement>(`#${cssEscape(id)}`);
    if (!el) return;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    activeHeadingId = id;
  }

  /// Update the TOC active item as the user scrolls. Picks the heading whose
  /// top is closest above the viewport top (with a small offset).
  function onScroll() {
    if (!scroller || toc.length === 0) return;
    const scrollerTop = scroller.getBoundingClientRect().top;
    const probe = scrollerTop + 40;
    let current: string | null = null;
    for (const entry of toc) {
      const el = host.querySelector<HTMLElement>(`#${cssEscape(entry.id)}`);
      if (!el) continue;
      if (el.getBoundingClientRect().top <= probe) current = entry.id;
      else break;
    }
    if (current !== activeHeadingId) activeHeadingId = current;
  }

  async function rewriteImages(filePath: string) {
    if (!host) return;
    const dir = parentDir(filePath);
    const imgs = host.querySelectorAll<HTMLImageElement>('img');
    for (const img of Array.from(imgs)) {
      const raw = img.getAttribute('src') ?? '';
      if (!raw || /^(?:[a-z]+:)?\/\//i.test(raw) || raw.startsWith('data:')) {
        continue;
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

  // ----- Zoom ---------------------------------------------------------------

  function clampZoom(z: number): number {
    return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 100) / 100));
  }

  function readZoom(): number {
    try {
      const v = parseFloat(localStorage.getItem('plaintext-ide.fileview.zoom') ?? '');
      if (Number.isFinite(v) && v > 0) return clampZoom(v);
    } catch {
      // localStorage unavailable — fine.
    }
    return 1.0;
  }

  function persistZoom(z: number) {
    try {
      localStorage.setItem('plaintext-ide.fileview.zoom', String(z));
    } catch {
      // ignore
    }
  }

  function setZoom(z: number) {
    zoom = clampZoom(z);
    persistZoom(zoom);
  }

  function zoomIn() {
    setZoom(zoom + ZOOM_STEP);
  }
  function zoomOut() {
    setZoom(zoom - ZOOM_STEP);
  }
  function zoomReset() {
    setZoom(1.0);
  }

  function onKey(ev: KeyboardEvent) {
    // Only intercept when this view is on screen.
    if (!scroller || !scroller.isConnected) return;
    const cmd = ev.metaKey || ev.ctrlKey;
    if (!cmd) return;
    // `=` and `+` share a key on most keyboards. `-` is Minus, `0` is Digit0.
    if (ev.key === '+' || ev.key === '=' || ev.code === 'Equal') {
      ev.preventDefault();
      zoomIn();
    } else if (ev.key === '-' || ev.code === 'Minus') {
      ev.preventDefault();
      zoomOut();
    } else if (ev.key === '0' || ev.code === 'Digit0') {
      ev.preventDefault();
      zoomReset();
    }
  }

  onMount(() => {
    if (path) void load(path);
    window.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDocClick);
    void ensureMdFilesLoaded();
  });

  onDestroy(() => {
    window.removeEventListener('keydown', onKey);
    document.removeEventListener('mousedown', onDocClick);
  });
</script>

<section class="root">
  <header class="bar">
    <span class="kind">{isMarkdown(path) ? 'markdown' : 'file'}</span>
    <code class="path" title={path}>{path}</code>
    <div class="spacer"></div>
    {#if mdFiles.length > 0}
      <div class="picker-wrap">
        <button
          id="md-picker-trigger"
          class="picker-trigger"
          class:active={pickerOpen}
          on:click={togglePicker}
          title="Open another markdown file in this project"
        >
          <span class="picker-icon">📄</span>
          Files
          <span class="picker-count">{mdFiles.length}</span>
          <span class="picker-caret">▾</span>
        </button>
        {#if pickerOpen}
          <div id="md-picker-popover" class="picker-popover" role="dialog">
            <input
              bind:this={pickerInput}
              bind:value={pickerQuery}
              on:keydown={onPickerKeydown}
              type="text"
              class="picker-search"
              placeholder="Search markdown files…"
              autocomplete="off"
              spellcheck="false"
            />
            {#if pickerFiltered.length === 0}
              <div class="picker-empty">No matches</div>
            {:else}
              <ul class="picker-list">
                {#each pickerFiltered as f, i (f.abs)}
                  <li>
                    <button
                      type="button"
                      class="picker-item"
                      class:current={f.abs === path}
                      class:highlight={i === pickerHighlight}
                      on:click={() => pickFile(f)}
                      on:mouseenter={() => (pickerHighlight = i)}
                    >
                      <span class="picker-title">{f.title}</span>
                      <span class="picker-path">{f.rel}</span>
                      <span class="picker-size">{fmtSize(f.size)}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
            <div class="picker-foot">
              {pickerFiltered.length} / {mdFiles.length} • ↑↓ navigate • Enter open • Esc close
            </div>
          </div>
        {/if}
      </div>
    {/if}
    <div class="zoom" title="Zoom: Cmd/Ctrl + / − / 0">
      <button class="zoom-btn" on:click={zoomOut} aria-label="Zoom out">−</button>
      <button class="zoom-pct" on:click={zoomReset} aria-label="Reset zoom"
        >{Math.round(zoom * 100)}%</button
      >
      <button class="zoom-btn" on:click={zoomIn} aria-label="Zoom in">+</button>
    </div>
  </header>
  {#if loading}
    <div class="status">Loading…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
  {:else}
    <div class="layout" class:has-toc={toc.length > 0}>
      {#if toc.length > 0}
        <aside class="toc" aria-label="Table of contents">
          <div class="toc-title">On this page</div>
          <ul>
            {#each toc as t (t.id)}
              <li class="lvl-{t.level}" class:active={activeHeadingId === t.id}>
                <button type="button" on:click={() => onTocClick(t.id)} title={t.text}>
                  {t.text}
                </button>
              </li>
            {/each}
          </ul>
        </aside>
      {/if}
      <div
        class="scroller"
        bind:this={scroller}
        on:scroll={onScroll}
      >
        <div
          class="content"
          bind:this={host}
          style="font-size: {zoom}em;"
        >
          {@html html}
        </div>
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

  .spacer {
    flex: 1;
  }

  .picker-wrap {
    position: relative;
  }

  .picker-trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-1);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .picker-trigger:hover,
  .picker-trigger.active {
    background: var(--bg-3);
    color: var(--fg-0);
    border-color: var(--accent-2);
  }
  .picker-icon {
    font-size: 13px;
  }
  .picker-count {
    background: var(--bg-1);
    border-radius: 10px;
    padding: 1px 6px;
    font-size: 10px;
    color: var(--fg-2);
    font-variant-numeric: tabular-nums;
  }
  .picker-caret {
    font-size: 9px;
    color: var(--fg-2);
  }

  .picker-popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    width: 460px;
    max-height: 70vh;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    display: flex;
    flex-direction: column;
    z-index: 100;
    overflow: hidden;
  }

  .picker-search {
    padding: 10px 12px;
    background: var(--bg-0);
    border: 0;
    border-bottom: 1px solid var(--bg-3);
    color: var(--fg-0);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .picker-search:focus {
    background: var(--bg-1);
  }

  .picker-list {
    list-style: none;
    margin: 0;
    padding: 4px 0;
    overflow-y: auto;
    flex: 1;
  }

  .picker-item {
    width: 100%;
    text-align: left;
    background: transparent;
    border: 0;
    border-left: 2px solid transparent;
    padding: 6px 12px;
    font: inherit;
    color: var(--fg-1);
    cursor: pointer;
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    column-gap: 8px;
  }
  .picker-item:hover,
  .picker-item.highlight {
    background: var(--bg-2);
    color: var(--fg-0);
  }
  .picker-item.current {
    border-left-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 12%, transparent);
  }
  .picker-title {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .picker-size {
    font-size: 10px;
    color: var(--fg-2);
    font-variant-numeric: tabular-nums;
    align-self: center;
  }
  .picker-path {
    grid-column: 1 / -1;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .picker-empty {
    padding: 24px;
    text-align: center;
    color: var(--fg-2);
    font-size: 12px;
  }

  .picker-foot {
    padding: 6px 12px;
    border-top: 1px solid var(--bg-3);
    font-size: 10px;
    color: var(--fg-2);
    background: var(--bg-2);
    font-variant-numeric: tabular-nums;
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

  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .toc {
    width: 240px;
    flex-shrink: 0;
    overflow-y: auto;
    background: var(--bg-1);
    border-right: 1px solid var(--bg-3);
    padding: 16px 8px 24px;
    font-size: 12px;
  }

  .toc-title {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-size: 10px;
    color: var(--fg-2);
    padding: 0 8px 8px;
  }

  .toc ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .toc li {
    margin: 0;
  }

  .toc li button {
    width: 100%;
    text-align: left;
    background: transparent;
    border: 0;
    border-left: 2px solid transparent;
    padding: 3px 8px;
    font: inherit;
    color: var(--fg-2);
    cursor: pointer;
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toc li button:hover {
    background: var(--bg-2);
    color: var(--fg-0);
  }
  .toc li.active button {
    color: var(--accent-2);
    border-left-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 12%, transparent);
  }

  .toc li.lvl-1 button { padding-left: 8px; font-weight: 600; }
  .toc li.lvl-2 button { padding-left: 16px; }
  .toc li.lvl-3 button { padding-left: 28px; font-size: 11px; }
  .toc li.lvl-4 button { padding-left: 40px; font-size: 11px; color: var(--fg-2); }

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

  /* Markdown styling */
  .content :global(h1),
  .content :global(h2),
  .content :global(h3),
  .content :global(h4) {
    color: var(--fg-0);
    margin-top: 1.4em;
    margin-bottom: 0.5em;
    font-weight: 600;
    scroll-margin-top: 12px;
  }
  .content :global(h1) { font-size: 1.7em; border-bottom: 1px solid var(--bg-3); padding-bottom: 6px; }
  .content :global(h2) { font-size: 1.35em; border-bottom: 1px solid var(--bg-3); padding-bottom: 4px; }
  .content :global(h3) { font-size: 1.15em; }

  .content :global(h1.flash),
  .content :global(h2.flash),
  .content :global(h3.flash),
  .content :global(h4.flash) {
    background: color-mix(in srgb, var(--accent-2) 25%, transparent);
    border-radius: 3px;
    transition: background 1s ease;
  }

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

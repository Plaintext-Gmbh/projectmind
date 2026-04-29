<script lang="ts">
  import {
    listHtmlFiles,
    findHtmlSnippets,
    readFileText,
  } from '../lib/api';
  import type { HtmlFile, HtmlSnippet } from '../lib/api';
  import { repo } from '../lib/store';
  import { resizable } from '../lib/resizable';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  // Shift + wheel zoom for the rendered iframe / source pre. Scoped to the
  // viewer column via `use:zoomAction` so shift-scrolling the sidebar doesn't
  // resize the doc.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.htmlindex.zoom');

  type Tab = 'files' | 'snippets';
  type RenderMode = 'rendered' | 'source';

  let tab: Tab = 'files';
  let renderMode: RenderMode = 'rendered';

  let files: HtmlFile[] = [];
  let snippets: HtmlSnippet[] = [];
  let loadedFor: string | null = null;
  let loading = false;
  let error: string | null = null;
  let query = '';

  /// Currently selected file or snippet. We key files by `abs` and snippets by
  /// `${abs}:${line}` so the two pools never collide.
  let selectedKey: string | null = null;
  let selectedFile: HtmlFile | null = null;
  let selectedSnippet: HtmlSnippet | null = null;
  let selectedSource = '';
  let detailLoading = false;
  let detailError: string | null = null;

  $: filteredFiles = filterFiles(files, query);
  $: filteredSnippets = filterSnippets(snippets, query);
  $: void load($repo?.root ?? null);

  function filterFiles(list: HtmlFile[], q: string): HtmlFile[] {
    if (!q.trim()) return list;
    const needle = q.toLowerCase();
    return list.filter((f) => f.rel.toLowerCase().includes(needle));
  }

  function filterSnippets(list: HtmlSnippet[], q: string): HtmlSnippet[] {
    if (!q.trim()) return list;
    const needle = q.toLowerCase();
    return list.filter(
      (s) =>
        s.rel.toLowerCase().includes(needle) ||
        s.content.toLowerCase().includes(needle),
    );
  }

  async function load(root: string | null) {
    if (!root) {
      files = [];
      snippets = [];
      loadedFor = null;
      clearSelection();
      return;
    }
    if (loadedFor === root) return;
    loading = true;
    error = null;
    try {
      const [f, s] = await Promise.all([
        listHtmlFiles(root),
        findHtmlSnippets(root),
      ]);
      files = f;
      snippets = s;
      loadedFor = root;
      clearSelection();
    } catch (err) {
      error = String(err);
      files = [];
      snippets = [];
    } finally {
      loading = false;
    }
  }

  function clearSelection() {
    selectedKey = null;
    selectedFile = null;
    selectedSnippet = null;
    selectedSource = '';
    detailError = null;
  }

  async function openFile(f: HtmlFile) {
    selectedKey = f.abs;
    selectedFile = f;
    selectedSnippet = null;
    selectedSource = '';
    detailLoading = true;
    detailError = null;
    try {
      selectedSource = await readFileText(f.abs);
    } catch (err) {
      detailError = String(err);
    } finally {
      detailLoading = false;
    }
  }

  function openSnippet(s: HtmlSnippet) {
    selectedKey = `${s.abs}:${s.line}`;
    selectedFile = null;
    selectedSnippet = s;
    selectedSource = s.content;
    detailLoading = false;
    detailError = null;
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  // The iframe must render with no script execution and no network access.
  // We wrap the snippet/file source in a strict CSP to enforce that even if
  // the source itself contains <script> tags or remote <img src>.
  function safeWrap(source: string): string {
    const csp =
      "default-src 'none'; img-src data:; style-src 'unsafe-inline' data:;" +
      " font-src data:; media-src data:; child-src 'none'; frame-src 'none';" +
      " form-action 'none'; base-uri 'none';";
    // If the source already has a full <html> document, inject our CSP into
    // its <head>. Otherwise wrap as a fragment.
    const hasHtmlTag = /<html[\s>]/i.test(source);
    if (hasHtmlTag) {
      // Simplistic injection: insert a <meta> right after <head> open. If
      // there's no <head>, the strict iframe sandbox still blocks scripts.
      return source.replace(
        /<head([^>]*)>/i,
        `<head$1><meta http-equiv="Content-Security-Policy" content="${csp}">`,
      );
    }
    return `<!doctype html>
<html>
<head>
<meta http-equiv="Content-Security-Policy" content="${csp}">
<style>
  body { font-family: system-ui, sans-serif; color: #222; background: #fff; padding: 16px; }
</style>
</head>
<body>${source}</body>
</html>`;
  }

  $: iframeSrc =
    selectedSource && renderMode === 'rendered'
      ? `data:text/html;charset=utf-8,${encodeURIComponent(safeWrap(selectedSource))}`
      : '';

  function isRenderable(): boolean {
    if (selectedFile) {
      return (
        selectedFile.kind === 'html' ||
        selectedFile.kind === 'xhtml' ||
        selectedFile.kind === 'jsp'
      );
    }
    return selectedSnippet !== null;
  }

  function topDir(rel: string): string {
    const idx = rel.indexOf('/');
    return idx === -1 ? '·' : rel.slice(0, idx);
  }

  $: groupedFiles = groupFiles(filteredFiles);
  $: groupedSnippets = groupSnippets(filteredSnippets);

  function groupFiles(list: HtmlFile[]): Array<[string, HtmlFile[]]> {
    const map = new Map<string, HtmlFile[]>();
    for (const f of list) {
      const key = topDir(f.rel);
      const arr = map.get(key) ?? [];
      arr.push(f);
      map.set(key, arr);
    }
    return Array.from(map.entries()).sort((a, b) => {
      if (a[0] === '·' && b[0] !== '·') return -1;
      if (b[0] === '·' && a[0] !== '·') return 1;
      return a[0].localeCompare(b[0]);
    });
  }

  function groupSnippets(list: HtmlSnippet[]): Array<[string, HtmlSnippet[]]> {
    const map = new Map<string, HtmlSnippet[]>();
    for (const s of list) {
      const arr = map.get(s.rel) ?? [];
      arr.push(s);
      map.set(s.rel, arr);
    }
    return Array.from(map.entries()).sort((a, b) => a[0].localeCompare(b[0]));
  }

  function snippetPreview(content: string): string {
    const compact = content.replace(/\s+/g, ' ').trim();
    return compact.length > 80 ? compact.slice(0, 77) + '…' : compact;
  }

</script>

<section class="root">
  <header class="bar">
    <div class="title-block">
      <h2>HTML</h2>
      {#if $repo}
        <span class="subtitle">
          {files.length} files · {snippets.length} snippets in {$repo.root}
        </span>
      {:else}
        <span class="subtitle">no repository open</span>
      {/if}
    </div>
    <div class="tabs">
      <button class:active={tab === 'files'} on:click={() => (tab = 'files')}>
        Files <span class="count">{filteredFiles.length}</span>
      </button>
      <button class:active={tab === 'snippets'} on:click={() => (tab = 'snippets')}>
        Snippets <span class="count">{filteredSnippets.length}</span>
      </button>
    </div>
    <input
      type="text"
      class="search"
      bind:value={query}
      placeholder="Search path or content…"
      autocomplete="off"
      spellcheck="false"
      disabled={!$repo}
    />
  </header>

  <div class="layout">
    <aside class="sidebar">
      {#if !$repo}
        <div class="empty">Open a repository to see HTML files and snippets.</div>
      {:else if loading}
        <div class="empty">Scanning…</div>
      {:else if error}
        <div class="error">⚠ {error}</div>
      {:else if tab === 'files'}
        {#if filteredFiles.length === 0}
          <div class="empty">{files.length === 0 ? 'No HTML/XHTML/JSP files found.' : 'No matches.'}</div>
        {:else}
          {#each groupedFiles as [dir, entries] (dir)}
            <section class="group">
              <h3 class="group-title">{dir === '·' ? '(root)' : dir}</h3>
              <ul class="list">
                {#each entries as f (f.abs)}
                  <li>
                    <button
                      type="button"
                      class="item"
                      class:selected={selectedKey === f.abs}
                      on:click={() => openFile(f)}
                    >
                      <span class="item-title">{f.rel.split('/').pop()}</span>
                      <span class="item-meta">
                        <span class="kind">{f.kind}</span>
                        <span class="size">{fmtSize(f.size)}</span>
                      </span>
                      <span class="item-path">{f.rel}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            </section>
          {/each}
        {/if}
      {:else if filteredSnippets.length === 0}
        <div class="empty">{snippets.length === 0 ? 'No HTML snippets found in source files.' : 'No matches.'}</div>
      {:else}
        {#each groupedSnippets as [rel, entries] (rel)}
          <section class="group">
            <h3 class="group-title">{rel}</h3>
            <ul class="list">
              {#each entries as s (`${s.abs}:${s.line}`)}
                <li>
                  <button
                    type="button"
                    class="item"
                    class:selected={selectedKey === `${s.abs}:${s.line}`}
                    on:click={() => openSnippet(s)}
                  >
                    <span class="item-title">line {s.line}</span>
                    <span class="item-meta">
                      <span class="kind">{s.lang}</span>
                      <span class="size">{s.tag_count} tags</span>
                    </span>
                    <span class="item-preview">{snippetPreview(s.content)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      {/if}
    </aside>

    <div
      class="resizer"
      use:resizable={{
        storageKey: 'projectmind.layout.html.col1',
        cssVar: '--html-col-1',
        min: 220,
        max: 720,
        initial: 360,
      }}
      title="Drag to resize · double-click to reset"
    ></div>

    <main class="viewer" use:zoomAction>
      {#if !selectedFile && !selectedSnippet}
        <div class="placeholder">Select a file or snippet on the left.</div>
      {:else}
        <div class="viewer-bar">
          <div class="viewer-title">
            {#if selectedFile}
              <span class="vt-name">{selectedFile.rel}</span>
              <span class="vt-meta">{selectedFile.kind} · {fmtSize(selectedFile.size)}</span>
            {:else if selectedSnippet}
              <span class="vt-name">{selectedSnippet.rel}:{selectedSnippet.line}</span>
              <span class="vt-meta">{selectedSnippet.lang} snippet · {selectedSnippet.tag_count} tags</span>
            {/if}
          </div>
          <div class="mode-tabs">
            <button
              class:active={renderMode === 'rendered'}
              disabled={!isRenderable()}
              title={isRenderable() ? '' : 'This file type is not rendered as HTML'}
              on:click={() => (renderMode = 'rendered')}
            >
              Rendered
            </button>
            <button
              class:active={renderMode === 'source'}
              on:click={() => (renderMode = 'source')}
            >
              Source
            </button>
          </div>
        </div>
        <div class="viewer-body">
          {#if detailLoading}
            <div class="empty">Loading…</div>
          {:else if detailError}
            <div class="error">⚠ {detailError}</div>
          {:else if renderMode === 'rendered' && isRenderable()}
            <iframe
              class="render-frame"
              title="HTML preview"
              sandbox=""
              src={iframeSrc}
              style="zoom: {$zoom};"
            ></iframe>
          {:else}
            <pre class="source" style="font-size: {12.5 * $zoom}px;"><code
              >{selectedSource}</code
            ></pre>
          {/if}
        </div>
      {/if}
    </main>
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
    gap: 16px;
    padding: 10px 24px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .title-block {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title-block h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--fg-0);
  }
  .subtitle {
    font-size: 11px;
    color: var(--fg-2);
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 460px;
  }

  .tabs {
    display: flex;
    gap: 6px;
  }
  .tabs button {
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    color: var(--fg-1);
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
  }
  .tabs button.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  .tabs .count {
    color: var(--fg-2);
    font-family: var(--mono);
    margin-left: 4px;
  }

  .search {
    margin-left: auto;
    flex-basis: 320px;
    background: var(--bg-0);
    border: 1px solid var(--bg-3);
    color: var(--fg-0);
    padding: 6px 10px;
    border-radius: 4px;
    font: inherit;
    font-size: 13px;
  }
  .search:focus {
    outline: none;
    border-color: var(--accent-2);
  }
  .search:disabled {
    opacity: 0.5;
  }

  .layout {
    display: grid;
    grid-template-columns: var(--html-col-1, 360px) 6px 1fr;
    flex: 1;
    overflow: hidden;
  }

  .resizer {
    background: transparent;
    cursor: col-resize;
    position: relative;
    z-index: 1;
    transition: background 80ms ease;
  }
  .resizer::after {
    content: '';
    position: absolute;
    inset: 0;
    border-left: 1px solid var(--bg-3);
  }
  .resizer:hover,
  .resizer:global(.dragging) {
    background: color-mix(in srgb, var(--accent-2) 25%, transparent);
  }

  .sidebar {
    background: var(--bg-1);
    border-right: 1px solid var(--bg-3);
    overflow-y: auto;
    padding: 12px 12px 32px;
  }

  .empty,
  .error {
    color: var(--fg-2);
    padding: 24px 4px;
    text-align: center;
    font-size: 13px;
  }
  .error {
    color: var(--error);
  }

  .group {
    margin-bottom: 16px;
  }

  .group-title {
    margin: 0 0 6px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    font-weight: 600;
    font-family: var(--mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .item {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    gap: 2px 8px;
    width: 100%;
    text-align: left;
    background: var(--bg-2);
    border: 1px solid transparent;
    border-left: 3px solid transparent;
    border-radius: 4px;
    padding: 6px 10px;
    color: var(--fg-1);
    cursor: pointer;
    font: inherit;
  }
  .item:hover {
    background: color-mix(in srgb, var(--accent-2) 10%, var(--bg-2));
    border-left-color: var(--accent-2);
  }
  .item.selected {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-1));
    border-color: var(--accent-2);
  }

  .item-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-meta {
    display: flex;
    gap: 6px;
    font-size: 10px;
    color: var(--fg-2);
    align-self: center;
  }
  .item-meta .kind {
    background: var(--bg-3);
    padding: 1px 5px;
    border-radius: 8px;
    font-family: var(--mono);
  }

  .item-path,
  .item-preview {
    grid-column: 1 / -1;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .viewer {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-0);
  }

  .placeholder {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-2);
  }

  .viewer-bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .viewer-title {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    overflow: hidden;
  }
  .vt-name {
    font-family: var(--mono);
    font-size: 13px;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .vt-meta {
    font-size: 11px;
    color: var(--fg-2);
  }

  .mode-tabs {
    display: flex;
    gap: 4px;
  }
  .mode-tabs button {
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    color: var(--fg-1);
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
  }
  .mode-tabs button.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  .mode-tabs button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .viewer-body {
    flex: 1;
    overflow: hidden;
    display: flex;
  }

  .render-frame {
    flex: 1;
    width: 100%;
    height: 100%;
    border: 0;
    background: white;
  }

  .source {
    flex: 1;
    margin: 0;
    padding: 16px 20px;
    background: var(--bg-0);
    color: var(--fg-0);
    font-family: var(--mono);
    font-size: 12.5px;
    line-height: 1.5;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { listMarkdownFiles } from '../lib/api';
  import type { MarkdownFile } from '../lib/api';
  import { repo, fileView, viewMode } from '../lib/store';

  let files: MarkdownFile[] = [];
  let loadedFor: string | null = null;
  let loading = false;
  let error: string | null = null;
  let query = '';

  $: filtered = filterFiles(files, query);
  $: void load($repo?.root ?? null);

  function filterFiles(list: MarkdownFile[], q: string): MarkdownFile[] {
    if (!q.trim()) return list;
    const needle = q.toLowerCase();
    return list.filter(
      (f) =>
        f.rel.toLowerCase().includes(needle) ||
        f.title.toLowerCase().includes(needle),
    );
  }

  async function load(root: string | null) {
    if (!root) {
      files = [];
      loadedFor = null;
      return;
    }
    if (loadedFor === root) return;
    loading = true;
    error = null;
    try {
      files = await listMarkdownFiles(root);
      loadedFor = root;
    } catch (err) {
      error = String(err);
      files = [];
    } finally {
      loading = false;
    }
  }

  function open(f: MarkdownFile) {
    fileView.update((cur) => ({
      path: f.abs,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set('file');
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  function topDir(rel: string): string {
    const idx = rel.indexOf('/');
    return idx === -1 ? '·' : rel.slice(0, idx);
  }

  // Group entries by their top-level directory for a tidier overview.
  $: grouped = groupByDir(filtered);

  function groupByDir(list: MarkdownFile[]): Array<[string, MarkdownFile[]]> {
    const map = new Map<string, MarkdownFile[]>();
    for (const f of list) {
      const key = topDir(f.rel);
      const arr = map.get(key) ?? [];
      arr.push(f);
      map.set(key, arr);
    }
    return Array.from(map.entries()).sort((a, b) => {
      // Root-level files (·) bubble up first, then alphabetical.
      if (a[0] === '·' && b[0] !== '·') return -1;
      if (b[0] === '·' && a[0] !== '·') return 1;
      return a[0].localeCompare(b[0]);
    });
  }

  onMount(() => {
    void load($repo?.root ?? null);
  });
</script>

<section class="root">
  <header class="bar">
    <div class="title-block">
      <h2>Markdown</h2>
      {#if $repo}
        <span class="subtitle">{files.length} files in {$repo.root}</span>
      {:else}
        <span class="subtitle">no repository open</span>
      {/if}
    </div>
    <input
      type="text"
      class="search"
      bind:value={query}
      placeholder="Search title or path…"
      autocomplete="off"
      spellcheck="false"
      disabled={!$repo || files.length === 0}
    />
  </header>

  <div class="body">
    {#if !$repo}
      <div class="empty">Open a repository to see its markdown files.</div>
    {:else if loading}
      <div class="empty">Scanning…</div>
    {:else if error}
      <div class="error">⚠ {error}</div>
    {:else if files.length === 0}
      <div class="empty">No markdown files found in this repository.</div>
    {:else if filtered.length === 0}
      <div class="empty">No matches for &ldquo;{query}&rdquo;.</div>
    {:else}
      {#each grouped as [dir, entries] (dir)}
        <section class="group">
          <h3 class="group-title">{dir === '·' ? '(root)' : dir}</h3>
          <ul class="list">
            {#each entries as f (f.abs)}
              <li>
                <button type="button" class="item" on:click={() => open(f)}>
                  <span class="item-title">{f.title}</span>
                  <span class="item-size">{fmtSize(f.size)}</span>
                  <span class="item-path">{f.rel}</span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
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
    gap: 16px;
    padding: 12px 24px;
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

  .body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px 48px;
  }

  .empty,
  .error {
    color: var(--fg-2);
    padding: 32px 0;
    text-align: center;
    font-size: 13px;
  }
  .error {
    color: var(--error);
  }

  .group {
    margin-bottom: 20px;
  }

  .group-title {
    margin: 0 0 6px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    font-weight: 600;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 8px;
  }

  .item {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    gap: 4px 8px;
    width: 100%;
    text-align: left;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-left: 3px solid var(--bg-3);
    border-radius: 4px;
    padding: 10px 14px;
    color: var(--fg-1);
    cursor: pointer;
    font: inherit;
    transition: background 100ms ease, border-color 100ms ease;
  }
  .item:hover {
    background: var(--bg-2);
    border-left-color: var(--accent-2);
    color: var(--fg-0);
  }

  .item-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-size {
    font-size: 10px;
    color: var(--fg-2);
    font-variant-numeric: tabular-nums;
    align-self: center;
  }
  .item-path {
    grid-column: 1 / -1;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

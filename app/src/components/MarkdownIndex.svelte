<script lang="ts">
  import { searchMarkdown } from '../lib/api';
  import type { MarkdownFile, MarkdownHit } from '../lib/api';
  import { repo, fileView, viewMode, modules, moduleFilter } from '../lib/store';
  import { t } from '../lib/i18n';
  import { createShiftWheelZoom } from '../lib/shiftWheelZoom';

  let hits: MarkdownHit[] = [];
  let loadedFor: string | null = null;
  let loading = false;
  let searching = false;
  let error: string | null = null;
  let query = '';
  let searchSeq = 0;
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  /// Reload (empty query) whenever the repo root changes.
  $: void load($repo?.root ?? null);
  /// Re-run the fuzzy search whenever the query changes.
  $: scheduleSearch(query, $repo?.root ?? null);

  async function load(root: string | null) {
    if (!root) {
      hits = [];
      loadedFor = null;
      return;
    }
    if (loadedFor === root) return;
    loading = true;
    error = null;
    try {
      hits = await searchMarkdown(root, '', 500);
      loadedFor = root;
    } catch (err) {
      error = String(err);
      hits = [];
    } finally {
      loading = false;
    }
  }

  function scheduleSearch(q: string, root: string | null) {
    if (!root || loadedFor !== root) return;
    if (searchTimer) clearTimeout(searchTimer);
    const seq = ++searchSeq;
    searchTimer = setTimeout(async () => {
      searching = true;
      try {
        const result = await searchMarkdown(root, q, q.trim() ? 200 : 500);
        if (seq !== searchSeq) return;
        hits = result;
        error = null;
      } catch (err) {
        if (seq !== searchSeq) return;
        error = String(err);
      } finally {
        if (seq === searchSeq) searching = false;
      }
    }, 80);
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

  /// When a module is selected in the modules sidebar, narrow the index to
  /// markdown files that live under that module's root. Lookup happens against
  /// `$modules` so we can match the module id back to its absolute root path.
  $: selectedModuleRoot = pickModuleRoot($moduleFilter, $modules);
  $: visibleHits = filterByModuleRoot(hits, selectedModuleRoot);

  function pickModuleRoot(
    moduleId: string | null,
    mods: Array<{ id: string; root: string }>,
  ): string | null {
    if (!moduleId) return null;
    return mods.find((m) => m.id === moduleId)?.root ?? null;
  }

  function filterByModuleRoot(list: MarkdownHit[], root: string | null): MarkdownHit[] {
    if (!root) return list;
    const prefix = root.endsWith('/') ? root : `${root}/`;
    return list.filter((h) => h.file.abs === root || h.file.abs.startsWith(prefix));
  }

  /// Group hits when no query is active (browsing mode); flat scored list when
  /// a query is in flight (relevance order matters more than directory).
  $: grouped = query.trim() ? null : groupByDir(visibleHits);

  function groupByDir(list: MarkdownHit[]): Array<[string, MarkdownHit[]]> {
    const map = new Map<string, MarkdownHit[]>();
    for (const h of list) {
      const key = topDir(h.file.rel);
      const arr = map.get(key) ?? [];
      arr.push(h);
      map.set(key, arr);
    }
    return Array.from(map.entries()).sort((a, b) => {
      if (a[0] === '·' && b[0] !== '·') return -1;
      if (b[0] === '·' && a[0] !== '·') return 1;
      return a[0].localeCompare(b[0]);
    });
  }

  // Shift + wheel zoom, persisted under the per-component key.
  const { zoom, action: zoomAction } = createShiftWheelZoom('projectmind.mdindex.zoom');
</script>

<section class="root" use:zoomAction style="font-size: {$zoom}em;">
  <header class="bar">
    <div class="title-block">
      <h2>{$t('markdown.title')}</h2>
      {#if $repo}
        <span class="subtitle">
          {#if query.trim()}
            {$t('markdown.hitsIn', { count: visibleHits.length, root: $repo.root })}
          {:else}
            {$t('markdown.filesIn', { count: visibleHits.length, root: $repo.root })}
          {/if}
        </span>
        {#if $moduleFilter}
          <button
            class="module-chip"
            on:click={() => moduleFilter.set(null)}
            title={$t('index.moduleFilter.clear')}
          >
            <span class="chip-label">{$moduleFilter}</span>
            <span class="chip-x" aria-hidden="true">×</span>
          </button>
        {/if}
      {:else}
        <span class="subtitle">{$t('markdown.noRepositoryOpen')}</span>
      {/if}
    </div>
    <input
      type="text"
      class="search"
      bind:value={query}
      placeholder={$t('markdown.searchPlaceholder')}
      autocomplete="off"
      spellcheck="false"
      disabled={!$repo || (loadedFor !== null && visibleHits.length === 0 && !query.trim())}
    />
    {#if searching}
      <span class="searching" aria-live="polite">{$t('markdown.searching')}</span>
    {/if}
  </header>

  <div class="body">
    {#if !$repo}
      <div class="empty">{$t('markdown.openRepo')}</div>
    {:else if loading}
      <div class="empty">{$t('markdown.scanning')}</div>
    {:else if error}
      <div class="error">⚠ {error}</div>
    {:else if visibleHits.length === 0 && !query.trim()}
      <div class="empty">{$t('markdown.noFiles')}</div>
    {:else if visibleHits.length === 0}
      <div class="empty">{$t('markdown.noMatchesFor', { query })}</div>
    {:else if grouped}
      {#each grouped as [dir, entries] (dir)}
        <section class="group">
          <h3 class="group-title">{dir === '·' ? $t('markdown.root') : dir}</h3>
          <ul class="list">
            {#each entries as h (h.file.abs)}
              <li>
                <button type="button" class="item" on:click={() => open(h.file)}>
                  <span class="item-title">{h.file.title}</span>
                  <span class="item-size">{fmtSize(h.file.size)}</span>
                  <span class="item-path">{h.file.rel}</span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    {:else}
      <ul class="list flat">
        {#each visibleHits as h (h.file.abs)}
          <li>
            <button type="button" class="item" on:click={() => open(h.file)}>
              <span class="item-title">{h.file.title}</span>
              <span class="match-kind {h.matched_in}">{h.matched_in}</span>
              <span class="item-path">{h.file.rel}</span>
              {#if h.snippet}
                <span class="item-snippet">{h.snippet}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
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

  .module-chip {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: 2px;
    padding: 1px 6px 1px 8px;
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-2));
    border: 1px solid color-mix(in srgb, var(--accent-2) 40%, var(--bg-3));
    border-radius: 10px;
    color: var(--fg-0);
    font-family: var(--mono);
    font-size: 11px;
    cursor: pointer;
  }
  .module-chip:hover {
    border-color: var(--accent-2);
  }
  .module-chip .chip-x {
    color: var(--fg-2);
    font-size: 13px;
    line-height: 1;
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

  .searching {
    font-size: 11px;
    color: var(--fg-2);
    font-style: italic;
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
  .list.flat {
    grid-template-columns: 1fr;
  }

  .item {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-auto-rows: auto;
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
  .item-snippet {
    grid-column: 1 / -1;
    font-size: 11.5px;
    color: var(--fg-1);
    line-height: 1.45;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .match-kind {
    align-self: center;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 6px;
    border-radius: 8px;
    background: var(--bg-2);
    color: var(--fg-2);
    font-family: var(--mono);
  }
  .match-kind.title {
    background: color-mix(in srgb, var(--accent-2) 25%, var(--bg-1));
    color: var(--accent-2);
  }
  .match-kind.content {
    background: color-mix(in srgb, var(--warn) 22%, var(--bg-1));
    color: var(--warn);
  }
</style>

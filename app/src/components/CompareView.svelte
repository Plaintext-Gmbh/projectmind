<script lang="ts">
  import { onMount } from 'svelte';
  import { listChangesSince, listRefs } from '../lib/api';
  import type { ChangedFile, GitRef } from '../lib/api';
  import { compareRefs, repo } from '../lib/store';
  import { t } from '../lib/i18n';
  import RefPicker from './RefPicker.svelte';
  import DiffView from './DiffView.svelte';
  import DiagramView from './DiagramView.svelte';

  /// Compare tab state. The store seeds default refs (master → HEAD when
  /// available) on first activation; switching repos resets it.
  let refs: GitRef[] = [];
  let loadingRefs = false;
  let refsError: string | null = null;
  let refsRoot: string | null = null;

  type TabKind = 'files' | 'diff' | 'architecture';
  let tab: TabKind = 'files';

  let changes: ChangedFile[] = [];
  let loadingChanges = false;
  let changesError: string | null = null;
  let changesKey = '';

  $: from = $compareRefs?.from ?? '';
  $: to = $compareRefs?.to ?? '';
  $: bothPicked = Boolean(from && to);

  /// Re-fetch refs when the repo changes. `repo.root` is the cache key —
  /// the seed (master → HEAD) is recomputed from the fresh ref list.
  $: if ($repo?.root && $repo.root !== refsRoot) {
    void loadRefs($repo.root);
  }

  async function loadRefs(root: string) {
    loadingRefs = true;
    refsError = null;
    try {
      refs = await listRefs();
      refsRoot = root;
      if (!$compareRefs) {
        compareRefs.set(defaultPair(refs));
      }
    } catch (err) {
      refsError = `${$t('compare.errLoading')}: ${err}`;
      refs = [];
    } finally {
      loadingRefs = false;
    }
  }

  function defaultPair(list: GitRef[]) {
    // Pick the canonical base (master/main if available, else the first
    // branch) and aim it at HEAD so the user immediately sees "what's on
    // my working tree compared to mainline".
    const branches = list.filter((r) => r.kind === 'branch');
    const base =
      branches.find((b) => b.name === 'master' || b.name === 'main') ??
      branches[0] ??
      null;
    if (!base) return null;
    return { from: base.name, to: 'HEAD' };
  }

  $: if (bothPicked && tab === 'files') {
    void loadChanges(from, to);
  }

  async function loadChanges(fromRef: string, toRef: string) {
    const key = `${fromRef}..${toRef}`;
    if (key === changesKey && !changesError) return;
    loadingChanges = true;
    changesError = null;
    try {
      changes = await listChangesSince(fromRef, toRef);
      changesKey = key;
    } catch (err) {
      changesError = String(err);
      changes = [];
      changesKey = key;
    } finally {
      loadingChanges = false;
    }
  }

  function pickFrom(name: string) {
    compareRefs.update((cur) => ({ from: name, to: cur?.to ?? 'HEAD' }));
  }
  function pickTo(name: string) {
    compareRefs.update((cur) => ({ from: cur?.from ?? '', to: name }));
  }
  function swap() {
    compareRefs.update((cur) => (cur ? { from: cur.to, to: cur.from } : cur));
  }

  function statusLabel(s: ChangedFile['status']): string {
    switch (s) {
      case 'added': return 'A';
      case 'modified': return 'M';
      case 'deleted': return 'D';
      case 'renamed': return 'R';
      case 'type_change': return 'T';
      default: return '?';
    }
  }

  onMount(() => {
    if ($repo?.root) void loadRefs($repo.root);
  });
</script>

<section class="root">
  <header class="bar">
    <RefPicker
      label={$t('compare.from')}
      value={from}
      {refs}
      disabled={loadingRefs}
      on:change={(e) => pickFrom(e.detail)}
    />
    <button
      type="button"
      class="swap"
      title={$t('compare.swap')}
      on:click={swap}
      disabled={!bothPicked}
    >↔</button>
    <RefPicker
      label={$t('compare.to')}
      value={to}
      {refs}
      disabled={loadingRefs}
      on:change={(e) => pickTo(e.detail)}
    />
    <span class="spacer"></span>
    <div class="tabs" role="tablist">
      <button
        type="button"
        role="tab"
        aria-selected={tab === 'files'}
        class:active={tab === 'files'}
        on:click={() => (tab = 'files')}
      >{$t('compare.tabFiles')}</button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === 'diff'}
        class:active={tab === 'diff'}
        on:click={() => (tab = 'diff')}
      >{$t('compare.tabDiff')}</button>
      <button
        type="button"
        role="tab"
        aria-selected={tab === 'architecture'}
        class:active={tab === 'architecture'}
        on:click={() => (tab = 'architecture')}
      >{$t('compare.tabArchitecture')}</button>
    </div>
  </header>

  {#if refsError}
    <div class="status error">⚠ {refsError}</div>
  {:else if loadingRefs && refs.length === 0}
    <div class="status">{$t('compare.loadingRefs')}</div>
  {:else if refs.length === 0}
    <div class="status">{$t('compare.noRefs')}</div>
  {:else if !bothPicked}
    <div class="status">{$t('compare.pickRef')}</div>
  {:else if tab === 'files'}
    {#if loadingChanges}
      <div class="status">{$t('diff.computing')}</div>
    {:else if changesError}
      <div class="status error">⚠ {changesError}</div>
    {:else if changes.length === 0}
      <div class="status">{$t('compare.noChanges')}</div>
    {:else}
      <ul class="files">
        {#each changes as c (c.path)}
          <li>
            <span class="status-pill status-{c.status}" title={c.status}>
              {statusLabel(c.status)}
            </span>
            <span class="path">{c.path}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if tab === 'diff'}
    <div class="embed">
      <DiffView reference={from} {to} />
    </div>
  {:else}
    <div class="embed">
      <DiagramView kind="folder-map" compareWith={from} diffRef={to} />
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
    gap: 12px;
    padding: 8px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-wrap: wrap;
  }
  .swap {
    background: var(--bg-2);
    color: var(--fg-0);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    width: 28px;
    height: 28px;
    cursor: pointer;
  }
  .swap:hover:not([disabled]) {
    background: var(--bg-3);
  }
  .swap[disabled] {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .spacer {
    flex: 1;
  }
  .tabs {
    display: flex;
    gap: 4px;
  }
  .tabs button {
    background: transparent;
    color: var(--fg-1);
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 4px 12px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.9em;
  }
  .tabs button:hover {
    background: var(--bg-2);
  }
  .tabs button.active {
    background: var(--bg-2);
    border-color: var(--accent);
    color: var(--fg-0);
  }
  .status {
    padding: 24px;
    color: var(--fg-2);
  }
  .status.error {
    color: var(--error);
  }
  .embed {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .embed > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .files {
    list-style: none;
    margin: 0;
    padding: 8px 0;
    overflow-y: auto;
    flex: 1;
  }
  .files li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 16px;
  }
  .files li:hover {
    background: var(--bg-1);
  }
  .path {
    font-family: var(--mono);
    color: var(--fg-0);
  }
  .status-pill {
    display: inline-block;
    min-width: 1.5em;
    text-align: center;
    padding: 1px 4px;
    border-radius: 3px;
    font-family: var(--mono);
    font-size: 0.8em;
    font-weight: 600;
    background: var(--bg-2);
    color: var(--fg-1);
  }
  .status-pill.status-added { background: #2ea043; color: #fff; }
  .status-pill.status-modified { background: #d29922; color: #1a1a1a; }
  .status-pill.status-deleted { background: #cf222e; color: #fff; }
  .status-pill.status-renamed { background: #8957e5; color: #fff; }
  .status-pill.status-type_change { background: #7a7a7a; color: #fff; }
</style>

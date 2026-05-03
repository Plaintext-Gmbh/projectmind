<script lang="ts">
  import DiagramView from './DiagramView.svelte';
  import { repo } from '../lib/store';
  import { t } from '../lib/i18n';
  import type { DiagramKind } from '../lib/navigation';

  /// Currently rendered diagram. Bound by App.svelte so the navigation
  /// history (← / →) keeps the kind in sync with the rest of the app.
  export let selectedKind: DiagramKind = 'folder-map';
  /// Layout knob the folder-map diagram uses; forwarded as-is.
  export let folderLayout: 'hierarchy' | 'solar' | 'td' = 'solar';

  $: available = ($repo?.available_diagrams ?? []) as DiagramKind[];

  // If the active kind isn't in the new repo's available set, snap to
  // the first one so the right pane is never staring at an unavailable
  // diagram. App.svelte does this too — the duplication is intentional
  // because this component can be the source of truth in the URL/history.
  $: if (available.length > 0 && !available.includes(selectedKind)) {
    selectedKind = available[0];
  }

  function labelFor(kind: string): string {
    switch (kind) {
      case 'bean-graph':
        return $t('diagram.beanGraph');
      case 'package-tree':
        return $t('diagram.packageTree');
      case 'folder-map':
        return $t('diagram.folderMap');
      case 'inheritance-tree':
        return $t('diagram.inheritanceTree');
      case 'doc-graph':
        return $t('diagram.docGraph');
      default:
        return kind;
    }
  }

  function descriptionFor(kind: string): string {
    switch (kind) {
      case 'bean-graph':
        return $t('diagram.description.beanGraph');
      case 'package-tree':
        return $t('diagram.description.packageTree');
      case 'folder-map':
        return $t('diagram.description.folderMap');
      case 'inheritance-tree':
        return $t('diagram.description.inheritanceTree');
      case 'doc-graph':
        return $t('diagram.description.docGraph');
      default:
        return '';
    }
  }
</script>

<section class="root">
  <header class="bar">
    <div class="title-block">
      <h2>{$t('diagram.title')}</h2>
      {#if $repo}
        <span class="subtitle">
          {$t('diagram.summary', {
            count: available.length,
            unit:
              available.length === 1 ? $t('diagram.summary.one') : $t('diagram.summary.other'),
            root: $repo.root,
          })}
        </span>
      {:else}
        <span class="subtitle">{$t('diagram.openRepo')}</span>
      {/if}
    </div>
  </header>

  <div class="layout">
    <aside class="sidebar">
      {#if !$repo}
        <div class="empty">{$t('diagram.openRepo')}</div>
      {:else if available.length === 0}
        <div class="empty">{$t('diagram.noDiagrams')}</div>
      {:else}
        <ul class="list">
          {#each available as k (k)}
            <li>
              <button
                type="button"
                class="item"
                class:selected={selectedKind === k}
                on:click={() => (selectedKind = k)}
              >
                <span class="item-title">{labelFor(k)}</span>
                <span class="item-meta">
                  <span class="kind">{k}</span>
                </span>
                <span class="item-desc">{descriptionFor(k)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </aside>

    <main class="viewer">
      {#if !$repo || available.length === 0}
        <div class="placeholder">{$t('diagram.placeholder')}</div>
      {:else}
        <DiagramView kind={selectedKind} folderLayout={folderLayout} />
      {/if}
    </main>
  </div>
</section>

<style>
  .root {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
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
    min-width: 0;
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

  .layout {
    flex: 1;
    display: grid;
    grid-template-columns: minmax(220px, 280px) 1fr;
    min-height: 0;
  }

  .sidebar {
    border-right: 1px solid var(--bg-3);
    background: var(--bg-1);
    overflow-y: auto;
    padding: 12px;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .item {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px 8px;
    width: 100%;
    text-align: left;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-0);
    padding: 8px 10px;
    font: inherit;
    cursor: pointer;
  }
  .item:hover {
    border-color: var(--accent-2);
  }
  .item.selected {
    border-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 14%, var(--bg-1));
  }

  .item-title {
    font-size: 13px;
    font-weight: 600;
  }

  .item-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    grid-column: 2;
    grid-row: 1;
  }
  .kind {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--fg-2);
    text-transform: lowercase;
  }

  .item-desc {
    grid-column: 1 / -1;
    font-size: 11px;
    color: var(--fg-2);
    line-height: 1.4;
  }

  .viewer {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .empty,
  .placeholder {
    color: var(--fg-2);
    padding: 32px 16px;
    text-align: center;
    font-size: 13px;
  }
</style>

<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from 'svelte';
  import type { GitRef } from '../lib/api';
  import { t } from '../lib/i18n';

  export let label: string;
  export let value: string;
  export let refs: GitRef[];
  export let disabled = false;

  const dispatch = createEventDispatcher<{ change: string }>();

  let open = false;
  let filter = '';
  let root: HTMLDivElement;

  $: branches = refs.filter((r) => r.kind === 'branch');
  $: tags = refs.filter((r) => r.kind === 'tag');
  $: filterLower = filter.trim().toLowerCase();
  $: visibleBranches = filterLower
    ? branches.filter((r) => r.name.toLowerCase().includes(filterLower))
    : branches;
  $: visibleTags = filterLower
    ? tags.filter((r) => r.name.toLowerCase().includes(filterLower))
    : tags;
  $: selected = refs.find((r) => r.name === value) ?? null;

  function toggle() {
    if (disabled) return;
    open = !open;
    if (open) filter = '';
  }

  function pick(name: string) {
    if (name === value) {
      open = false;
      return;
    }
    open = false;
    dispatch('change', name);
  }

  function onDocumentClick(event: MouseEvent) {
    if (!open) return;
    if (root && !root.contains(event.target as Node)) {
      open = false;
    }
  }

  onMount(() => {
    document.addEventListener('mousedown', onDocumentClick);
  });
  onDestroy(() => {
    document.removeEventListener('mousedown', onDocumentClick);
  });
</script>

<div class="picker" bind:this={root} class:open>
  <span class="label">{label}</span>
  <button
    type="button"
    class="trigger"
    {disabled}
    on:click={toggle}
    aria-haspopup="listbox"
    aria-expanded={open}
  >
    {#if selected}
      <span class="kind kind-{selected.kind}" title={selected.kind}>
        {selected.kind === 'branch' ? '⎇' : '⌖'}
      </span>
      <span class="name">{selected.name}</span>
      <span class="sha">{selected.target_sha}</span>
    {:else}
      <span class="placeholder">{value || $t('compare.pickRef')}</span>
    {/if}
    <span class="chevron">▾</span>
  </button>
  {#if open}
    <div class="menu" role="listbox">
      <input
        class="search"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder={$t('compare.searchRefs')}
        bind:value={filter}
      />
      {#if visibleBranches.length === 0 && visibleTags.length === 0}
        <div class="empty">{$t('compare.noRefs')}</div>
      {/if}
      {#if visibleBranches.length > 0}
        <div class="group">{$t('compare.groupBranches')}</div>
        {#each visibleBranches as r (`branch:${r.name}`)}
          <button
            type="button"
            class="item"
            class:selected={r.name === value}
            on:click={() => pick(r.name)}
          >
            <span class="kind kind-branch">⎇</span>
            <span class="name">{r.name}</span>
            <span class="sha">{r.target_sha}</span>
          </button>
        {/each}
      {/if}
      {#if visibleTags.length > 0}
        <div class="group">{$t('compare.groupTags')}</div>
        {#each visibleTags as r (`tag:${r.name}`)}
          <button
            type="button"
            class="item"
            class:selected={r.name === value}
            on:click={() => pick(r.name)}
          >
            <span class="kind kind-tag">⌖</span>
            <span class="name">{r.name}</span>
            <span class="sha">{r.target_sha}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 220px;
  }
  .label {
    color: var(--fg-2);
    font-size: 0.78em;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-0);
    font-family: inherit;
    font-size: 0.9em;
    cursor: pointer;
    flex: 1;
    min-width: 0;
  }
  .trigger[disabled] {
    opacity: 0.55;
    cursor: not-allowed;
  }
  .trigger:hover:not([disabled]) {
    background: var(--bg-2);
  }
  .picker.open .trigger {
    border-color: var(--accent);
  }
  .placeholder {
    color: var(--fg-2);
    flex: 1;
    text-align: left;
  }
  .name {
    font-family: var(--mono);
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sha {
    font-family: var(--mono);
    color: var(--fg-2);
    font-size: 0.85em;
  }
  .kind {
    width: 1em;
    text-align: center;
    color: var(--fg-2);
  }
  .kind-branch {
    color: var(--accent);
  }
  .kind-tag {
    color: var(--accent-2);
  }
  .chevron {
    color: var(--fg-2);
    font-size: 0.75em;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    z-index: 50;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    max-height: 360px;
    overflow-y: auto;
    min-width: 280px;
  }
  .search {
    width: 100%;
    box-sizing: border-box;
    border: 0;
    border-bottom: 1px solid var(--bg-3);
    background: var(--bg-0);
    color: var(--fg-0);
    padding: 6px 10px;
    font-family: inherit;
    font-size: 0.9em;
    outline: none;
  }
  .group {
    padding: 6px 10px 2px;
    color: var(--fg-2);
    font-size: 0.72em;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--fg-0);
    font-family: inherit;
    font-size: 0.9em;
    cursor: pointer;
    text-align: left;
  }
  .item:hover {
    background: var(--bg-2);
  }
  .item.selected {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }
  .empty {
    padding: 10px;
    color: var(--fg-2);
    font-size: 0.85em;
    text-align: center;
  }
</style>

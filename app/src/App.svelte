<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import {
    repo,
    classes,
    modules,
    selectedClass,
    stereotypeFilter,
    moduleFilter,
    errorMessage,
    filteredClasses,
    stereotypeCounts,
  } from './lib/store';
  import { openRepo, listClasses, listModules, showClass } from './lib/api';
  import type { ClassEntry } from './lib/api';
  import ClassViewer from './components/ClassViewer.svelte';
  import DiagramView from './components/DiagramView.svelte';
  import ModuleSidebar from './components/ModuleSidebar.svelte';

  let viewMode: 'classes' | 'diagram' = 'classes';
  let diagramKind: 'bean-graph' | 'package-tree' = 'bean-graph';
  let classSource = '';
  let classMeta: { file: string; line_start: number; line_end: number } | null = null;
  let loading = false;

  function basename(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? p : p.slice(idx + 1);
  }

  async function pickAndOpen() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || Array.isArray(picked)) return;
    await load(picked);
  }

  async function load(path: string) {
    loading = true;
    errorMessage.set(null);
    try {
      const summary = await openRepo(path);
      repo.set(summary);
      const [list, mods] = await Promise.all([listClasses(), listModules()]);
      classes.set(list);
      modules.set(mods);
      selectedClass.set(null);
      moduleFilter.set(null);
      stereotypeFilter.set(null);
      classSource = '';
    } catch (err) {
      errorMessage.set(String(err));
    } finally {
      loading = false;
    }
  }

  async function handleSelect(c: ClassEntry) {
    selectedClass.set(c);
    try {
      const r = await showClass(c.fqn);
      classSource = r.source;
      classMeta = { file: r.file, line_start: r.line_start, line_end: r.line_end };
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  function setFilter(s: string | null) {
    stereotypeFilter.update((cur) => (cur === s ? null : s));
  }

  onMount(() => {
    // No autoload — wait for user to pick.
  });
</script>

<main>
  <header>
    <div class="brand">
      <span class="logo">◊</span>
      <span class="title">plaintext-ide</span>
      {#if $repo}
        <span class="repo" title={$repo.root}>
          <span class="repo-name">{basename($repo.root)}</span>
          <span class="repo-path">{$repo.root}</span>
        </span>
        <span class="status">
          <span class="dot"></span>
          {$repo.classes} classes • {$repo.modules} module{$repo.modules === 1 ? '' : 's'}
        </span>
      {:else}
        <span class="status">
          <span class="dot dim"></span>
          no repository
        </span>
      {/if}
    </div>
    <nav>
      <button class:active={viewMode === 'classes'} on:click={() => (viewMode = 'classes')}>
        Classes
      </button>
      <button class:active={viewMode === 'diagram'} on:click={() => (viewMode = 'diagram')}>
        Diagrams
      </button>
      <button on:click={pickAndOpen} disabled={loading}>
        {loading ? '…' : 'Open repo'}
      </button>
    </nav>
  </header>

  {#if $errorMessage}
    <div class="error">⚠ {$errorMessage}</div>
  {/if}

  {#if !$repo}
    <section class="empty">
      <div class="welcome">
        <h1>plaintext-ide</h1>
        <p>A read-only architecture browser.</p>
        <button on:click={pickAndOpen}>Open a repository to begin</button>
        <p class="hint">
          Or use the <code>plaintext-ide-mcp</code> server with Claude Code — see the README.
        </p>
      </div>
    </section>
  {:else if viewMode === 'classes'}
    <section class="layout">
      <ModuleSidebar />
      <aside class="sidebar">
        <div class="filter">
          <button class="chip" class:active={$stereotypeFilter === null} on:click={() => setFilter(null)}>
            all <span class="count">{$classes.length}</span>
          </button>
          {#each Object.entries($stereotypeCounts) as [name, count]}
            <button
              class="chip {name}"
              class:active={$stereotypeFilter === name}
              on:click={() => setFilter(name)}
            >
              {name} <span class="count">{count}</span>
            </button>
          {/each}
        </div>
        <ul class="class-list" role="listbox" aria-label="Classes">
          {#each $filteredClasses as c (`${c.module}::${c.fqn}`)}
            <li role="option" aria-selected={$selectedClass?.fqn === c.fqn}>
              <button
                type="button"
                class="class-row"
                class:selected={$selectedClass?.fqn === c.fqn}
                on:click={() => handleSelect(c)}
              >
                <span class="class-name">{c.name}</span>
                <span class="class-fqn">{c.fqn}</span>
                <span class="stereotypes">
                  {#each c.stereotypes as s}
                    <span class="badge {s}">{s}</span>
                  {/each}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </aside>
      <main class="viewer">
        {#if $selectedClass}
          <ClassViewer
            klass={$selectedClass}
            source={classSource}
            meta={classMeta}
          />
        {:else}
          <div class="placeholder">Select a class on the left.</div>
        {/if}
      </main>
    </section>
  {:else}
    <section class="diagram-view">
      <div class="diagram-tabs">
        <button class:active={diagramKind === 'bean-graph'} on:click={() => (diagramKind = 'bean-graph')}>
          Bean graph
        </button>
        <button class:active={diagramKind === 'package-tree'} on:click={() => (diagramKind = 'package-tree')}>
          Package tree
        </button>
      </div>
      <DiagramView kind={diagramKind} />
    </section>
  {/if}
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .logo {
    font-size: 18px;
    color: var(--accent);
  }

  .title {
    font-weight: 600;
    font-size: 15px;
    color: var(--fg-2);
  }

  .repo {
    display: inline-flex;
    align-items: baseline;
    gap: 8px;
    padding: 2px 10px;
    background: var(--bg-2);
    border-radius: 4px;
    border: 1px solid var(--bg-3);
    cursor: default;
  }

  .repo-name {
    font-weight: 600;
    font-size: 14px;
    color: var(--fg-0);
  }

  .repo-path {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .status {
    color: var(--fg-2);
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }
  .dot.dim {
    background: var(--fg-2);
  }

  nav {
    display: flex;
    gap: 8px;
  }

  nav button.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }

  .error {
    background: color-mix(in srgb, var(--error) 20%, var(--bg-1));
    color: var(--error);
    padding: 8px 16px;
    font-family: var(--mono);
    font-size: 12px;
  }

  .empty {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
  }

  .welcome {
    text-align: center;
  }

  .welcome h1 {
    margin: 0 0 8px;
    font-weight: 600;
    font-size: 28px;
  }

  .welcome p {
    color: var(--fg-1);
    margin: 0 0 20px;
  }

  .welcome button {
    background: var(--accent-2);
    color: var(--bg-0);
    border-color: var(--accent-2);
    padding: 10px 20px;
    font-weight: 500;
  }

  .welcome button:hover {
    background: color-mix(in srgb, var(--accent-2) 80%, white);
  }

  .welcome .hint {
    margin-top: 32px;
    color: var(--fg-2);
    font-size: 12px;
  }

  .welcome code {
    font-family: var(--mono);
    background: var(--bg-2);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .layout {
    display: grid;
    grid-template-columns: 220px 360px 1fr;
    flex: 1;
    overflow: hidden;
  }

  .sidebar {
    background: var(--bg-1);
    border-right: 1px solid var(--bg-3);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .filter {
    padding: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    border-bottom: 1px solid var(--bg-3);
  }

  .chip {
    background: var(--bg-2);
    padding: 3px 8px;
    border: 1px solid transparent;
    border-radius: 12px;
    font-size: 11px;
    cursor: pointer;
    color: var(--fg-1);
  }
  .chip.active {
    border-color: var(--accent-2);
    color: var(--fg-0);
  }
  .chip .count {
    color: var(--fg-2);
    font-family: var(--mono);
  }

  .class-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    flex: 1;
  }

  .class-list li {
    border-bottom: 1px solid var(--bg-2);
  }

  .class-row {
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: 0;
    border-left: 3px solid transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font: inherit;
  }

  .class-row:hover {
    background: var(--bg-2);
  }

  .class-row:focus-visible {
    outline: 2px solid var(--accent-2);
    outline-offset: -2px;
  }

  .class-row.selected {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-1));
    border-left-color: var(--accent-2);
  }

  .class-name {
    font-weight: 600;
    font-size: 13px;
  }

  .class-fqn {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
  }

  .stereotypes {
    margin-top: 2px;
  }

  .viewer {
    overflow-y: auto;
    background: var(--bg-0);
  }

  .placeholder {
    padding: 40px;
    color: var(--fg-2);
    text-align: center;
  }

  .diagram-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .diagram-tabs {
    display: flex;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
  }

  .diagram-tabs button.active {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
</style>

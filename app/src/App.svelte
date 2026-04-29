<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { onMount, onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { get } from 'svelte/store';
  import {
    repo,
    classes,
    modules,
    selectedClass,
    stereotypeFilter,
    moduleFilter,
    packageFilter,
    errorMessage,
    filteredClasses,
    stereotypeCounts,
    viewMode,
    fileView,
    walkthroughCursor,
    diffViewRef,
    followingMcp,
  } from './lib/store';
  import {
    openRepo,
    listClasses,
    listModules,
    showClass,
    currentState,
  } from './lib/api';
  import type { ClassEntry, UiState } from './lib/api';
  import ClassViewer from './components/ClassViewer.svelte';
  import DiagramView from './components/DiagramView.svelte';
  import DiffView from './components/DiffView.svelte';
  import FileView from './components/FileView.svelte';
  import HtmlIndex from './components/HtmlIndex.svelte';
  import MarkdownIndex from './components/MarkdownIndex.svelte';
  import ModuleSidebar from './components/ModuleSidebar.svelte';
  import WalkthroughView from './components/WalkthroughView.svelte';
  import { resizable } from './lib/resizable';

  type Theme = 'dark' | 'light';
  let theme: Theme = readTheme();
  $: applyTheme(theme);

  function readTheme(): Theme {
    try {
      const v = localStorage.getItem('plaintext-ide.theme');
      if (v === 'dark' || v === 'light') return v;
    } catch {
      // localStorage unavailable
    }
    return 'dark';
  }

  function applyTheme(t: Theme) {
    if (typeof document === 'undefined') return;
    document.documentElement.dataset.theme = t;
    try {
      localStorage.setItem('plaintext-ide.theme', t);
    } catch {
      // ignore
    }
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
  }

  // The Code tab falls back to "Files" when the repo has no parsed classes
  // (e.g. a docs-only or office-style folder).
  $: codeTabLabel = $repo && $repo.classes === 0 ? 'Files' : 'Code';

  let diagramKind: 'bean-graph' | 'package-tree' = 'bean-graph';
  let classSource = '';
  let classMeta: { file: string; line_start: number; line_end: number } | null = null;
  let loading = false;
  let unlistenState: (() => void) | null = null;
  let lastSeq = 0;
  /// True while we're applying an MCP-driven state change. Prevents the
  /// resulting load() from re-publishing and triggering an event loop.
  let applyingExternal = false;

  // Whenever selectedClass changes (from sidebar click *or* a diagram drilldown)
  // load the source for the right-hand viewer.
  let lastLoadedFqn: string | null = null;
  $: void loadSourceFor($selectedClass);

  async function loadSourceFor(c: ClassEntry | null) {
    if (!c) {
      classSource = '';
      classMeta = null;
      lastLoadedFqn = null;
      return;
    }
    if (c.fqn === lastLoadedFqn) return;
    lastLoadedFqn = c.fqn;
    try {
      const r = await showClass(c.fqn);
      classSource = r.source;
      classMeta = { file: r.file, line_start: r.line_start, line_end: r.line_end };
    } catch (err) {
      errorMessage.set(String(err));
    }
  }

  function basename(p: string): string {
    const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return idx === -1 ? p : p.slice(idx + 1);
  }

  async function pickAndOpen() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || Array.isArray(picked)) return;
    await load(picked);
  }

  async function load(path: string, opts: { silent?: boolean } = {}) {
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
      packageFilter.set(null);
      classSource = '';
    } catch (err) {
      if (opts.silent) {
        // Re-throw so caller can decide whether to show or swallow.
        throw err;
      }
      errorMessage.set(String(err));
    } finally {
      loading = false;
    }
  }

  function handleSelect(c: ClassEntry) {
    selectedClass.set(c);
  }

  function setFilter(s: string | null) {
    stereotypeFilter.update((cur) => (cur === s ? null : s));
  }

  // ----- MCP↔GUI sync: listen for state changes, apply intents -----------

  async function applyState(s: UiState) {
    if (s.seq <= lastSeq) return;
    lastSeq = s.seq;
    applyingExternal = true;
    try {
      // Switch repos if needed. Swallow open errors silently — a stale
      // statefile (e.g. a test run that left behind a tmp path) shouldn't
      // pop up as a blocking error; the user can just open a fresh repo.
      const currentRoot = get(repo)?.root;
      if (s.repo_root && s.repo_root !== currentRoot) {
        try {
          await load(s.repo_root, { silent: true });
        } catch {
          // Stale or vanished path. Silently abandon — keep the GUI on
          // whatever state it's in (probably "no repo").
          return;
        }
      }
      followingMcp.set(true);
      // Apply view intent.
      const v = s.view;
      switch (v.kind) {
        case 'classes':
          viewMode.set('classes');
          if (v.selected_fqn) {
            const match = get(classes).find((c) => c.fqn === v.selected_fqn);
            if (match) selectedClass.set(match);
          }
          break;
        case 'diagram':
          if (v.diagram_kind === 'bean-graph' || v.diagram_kind === 'package-tree') {
            diagramKind = v.diagram_kind;
          }
          viewMode.set('diagram');
          break;
        case 'diff':
          diffViewRef.set({ reference: v.reference, to: v.to ?? null });
          viewMode.set('diff');
          break;
        case 'file':
          fileView.update((cur) => ({
            path: v.path,
            anchor: v.anchor ?? null,
            nonce: (cur?.nonce ?? 0) + 1,
          }));
          viewMode.set('file');
          break;
        case 'walkthrough':
          walkthroughCursor.update((cur) => ({
            id: v.id,
            step: v.step,
            nonce: (cur?.nonce ?? 0) + 1,
          }));
          viewMode.set('walkthrough');
          break;
      }
    } catch (err) {
      errorMessage.set(String(err));
    } finally {
      applyingExternal = false;
    }
  }

  onMount(async () => {
    // Pick up wherever we left off (or whatever the MCP server has set since).
    const initial = await currentState();
    if (initial) await applyState(initial);

    unlistenState = await listen<UiState>('state-changed', (ev) => {
      void applyState(ev.payload);
    });
  });

  onDestroy(() => {
    unlistenState?.();
  });
</script>

<main>
  <header>
    <div class="brand">
      <img class="logo" src="/logo.png" alt="plaintext-ide" />
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
      <button
        class:active={$viewMode === 'classes'}
        disabled={!$repo}
        on:click={() => {
          followingMcp.set(false);
          viewMode.set('classes');
        }}
      >
        {codeTabLabel}
      </button>
      {#if !$repo || ($repo && $repo.classes > 0)}
        <button
          class:active={$viewMode === 'diagram'}
          disabled={!$repo}
          on:click={() => {
            followingMcp.set(false);
            viewMode.set('diagram');
          }}
        >
          Diagrams
        </button>
      {/if}
      {#if !$repo || ($repo && $repo.markdown_count > 0)}
        <button
          class:active={$viewMode === 'md' || $viewMode === 'file'}
          disabled={!$repo}
          on:click={() => {
            followingMcp.set(false);
            viewMode.set('md');
          }}
          title="Browse markdown files in this repository"
        >
          MD
        </button>
      {/if}
      {#if !$repo || ($repo && $repo.html_count > 0)}
        <button
          class:active={$viewMode === 'html'}
          disabled={!$repo}
          on:click={() => {
            followingMcp.set(false);
            viewMode.set('html');
          }}
          title="Browse HTML files and snippets in this repository"
        >
          HTML
        </button>
      {/if}
      {#if $walkthroughCursor}
        <button
          class:active={$viewMode === 'walkthrough'}
          class="walkthrough-btn"
          on:click={() => viewMode.set('walkthrough')}
          title="Resume the active walk-through"
        >
          ▶ Walk-through
        </button>
      {/if}
      {#if $viewMode === 'diff'}
        <button class="active">Diff</button>
      {/if}
      {#if $followingMcp}
        <span class="follow" title="GUI is following an MCP-issued view intent. Click any tab to continue manually.">
          following MCP
        </span>
      {/if}
      <button on:click={pickAndOpen} disabled={loading}>
        {loading ? '…' : 'Open repo'}
      </button>
      <button
        class="theme-toggle"
        on:click={toggleTheme}
        title="Switch to {theme === 'dark' ? 'light' : 'dark'} mode"
        aria-label="Toggle theme"
      >
        {theme === 'dark' ? '☀' : '☾'}
      </button>
    </nav>
  </header>

  {#if $errorMessage}
    <div class="error">⚠ {$errorMessage}</div>
  {/if}

  {#if !$repo}
    <section class="empty">
      <div class="welcome">
        <img class="welcome-logo" src="/logo.png" alt="plaintext-ide" />
        <h1>plaintext-ide</h1>
        <p>A read-only architecture browser.</p>
        <button on:click={pickAndOpen}>Open a repository to begin</button>
        <p class="hint">
          Or use the <code>plaintext-ide-mcp</code> server with your favourite LLM CLI — see the README.
        </p>
      </div>
    </section>
  {:else if $viewMode === 'classes'}
    <section class="layout">
      <ModuleSidebar />
      <div
        class="resizer"
        use:resizable={{
          storageKey: 'plaintext-ide.layout.code.col1',
          cssVar: '--code-col-1',
          min: 140,
          max: 480,
          initial: 220,
        }}
        title="Drag to resize · double-click to reset"
      ></div>
      <aside class="sidebar">
        {#if $packageFilter !== null}
          <div class="path-bar">
            <span class="path-label">package</span>
            <code class="path-value">{$packageFilter || '(default)'}</code>
            <button class="path-clear" on:click={() => packageFilter.set(null)} title="Clear package filter">×</button>
          </div>
        {/if}
        <div class="filter">
          <button class="chip" class:active={$stereotypeFilter === null} on:click={() => setFilter(null)}>
            all <span class="count">{$filteredClasses.length}</span>
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
      <div
        class="resizer"
        use:resizable={{
          storageKey: 'plaintext-ide.layout.code.col2',
          cssVar: '--code-col-2',
          min: 220,
          max: 720,
          initial: 360,
        }}
        title="Drag to resize · double-click to reset"
      ></div>
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
  {:else if $viewMode === 'diagram'}
    <section class="diagram-view">
      <div class="diagram-tabs">
        <button class:active={diagramKind === 'bean-graph'} on:click={() => (diagramKind = 'bean-graph')}>
          Bean graph
        </button>
        <button class:active={diagramKind === 'package-tree'} on:click={() => (diagramKind = 'package-tree')}>
          Package tree
        </button>
        <span class="diagram-hint">Click a node to drill into it</span>
      </div>
      <DiagramView kind={diagramKind} />
    </section>
  {:else if $viewMode === 'walkthrough' && $walkthroughCursor}
    <WalkthroughView
      cursorId={$walkthroughCursor.id}
      cursorStep={$walkthroughCursor.step}
      nonce={$walkthroughCursor.nonce}
    />
  {:else if $viewMode === 'md'}
    <MarkdownIndex />
  {:else if $viewMode === 'html'}
    <HtmlIndex />
  {:else if $viewMode === 'file' && $fileView}
    <FileView
      path={$fileView.path}
      anchor={$fileView.anchor}
      nonce={$fileView.nonce}
    />
  {:else if $viewMode === 'diff' && $diffViewRef}
    <DiffView reference={$diffViewRef.reference} to={$diffViewRef.to} />
  {:else}
    <section class="empty">
      <div class="welcome">
        <p class="hint">No view selected. Pick Code, Diagrams or HTML above, or send an MCP intent.</p>
      </div>
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
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: block;
    flex-shrink: 0;
  }

  .welcome-logo {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    margin-bottom: 16px;
    display: block;
    margin-left: auto;
    margin-right: auto;
    box-shadow: 0 8px 32px color-mix(in srgb, #2d2bfe 35%, transparent);
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

  .theme-toggle {
    width: 34px;
    padding: 6px 0;
    text-align: center;
    font-size: 15px;
    line-height: 1;
  }

  .walkthrough-btn {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-1));
    color: var(--accent-2);
    border-color: var(--accent-2);
    font-weight: 500;
  }
  .walkthrough-btn:hover {
    background: color-mix(in srgb, var(--accent-2) 28%, var(--bg-1));
  }

  .follow {
    font-size: 11px;
    padding: 4px 8px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent-2) 25%, var(--bg-1));
    color: var(--accent-2);
    border: 1px solid var(--accent-2);
    font-weight: 500;
    align-self: center;
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
    grid-template-columns:
      var(--code-col-1, 220px) 6px var(--code-col-2, 360px) 6px 1fr;
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
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .path-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    background: color-mix(in srgb, var(--accent-2) 15%, var(--bg-1));
    border-bottom: 1px solid var(--bg-3);
    font-size: 12px;
  }

  .path-label {
    color: var(--fg-2);
    text-transform: uppercase;
    font-size: 10px;
    letter-spacing: 0.05em;
  }

  .path-value {
    flex: 1;
    font-family: var(--mono);
    color: var(--fg-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .path-clear {
    width: 22px;
    height: 22px;
    padding: 0;
    border-radius: 50%;
    font-size: 14px;
    line-height: 1;
    background: var(--bg-2);
    color: var(--fg-1);
  }
  .path-clear:hover {
    background: var(--bg-3);
    color: var(--fg-0);
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

  .diagram-hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-2);
  }
</style>

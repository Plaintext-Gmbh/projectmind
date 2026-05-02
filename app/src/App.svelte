<script lang="ts">
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { onMount, onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { get } from 'svelte/store';
  import {
    repo,
    classes,
    modules,
    selectedClass,
    stereotypeFilter,
    fileKindFilter,
    moduleFilter,
    packageFilter,
    errorMessage,
    filteredClasses,
    stereotypeCounts,
    moduleFilesByModule,
    filteredModuleFiles,
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
    listModuleFiles,
    showClass,
    currentState,
    browserToken,
    clearBrowserToken,
    isTauriRuntime,
    setBrowserToken,
  } from './lib/api';
  import type { ClassEntry, ModuleEntry, ModuleFile, TabDescriptor, UiState } from './lib/api';
  // Eagerly imported — small, almost-always rendered:
  import ClassViewer from './components/ClassViewer.svelte';
  import ModuleSidebar from './components/ModuleSidebar.svelte';
  import ImageView from './components/ImageView.svelte';
  import PdfView from './components/PdfView.svelte';
  import DiffView from './components/DiffView.svelte';
  import KeyboardHelp from './components/KeyboardHelp.svelte';
  import StatusBar from './components/StatusBar.svelte';
  // Heavy components — pulled in dynamically the first time the user
  // visits the matching tab. mermaid (~640 KB) and marked (~40 KB) ride
  // along with DiagramView / FileView / WalkthroughView etc., so keeping
  // those imports lazy keeps the initial bundle under 200 KB.
  import { resizable } from './lib/resizable';
  import { t, language, setLanguage, languages } from './lib/i18n';
  import { loadComponent } from './lib/lazyLoad';
  import * as nav from './lib/navigation';
  import { canBack as nav_canBack, canForward as nav_canForward } from './lib/navigation';
  import type { HistoryEntry, DiagramKind, FolderMapLayout } from './lib/navigation';
  import * as recents from './lib/recentRepos';
  import { recentRepos } from './lib/recentRepos';
  const lazyDiagramView = () =>
    loadComponent('DiagramView', () => import('./components/DiagramView.svelte'));
  const lazyFileView = () =>
    loadComponent('FileView', () => import('./components/FileView.svelte'));
  const lazyDrawIoView = () =>
    loadComponent('DrawIoView', () => import('./components/DrawIoView.svelte'));
  const lazyHtmlIndex = () =>
    loadComponent('HtmlIndex', () => import('./components/HtmlIndex.svelte'));
  const lazyMarkdownIndex = () =>
    loadComponent('MarkdownIndex', () => import('./components/MarkdownIndex.svelte'));
  const lazyWalkthroughView = () =>
    loadComponent('WalkthroughView', () => import('./components/WalkthroughView.svelte'));

  type Theme = 'dark' | 'light';
  const BROWSER_STATE_POLL_MS = 500;
  let theme: Theme = readTheme();
  $: applyTheme(theme);

  function readTheme(): Theme {
    try {
      const v = localStorage.getItem('projectmind.theme');
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
      localStorage.setItem('projectmind.theme', t);
    } catch {
      // ignore
    }
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
  }

  // The main browse tab is always labelled with the i18n entry — the term
  // covers parsed classes, plain assets (PDFs, images), and (after the
  // chip refactor) markdown / HTML alike. "Modules" stays as the synonym
  // for folders.
  $: codeTabLabel = $t('nav.files');

  // The "files" tab hosts several render modes (classes / pdf / image /
  // markdown / html / single-file viewer) that are all reachable from
  // chips inside the same surface. Other tabs are 1:1 with their
  // viewMode so the default below covers them.
  const FILES_VIEW_MODES = new Set(['classes', 'pdf', 'image', 'md', 'html', 'file']);

  function isTabActive(tab: TabDescriptor, mode: string): boolean {
    if (tab.id === 'files') return FILES_VIEW_MODES.has(mode);
    return tab.view_mode === mode;
  }

  function activateTab(tab: TabDescriptor) {
    followingMcp.set(false);
    viewMode.set(tab.view_mode as Parameters<typeof viewMode.set>[0]);
  }

  function toggleLang() {
    // Cycle through the configured languages (codex's i18n module ships
    // five). Wrap to the first one once we hit the end.
    const codes = languages.map((l) => l.code);
    const idx = codes.indexOf($language);
    setLanguage(codes[(idx + 1) % codes.length]);
  }

  let diagramKind: 'bean-graph' | 'package-tree' | 'folder-map' = 'bean-graph';
  let folderMapLayout: 'hierarchy' | 'solar' | 'td' = 'solar';

  // ----- Navigation history -------------------------------------------------
  // Browser-style ←/→ over every user-visible state change. The reactive
  // block at the bottom builds a HistoryEntry from the current store + local
  // values; `nav.push` is a no-op when the new entry equals the current one,
  // so re-clicking the same class doesn't pollute the back trail.
  function buildHistoryEntry(): HistoryEntry {
    const fv = get(fileView);
    const wt = get(walkthroughCursor);
    const dv = get(diffViewRef);
    const cls = get(selectedClass);
    return {
      viewMode: get(viewMode),
      selectedFqn: cls?.fqn ?? null,
      filePath: fv?.path ?? null,
      fileAnchor: fv?.anchor ?? null,
      diagramKind: diagramKind as DiagramKind,
      folderMapLayout: folderMapLayout as FolderMapLayout,
      diffRef: dv?.reference ?? null,
      diffTo: dv?.to ?? null,
      walkthroughId: wt?.id ?? null,
      walkthroughStep: wt?.step ?? null,
      moduleFilter: get(moduleFilter),
      packageFilter: get(packageFilter),
      stereotypeFilter: get(stereotypeFilter),
      fileKindFilter: get(fileKindFilter),
      label: describeEntry(get(viewMode), cls?.fqn ?? null, fv?.path ?? null, diagramKind),
      ts: Date.now(),
    };
  }

  function describeEntry(
    mode: string,
    fqn: string | null,
    file: string | null,
    diagram: string,
  ): string {
    if (mode === 'classes' && fqn) return `Class · ${fqn.split('.').pop()}`;
    if (mode === 'classes') return 'Code';
    if (mode === 'diagram') return `Diagram · ${diagram}`;
    if (mode === 'md') return 'Markdown';
    if (mode === 'html') return 'HTML';
    if (mode === 'file' && file) return `File · ${file.split('/').pop()}`;
    if (mode === 'pdf' && file) return `PDF · ${file.split('/').pop()}`;
    if (mode === 'image' && file) return `Image · ${file.split('/').pop()}`;
    if (mode === 'diff') return 'Diff';
    if (mode === 'walkthrough') return 'Walkthrough';
    return mode;
  }

  function applyHistoryEntry(entry: HistoryEntry) {
    // Order matters: filters first, then containers, then leaves —
    // mirrors how the GUI normally settles on a new view.
    moduleFilter.set(entry.moduleFilter);
    packageFilter.set(entry.packageFilter);
    stereotypeFilter.set(entry.stereotypeFilter);
    fileKindFilter.set(entry.fileKindFilter);
    if (entry.diagramKind) diagramKind = entry.diagramKind;
    if (entry.folderMapLayout) folderMapLayout = entry.folderMapLayout;
    if (entry.selectedFqn) {
      const match = get(classes).find((c) => c.fqn === entry.selectedFqn);
      selectedClass.set(match ?? null);
    } else {
      selectedClass.set(null);
    }
    if (entry.filePath) {
      fileView.update((cur) => ({
        path: entry.filePath as string,
        anchor: entry.fileAnchor ?? null,
        nonce: (cur?.nonce ?? 0) + 1,
      }));
    } else {
      fileView.set(null);
    }
    if (entry.walkthroughId !== null && entry.walkthroughStep !== null) {
      walkthroughCursor.update((cur) => ({
        id: entry.walkthroughId as string,
        step: entry.walkthroughStep as number,
        nonce: (cur?.nonce ?? 0) + 1,
      }));
    } else {
      walkthroughCursor.set(null);
    }
    if (entry.diffRef) {
      diffViewRef.set({ reference: entry.diffRef, to: entry.diffTo });
    } else {
      diffViewRef.set(null);
    }
    viewMode.set(entry.viewMode as never);
  }

  function navBack() {
    nav.back(applyHistoryEntry);
  }

  function navForward() {
    nav.forward(applyHistoryEntry);
  }

  let kbdHelpOpen = false;

  function onNavKey(ev: KeyboardEvent) {
    // Don't steal navigation keys while the user is typing in a field.
    const t = ev.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    // ⌘[ / ⌘] (macOS) and Alt+← / Alt+→ (everywhere else) match Finder + browser conventions.
    const cmdOrAlt = ev.metaKey || ev.altKey;
    if (cmdOrAlt && (ev.key === '[' || ev.key === 'ArrowLeft')) {
      ev.preventDefault();
      navBack();
    } else if (cmdOrAlt && (ev.key === ']' || ev.key === 'ArrowRight')) {
      ev.preventDefault();
      navForward();
    } else if (ev.key === '?' && !ev.metaKey && !ev.ctrlKey) {
      // Bare `?` (Shift+/) opens the keyboard cheatsheet — same shortcut as
      // every modern web app + Slack + GitHub. The carve-out above keeps it
      // from firing while the user is typing inside an input or textarea.
      ev.preventDefault();
      kbdHelpOpen = true;
    }
  }
  // Reactive auto-push: any time a navigation-relevant store / local var
  // changes, snapshot it and hand it to nav.push. nav.push de-dupes against
  // the current entry, so re-clicks of the same class don't fill the back
  // trail with copies of the same state. nav.push is also a no-op while a
  // back/forward navigation is being applied.
  $: void _autoPushOnNavChange(
    $viewMode,
    $selectedClass,
    $fileView,
    diagramKind,
    folderMapLayout,
    $diffViewRef,
    $walkthroughCursor,
    $moduleFilter,
    $packageFilter,
    $stereotypeFilter,
    $fileKindFilter,
    $repo,
  );
  function _autoPushOnNavChange(..._: unknown[]) {
    if (!get(repo)) return; // no repo → nothing to remember
    nav.push(buildHistoryEntry());
  }
  // ----- end navigation history ---------------------------------------------

  let classSource = '';
  let classMeta: { file: string; line_start: number; line_end: number } | null = null;
  let loading = false;
  let browserMode = false;
  let browserAuthorized = true;
  let tokenInput = '';
  let unlistenState: (() => void) | null = null;
  let statePoll: ReturnType<typeof setInterval> | null = null;
  let lastSeq = 0;
  // Drag-and-drop state. `dragOver` toggles the full-window overlay; in
  // browser mode `browserDropHint` flashes a transient inline notice telling
  // the user the desktop app supports the gesture but the browser can't see
  // absolute paths.
  let dragOver = false;
  let browserDropHint: string | null = null;
  let browserDropHintTimer: ReturnType<typeof setTimeout> | null = null;
  let unlistenDragDrop: (() => void) | null = null;
  /// True while we're applying an MCP-driven state change. Prevents the
  /// resulting load() from re-publishing and triggering an event loop.
  let applyingExternal = false;

  // Whenever selectedClass changes (from sidebar click *or* a diagram drilldown)
  // load the source for the right-hand viewer.
  let lastLoadedFqn: string | null = null;
  // If the active diagram kind isn't in the new repo's available_diagrams
  // (e.g. switching from a Java repo to a docs-only repo would orphan the
  // bean-graph), fall back to the first available kind. folder-map is
  // always present so this never fails.
  $: if ($repo && !$repo.available_diagrams.includes(diagramKind)) {
    diagramKind = ($repo.available_diagrams[0] ?? 'folder-map') as typeof diagramKind;
  }

  function diagramLabel(kind: string): string {
    switch (kind) {
      case 'bean-graph': return $t('diagram.beanGraph');
      case 'package-tree': return $t('diagram.packageTree');
      case 'folder-map': return $t('diagram.folderMap');
      default: return kind; // unknown plugin-contributed diagram — show id
    }
  }
  $: void loadSourceFor($selectedClass);

  // PDFs / images that live inside each module. The map is reloaded whenever
  // the module filter changes (or new modules arrive); the right-pane list
  // and per-module counters subscribe to the derived stores in store.ts.
  let moduleFilesLoadedFor: string | null = null;
  // Re-run on either change: a different filter, OR new modules arriving
  // (the "all modules" path needs the populated $modules list).
  $: void loadModuleFilesFor($moduleFilter, $modules);

  async function loadModuleFilesFor(moduleId: string | null, mods: ModuleEntry[]) {
    // Cache key — id of the filter plus the module-set fingerprint, so that
    // a repo-open which repopulates $modules invalidates a previous "0 mods"
    // result.
    const token = `${moduleId ?? '__all__'}::${mods.map((m) => m.id).join(',')}`;
    if (moduleFilesLoadedFor === token) return;
    moduleFilesLoadedFor = token;
    try {
      let map: Record<string, ModuleFile[]>;
      if (moduleId) {
        const items = await listModuleFiles(moduleId);
        map = { [moduleId]: items };
      } else if (mods.length === 0) {
        map = {};
      } else {
        // "All modules" filter — fan out across every module and merge.
        const lists = await Promise.all(mods.map((m) => listModuleFiles(m.id)));
        map = {};
        mods.forEach((m, i) => {
          map[m.id] = lists[i];
        });
      }
      // Race guard: ignore the result if the user switched while we were fetching.
      if (moduleFilesLoadedFor === token) moduleFilesByModule.set(map);
    } catch (err) {
      // Don't blow up the whole Code tab — non-fatal, just hide the section.
      console.warn('list_module_files failed:', err);
      if (moduleFilesLoadedFor === token) moduleFilesByModule.set({});
    }
  }

  // Discriminated row type for the mixed Code-tab list. Either a parsed
  // class or a non-source file (PDF / image) sourced from list_module_files.
  type DisplayItem =
    | { kind: 'class'; entry: ClassEntry }
    | { kind: 'file'; entry: ModuleFile };

  // Distinct kinds present in the visible files — drives the filter pills
  // shown next to the stereotype pills. Sorted for stable UI ordering.
  $: fileKindsPresent = (() => {
    const set = new Set<string>();
    for (const f of $filteredModuleFiles) set.add(f.kind);
    return Array.from(set).sort();
  })();

  // Mixed list rendered on the right: classes + files, filtered by the
  // active filter axis (stereotype, file-kind, module).
  //
  //  - fileKindFilter set → only files of that kind, no classes.
  //  - stereotypeFilter set → only classes with that stereotype, no files.
  //  - otherwise → classes (already module/package-filtered) + files for
  //    the same module scope, classes first then files (each block sorted).
  $: displayItems = (() => {
    const out: DisplayItem[] = [];
    if ($fileKindFilter !== null) {
      const files = $filteredModuleFiles
        .filter((f) => f.kind === $fileKindFilter)
        .slice()
        .sort((a, b) => a.rel.localeCompare(b.rel));
      for (const f of files) out.push({ kind: 'file', entry: f });
      return out;
    }
    if ($stereotypeFilter !== null) {
      for (const c of $filteredClasses) out.push({ kind: 'class', entry: c });
      return out;
    }
    // No file/stereotype filter — classes first (filteredClasses already
    // honours moduleFilter + packageFilter), then module files.
    const sortedClasses = $filteredClasses.slice().sort((a, b) => a.fqn.localeCompare(b.fqn));
    for (const c of sortedClasses) out.push({ kind: 'class', entry: c });
    const sortedFiles = $filteredModuleFiles.slice().sort((a, b) => a.rel.localeCompare(b.rel));
    for (const f of sortedFiles) out.push({ kind: 'file', entry: f });
    return out;
  })();

  function openModuleFile(f: ModuleFile) {
    fileView.update((cur) => ({
      path: f.abs,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    if (f.kind === 'pdf') {
      viewMode.set('pdf');
    } else {
      viewMode.set('image');
    }
  }

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
    if (browserMode) {
      const picked = window.prompt('Absolute repository path on the ProjectMind host');
      if (picked) await load(picked);
      return;
    }
    const picked = await openDialog({ directory: true, multiple: false });
    if (!picked || Array.isArray(picked)) return;
    await load(picked);
  }

  async function useBrowserToken() {
    const token = tokenInput.trim();
    if (!token) return;
    setBrowserToken(token);
    browserAuthorized = true;
    errorMessage.set(null);
    try {
      const initial = await currentState();
      if (initial) await applyState(initial);
    } catch (err) {
      browserAuthorized = false;
      errorMessage.set(String(err));
    }
  }

  function forgetBrowserToken() {
    clearBrowserToken();
    browserAuthorized = false;
    tokenInput = '';
    repo.set(null);
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
      fileKindFilter.set(null);
      packageFilter.set(null);
      classSource = '';
      recents.record(summary.root, summary.classes, summary.modules);
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
    if (s === null) {
      // "all" — clear both filter axes.
      stereotypeFilter.set(null);
      fileKindFilter.set(null);
      return;
    }
    fileKindFilter.set(null);
    stereotypeFilter.update((cur) => (cur === s ? null : s));
  }

  function setKindFilter(k: string) {
    stereotypeFilter.set(null);
    fileKindFilter.update((cur) => (cur === k ? null : k));
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
          if (
            v.diagram_kind === 'bean-graph' ||
            v.diagram_kind === 'package-tree' ||
            v.diagram_kind === 'folder-map'
          ) {
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

  // ----- Drag-and-drop ------------------------------------------------------

  function dirname(p: string): string {
    // Strip a trailing slash (so `/foo/bar/` → `/foo/bar` → `/foo`) before
    // looking for the separator. Works for both POSIX and Windows-style
    // paths since Tauri returns forward slashes on POSIX and backslashes on
    // Windows; we honour both.
    const trimmed = p.replace(/[\\/]+$/, '');
    const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
    if (idx === -1) return trimmed;
    if (idx === 0) return trimmed.slice(0, 1); // root: "/"
    return trimmed.slice(0, idx);
  }

  function extOf(p: string): string {
    const base = p.replace(/[\\/]+$/, '');
    const slash = Math.max(base.lastIndexOf('/'), base.lastIndexOf('\\'));
    const name = slash === -1 ? base : base.slice(slash + 1);
    const dot = name.lastIndexOf('.');
    if (dot <= 0) return '';
    return name.slice(dot + 1).toLowerCase();
  }

  function viewModeForExt(ext: string): 'file' | 'pdf' | 'image' | null {
    if (ext === 'md' || ext === 'markdown' || ext === 'mdx') return 'file';
    if (ext === 'pdf') return 'pdf';
    if (ext === 'png' || ext === 'jpg' || ext === 'jpeg' || ext === 'webp' || ext === 'gif') {
      return 'image';
    }
    return null;
  }

  /// Open the repo for a dropped OS path. If the path is a file, route to
  /// the matching content view (markdown / pdf / image). For directories or
  /// unrecognised extensions, leave the default Code-tab view.
  ///
  /// `isDirectory` is only known reliably under Tauri (we'd need a backend
  /// stat for browsers, but browser-mode never reaches this path). On the
  /// desktop side we infer directory-ness from the absence of an extension —
  /// good enough for the common drop targets (Finder folders, single files).
  async function handleDroppedPath(absPath: string, isDirectory: boolean): Promise<void> {
    const repoPath = isDirectory ? absPath : dirname(absPath);
    if (!repoPath) {
      errorMessage.set(`Cannot derive repository directory from: ${absPath}`);
      return;
    }
    followingMcp.set(false);
    await load(repoPath);
    if (get(errorMessage)) return; // load() already surfaced a message
    if (isDirectory) {
      viewMode.set('classes');
      return;
    }
    const ext = extOf(absPath);
    const target = viewModeForExt(ext);
    if (target === null) {
      // Source files / unknown extensions land on the Code tab.
      viewMode.set('classes');
      return;
    }
    fileView.update((cur) => ({
      path: absPath,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set(target);
  }

  function flashBrowserDropHint(message: string) {
    browserDropHint = message;
    if (browserDropHintTimer) clearTimeout(browserDropHintTimer);
    browserDropHintTimer = setTimeout(() => {
      browserDropHint = null;
      browserDropHintTimer = null;
    }, 6000);
  }

  // Browser-mode handlers — registered as DOM listeners on `window`.
  // We can't read absolute paths in browsers (the File API hides them), so we
  // intercept the drop, prevent the default navigation, and surface a hint.
  function onBrowserDragOver(ev: DragEvent) {
    if (!ev.dataTransfer) return;
    const types = Array.from(ev.dataTransfer.types ?? []);
    if (!types.includes('Files')) return;
    ev.preventDefault();
    dragOver = true;
  }

  function onBrowserDragLeave(ev: DragEvent) {
    // `dragleave` fires on every child node we cross. Only clear the overlay
    // when the cursor genuinely left the window — relatedTarget is null in
    // that case in Chromium/WebKit.
    if (ev.relatedTarget !== null) return;
    dragOver = false;
  }

  function onBrowserDrop(ev: DragEvent) {
    if (!ev.dataTransfer) return;
    const types = Array.from(ev.dataTransfer.types ?? []);
    if (!types.includes('Files')) return;
    ev.preventDefault();
    dragOver = false;
    flashBrowserDropHint(
      "Drag-and-drop opens a repo only in the desktop app — in browser mode, paste the absolute path into 'Open repo'.",
    );
  }

  onMount(async () => {
    window.addEventListener('keydown', onNavKey);
    browserMode = !isTauriRuntime();
    if (browserMode && !browserToken()) {
      browserAuthorized = false;
      return;
    }
    // Pick up wherever we left off (or whatever the MCP server has set since).
    try {
      const initial = await currentState();
      if (initial) await applyState(initial);
    } catch (err) {
      if (browserMode) {
        browserAuthorized = false;
        errorMessage.set(String(err));
        return;
      }
      throw err;
    }

    if (!browserMode) {
      unlistenState = await listen<UiState>('state-changed', (ev) => {
        void applyState(ev.payload);
      });
      // Register Tauri 2 webview-level drag-drop. Reports absolute paths
      // (which the browser DOM API hides) and fires enter/over/drop/leave.
      unlistenDragDrop = await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === 'enter' || p.type === 'over') {
          dragOver = true;
        } else if (p.type === 'leave') {
          dragOver = false;
        } else if (p.type === 'drop') {
          dragOver = false;
          const paths = p.paths ?? [];
          if (paths.length === 0) return;
          const first = paths[0];
          // Heuristic: if the path has no extension, treat it as a directory.
          // The OS sends absolute paths for both files and folders; on macOS
          // a folder name without a `.something` suffix is the typical case.
          const looksLikeDir = extOf(first) === '';
          void handleDroppedPath(first, looksLikeDir);
        }
      });
    } else {
      statePoll = setInterval(() => {
        void currentState()
          .then((s) => {
            if (s) return applyState(s);
            return undefined;
          })
          .catch((err) => {
            errorMessage.set(String(err));
          });
      }, BROWSER_STATE_POLL_MS);
      // Browser-mode visual + hint. We can't get absolute paths here, so
      // the goal is just: prevent the browser's default file-navigation,
      // mirror the same drag-over visual the desktop app shows, and tell
      // the user how to actually open a repo.
      window.addEventListener('dragenter', onBrowserDragOver);
      window.addEventListener('dragover', onBrowserDragOver);
      window.addEventListener('dragleave', onBrowserDragLeave);
      window.addEventListener('drop', onBrowserDrop);
    }
  });

  onDestroy(() => {
    window.removeEventListener('keydown', onNavKey);
    unlistenState?.();
    unlistenDragDrop?.();
    if (statePoll) clearInterval(statePoll);
    if (browserDropHintTimer) clearTimeout(browserDropHintTimer);
    if (browserMode) {
      window.removeEventListener('dragenter', onBrowserDragOver);
      window.removeEventListener('dragover', onBrowserDragOver);
      window.removeEventListener('dragleave', onBrowserDragLeave);
      window.removeEventListener('drop', onBrowserDrop);
    }
  });
</script>

<main>
  <header>
    <div class="brand">
      <div class="nav-history" role="group" aria-label="Navigation history">
        <button
          class="nav-arrow"
          disabled={!$nav_canBack || !$repo}
          on:click={navBack}
          title="{$t('nav.back') || 'Back'} (⌘[ / Alt+←)"
          aria-label={$t('nav.back') || 'Back'}
        >‹</button>
        <button
          class="nav-arrow"
          disabled={!$nav_canForward || !$repo}
          on:click={navForward}
          title="{$t('nav.forward') || 'Forward'} (⌘] / Alt+→)"
          aria-label={$t('nav.forward') || 'Forward'}
        >›</button>
      </div>
      <img class="logo" src="/logo.png" alt="ProjectMind" />
      <span class="title">ProjectMind</span>
      {#if $repo}
        <span class="repo" title={$repo.root}>
          <span class="repo-name">{basename($repo.root)}</span>
          <span class="repo-path">{$repo.root}</span>
        </span>
        <span class="status">
          <span class="dot"></span>
          {$t('status.repoCount', {
            files: $repo.classes,
            filesUnit: $t($repo.classes === 1 ? 'status.files.one' : 'status.files.other'),
            modules: $repo.modules,
            modulesUnit: $t($repo.modules === 1 ? 'status.modules.one' : 'status.modules.other'),
          })}
        </span>
      {:else}
        <span class="status">
          <span class="dot dim"></span>
          {$t('status.noRepo')}
        </span>
      {/if}
    </div>
    <nav>
      {#if $repo}
        {#each $repo.tabs as tab (tab.id)}
          <button
            class:active={isTabActive(tab, $viewMode)}
            on:click={() => activateTab(tab)}
          >
            {$t(tab.label_key)}
          </button>
        {/each}
      {:else}
        <button disabled>{codeTabLabel}</button>
        <button disabled>{$t('nav.diagrams')}</button>
      {/if}
      {#if $walkthroughCursor}
        <button
          class:active={$viewMode === 'walkthrough'}
          class="walkthrough-btn"
          on:click={() => viewMode.set('walkthrough')}
          title={$t('nav.walkthrough')}
        >
          ▶ {$t('nav.walkthrough')}
        </button>
      {/if}
      {#if $viewMode === 'diff'}
        <button class="active">{$t('nav.diff')}</button>
      {/if}
      {#if $followingMcp}
        <span class="follow" title={$t('nav.followingMcp.title')}>
          {$t('nav.followingMcp')}
        </span>
      {/if}
      {#if browserMode}
        <button on:click={forgetBrowserToken} title={$t('nav.token')}>{$t('nav.token')}</button>
      {/if}
      <button on:click={pickAndOpen} disabled={loading}>
        {loading ? $t('status.loading') : $t('nav.openRepo')}
      </button>
      <button
        class="lang-toggle"
        on:click={toggleLang}
        title={$t('nav.langToggle')}
        aria-label={$t('nav.langToggle')}
      >
        {$language.toUpperCase()}
      </button>
      <button
        class="theme-toggle"
        on:click={toggleTheme}
        title={theme === 'dark' ? $t('nav.themeToggle.toLight') : $t('nav.themeToggle.toDark')}
        aria-label={theme === 'dark' ? $t('nav.themeToggle.toLight') : $t('nav.themeToggle.toDark')}
      >
        {theme === 'dark' ? '☀' : '☾'}
      </button>
      <button
        class="kbd-help-toggle"
        on:click={() => (kbdHelpOpen = true)}
        title="{$t('keyboard.title')} (?)"
        aria-label={$t('keyboard.title')}
      >?</button>
    </nav>
  </header>

  {#if $errorMessage}
    <div class="error">⚠ {$errorMessage}</div>
  {/if}

  {#if browserDropHint}
    <div class="drop-hint" role="status">{browserDropHint}</div>
  {/if}

  {#if browserMode && !browserAuthorized}
    <section class="empty">
      <form class="token-panel" on:submit|preventDefault={useBrowserToken}>
        <img class="welcome-logo" src="/logo.png" alt="ProjectMind" />
        <h1>{$t('welcome.title')}</h1>
        <p class="claim">{$t('browserMode.banner')}</p>
        <label for="browser-token">{$t('browserMode.tokenLabel')}</label>
        <input
          id="browser-token"
          bind:value={tokenInput}
          autocomplete="off"
          spellcheck="false"
          placeholder={$t('browserMode.tokenLabel')}
        />
        <button type="submit">{$t('browserMode.tokenSubmit')}</button>
      </form>
    </section>
  {:else if !$repo}
    <section class="empty">
      <div class="welcome">
        <img class="welcome-logo" src="/logo.png" alt="ProjectMind" />
        <h1>{$t('welcome.title')}</h1>
        <p class="claim">{$t('welcome.tagline')}</p>
        <p class="by">{$t('welcome.by')}</p>
        <button on:click={pickAndOpen}>{$t('welcome.openButton')}</button>
        {#if !browserMode && $recentRepos.length > 0}
          <div class="recents">
            <h2>{$t('welcome.recent') || 'Recent'}</h2>
            <ul>
              {#each $recentRepos as r (r.path)}
                <li>
                  <button class="recent-row" on:click={() => load(r.path)} title={r.path}>
                    <span class="recent-name">{r.name}</span>
                    <span class="recent-meta">{r.classes} · {r.modules}m</span>
                    <span class="recent-path">{r.path}</span>
                  </button>
                  <button
                    class="recent-x"
                    on:click|stopPropagation={() => recents.forget(r.path)}
                    title={$t('welcome.forget') || 'Remove from list'}
                    aria-label="Remove {r.name} from recent list"
                  >×</button>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
        <p class="hint">
          {#if browserMode}
            {$t('welcome.hint.browser')}
          {:else}
            {$t('welcome.hint.tauri')}
          {/if}
        </p>
      </div>
    </section>
  {:else if $viewMode === 'classes' || $viewMode === 'pdf' || $viewMode === 'image' || $viewMode === 'md' || $viewMode === 'html' || $viewMode === 'file'}
    <section class="layout">
      <ModuleSidebar />
      <div
        class="resizer"
        use:resizable={{
          storageKey: 'projectmind.layout.code.col1',
          cssVar: '--code-col-1',
          min: 140,
          max: 480,
          initial: 220,
        }}
        title="Drag to resize · double-click to reset"
      ></div>
      {#if $viewMode === 'md'}
        <div class="files-fullspan">
          {#await lazyMarkdownIndex() then mod}
            <svelte:component this={mod.default} />
          {/await}
        </div>
      {:else if $viewMode === 'html'}
        <div class="files-fullspan">
          {#await lazyHtmlIndex() then mod}
            <svelte:component this={mod.default} />
          {/await}
        </div>
      {:else}
        <aside class="sidebar">
          {#if $packageFilter !== null}
            <div class="path-bar">
              <span class="path-label">{$t('files.package.label')}</span>
              <code class="path-value">{$packageFilter || '(default)'}</code>
              <button class="path-clear" on:click={() => packageFilter.set(null)} title={$t('files.package.clear')}>×</button>
            </div>
          {/if}
          <div class="filter">
            <button
              class="chip"
              class:active={$stereotypeFilter === null && $fileKindFilter === null}
              on:click={() => setFilter(null)}
            >
              {$t('files.filter.all')} <span class="count">{displayItems.length}</span>
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
            {#each fileKindsPresent as kind (kind)}
              <button
                class="chip kind"
                class:active={$fileKindFilter === kind}
                on:click={() => setKindFilter(kind)}
              >
                {kind} <span class="count">{$filteredModuleFiles.filter((f) => f.kind === kind).length}</span>
              </button>
            {/each}
            {#if $repo && $repo.markdown_count > 0}
              <button
                class="chip md"
                on:click={() => {
                  followingMcp.set(false);
                  viewMode.set('md');
                }}
                title={$t('files.filter.md.title')}
              >
                md <span class="count">{$repo.markdown_count}</span>
              </button>
            {/if}
            {#if $repo && $repo.html_count > 0}
              <button
                class="chip html"
                on:click={() => {
                  followingMcp.set(false);
                  viewMode.set('html');
                }}
                title={$t('files.filter.html.title')}
              >
                html <span class="count">{$repo.html_count}</span>
              </button>
            {/if}
          </div>
          <ul class="class-list" role="listbox" aria-label={$t('files.aria.list')}>
            {#each displayItems as item (item.kind === 'class' ? `class::${item.entry.module}::${item.entry.fqn}` : `file::${item.entry.abs}`)}
              {#if item.kind === 'class'}
                <li role="option" aria-selected={$selectedClass?.fqn === item.entry.fqn}>
                  <button
                    type="button"
                    class="class-row"
                    class:selected={$selectedClass?.fqn === item.entry.fqn}
                    on:click={() => handleSelect(item.entry)}
                  >
                    <span class="class-name">{item.entry.name}</span>
                    <span class="class-fqn">{item.entry.fqn}</span>
                    <span class="stereotypes">
                      {#each item.entry.stereotypes as s}
                        <span class="badge {s}">{s}</span>
                      {/each}
                    </span>
                  </button>
                </li>
              {:else}
                <li
                  role="option"
                  aria-selected={($viewMode === 'pdf' || $viewMode === 'image') &&
                    $fileView?.path === item.entry.abs}
                >
                  <button
                    type="button"
                    class="class-row file-row"
                    class:selected={($viewMode === 'pdf' || $viewMode === 'image') &&
                      $fileView?.path === item.entry.abs}
                    on:click={() => openModuleFile(item.entry)}
                    title={item.entry.abs}
                  >
                    <span class="class-name file-name">{item.entry.rel}</span>
                    <span class="stereotypes">
                      <span class="badge file-kind">{item.entry.kind}</span>
                    </span>
                  </button>
                </li>
              {/if}
            {/each}
          </ul>
        </aside>
        <div
          class="resizer"
          use:resizable={{
            storageKey: 'projectmind.layout.code.col2',
            cssVar: '--code-col-2',
            min: 220,
            max: 720,
            initial: 360,
          }}
          title="Drag to resize · double-click to reset"
        ></div>
        <main class="viewer">
          {#if $viewMode === 'pdf' && $fileView}
            <PdfView path={$fileView.path} />
          {:else if $viewMode === 'image' && $fileView}
            <ImageView path={$fileView.path} />
          {:else if $viewMode === 'file' && $fileView && /\.drawio$/i.test($fileView.path ?? '')}
            {#await lazyDrawIoView() then mod}
              <svelte:component this={mod.default} path={$fileView.path} />
            {/await}
          {:else if $viewMode === 'file' && $fileView}
            {#await lazyFileView() then mod}
              <svelte:component
                this={mod.default}
                path={$fileView.path}
                anchor={$fileView.anchor}
                nonce={$fileView.nonce}
              />
            {/await}
          {:else if $selectedClass}
            <ClassViewer
              klass={$selectedClass}
              source={classSource}
              meta={classMeta}
            />
          {:else}
            <div class="placeholder">{$t('files.placeholder')}</div>
          {/if}
        </main>
      {/if}
    </section>
  {:else if $viewMode === 'diagram'}
    <section class="diagram-view">
      <div class="diagram-tabs">
        {#each $repo.available_diagrams as d (d)}
          <button class:active={diagramKind === d} on:click={() => (diagramKind = d as typeof diagramKind)}>
            {diagramLabel(d)}
          </button>
        {/each}
        <span class="diagram-hint">{$t('diagram.hint')}</span>
      </div>
      {#await lazyDiagramView() then mod}
        <svelte:component this={mod.default} kind={diagramKind} folderLayout={folderMapLayout} />
      {/await}
    </section>
  {:else if $viewMode === 'walkthrough' && $walkthroughCursor}
    {#await lazyWalkthroughView() then mod}
      <svelte:component
        this={mod.default}
        cursorId={$walkthroughCursor.id}
        cursorStep={$walkthroughCursor.step}
        nonce={$walkthroughCursor.nonce}
      />
    {/await}
  {:else if $viewMode === 'diff' && $diffViewRef}
    <DiffView reference={$diffViewRef.reference} to={$diffViewRef.to} />
  {:else}
    <section class="empty">
      <div class="welcome">
        <p class="hint">{$t('welcome.empty')}</p>
      </div>
    </section>
  {/if}

  {#if dragOver}
    <div class="drop-overlay" aria-hidden="true">
      <div class="drop-overlay-inner">
        <div class="drop-overlay-icon">⤓</div>
        <div class="drop-overlay-text">{$t('drop.overlay')}</div>
      </div>
    </div>
  {/if}

  <KeyboardHelp bind:open={kbdHelpOpen} />
  <StatusBar />
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

  .nav-history {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-right: 4px;
  }
  .nav-arrow {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    color: var(--fg-1);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    transition: background 80ms ease, color 80ms ease, border-color 80ms ease;
  }
  .nav-arrow:hover:not(:disabled) {
    background: var(--bg-2);
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  .nav-arrow:disabled {
    opacity: 0.35;
    cursor: not-allowed;
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

  .token-panel {
    display: grid;
    gap: 12px;
    width: min(420px, calc(100vw - 40px));
    padding: 28px;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-radius: 8px;
    box-shadow: 0 18px 48px color-mix(in srgb, #000 28%, transparent);
  }

  .token-panel h1,
  .token-panel p {
    margin: 0;
    text-align: center;
  }

  .token-panel label {
    font-size: 12px;
    color: var(--fg-3);
  }

  .token-panel input {
    min-width: 0;
    padding: 10px 12px;
    color: var(--fg-1);
    background: var(--bg-0);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
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

  .kbd-help-toggle {
    width: 28px;
    padding: 6px 0;
    text-align: center;
    font-family: var(--mono);
    font-size: 13px;
    font-weight: 600;
    line-height: 1;
  }

  .lang-toggle {
    width: 36px;
    padding: 6px 0;
    text-align: center;
    font-family: var(--mono);
    font-size: 11px;
    font-weight: 600;
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

  .welcome p.claim {
    font-size: 16px;
    color: var(--accent-2);
    margin: 0 0 4px;
    font-weight: 500;
  }

  .welcome p.by {
    margin: 0 0 20px;
    color: var(--fg-2);
    font-size: 12px;
  }

  .recents {
    margin: 24px auto 8px;
    max-width: 540px;
    text-align: left;
  }
  .recents h2 {
    margin: 0 0 8px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
    font-weight: 600;
  }
  .recents ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .recents li {
    display: flex;
    align-items: stretch;
    gap: 4px;
  }
  .recent-row {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    gap: 2px 8px;
    flex: 1;
    text-align: left;
    background: var(--bg-1);
    border: 1px solid var(--bg-3);
    border-left: 3px solid transparent;
    border-radius: 4px;
    padding: 8px 12px;
    color: var(--fg-1);
    cursor: pointer;
    font: inherit;
    transition: background 80ms ease, border-left-color 80ms ease;
  }
  .recent-row:hover {
    background: var(--bg-2);
    border-left-color: var(--accent-2);
  }
  .recent-name {
    font-family: var(--mono);
    font-size: 13px;
    font-weight: 600;
    color: var(--fg-0);
  }
  .recent-meta {
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--fg-2);
    align-self: center;
  }
  .recent-path {
    grid-column: 1 / -1;
    font-family: var(--mono);
    font-size: 10.5px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recent-x {
    background: transparent;
    border: 1px solid var(--bg-3);
    border-radius: 4px;
    color: var(--fg-2);
    cursor: pointer;
    width: 28px;
    font-size: 16px;
    line-height: 1;
    padding: 0;
  }
  .recent-x:hover {
    color: var(--accent-2);
    border-color: var(--accent-2);
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

  /* When the Files tab hosts MD or HTML browsers (no class-list / viewer
     split), the embedded component should span the second resizer + the
     remaining three grid tracks so the right pane is one continuous
     surface rather than a tiny 360px column. */
  .files-fullspan {
    grid-column: 3 / -1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .files-fullspan > :global(*) {
    flex: 1;
    min-height: 0;
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

  /* File rows reuse the class-row layout but are visually distinguishable
     by the monospaced label and the extension badge in the stereotypes
     slot. The .badge.file-kind variant mirrors the muted-grey look used
     for unrecognised stereotype names. */
  .class-row.file-row .class-name.file-name {
    font-family: var(--mono);
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge.file-kind {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--bg-2);
    color: var(--fg-2);
    font-family: var(--mono);
  }

  /* File-kind filter pill sits next to the stereotype chips. Distinct
     muted background so it reads as "non-stereotype" without competing
     with the coloured stereotype chips. */
  .chip.kind {
    background: var(--bg-2);
    color: var(--fg-1);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* Markdown / HTML pills jump into a dedicated browser inside the same
     Files tab. Subtly tinted so they stand apart from stereotype + kind
     chips without screaming. */
  .chip.md {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-2));
    color: var(--fg-0);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .chip.html {
    background: color-mix(in srgb, var(--component) 22%, var(--bg-2));
    color: var(--fg-0);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .chip.md:hover,
  .chip.html:hover {
    border-color: var(--accent-2);
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

  /* ----- Drag-and-drop ---------------------------------------------------- */

  /* Full-window overlay shown while a drag is over the app. `pointer-events:
     none` keeps the underlying UI clickable when no drag is in progress
     (the overlay only renders when {#if dragOver}, but keeping it
     non-interactive is also useful for screen-reader / focus traps). */
  .drop-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    pointer-events: none;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent-2) 12%, transparent);
    border: 3px dashed var(--accent-2);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent-2) 50%, transparent);
    backdrop-filter: blur(2px);
    animation: drop-overlay-fade 120ms ease-out;
  }

  @keyframes drop-overlay-fade {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .drop-overlay-inner {
    text-align: center;
    padding: 28px 36px;
    background: color-mix(in srgb, var(--bg-1) 92%, transparent);
    border: 1px solid var(--accent-2);
    border-radius: 12px;
    box-shadow: 0 18px 48px color-mix(in srgb, #000 40%, transparent);
  }

  .drop-overlay-icon {
    font-size: 36px;
    line-height: 1;
    color: var(--accent-2);
    margin-bottom: 8px;
  }

  .drop-overlay-text {
    font-size: 16px;
    font-weight: 600;
    color: var(--fg-0);
  }

  /* Browser-mode hint that flashes after a (futile) drop so the user knows
     why nothing happened and where to go next. */
  .drop-hint {
    background: color-mix(in srgb, var(--accent-2) 18%, var(--bg-1));
    color: var(--fg-0);
    padding: 8px 16px;
    font-size: 12px;
    border-bottom: 1px solid var(--accent-2);
  }
</style>

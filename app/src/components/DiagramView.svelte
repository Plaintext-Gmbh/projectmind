<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { get } from 'svelte/store';
  import mermaid from 'mermaid';
  import DrawIoFrame from './DrawIoFrame.svelte';
  import { showDiagram, fileRecency, listChangesSince, revealInFileManager } from '../lib/api';
  import type { ChangedFile, ClassEntry, DiagramKind } from '../lib/api';
  import {
    classes,
    selectedClass,
    fileView,
    moduleFilter,
    packageFilter,
    stereotypeFilter,
    viewMode,
    repo,
    followingMcp,
  } from '../lib/store';
  import {
    recencyColor,
    authorColor,
    authorIdentity,
  } from '../lib/folderMapColors';
  import { wheelDelta } from '../lib/shiftWheelZoom';
  import {
    renderFolderMap,
    type FolderMap,
    type FolderMapNode,
    type FillFor,
  } from '../lib/diagrams/folderMap';
  import { renderDocGraph, type DocGraph } from '../lib/diagrams/docGraph';
  import { renderLanguageStats, type LanguageStats } from '../lib/diagrams/languageStats';
  import {
    renderArchitectureFlow,
    type ArchitectureFlow,
  } from '../lib/diagrams/architectureFlow';
  import { renderModuleChord, type ModuleChord } from '../lib/diagrams/moduleChord';
  import {
    renderActivityHeatmap,
    type ActivityHeatmap,
  } from '../lib/diagrams/activityHeatmap';
  import { createViewportStore } from '../lib/diagrams/viewport';

  export let kind: DiagramKind;
  export let folderLayout: 'hierarchy' | 'solar' | 'td' = 'solar';
  /// Optional base ref for two-ref compare mode. When set, the folder-map is
  /// rendered with diff-overlay between `compareWith` (base) and `diffRef`
  /// (target), and the toolbar's colour-by / diff-ref controls are hidden —
  /// they're driven by the embedding view instead.
  export let compareWith: string | null = null;

  let stage: HTMLDivElement;
  let mermaidSource = '';
  let svg = '';
  let drawIoXml = '';
  let docGraph: DocGraph | null = null;
  let folderMap: FolderMap | null = null;
  let languageStats: LanguageStats | null = null;
  let architectureFlow: ArchitectureFlow | null = null;
  let moduleChord: ModuleChord | null = null;
  let activityHeatmap: ActivityHeatmap | null = null;
  let selectedDocId: string | null = null;
  let selectedFolderNode: FolderMapNode | null = null;
  let revealError: string | null = null;
  let docGraphLayout: 'network' | 'radial' | 'orphans' = 'network';
  let loading = false;
  let error: string | null = null;

  $: selectedDoc = docGraph?.nodes.find((n) => n.id === selectedDocId) ?? null;
  $: selectedOutgoing = selectedDocId
    ? docGraph?.edges.filter((e) => e.from === selectedDocId) ?? []
    : [];
  $: selectedIncoming = selectedDocId
    ? docGraph?.edges.filter((e) => e.to === selectedDocId) ?? []
    : [];
  $: selectedDangling = selectedDocId
    ? docGraph?.dangling.filter((d) => d.from === selectedDocId) ?? []
    : [];

  // ----- Folder-map colour-by state ----------------------------------------

  // 'structure' (default) keeps the existing per-kind palette; 'recency'
  // tints each leaf by how long ago it was last touched; 'author' tints
  // by the most-recent committer; 'diff' overlays git status against a
  // ref. Folders inherit the most-prominent fact from their descendants
  // so each git-driven mode stays consistent.
  // Persisted per-browser so the user's preference sticks across sessions.
  type ColorBy = 'structure' | 'recency' | 'author' | 'diff';
  const COLOR_BY_KEY = 'projectmind.diagram.folderMap.colorBy';
  const DIFF_REF_KEY = 'projectmind.diagram.folderMap.diffRef';
  let colorBy: ColorBy = readColorByPref();
  export let diffRef: string = readDiffRefPref();
  /// Effective colour mode after compare-mode override. When `compareWith`
  /// is set the diagram is forced to diff-overlay regardless of the user's
  /// stored preference — that's the whole point of the embedded compare
  /// view. Standalone usage falls back to the persisted `colorBy`.
  $: effectiveColorBy = compareWith ? ('diff' as ColorBy) : colorBy;
  /// Per-path git fact: how long ago + who. Both recency and author modes
  /// read from this single cache so toggling between them doesn't re-fetch.
  /// `null` means "haven't loaded yet"; an empty Map means "loaded, repo
  /// has no git history".
  type GitFact = { secs_ago: number; author: string | null };
  let factsByPath: Map<string, GitFact> | null = null;
  let factsForRoot: string | null = null;
  let gitError: string | null = null;

  /// Per-path change status against `diffRef`. Same load-once-per-(repo,ref)
  /// caching shape as the recency/author cache above. `null` = not loaded;
  /// empty Map = loaded but no changes (clean working tree at that ref).
  type DiffStatus = ChangedFile['status'];
  let changesByPath: Map<string, DiffStatus> | null = null;
  let changesForKey: string | null = null;
  let diffError: string | null = null;

  function readColorByPref(): ColorBy {
    try {
      const v = localStorage.getItem(COLOR_BY_KEY);
      if (v === 'recency' || v === 'author' || v === 'structure' || v === 'diff') return v;
    } catch {
      // localStorage unavailable
    }
    return 'structure';
  }

  function writeColorByPref(v: ColorBy) {
    try {
      localStorage.setItem(COLOR_BY_KEY, v);
    } catch {
      // ignore
    }
  }

  function readDiffRefPref(): string {
    try {
      const v = localStorage.getItem(DIFF_REF_KEY);
      if (v && v.trim()) return v.trim();
    } catch {
      // localStorage unavailable
    }
    return 'HEAD~1';
  }

  function writeDiffRefPref(v: string) {
    try {
      localStorage.setItem(DIFF_REF_KEY, v);
    } catch {
      // ignore
    }
  }

  function setColorBy(v: ColorBy) {
    if (colorBy === v) return;
    colorBy = v;
    writeColorByPref(v);
    void render(kind, folderLayout, docGraphLayout);
  }

  function setDiffRef(v: string) {
    const next = v.trim();
    if (!next || next === diffRef) return;
    diffRef = next;
    writeDiffRefPref(next);
    // Force a re-fetch on the next render — the cache key includes the ref,
    // so changing it invalidates the prior load automatically, but we want
    // the new fetch to happen now rather than on the next colorBy toggle.
    changesByPath = null;
    changesForKey = null;
    if (colorBy === 'diff') void render(kind, folderLayout, docGraphLayout);
  }

  /// Fetch the per-file git facts (recency + author) for the current repo
  /// if we haven't already, then trigger a re-render. Best-effort: failure
  /// is non-fatal — the diagram falls back to structure colouring and the
  /// toolbar shows a "git unavailable" hint.
  async function ensureGitFactsForCurrentRepo(): Promise<void> {
    const root = $repo?.root ?? null;
    if (!root) return;
    if (factsForRoot === root && factsByPath !== null) return;
    try {
      const items = await fileRecency();
      const map = new Map<string, GitFact>();
      for (const item of items) {
        // Normalise Windows backslashes so the lookup matches the
        // forward-slash ids the folder-map renderer uses.
        map.set(item.path.replace(/\\/g, '/'), {
          secs_ago: item.secs_ago,
          author: authorIdentity(item.author_name, item.author_email),
        });
      }
      factsByPath = map;
      factsForRoot = root;
      gitError = null;
    } catch (err) {
      gitError = String(err);
      factsByPath = new Map();
      factsForRoot = root;
    }
  }

  /// Fetch (and cache) the per-file change status against `diffRef` for the
  /// current repo. Cache key combines the repo root and the ref so flipping
  /// either invalidates correctly. Best-effort: a failure (bad ref, no git)
  /// surfaces the message in the toolbar; the diagram falls back to the
  /// structure palette.
  async function ensureChangesForCurrentRepo(): Promise<void> {
    const root = $repo?.root ?? null;
    if (!root) return;
    // Cache key encodes both refs: in standalone mode the FROM half is
    // empty (single-ref vs working tree); in compare mode it carries the
    // base ref so flipping either side invalidates correctly.
    const key = `${root}@${compareWith ?? ''}..${diffRef}`;
    if (changesForKey === key && changesByPath !== null) return;
    try {
      const items = compareWith
        ? await listChangesSince(compareWith, diffRef)
        : await listChangesSince(diffRef);
      const map = new Map<string, DiffStatus>();
      for (const item of items) {
        map.set(item.path.replace(/\\/g, '/'), item.status);
      }
      changesByPath = map;
      changesForKey = key;
      diffError = null;
    } catch (err) {
      diffError = String(err);
      changesByPath = new Map();
      changesForKey = key;
    }
  }

  /// Compare-mode prop changes from outside invalidate the diff cache and
  /// trigger a re-render. Standalone changes go through `setDiffRef` so
  /// we don't double-invoke here.
  $: if (compareWith !== null) {
    void onCompareRefsChanged(compareWith, diffRef);
  }
  async function onCompareRefsChanged(_from: string, _to: string): Promise<void> {
    changesByPath = null;
    changesForKey = null;
    await render(kind, folderLayout, docGraphLayout);
  }

  // Viewport (pan / zoom) state now lives in a small per-instance Svelte
  // store (`lib/diagrams/viewport.ts`) so the Mini-Map (#66) can read the
  // viewport rectangle and drive pan/zoom through the same pure reducers the
  // stage uses. `scale`/`tx`/`ty` are the live transform; `baseW`/`baseH`
  // are the fit-to-stage SVG size at scale 1, stamped once per render.
  //
  // The base size still drives an explicit width/height on the SVG (see
  // `applyBaseSize`) — the previous approach of mutating SVG width/height on
  // every wheel tick was crisper at extreme zoom but unreliable under
  // WKWebView (querySelector('svg') sometimes missed the freshly-attached
  // node after HMR / async render, so the wrapper translated without the SVG
  // scaling — "ganze Panel verschoben, aber nicht skaliert"). The CSS
  // transform below always applies regardless of when the SVG mounts.
  const viewport = createViewportStore();
  $: scale = $viewport.scale;
  $: tx = $viewport.tx;
  $: ty = $viewport.ty;
  $: baseW = $viewport.baseW;
  $: baseH = $viewport.baseH;

  let dragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartTx = 0;
  let dragStartTy = 0;

  // Combined translate + scale for the .diagram wrapper. Reactive so any
  // tx/ty/scale update lands in one transform string.
  $: diagramTransform = `translate(${tx}px, ${ty}px) scale(${scale})`;

  $: if (kind) {
    void render(kind, folderLayout, docGraphLayout);
  }

  onMount(() => {
    mermaid.initialize({
      startOnLoad: false,
      theme: 'dark',
      securityLevel: 'loose',
      // Large repositories produce diagrams well past Mermaid's defaults
      // (50 000 chars / 500 edges). Allow up to ~1 MB and 10 000 edges.
      maxTextSize: 1_000_000,
      maxEdges: 10_000,
      // Render labels as SVG <text> instead of HTML inside <foreignObject>.
      // HTML labels rasterise once and scale as a bitmap when the SVG is
      // resized — SVG text re-renders crisply at any zoom level.
      flowchart: { htmlLabels: false, useMaxWidth: false },
      class: { htmlLabels: false, useMaxWidth: false },
    });
    // Mermaid `click N call onNodeClick("kind","module","target")` directives
    // resolve against window. Drilldown: class → open it, package → filter
    // the class list, module → filter by module.
    (window as unknown as Record<string, unknown>).onNodeClick = handleNodeClick;
  });

  onDestroy(() => {
    delete (window as unknown as Record<string, unknown>).onNodeClick;
  });

  function handleNodeClick(kind: string, moduleId: string, target: string) {
    if (kind === 'class') {
      const match = get(classes).find(
        (c: ClassEntry) => c.module === moduleId && c.fqn === target,
      );
      if (match) {
        selectedClass.set(match);
        viewMode.set('classes');
      }
    } else if (kind === 'package') {
      moduleFilter.set(moduleId);
      packageFilter.set(target);
      stereotypeFilter.set(null);
      selectedClass.set(null);
      viewMode.set('classes');
    }
  }

  function resetView() {
    viewport.reset();
  }

  function openFolderNode(path: string, nodeKind: string) {
    if (nodeKind !== 'file') return;
    followingMcp.set(false);
    fileView.update((cur) => ({
      path,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set('file');
  }

  function showFolderInfo(node: FolderMapNode) {
    selectedFolderNode = node;
    revealError = null;
  }

  function closeFolderInfo() {
    selectedFolderNode = null;
    revealError = null;
  }

  async function revealSelectedFolder() {
    if (!selectedFolderNode) return;
    revealError = null;
    try {
      await revealInFileManager(selectedFolderNode.path);
    } catch (err) {
      revealError = String(err);
    }
  }

  async function render(
    k: DiagramKind,
    layout: 'hierarchy' | 'solar' | 'td',
    docLayout: 'network' | 'radial' | 'orphans',
  ) {
    loading = true;
    error = null;
    try {
      // Fetch git facts (recency + author) when the user is looking at
      // the folder map in either git-driven mode. Cached per repo root,
      // so toggling between R and A re-renders without a re-fetch.
      const wantGitFacts =
        k === 'folder-map' && (effectiveColorBy === 'recency' || effectiveColorBy === 'author');
      if (wantGitFacts) await ensureGitFactsForCurrentRepo();
      const wantChanges = k === 'folder-map' && effectiveColorBy === 'diff';
      if (wantChanges) await ensureChangesForCurrentRepo();

      const payload = await showDiagram(k);
      docGraph = null;
      drawIoXml = '';
      if (k !== 'folder-map') {
        folderMap = null;
        selectedFolderNode = null;
      }
      if (k !== 'language-stats') {
        languageStats = null;
      }
      if (k !== 'architecture-flow') {
        architectureFlow = null;
      }
      if (k !== 'module-chord') {
        moduleChord = null;
      }
      if (k !== 'activity-heatmap') {
        activityHeatmap = null;
      }
      if (k === 'folder-map') {
        mermaidSource = '';
        folderMap = JSON.parse(payload) as FolderMap;
        if (
          selectedFolderNode &&
          !folderMap.nodes.some((n) => n.path === selectedFolderNode!.path)
        ) {
          selectedFolderNode = null;
        }
        svg = renderFolderMap(folderMap, layout, buildFillFor(folderMap));
      } else if (k === 'doc-graph') {
        mermaidSource = '';
        docGraph = JSON.parse(payload) as DocGraph;
        if (selectedDocId && !docGraph.nodes.some((n) => n.id === selectedDocId)) {
          selectedDocId = null;
        }
        svg = renderDocGraph(docGraph, docLayout, selectedDocId);
      } else if (k === 'architecture-layers') {
        mermaidSource = '';
        svg = '';
        drawIoXml = payload;
      } else if (k === 'language-stats') {
        mermaidSource = '';
        languageStats = JSON.parse(payload) as LanguageStats;
        svg = renderLanguageStats(languageStats);
      } else if (k === 'architecture-flow') {
        mermaidSource = '';
        architectureFlow = JSON.parse(payload) as ArchitectureFlow;
        svg = renderArchitectureFlow(architectureFlow);
      } else if (k === 'module-chord') {
        mermaidSource = '';
        moduleChord = JSON.parse(payload) as ModuleChord;
        svg = renderModuleChord(moduleChord);
      } else if (k === 'activity-heatmap') {
        mermaidSource = '';
        activityHeatmap = JSON.parse(payload) as ActivityHeatmap;
        svg = renderActivityHeatmap(activityHeatmap);
      } else {
        mermaidSource = payload;
        const id = `mermaid-${Date.now()}`;
        const result = await mermaid.render(id, mermaidSource);
        svg = result.svg;
      }
      resetView();
      await tick();
      if (drawIoXml) return;
      const node = stage?.querySelector('svg') as SVGSVGElement | null;
      if (node) {
        // Drop Mermaid's inline width/maxWidth so we control sizing.
        node.removeAttribute('style');
        // Compute fit-to-stage at scale=1 from the SVG's viewBox aspect ratio.
        const vb = (node.getAttribute('viewBox') ?? '').split(/\s+/).map(Number);
        const [, , vbW = 0, vbH = 0] = vb;
        const sw = stage?.clientWidth ?? 0;
        const sh = stage?.clientHeight ?? 0;
        if (vbW > 0 && vbH > 0 && sw > 0 && sh > 0) {
          const fit = Math.min(sw / vbW, sh / vbH);
          viewport.setBaseSize(vbW * fit, vbH * fit);
        } else {
          viewport.setBaseSize(sw, sh);
        }
        node.style.display = 'block';
        applyBaseSize();
      }
    } catch (err) {
      error = String(err);
      svg = '';
    } finally {
      loading = false;
    }
  }

  /// Build the per-render fill resolver from the current colour-by state.
  /// The pure `renderFolderMap` (in `lib/diagrams/folderMap.ts`) calls this
  /// resolver per node; it lives here because it depends on the fetched git
  /// facts / diff status the component owns.
  function buildFillFor(map: FolderMap): FillFor {
    if (effectiveColorBy === 'diff') {
      return buildDiffFillFor(map);
    }
    if (
      (effectiveColorBy !== 'recency' && effectiveColorBy !== 'author') ||
      !factsByPath ||
      factsByPath.size === 0
    ) {
      return () => null;
    }
    // Aggregate per id: each node inherits the GitFact of its most recent
    // descendant (min secs_ago across the subtree). Recency mode reads the
    // age, author mode reads the author identity — both stay consistent
    // because they share the same "winning leaf".
    const byParent = new Map<string, FolderMapNode[]>();
    for (const n of map.nodes) {
      if (!n.parent) continue;
      const arr = byParent.get(n.parent) ?? [];
      arr.push(n);
      byParent.set(n.parent, arr);
    }
    const factById = new Map<string, GitFact>();
    const facts = factsByPath;
    function visit(node: FolderMapNode): GitFact | null {
      if (node.kind === 'file') {
        const f = facts.get(node.id) ?? null;
        if (f) factById.set(node.id, f);
        return f;
      }
      const children = byParent.get(node.id) ?? [];
      let best: GitFact | null = null;
      for (const c of children) {
        const v = visit(c);
        if (v !== null && (best === null || v.secs_ago < best.secs_ago)) best = v;
      }
      if (best !== null) factById.set(node.id, best);
      return best;
    }
    const rootNode = map.nodes.find((n) => n.parent === null);
    if (rootNode) visit(rootNode);

    if (effectiveColorBy === 'recency') {
      return (id) => {
        const f = factById.get(id);
        return f ? recencyColor(f.secs_ago) : null;
      };
    }
    // author mode
    return (id) => {
      const f = factById.get(id);
      if (!f || !f.author) return null;
      return authorColor(f.author);
    };
  }

  /// Pick the most prominent change status from a node and its descendants.
  /// Priority deleted > added > renamed > modified > type_change > other —
  /// "things vanished" is the first thing a reviewer needs to notice, fresh
  /// files are next, in-place edits last. Folders adopt the winning status
  /// so a tinted parent says "something interesting happened in here";
  /// untouched files / folders stay null and fall back to the structure
  /// palette.
  function buildDiffFillFor(map: FolderMap): FillFor {
    if (!changesByPath || changesByPath.size === 0) {
      return () => null;
    }
    const byParent = new Map<string, FolderMapNode[]>();
    for (const n of map.nodes) {
      if (!n.parent) continue;
      const arr = byParent.get(n.parent) ?? [];
      arr.push(n);
      byParent.set(n.parent, arr);
    }
    const changes = changesByPath;
    const statusById = new Map<string, DiffStatus>();
    function visit(node: FolderMapNode): DiffStatus | null {
      if (node.kind === 'file') {
        const s = changes.get(node.id) ?? null;
        if (s) statusById.set(node.id, s);
        return s;
      }
      const children = byParent.get(node.id) ?? [];
      let best: DiffStatus | null = null;
      for (const c of children) {
        const v = visit(c);
        if (v !== null && (best === null || diffPriority(v) > diffPriority(best))) best = v;
      }
      if (best !== null) statusById.set(node.id, best);
      return best;
    }
    const rootNode = map.nodes.find((n) => n.parent === null);
    if (rootNode) visit(rootNode);
    return (id, kind) => {
      const s = statusById.get(id);
      if (!s) return null;
      // Folders get a dimmed version so the eye still tracks the leaves
      // as the primary signal; root + plain folders read as "contains
      // something changed" without competing with their changed children.
      return diffColor(s, kind !== 'file');
    };
  }

  /// Rank statuses for parent aggregation — deleted wins so a vanished file
  /// is never visually masked by a sibling rename or modification.
  function diffPriority(s: DiffStatus): number {
    switch (s) {
      case 'deleted':
        return 5;
      case 'added':
        return 4;
      case 'renamed':
        return 3;
      case 'modified':
        return 2;
      case 'type_change':
        return 1;
      case 'other':
      default:
        return 0;
    }
  }

  /// Status palette. Hues match the conventional "red = removed, green =
  /// added, amber = changed" review vocabulary so the legend doesn't have
  /// to be looked up. `dim` drops saturation + lifts lightness for folder
  /// aggregates so files still pop against their containers.
  function diffColor(status: DiffStatus, dim: boolean): string {
    const palette: Record<DiffStatus, [number, number, number]> = {
      added: [140, 60, 45],
      modified: [35, 80, 50],
      deleted: [0, 65, 50],
      renamed: [270, 50, 55],
      type_change: [200, 50, 50],
      other: [220, 20, 50],
    };
    const [h, s, l] = palette[status];
    if (dim) {
      return `hsl(${h}, ${Math.max(20, s - 25)}%, ${Math.min(75, l + 18)}%)`;
    }
    return `hsl(${h}, ${s}%, ${l}%)`;
  }

  function openDoc(path: string) {
    // The user is taking an explicit action; if we were mirroring an MCP
    // intent, stop doing so or applyState will clobber the view shortly.
    followingMcp.set(false);
    fileView.update((cur) => ({
      path,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set('file');
  }

  function jumpToDoc(docId: string) {
    selectedDocId = docId;
    const target = docGraph?.nodes.find((n) => n.id === docId);
    if (target) openDoc(target.abs);
  }

  function onClick(e: MouseEvent) {
    if (kind !== 'folder-map' && kind !== 'doc-graph') return;
    const target = e.target as Element | null;
    const node = target?.closest?.('.node') as SVGGElement | null;
    if (!node) return;
    if (kind === 'folder-map') {
      const path = node.dataset.path;
      const nodeKind = node.dataset.kind;
      if (!path || !nodeKind) return;
      if (nodeKind === 'file') {
        openFolderNode(path, nodeKind);
        return;
      }
      // folder / root → show info popover with "reveal in file manager"
      const fmNode = folderMap?.nodes.find((n) => n.path === path) ?? null;
      if (fmNode) showFolderInfo(fmNode);
      return;
    }
    const docId = node.dataset.id;
    if (docId) {
      // Click on a doc-graph node opens the info panel only. The user must
      // press the "Open" button in the panel to actually navigate into the
      // file. Auto-opening on the first click duplicates the Open button,
      // makes the user lose the diagram view they were exploring, and used
      // to race with applyState (which clobbered the freshly-set viewMode
      // back to 'diagram', leaving the impression that nothing reacted).
      selectedDocId = docId;
    }
  }

  function applyBaseSize() {
    // Stamp the SVG at scale=1 once per render so it fills the stage; live
    // zoom then happens via `diagramTransform`. Safe to call multiple times
    // — idempotent for a given baseW/baseH and current SVG node. Reads the
    // base size straight from the store (`get`) rather than the reactive
    // `$:` locals, since this runs synchronously right after
    // `viewport.setBaseSize()` — before Svelte flushes the reactive update.
    const { baseW: bw, baseH: bh } = get(viewport);
    if (!stage || !bw || !bh) return;
    const node = stage.querySelector('svg');
    if (!node) return;
    node.setAttribute('width', String(bw));
    node.setAttribute('height', String(bh));
  }

  function onWheel(e: WheelEvent) {
    // Plain wheel = zoom (matches user expectation from every IDE / map app).
    // Shift+wheel also zooms, for parity with the text/code viewers across
    // the rest of the app — the user-facing rule is "Shift+Wheel zooms in
    // every viewer". `wheelDelta` handles the macOS axis-swap so the same
    // gesture works on Linux/Windows where deltaY survives intact.
    // `preventDefault` only works when the listener is registered as
    // non-passive, which `nonPassiveWheel` (below) guarantees.
    if (e.cancelable) e.preventDefault();
    e.stopPropagation();
    const delta = wheelDelta(e);
    if (delta === 0) return;
    const rect = stage.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = Math.exp(-delta * 0.0015);
    // Zoom toward cursor: the store reducer keeps the world-point under the
    // cursor stable and clamps scale to [MIN_SCALE, MAX_SCALE]. The
    // `diagramTransform` reactive picks scale + tx + ty up in a single
    // transform string applied via inline style — no querySelector needed.
    viewport.zoomAround(factor, cx, cy);
  }

  // Svelte's `on:wheel` registers a passive listener on browsers that
  // default `wheel` to passive (Chrome on document scrollers, some
  // embeddings). A passive listener silently ignores `preventDefault`,
  // which means the browser keeps its default scroll behaviour — the
  // exact "wheel only scrolls left/right" symptom users were seeing.
  // This action explicitly registers a non-passive listener so the zoom
  // handler can suppress the default scroll.
  function nonPassiveWheel(node: HTMLDivElement, handler: (e: WheelEvent) => void) {
    let current = handler;
    const fn = (e: WheelEvent) => current(e);
    node.addEventListener('wheel', fn, { passive: false });
    return {
      update(next: (e: WheelEvent) => void) {
        current = next;
      },
      destroy() {
        node.removeEventListener('wheel', fn);
      },
    };
  }

  function onMouseDown(e: MouseEvent) {
    if ((e.target as Element | null)?.closest?.('.node.file,.doc-node')) return;
    if (e.button !== 0) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartTx = tx;
    dragStartTy = ty;
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return;
    viewport.panTo(
      dragStartTx + (e.clientX - dragStartX),
      dragStartTy + (e.clientY - dragStartY),
    );
  }

  function endDrag() {
    dragging = false;
  }

  function zoomBy(factor: number) {
    if (!stage) return;
    const rect = stage.getBoundingClientRect();
    // Toolbar zoom anchors on the stage centre.
    viewport.zoomAround(factor, rect.width / 2, rect.height / 2);
  }
</script>

<div class="root">
  <div class="toolbar">
    <button on:click={() => zoomBy(1.25)} title="Zoom in" disabled={!!drawIoXml}>＋</button>
    <button on:click={() => zoomBy(0.8)} title="Zoom out" disabled={!!drawIoXml}>－</button>
    <button on:click={resetView} title="Reset view" disabled={!!drawIoXml}>⌂</button>
    {#if kind === 'folder-map'}
      <span class="divider"></span>
      <button
        class:active={folderLayout === 'solar'}
        on:click={() => (folderLayout = 'solar')}
        title="Solar layout"
      >☉</button>
      <button
        class:active={folderLayout === 'hierarchy'}
        on:click={() => (folderLayout = 'hierarchy')}
        title="Hierarchy layout"
      >H</button>
      <button
        class:active={folderLayout === 'td'}
        on:click={() => (folderLayout = 'td')}
        title="Top-down layout"
      >TD</button>
      {#if !compareWith}
        <span class="divider"></span>
        <button
          class:active={colorBy === 'structure'}
          on:click={() => setColorBy('structure')}
          title="Colour by node kind (default)"
          aria-pressed={colorBy === 'structure'}
        >S</button>
        <button
          class:active={colorBy === 'recency'}
          on:click={() => setColorBy('recency')}
          title="Colour by last commit recency — recent files glow, stale ones recede"
          aria-pressed={colorBy === 'recency'}
        >R</button>
        <button
          class:active={colorBy === 'author'}
          on:click={() => setColorBy('author')}
          title="Colour by last committer — same author always gets the same hue"
          aria-pressed={colorBy === 'author'}
        >A</button>
        <button
          class:active={colorBy === 'diff'}
          on:click={() => setColorBy('diff')}
          title="Overlay git status against a ref — added / modified / deleted"
          aria-pressed={colorBy === 'diff'}
        >D</button>
        {#if colorBy === 'diff'}
          <input
            class="diff-ref"
            type="text"
            spellcheck="false"
            autocomplete="off"
            value={diffRef}
            title="Git ref to compare against (e.g. HEAD~1, main, a1b2c3d)"
            on:change={(e) => setDiffRef((e.target as HTMLInputElement).value)}
            on:keydown={(e) => {
              if (e.key === 'Enter') setDiffRef((e.target as HTMLInputElement).value);
            }}
          />
        {/if}
        {#if (colorBy === 'recency' || colorBy === 'author') && gitError}
          <span class="hint" style="color: var(--error);" title={gitError}>⚠ git unavailable</span>
        {/if}
        {#if colorBy === 'diff' && diffError}
          <span class="hint" style="color: var(--error);" title={diffError}>⚠ git unavailable</span>
        {/if}
      {:else if diffError}
        <span class="hint" style="color: var(--error);" title={diffError}>⚠ git unavailable</span>
      {/if}
    {:else if kind === 'doc-graph'}
      <span class="divider"></span>
      <button
        class:active={docGraphLayout === 'network'}
        on:click={() => (docGraphLayout = 'network')}
        title="Network layout"
      >N</button>
      <button
        class:active={docGraphLayout === 'radial'}
        on:click={() => (docGraphLayout = 'radial')}
        title="Radial layout"
      >R</button>
      <button
        class:active={docGraphLayout === 'orphans'}
        on:click={() => (docGraphLayout = 'orphans')}
        title="Orphans layout"
      >O</button>
    {/if}
    <span class="zoom-readout">{Math.round(scale * 100)}%</span>
    {#if kind === 'doc-graph' && docGraph}
      <span class="doc-summary">
        {docGraph.nodes.length} docs · {docGraph.edges.length} links · {docGraph.orphan_count} orphans · {docGraph.dangling_count} dangling
      </span>
    {:else if kind === 'architecture-layers'}
      <span class="doc-summary">draw.io architecture layer map — use the embedded toolbar for zoom/pan</span>
    {:else if kind === 'language-stats' && languageStats}
      <span class="doc-summary">
        {languageStats.total_files} Dateien · {languageStats.buckets.length} Buckets
      </span>
    {:else if kind === 'architecture-flow' && architectureFlow}
      <span class="doc-summary">
        {architectureFlow.total_classes} Klassen · {architectureFlow.layers.length} Schichten · {architectureFlow.cross_module_edges} Cross-Module
      </span>
    {:else if kind === 'module-chord' && moduleChord}
      <span class="doc-summary">
        {moduleChord.modules.length} Module · {moduleChord.edges.length} Kanten · {moduleChord.total_relations} Beziehungen
      </span>
    {:else if kind === 'activity-heatmap' && activityHeatmap}
      <span class="doc-summary">
        {activityHeatmap.total_commits} Commits · {activityHeatmap.distinct_authors} Autoren · Streak {activityHeatmap.longest_streak_days}d
      </span>
    {/if}
    {#if !drawIoXml}
      <span class="hint">Drag to pan • Wheel or Shift+Wheel to zoom</span>
    {/if}
  </div>
  {#if loading}
    <div class="placeholder">Rendering diagram…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
    <pre>{mermaidSource}</pre>
  {:else if drawIoXml}
    <div class="drawio-stage">
      <DrawIoFrame xml={drawIoXml} title="draw.io architecture layer map" />
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="stage"
      class:dragging
      bind:this={stage}
      use:nonPassiveWheel={onWheel}
      on:click={onClick}
      on:mousedown={onMouseDown}
      on:mousemove={onMouseMove}
      on:mouseup={endDrag}
      on:mouseleave={endDrag}
      role="img"
      aria-label="Diagram canvas (drag to pan, wheel to zoom)"
    >
      <div
        class="diagram"
        style="transform: {diagramTransform}; transform-origin: 0 0;"
      >
        {@html svg}
      </div>
      {#if kind === 'doc-graph' && docGraph}
        <aside class="doc-panel">
          {#if selectedDoc}
            <header>
              <h3>{selectedDoc.title}</h3>
              <button on:click={() => openDoc(selectedDoc.abs)}>Open</button>
            </header>
            <div class="doc-path" title={selectedDoc.rel}>{selectedDoc.rel}</div>
            <div class="metrics">
              <span>{selectedDoc.inbound} in</span>
              <span>{selectedDoc.outbound} out</span>
              <span>{selectedDoc.external} external</span>
            </div>
            {#if selectedIncoming.length > 0}
              <h4>Linked from</h4>
              <ul>
                {#each selectedIncoming.slice(0, 8) as edge}
                  <li>
                    <button on:click={() => jumpToDoc(edge.from)}>{edge.from}</button>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if selectedOutgoing.length > 0}
              <h4>Links to</h4>
              <ul>
                {#each selectedOutgoing.slice(0, 8) as edge}
                  <li>
                    <button on:click={() => jumpToDoc(edge.to)}>{edge.to}</button>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if selectedDangling.length > 0}
              <h4>Dangling</h4>
              <ul>
                {#each selectedDangling.slice(0, 6) as link}
                  <li title={link.resolved}>{link.href}</li>
                {/each}
              </ul>
            {/if}
          {:else}
            <header>
              <h3>Documentation graph</h3>
            </header>
            <div class="metrics stacked">
              <span>{docGraph.nodes.length} documents</span>
              <span>{docGraph.edges.length} internal links</span>
              <span>{docGraph.orphan_count} orphans</span>
              <span>{docGraph.dangling_count} dangling links</span>
              <span>{docGraph.external_count} external links</span>
            </div>
          {/if}
        </aside>
      {/if}
      {#if kind === 'folder-map' && selectedFolderNode}
        <aside class="doc-panel">
          <header>
            <h3>{selectedFolderNode.label}</h3>
            <button on:click={closeFolderInfo} title="Close">×</button>
          </header>
          <div class="doc-path" title={selectedFolderNode.path}>{selectedFolderNode.path}</div>
          <div class="metrics">
            <span>{selectedFolderNode.kind}</span>
            <span>depth {selectedFolderNode.depth}</span>
            <span>{selectedFolderNode.weight} files</span>
          </div>
          <div class="actions">
            <button on:click={revealSelectedFolder}>Im Finder zeigen</button>
          </div>
          {#if revealError}
            <div class="reveal-error">{revealError}</div>
          {/if}
        </aside>
      {/if}
    </div>
  {/if}
</div>

<style>
  .root {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg-0);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--bg-3);
    flex-shrink: 0;
  }

  .toolbar button {
    width: 28px;
    height: 28px;
    padding: 0;
    font-size: 14px;
    line-height: 1;
  }

  .toolbar button.active {
    color: var(--accent-2);
    border-color: var(--accent-2);
    background: color-mix(in srgb, var(--accent-2) 16%, var(--bg-2));
  }

  .divider {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--bg-3);
  }

  .zoom-readout {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    min-width: 44px;
    text-align: right;
  }

  .diff-ref {
    width: 96px;
    height: 24px;
    padding: 0 6px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-1);
    background: var(--bg-2);
    border: 1px solid var(--bg-3);
    border-radius: 4px;
  }

  .diff-ref:focus {
    outline: none;
    border-color: var(--accent-2);
  }

  .doc-summary {
    font-size: 11px;
    color: var(--fg-2);
    padding-left: 6px;
  }

  .hint {
    margin-left: auto;
    font-size: 11px;
    color: var(--fg-2);
  }

  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    cursor: grab;
    /* let SVG fill the whole canvas */
    background:
      radial-gradient(circle at 1px 1px, var(--bg-2) 1px, transparent 0) 0 0 / 24px 24px;
  }
  .drawio-stage {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
    background: var(--bg-0);
  }

  .stage.dragging {
    cursor: grabbing;
  }

  .diagram {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    will-change: transform;
  }

  /* Width/height are set explicitly on the SVG by JS so zoom triggers a vector
     re-render. Make sure no UA stylesheet caps the size. */
  .diagram :global(svg) {
    max-width: none;
    display: block;
  }

  .doc-panel {
    position: absolute;
    top: 14px;
    right: 14px;
    width: min(340px, calc(100% - 28px));
    max-height: calc(100% - 28px);
    overflow: auto;
    padding: 14px;
    background: color-mix(in srgb, var(--bg-1) 94%, transparent);
    border: 1px solid var(--bg-3);
    border-radius: 6px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.28);
  }

  .doc-panel header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 8px;
  }

  .doc-panel h3,
  .doc-panel h4 {
    margin: 0;
    color: var(--fg-0);
  }

  .doc-panel h3 {
    font-size: 14px;
    line-height: 1.25;
  }

  .doc-panel h4 {
    margin-top: 14px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--fg-2);
  }

  .doc-panel button {
    font-size: 12px;
  }

  .actions {
    margin-top: 12px;
  }

  .actions button {
    padding: 6px 10px;
    background: var(--accent, #3b82f6);
    color: white;
    border: 0;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
  }

  .actions button:hover {
    filter: brightness(1.1);
  }

  .reveal-error {
    margin-top: 8px;
    padding: 6px 8px;
    background: color-mix(in srgb, var(--error, #ef4444) 18%, transparent);
    border-radius: 4px;
    color: var(--fg-1);
    font-size: 11px;
  }

  .doc-path {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .metrics {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }

  .metrics span {
    padding: 3px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    color: var(--fg-1);
    font-size: 11px;
  }

  .metrics.stacked {
    flex-direction: column;
    align-items: flex-start;
  }

  .doc-panel ul {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
  }

  .doc-panel li {
    margin: 2px 0;
    color: var(--fg-1);
    font-family: var(--mono);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .doc-panel li button {
    width: 100%;
    padding: 4px 6px;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
    background: transparent;
    border: 0;
    color: var(--accent-2);
  }

  .placeholder {
    color: var(--fg-2);
    text-align: center;
    padding: 40px;
  }

  .error {
    color: var(--error);
    margin: 12px;
    padding: 12px;
    border: 1px solid var(--error);
    border-radius: var(--radius-sm);
  }

  pre {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg-1);
    background: var(--bg-1);
    margin: 0 12px 12px;
    padding: 12px;
    border-radius: var(--radius-sm);
    overflow-x: auto;
  }
</style>

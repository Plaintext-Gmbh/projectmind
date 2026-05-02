<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { get } from 'svelte/store';
  import mermaid from 'mermaid';
  import { showDiagram, fileRecency } from '../lib/api';
  import type { ClassEntry, DiagramKind } from '../lib/api';
  import {
    classes,
    selectedClass,
    fileView,
    moduleFilter,
    packageFilter,
    stereotypeFilter,
    viewMode,
    repo,
  } from '../lib/store';

  export let kind: DiagramKind;
  export let folderLayout: 'hierarchy' | 'solar' | 'td' = 'solar';

  interface FolderMapNode {
    id: string;
    parent: string | null;
    label: string;
    path: string;
    kind: 'root' | 'folder' | 'file';
    depth: number;
    weight: number;
  }

  interface FolderMap {
    root: string;
    max_depth: number;
    truncated: boolean;
    nodes: FolderMapNode[];
  }

  let stage: HTMLDivElement;
  let mermaidSource = '';
  let svg = '';
  let loading = false;
  let error: string | null = null;

  // ----- Folder-map colour-by state ----------------------------------------

  // 'structure' (default) keeps the existing per-kind palette; 'recency'
  // tints each leaf by how long ago it was last touched; 'author' tints
  // by the most-recent committer. Folders inherit the fact of the most
  // recent descendant so both git-driven modes stay consistent.
  // Persisted per-browser so the user's preference sticks across sessions.
  type ColorBy = 'structure' | 'recency' | 'author';
  const COLOR_BY_KEY = 'projectmind.diagram.folderMap.colorBy';
  let colorBy: ColorBy = readColorByPref();
  /// Per-path git fact: how long ago + who. Both recency and author modes
  /// read from this single cache so toggling between them doesn't re-fetch.
  /// `null` means "haven't loaded yet"; an empty Map means "loaded, repo
  /// has no git history".
  type GitFact = { secs_ago: number; author: string | null };
  let factsByPath: Map<string, GitFact> | null = null;
  let factsForRoot: string | null = null;
  let gitError: string | null = null;

  function readColorByPref(): ColorBy {
    try {
      const v = localStorage.getItem(COLOR_BY_KEY);
      if (v === 'recency' || v === 'author' || v === 'structure') return v;
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

  function setColorBy(v: ColorBy) {
    if (colorBy === v) return;
    colorBy = v;
    writeColorByPref(v);
    void render(kind, folderLayout);
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

  /// Pick the stablest per-author identity from a name + email pair. Email
  /// wins when present (people change display names but not email); falls
  /// back to the display name; null when the signature was empty.
  function authorIdentity(name: string | null, email: string | null): string | null {
    if (email && email.trim()) return email.trim().toLowerCase();
    if (name && name.trim()) return name.trim();
    return null;
  }

  // viewport state
  let scale = 1;
  let tx = 0;
  let ty = 0;
  let dragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartTx = 0;
  let dragStartTy = 0;

  // SVG size at scale=1 (after fit-to-stage). Zoom is applied by resizing the
  // SVG itself (so the vector re-rasterises crisply at the new resolution)
  // rather than by CSS `transform: scale()` which would blur a bitmap.
  let baseW = 0;
  let baseH = 0;

  $: applyScale(scale);

  $: if (kind) {
    void render(kind, folderLayout);
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
    scale = 1;
    tx = 0;
    ty = 0;
  }

  function openFolderNode(path: string, nodeKind: string) {
    if (nodeKind !== 'file') return;
    fileView.update((cur) => ({
      path,
      anchor: null,
      nonce: (cur?.nonce ?? 0) + 1,
    }));
    viewMode.set('file');
  }

  async function render(k: DiagramKind, layout: 'hierarchy' | 'solar' | 'td') {
    loading = true;
    error = null;
    try {
      // Fetch git facts (recency + author) when the user is looking at
      // the folder map in either git-driven mode. Cached per repo root,
      // so toggling between R and A re-renders without a re-fetch.
      const wantGitFacts =
        k === 'folder-map' && (colorBy === 'recency' || colorBy === 'author');
      if (wantGitFacts) await ensureGitFactsForCurrentRepo();

      const payload = await showDiagram(k);
      if (k === 'folder-map') {
        mermaidSource = '';
        svg = renderFolderMap(JSON.parse(payload) as FolderMap, layout);
      } else {
        mermaidSource = payload;
        const id = `mermaid-${Date.now()}`;
        const result = await mermaid.render(id, mermaidSource);
        svg = result.svg;
      }
      resetView();
      await tick();
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
          baseW = vbW * fit;
          baseH = vbH * fit;
        } else {
          baseW = sw;
          baseH = sh;
        }
        node.style.display = 'block';
        applyScale(scale);
      }
    } catch (err) {
      error = String(err);
      svg = '';
    } finally {
      loading = false;
    }
  }

  function renderFolderMap(map: FolderMap, layout: 'hierarchy' | 'solar' | 'td'): string {
    // Build the fill resolver once per render, then capture it in the
    // closure. The three layout renderers below all consult `currentFillFor`
    // when emitting circles.
    currentFillFor = buildFillFor(map);
    if (layout === 'solar') return renderFolderSolar(map);
    if (layout === 'td') return renderFolderTopDown(map);
    return renderFolderHierarchy(map);
  }

  // Captured by each layout renderer so we don't have to thread the resolver
  // through three function signatures. Reset at the top of every render.
  let currentFillFor: ((id: string, kind: 'root' | 'folder' | 'file') => string | null) =
    () => null;

  function buildFillFor(
    map: FolderMap,
  ): (id: string, kind: 'root' | 'folder' | 'file') => string | null {
    if (
      (colorBy !== 'recency' && colorBy !== 'author') ||
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

    if (colorBy === 'recency') {
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

  /// Map a "seconds since last commit" value onto an HSL colour. Brand-new
  /// edits land in hot orange (~hue 18°); a year-old file decays into cool
  /// grey-blue (~hue 220°). Saturation drops with age so old code recedes
  /// visually. Log scale because the interesting structure lives in the
  /// last few days vs. the long tail of stale files.
  function recencyColor(secs_ago: number): string {
    const day = 86_400;
    const safe = Math.max(secs_ago, 60);
    // t=0 at <1 day, t≈0.33 at ~10 days, t≈0.67 at ~year, t≥1 at 1000+ days.
    const t = Math.min(1, Math.max(0, Math.log10(safe / day) / 3));
    const hue = 18 + (220 - 18) * t;
    const sat = 78 - 50 * t;
    const light = 52 - 18 * t;
    return `hsl(${hue.toFixed(0)}, ${sat.toFixed(0)}%, ${light.toFixed(0)}%)`;
  }

  /// Map an author identity (email when available, else display name) onto
  /// a stable HSL colour. djb2-style 32-bit hash → hue; saturation and
  /// lightness are fixed so all authors render at the same chroma. This
  /// is intentionally not "primary author by line count" — that would
  /// require git blame and far more work; "most recent committer per file"
  /// is a cheap proxy that correlates well in practice.
  function authorColor(identity: string): string {
    let h = 5381;
    for (let i = 0; i < identity.length; i++) {
      h = ((h << 5) + h + identity.charCodeAt(i)) | 0;
    }
    // Use the unsigned 32-bit mod 360 so identical strings always map to
    // the same hue across reloads / processes.
    const hue = ((h >>> 0) % 360);
    return `hsl(${hue}, 60%, 52%)`;
  }

  /// Helper used by all three folder-map layouts. Returns the `<circle>`
  /// element string — without a fill override when colour-by is `structure`,
  /// with an inline fill (and a tinted, lighter stroke) in `recency` mode.
  function circleSvg(
    n: FolderMapNode,
    r: number,
  ): string {
    const fill = currentFillFor(n.id, n.kind);
    if (fill === null) {
      return `<circle r="${r}"/>`;
    }
    // Stroke is the same hue ~25% lighter so the rim still reads.
    return `<circle r="${r}" style="fill:${fill};stroke:color-mix(in srgb, ${fill} 60%, white);"/>`;
  }

  function renderFolderHierarchy(map: FolderMap): string {
    const nodes = [...map.nodes].sort((a, b) => a.depth - b.depth || a.id.localeCompare(b.id));
    const byParent = groupByParent(nodes);
    const rows: Array<{ n: FolderMapNode; x: number; y: number }> = [];
    const nextY = { value: 70 };
    const xGap = 210;
    const yGap = 58;
    function place(id: string, depth: number) {
      const n = nodes.find((node) => node.id === id);
      if (!n) return;
      const children = byParent.get(id) ?? [];
      if (children.length === 0) {
        rows.push({ n, x: 80 + depth * xGap, y: nextY.value });
        nextY.value += yGap;
        return;
      }
      const before = nextY.value;
      for (const child of children) place(child.id, depth + 1);
      const after = nextY.value - yGap;
      rows.push({ n, x: 80 + depth * xGap, y: (before + after) / 2 });
    }
    place('.', 0);
    const byId = new Map(rows.map((r) => [r.n.id, r]));
    const width = Math.max(900, Math.max(...rows.map((r) => r.x), 0) + 260);
    const height = Math.max(520, nextY.value + 70);
    const edges = rows
      .filter((r) => r.n.parent)
      .map((r) => {
        const p = byId.get(r.n.parent ?? '');
        if (!p) return '';
        return `<path d="M${p.x + 70} ${p.y} C${p.x + 135} ${p.y}, ${r.x - 70} ${r.y}, ${r.x - 10} ${r.y}" class="edge"/>`;
      })
      .join('');
    const body = rows
      .map(({ n, x, y }) => {
        const radius = nodeRadius(n);
        return `<g class="node ${n.kind}" data-path="${esc(n.path)}" data-kind="${n.kind}" transform="translate(${x} ${y})">
          ${circleSvg(n, radius)}
          <text x="${radius + 8}" y="-3">${esc(shortLabel(n.label, 22))}</text>
          <text x="${radius + 8}" y="13" class="meta">${n.kind} · ${n.weight}</text>
        </g>`;
      })
      .join('');
    return folderSvg(width, height, edges + body, map);
  }

  function renderFolderTopDown(map: FolderMap): string {
    const nodes = [...map.nodes].sort((a, b) => a.depth - b.depth || a.id.localeCompare(b.id));
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const byParent = groupByParent(nodes);
    const placed = new Map<string, { n: FolderMapNode; x: number; y: number }>();
    const leafX = { value: 95 };
    const xGap = 120;
    const yGap = 112;

    function place(id: string, depth: number): number {
      const n = byId.get(id);
      if (!n) return leafX.value;
      const children = byParent.get(id) ?? [];
      let x: number;
      if (children.length === 0) {
        x = leafX.value;
        leafX.value += xGap;
      } else {
        const childXs = children.map((child) => place(child.id, depth + 1));
        x = (childXs[0] + childXs[childXs.length - 1]) / 2;
      }
      placed.set(id, { n, x, y: 70 + depth * yGap });
      return x;
    }

    place('.', 0);
    const rows = [...placed.values()];
    const width = Math.max(900, leafX.value + 95);
    const height = Math.max(520, Math.max(...rows.map((r) => r.y), 0) + 120);
    const edges = rows
      .filter((r) => r.n.parent)
      .map((r) => {
        const p = placed.get(r.n.parent ?? '');
        if (!p) return '';
        return `<path d="M${p.x} ${p.y + 32} C${p.x} ${p.y + 70}, ${r.x} ${r.y - 70}, ${r.x} ${r.y - 18}" class="edge"/>`;
      })
      .join('');
    const body = rows
      .map(({ n, x, y }) => {
        const radius = nodeRadius(n);
        return `<g class="node ${n.kind}" data-path="${esc(n.path)}" data-kind="${n.kind}" transform="translate(${x} ${y})">
          ${circleSvg(n, radius)}
          <text y="${radius + 17}" text-anchor="middle">${esc(shortLabel(n.label, 14))}</text>
          <text y="${radius + 31}" class="meta" text-anchor="middle">${n.kind} · ${n.weight}</text>
        </g>`;
      })
      .join('');
    return folderSvg(width, height, edges + body, map);
  }

  function renderFolderSolar(map: FolderMap): string {
    const nodes = map.nodes;
    const byParent = groupByParent(nodes);
    const width = 1400;
    const height = 900;
    const cx = width / 2;
    const cy = height / 2;
    const placed = new Map<string, { n: FolderMapNode; x: number; y: number }>();
    placed.set('.', { n: nodes[0], x: cx, y: cy });
    const maxDepth = Math.max(...nodes.map((n) => n.depth), 1);
    const rings = Array.from({ length: maxDepth }, (_, i) => {
      const r = 105 + i * 118;
      return `<circle class="orbit" cx="${cx}" cy="${cy}" r="${r}"/>`;
    }).join('');
    for (let depth = 1; depth <= maxDepth; depth++) {
      const level = nodes.filter((n) => n.depth === depth);
      const radius = 105 + (depth - 1) * 118;
      level.forEach((n, i) => {
        const angle = -Math.PI / 2 + (i / Math.max(level.length, 1)) * Math.PI * 2;
        placed.set(n.id, {
          n,
          x: cx + Math.cos(angle) * radius,
          y: cy + Math.sin(angle) * radius,
        });
      });
    }
    const edges = nodes
      .filter((n) => n.parent)
      .map((n) => {
        const a = placed.get(n.parent ?? '');
        const b = placed.get(n.id);
        if (!a || !b) return '';
        return `<line class="edge" x1="${a.x}" y1="${a.y}" x2="${b.x}" y2="${b.y}"/>`;
      })
      .join('');
    const body = [...placed.values()]
      .map(({ n, x, y }) => {
        const r = nodeRadius(n);
        return `<g class="node ${n.kind}" data-path="${esc(n.path)}" data-kind="${n.kind}" transform="translate(${x} ${y})">
          ${circleSvg(n, r)}
          <text y="${r + 16}" text-anchor="middle">${esc(shortLabel(n.label, 18))}</text>
        </g>`;
      })
      .join('');
    return folderSvg(width, height, rings + edges + body, map);
  }

  function groupByParent(nodes: FolderMapNode[]): Map<string, FolderMapNode[]> {
    const out = new Map<string, FolderMapNode[]>();
    for (const n of nodes) {
      if (!n.parent) continue;
      const arr = out.get(n.parent) ?? [];
      arr.push(n);
      out.set(n.parent, arr);
    }
    for (const arr of out.values()) {
      arr.sort((a, b) => folderRank(a) - folderRank(b) || a.label.localeCompare(b.label));
    }
    return out;
  }

  function folderRank(n: FolderMapNode): number {
    return n.kind === 'root' ? 0 : n.kind === 'folder' ? 1 : 2;
  }

  function nodeRadius(n: FolderMapNode): number {
    const base = n.kind === 'root' ? 30 : n.kind === 'folder' ? 18 : 7;
    return Math.min(base + Math.sqrt(n.weight) * 2.5, n.kind === 'file' ? 13 : 46);
  }

  function folderSvg(width: number, height: number, body: string, map: FolderMap): string {
    const note = map.truncated
      ? `<text x="24" y="${height - 24}" class="caption">truncated at ${map.nodes.length} nodes / depth ${map.max_depth}</text>`
      : '';
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
      <style>
        .edge{stroke:#3d4657;stroke-width:1.4;fill:none;opacity:.75}
        .orbit{stroke:#2a3344;stroke-width:1;fill:none;stroke-dasharray:6 10}
        .node circle{stroke-width:2;filter:drop-shadow(0 8px 14px rgba(0,0,0,.28))}
        .node{cursor:default}
        .node.file{cursor:pointer}
        .node.root circle{fill:#4f46e5;stroke:#c4b5fd}
        .node.folder circle{fill:#0f766e;stroke:#5eead4}
        .node.file circle{fill:#334155;stroke:#94a3b8}
        text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
        .meta,.caption{fill:#8b98aa;font-size:11px}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      ${body}
      ${note}
    </svg>`;
  }

  function shortLabel(label: string, max: number): string {
    return label.length <= max ? label : `${label.slice(0, max - 1)}…`;
  }

  function esc(s: string): string {
    return s.replace(/[&<>"']/g, (ch) => {
      const map: Record<string, string> = {
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
      };
    return map[ch] ?? ch;
    });
  }

  function onClick(e: MouseEvent) {
    if (kind !== 'folder-map') return;
    const target = e.target as Element | null;
    const node = target?.closest?.('.node') as SVGGElement | null;
    const path = node?.dataset.path;
    const nodeKind = node?.dataset.kind;
    if (path && nodeKind) openFolderNode(path, nodeKind);
  }

  function applyScale(s: number) {
    if (!stage || !baseW || !baseH) return;
    const node = stage.querySelector('svg');
    if (!node) return;
    // Resize the SVG so the renderer re-rasterises the vector at the new size.
    // `width`/`height` attributes (rather than CSS) keep `viewBox` scaling
    // crisp at any zoom level.
    node.setAttribute('width', String(baseW * s));
    node.setAttribute('height', String(baseH * s));
  }

  function onWheel(e: WheelEvent) {
    if (!e.shiftKey) return;
    e.preventDefault();
    const delta = Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : e.deltaX;
    if (delta === 0) return;
    const rect = stage.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = Math.exp(-delta * 0.0015);
    const nextScale = Math.min(8, Math.max(0.2, scale * factor));
    // Zoom toward cursor: keep the world-point under the cursor stable.
    tx = cx - (cx - tx) * (nextScale / scale);
    ty = cy - (cy - ty) * (nextScale / scale);
    scale = nextScale;
  }

  function onMouseDown(e: MouseEvent) {
    if ((e.target as Element | null)?.closest?.('.node.file')) return;
    if (e.button !== 0) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartTx = tx;
    dragStartTy = ty;
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging) return;
    tx = dragStartTx + (e.clientX - dragStartX);
    ty = dragStartTy + (e.clientY - dragStartY);
  }

  function endDrag() {
    dragging = false;
  }

  function zoomBy(factor: number) {
    if (!stage) return;
    const rect = stage.getBoundingClientRect();
    const cx = rect.width / 2;
    const cy = rect.height / 2;
    const nextScale = Math.min(8, Math.max(0.2, scale * factor));
    tx = cx - (cx - tx) * (nextScale / scale);
    ty = cy - (cy - ty) * (nextScale / scale);
    scale = nextScale;
  }
</script>

<div class="root">
  <div class="toolbar">
    <button on:click={() => zoomBy(1.25)} title="Zoom in">＋</button>
    <button on:click={() => zoomBy(0.8)} title="Zoom out">－</button>
    <button on:click={resetView} title="Reset view">⌂</button>
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
      {#if (colorBy === 'recency' || colorBy === 'author') && gitError}
        <span class="hint" style="color: var(--error);" title={gitError}>⚠ git unavailable</span>
      {/if}
    {/if}
    <span class="zoom-readout">{Math.round(scale * 100)}%</span>
    <span class="hint">Drag to pan • Shift + wheel to zoom</span>
  </div>
  {#if loading}
    <div class="placeholder">Rendering diagram…</div>
  {:else if error}
    <div class="error">⚠ {error}</div>
    <pre>{mermaidSource}</pre>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="stage"
      class:dragging
      bind:this={stage}
      on:wheel={onWheel}
      on:click={onClick}
      on:mousedown={onMouseDown}
      on:mousemove={onMouseMove}
      on:mouseup={endDrag}
      on:mouseleave={endDrag}
      role="img"
      aria-label="Diagram canvas (drag to pan, Shift plus wheel to zoom)"
    >
      <div
        class="diagram"
        style="transform: translate({tx}px, {ty}px); transform-origin: 0 0;"
      >
        {@html svg}
      </div>
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

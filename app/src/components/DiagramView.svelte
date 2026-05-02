<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { get } from 'svelte/store';
  import mermaid from 'mermaid';
  import { showDiagram } from '../lib/api';
  import type { ClassEntry, DiagramKind } from '../lib/api';
  import {
    classes,
    selectedClass,
    moduleFilter,
    navigateTo,
    packageFilter,
    stereotypeFilter,
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

  interface DocNode {
    id: string;
    abs: string;
    rel: string;
    title: string;
    inbound: number;
    outbound: number;
    external: number;
    orphan: boolean;
  }

  interface DocEdge {
    from: string;
    to: string;
    label: string;
    href: string;
  }

  interface DanglingDocLink {
    from: string;
    label: string;
    href: string;
    resolved: string;
  }

  interface DocGraph {
    root: string;
    nodes: DocNode[];
    edges: DocEdge[];
    dangling: DanglingDocLink[];
    orphan_count: number;
    dangling_count: number;
    external_count: number;
  }

  let stage: HTMLDivElement;
  let mermaidSource = '';
  let svg = '';
  let docGraph: DocGraph | null = null;
  let selectedDocId: string | null = null;
  let docGraphLayout: 'network' | 'radial' | 'orphans' = 'network';
  let loading = false;
  let error: string | null = null;

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
        navigateTo({ viewMode: 'classes', selectedFqn: match.fqn });
      }
    } else if (kind === 'package') {
      moduleFilter.set(moduleId);
      packageFilter.set(target);
      stereotypeFilter.set(null);
      selectedClass.set(null);
      navigateTo({ viewMode: 'classes', selectedFqn: null });
    }
  }

  function resetView() {
    scale = 1;
    tx = 0;
    ty = 0;
  }

  function openFolderNode(path: string, nodeKind: string) {
    if (nodeKind !== 'file') return;
    navigateTo({
      viewMode: 'file',
      fileView: { path, anchor: null, nonce: Date.now() },
    });
  }

  async function render(
    k: DiagramKind,
    layout: 'hierarchy' | 'solar' | 'td',
    docLayout: 'network' | 'radial' | 'orphans',
  ) {
    loading = true;
    error = null;
    try {
      const payload = await showDiagram(k);
      docGraph = null;
      if (k === 'folder-map') {
        mermaidSource = '';
        svg = renderFolderMap(JSON.parse(payload) as FolderMap, layout);
      } else if (k === 'doc-graph') {
        mermaidSource = '';
        docGraph = JSON.parse(payload) as DocGraph;
        if (selectedDocId && !docGraph.nodes.some((n) => n.id === selectedDocId)) {
          selectedDocId = null;
        }
        svg = renderDocGraph(docGraph, docLayout);
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
    if (layout === 'solar') return renderFolderSolar(map);
    if (layout === 'td') return renderFolderTopDown(map);
    return renderFolderHierarchy(map);
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
          <circle r="${radius}"/>
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
          <circle r="${radius}"/>
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
          <circle r="${r}"/>
          <text y="${r + 16}" text-anchor="middle">${esc(shortLabel(n.label, 18))}</text>
        </g>`;
      })
      .join('');
    return folderSvg(width, height, rings + edges + body, map);
  }

  function renderDocGraph(
    graph: DocGraph,
    layout: 'network' | 'radial' | 'orphans',
  ): string {
    if (graph.nodes.length === 0) return emptyDocGraphSvg();
    const placed = placeDocNodes(graph, layout);
    const width = 1400;
    const height = 900;
    const defs = `<defs>
      <marker id="arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
        <path d="M 0 0 L 10 5 L 0 10 z" fill="#64748b"/>
      </marker>
    </defs>`;
    const edges = graph.edges
      .map((e) => {
        const from = placed.get(e.from);
        const to = placed.get(e.to);
        if (!from || !to) return '';
        const dx = to.x - from.x;
        const dy = to.y - from.y;
        const len = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        const fromR = docNodeRadius(from.node) + 4;
        const toR = docNodeRadius(to.node) + 10;
        const x1 = from.x + (dx / len) * fromR;
        const y1 = from.y + (dy / len) * fromR;
        const x2 = to.x - (dx / len) * toR;
        const y2 = to.y - (dy / len) * toR;
        const curve = Math.min(80, Math.max(-80, (from.node.rel.localeCompare(to.node.rel) - 0.5) * 80));
        const mx = (x1 + x2) / 2 - (dy / len) * curve;
        const my = (y1 + y2) / 2 + (dx / len) * curve;
        return `<path class="doc-edge" d="M${x1} ${y1} Q${mx} ${my} ${x2} ${y2}">
          <title>${esc(e.from)} → ${esc(e.to)} (${esc(e.label)})</title>
        </path>`;
      })
      .join('');
    const nodes = [...placed.values()]
      .map(({ node, x, y }) => {
        const r = docNodeRadius(node);
        const classes = ['node', 'doc-node'];
        if (node.orphan) classes.push('orphan');
        if (node.id === selectedDocId) classes.push('selected');
        const subtitle = `${node.inbound} in · ${node.outbound} out · ${node.external} external`;
        return `<g class="${classes.join(' ')}" data-id="${esc(node.id)}" transform="translate(${x} ${y})">
          <circle r="${r}"/>
          <text y="${r + 18}" text-anchor="middle">${esc(shortLabel(node.title || node.rel, 22))}</text>
          <text y="${r + 33}" class="meta" text-anchor="middle">${esc(shortLabel(node.rel, 26))}</text>
          <title>${esc(node.rel)}\n${esc(subtitle)}</title>
        </g>`;
      })
      .join('');
    const stats = `<g class="doc-stats" transform="translate(24 30)">
      <text>docs ${graph.nodes.length}</text>
      <text y="18">links ${graph.edges.length}</text>
      <text y="36">orphans ${graph.orphan_count}</text>
      <text y="54">dangling ${graph.dangling_count}</text>
      <text y="72">external ${graph.external_count}</text>
    </g>`;
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
      ${defs}
      <style>
        .doc-edge{stroke:#64748b;stroke-width:1.6;fill:none;opacity:.58;marker-end:url(#arrow)}
        .doc-node{cursor:pointer}
        .doc-node circle{fill:#1f2937;stroke:#38bdf8;stroke-width:2;filter:drop-shadow(0 10px 18px rgba(0,0,0,.35))}
        .doc-node.orphan circle{stroke:#f59e0b;stroke-dasharray:5 4}
        .doc-node.selected circle{fill:#0f766e;stroke:#5eead4;stroke-width:4}
        text{fill:#e5edf8;font:13px ui-sans-serif,system-ui,sans-serif;paint-order:stroke;stroke:#090d14;stroke-width:3px;stroke-linejoin:round}
        .meta{fill:#9aa8ba;font-size:11px}
        .doc-stats text{fill:#cbd5e1;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;stroke:none}
      </style>
      <rect width="100%" height="100%" fill="#090d14"/>
      ${edges}
      ${nodes}
      ${stats}
    </svg>`;
  }

  function placeDocNodes(
    graph: DocGraph,
    layout: 'network' | 'radial' | 'orphans',
  ): Map<string, { node: DocNode; x: number; y: number }> {
    if (layout === 'orphans') return placeDocOrphans(graph);
    if (layout === 'radial') return placeDocRadial(graph);
    return placeDocNetwork(graph);
  }

  function placeDocNetwork(graph: DocGraph): Map<string, { node: DocNode; x: number; y: number }> {
    const out = new Map<string, { node: DocNode; x: number; y: number }>();
    const nodes = [...graph.nodes].sort(
      (a, b) => b.inbound + b.outbound - (a.inbound + a.outbound) || a.rel.localeCompare(b.rel),
    );
    const cx = 700;
    const cy = 450;
    nodes.forEach((node, i) => {
      if (i === 0) {
        out.set(node.id, { node, x: cx, y: cy });
        return;
      }
      const ring = Math.floor(Math.sqrt(i));
      const inRingStart = ring * ring;
      const inRingCount = Math.max(1, (ring + 1) * (ring + 1) - inRingStart);
      const pos = i - inRingStart;
      const angle = -Math.PI / 2 + (pos / inRingCount) * Math.PI * 2;
      const radius = 120 + ring * 105;
      out.set(node.id, {
        node,
        x: cx + Math.cos(angle) * radius,
        y: cy + Math.sin(angle) * radius,
      });
    });
    return out;
  }

  function placeDocRadial(graph: DocGraph): Map<string, { node: DocNode; x: number; y: number }> {
    const out = new Map<string, { node: DocNode; x: number; y: number }>();
    const root =
      graph.nodes.find((n) => /^readme\.md$/i.test(n.rel)) ??
      [...graph.nodes].sort((a, b) => b.outbound + b.inbound - (a.outbound + a.inbound))[0];
    out.set(root.id, { node: root, x: 700, y: 450 });
    const linked = new Set(graph.edges.filter((e) => e.from === root.id).map((e) => e.to));
    const rings = [
      graph.nodes.filter((n) => linked.has(n.id)),
      graph.nodes.filter((n) => n.id !== root.id && !linked.has(n.id) && !n.orphan),
      graph.nodes.filter((n) => n.id !== root.id && n.orphan),
    ];
    rings.forEach((ringNodes, ringIdx) => {
      const radius = 150 + ringIdx * 180;
      ringNodes
        .sort((a, b) => a.rel.localeCompare(b.rel))
        .forEach((node, i) => {
          const angle = -Math.PI / 2 + (i / Math.max(1, ringNodes.length)) * Math.PI * 2;
          out.set(node.id, {
            node,
            x: 700 + Math.cos(angle) * radius,
            y: 450 + Math.sin(angle) * radius,
          });
        });
    });
    return out;
  }

  function placeDocOrphans(graph: DocGraph): Map<string, { node: DocNode; x: number; y: number }> {
    const out = new Map<string, { node: DocNode; x: number; y: number }>();
    const columns = [
      graph.nodes.filter((n) => n.orphan).sort((a, b) => a.rel.localeCompare(b.rel)),
      graph.nodes.filter((n) => !n.orphan).sort((a, b) => b.inbound - a.inbound || a.rel.localeCompare(b.rel)),
    ];
    columns.forEach((nodes, col) => {
      const x = col === 0 ? 360 : 980;
      const gap = Math.min(92, Math.max(44, 780 / Math.max(1, nodes.length)));
      nodes.forEach((node, i) => {
        out.set(node.id, { node, x, y: 80 + i * gap });
      });
    });
    return out;
  }

  function docNodeRadius(n: DocNode): number {
    return Math.min(46, 16 + Math.sqrt(n.inbound + n.outbound + n.external + 1) * 5);
  }

  function emptyDocGraphSvg(): string {
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 900 520">
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="450" y="260" text-anchor="middle" fill="#94a3b8" font-size="18" font-family="ui-sans-serif,system-ui">No markdown documents found</text>
    </svg>`;
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
    if (kind !== 'folder-map' && kind !== 'doc-graph') return;
    const target = e.target as Element | null;
    const node = target?.closest?.('.node') as SVGGElement | null;
    if (!node) return;
    if (kind === 'folder-map') {
      const path = node.dataset.path;
      const nodeKind = node.dataset.kind;
      if (path && nodeKind) openFolderNode(path, nodeKind);
      return;
    }
    const docId = node.dataset.id;
    if (docId) selectedDocId = docId;
  }

  function openDoc(path: string) {
    navigateTo({
      viewMode: 'file',
      fileView: { path, anchor: null, nonce: Date.now() },
    });
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
    {/if}
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
                    <button on:click={() => (selectedDocId = edge.from)}>{edge.from}</button>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if selectedOutgoing.length > 0}
              <h4>Links to</h4>
              <ul>
                {#each selectedOutgoing.slice(0, 8) as edge}
                  <li>
                    <button on:click={() => (selectedDocId = edge.to)}>{edge.to}</button>
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

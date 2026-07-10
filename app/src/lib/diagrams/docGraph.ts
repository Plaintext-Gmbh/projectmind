/// Documentation-graph SVG renderer — extracted from `DiagramView.svelte`
/// (Viz-Katalog V1.3, #61). Three placement strategies (network / radial /
/// orphans) feed a single SVG emitter. Pure apart from the `selectedDocId`
/// highlight, which the component passes in.

import { esc, shortLabel } from './common';

export interface DocNode {
  id: string;
  abs: string;
  rel: string;
  title: string;
  inbound: number;
  outbound: number;
  external: number;
  orphan: boolean;
}

export interface DocEdge {
  from: string;
  to: string;
  label: string;
  href: string;
}

export interface DanglingDocLink {
  from: string;
  label: string;
  href: string;
  resolved: string;
}

export interface DocGraph {
  root: string;
  nodes: DocNode[];
  edges: DocEdge[];
  dangling: DanglingDocLink[];
  orphan_count: number;
  dangling_count: number;
  external_count: number;
}

export type DocGraphLayout = 'network' | 'radial' | 'orphans';

export function renderDocGraph(
  graph: DocGraph,
  layout: DocGraphLayout,
  selectedDocId: string | null = null,
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

export function placeDocNodes(
  graph: DocGraph,
  layout: DocGraphLayout,
): Map<string, { node: DocNode; x: number; y: number }> {
  if (layout === 'orphans') return placeDocOrphans(graph);
  if (layout === 'radial') return placeDocRadial(graph);
  return placeDocNetwork(graph);
}

export function placeDocNetwork(
  graph: DocGraph,
): Map<string, { node: DocNode; x: number; y: number }> {
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

export function placeDocRadial(
  graph: DocGraph,
): Map<string, { node: DocNode; x: number; y: number }> {
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

export function placeDocOrphans(
  graph: DocGraph,
): Map<string, { node: DocNode; x: number; y: number }> {
  const out = new Map<string, { node: DocNode; x: number; y: number }>();
  const columns = [
    graph.nodes.filter((n) => n.orphan).sort((a, b) => a.rel.localeCompare(b.rel)),
    graph.nodes
      .filter((n) => !n.orphan)
      .sort((a, b) => b.inbound - a.inbound || a.rel.localeCompare(b.rel)),
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

export function docNodeRadius(n: DocNode): number {
  return Math.min(46, 16 + Math.sqrt(n.inbound + n.outbound + n.external + 1) * 5);
}

function emptyDocGraphSvg(): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 900 520">
      <rect width="100%" height="100%" fill="#090d14"/>
      <text x="450" y="260" text-anchor="middle" fill="#94a3b8" font-size="18" font-family="ui-sans-serif,system-ui">No markdown documents found</text>
    </svg>`;
}

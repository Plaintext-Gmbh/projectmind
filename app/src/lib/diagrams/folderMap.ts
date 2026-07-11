/// Folder-map SVG renderer — extracted from `DiagramView.svelte`
/// (Viz-Katalog V1.3, #61). Three layouts (solar / hierarchy / top-down)
/// share the node-radius, edge-grouping and chrome helpers below.
///
/// Colour-by overlays (recency / author / diff) stay in the component: they
/// depend on fetched git facts. The component builds a `FillFor` resolver
/// and passes it in, so this module stays a pure layout function that can be
/// unit-tested without any git plumbing.

import { esc, shortLabel } from './common';

export interface FolderMapNode {
  id: string;
  parent: string | null;
  label: string;
  path: string;
  kind: 'root' | 'folder' | 'file';
  depth: number;
  weight: number;
}

export interface FolderMap {
  root: string;
  max_depth: number;
  truncated: boolean;
  nodes: FolderMapNode[];
}

export type FolderLayout = 'hierarchy' | 'solar' | 'td';

/// Resolve the inline fill for a node, or `null` to keep the structural
/// per-kind palette. Supplied by the component from its colour-by state.
export type FillFor = (id: string, kind: FolderMapNode['kind']) => string | null;

const noFill: FillFor = () => null;

export function renderFolderMap(
  map: FolderMap,
  layout: FolderLayout,
  fillFor: FillFor = noFill,
): string {
  if (layout === 'solar') return renderFolderSolar(map, fillFor);
  if (layout === 'td') return renderFolderTopDown(map, fillFor);
  return renderFolderHierarchy(map, fillFor);
}

/// `<circle>` element string — plain in `structure` mode, or with an inline
/// fill + a lighter, hue-matched stroke when a colour-by mode supplies one.
function circleSvg(n: FolderMapNode, r: number, fillFor: FillFor): string {
  const fill = fillFor(n.id, n.kind);
  if (fill === null) {
    return `<circle r="${r}"/>`;
  }
  return `<circle r="${r}" style="fill:${fill};stroke:color-mix(in srgb, ${fill} 60%, white);"/>`;
}

export function renderFolderHierarchy(map: FolderMap, fillFor: FillFor = noFill): string {
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
          ${circleSvg(n, radius, fillFor)}
          <text x="${radius + 8}" y="-3">${esc(shortLabel(n.label, 22))}</text>
          <text x="${radius + 8}" y="13" class="meta">${n.kind} · ${n.weight}</text>
        </g>`;
    })
    .join('');
  return folderSvg(width, height, edges + body, map);
}

export function renderFolderTopDown(map: FolderMap, fillFor: FillFor = noFill): string {
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
          ${circleSvg(n, radius, fillFor)}
          <text y="${radius + 17}" text-anchor="middle">${esc(shortLabel(n.label, 14))}</text>
          <text y="${radius + 31}" class="meta" text-anchor="middle">${n.kind} · ${n.weight}</text>
        </g>`;
    })
    .join('');
  return folderSvg(width, height, edges + body, map);
}

export function renderFolderSolar(map: FolderMap, fillFor: FillFor = noFill): string {
  const nodes = map.nodes;
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
          ${circleSvg(n, r, fillFor)}
          <text y="${r + 16}" text-anchor="middle">${esc(shortLabel(n.label, 18))}</text>
        </g>`;
    })
    .join('');
  return folderSvg(width, height, rings + edges + body, map);
}

export function groupByParent(nodes: FolderMapNode[]): Map<string, FolderMapNode[]> {
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

export function nodeRadius(n: FolderMapNode): number {
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

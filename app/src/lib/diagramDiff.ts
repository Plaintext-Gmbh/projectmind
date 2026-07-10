/// Before/after architecture-snapshot helpers for the `diagram-diff` tour
/// step (#125).
///
/// The step shows one diagram kind (folder map, for the first cut) in three
/// static modes — `before`, `after`, `changed-only` — so a tour explaining a
/// refactor makes additions, removals and edits obvious without the reader
/// diffing two screenshots by eye.
///
/// The logic lives here, out of the Svelte component, so vitest can pin the
/// changed-node derivation, mode filtering and pulse selection without booting
/// a DOM. The component is a thin shell around `renderFolderDiff`.
///
/// ## Why file-level diff metadata, not two payloads
///
/// `show_diagram('folder-map')` renders the *current* working tree — there is
/// no MCP tool that renders a folder map at an arbitrary git ref. So instead
/// of fetching two structural payloads (which the backend can't produce today)
/// we render the current folder map once and overlay the file-level change set
/// from `list_changes_since(from, to)`. Added / modified / deleted / renamed
/// files tint their node; folders inherit the most-prominent status of their
/// subtree. This is exactly the "derive changed nodes from file-level diff
/// metadata" path the issue calls out, and it keeps the step dependency-free
/// (no Cytoscape, no animation).

import type { ChangedFile } from './api';

/// A folder-map node as produced by the Rust `render_folder_map` payload.
/// `id` is the repo-relative, forward-slash path (`.` for the root); `path`
/// is absolute. `list_changes_since` returns repo-relative paths, so we match
/// its entries against `id`.
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

/// The three static toggle modes. The issue also sketches a `morph` mode; it
/// needs animation / a Cytoscape renderer and is deliberately left out of this
/// first, dependency-free cut (tracked as a follow-up).
export type DiagramDiffMode = 'before' | 'after' | 'changed-only';

export const DIAGRAM_DIFF_MODES: readonly DiagramDiffMode[] = [
  'before',
  'after',
  'changed-only',
];

export type DiffStatus = ChangedFile['status'];

/// Normalise a diff path to the forward-slash, no-`./`-prefix form the
/// folder-map node ids use, so Windows deltas and `./a/b` both match.
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '');
}

/// Rank statuses for parent aggregation — `deleted` wins so a vanished file is
/// never visually masked by a sibling rename or edit; fresh files come next,
/// in-place edits last. Mirrors the priority the standalone folder-map diff
/// overlay uses in DiagramView so the two views agree.
export function diffPriority(s: DiffStatus): number {
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
    default:
      return 0;
  }
}

/// Status → fill colour. Hues follow the conventional review vocabulary
/// (green added, amber modified, red removed) so the legend doesn't need
/// looking up. `dim` lifts folder aggregates so leaf files still pop.
export function diffColor(status: DiffStatus, dim: boolean): string {
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

/// Single-letter status glyph, shared with the legend + tooltips.
export function diffStatusGlyph(status: DiffStatus): string {
  switch (status) {
    case 'added':
      return 'A';
    case 'modified':
      return 'M';
    case 'deleted':
      return 'D';
    case 'renamed':
      return 'R';
    case 'type_change':
      return 'T';
    default:
      return '?';
  }
}

/// Per-node change status, keyed by node id. A leaf file gets its own status
/// from the change set; a folder inherits the most-prominent status across its
/// subtree (so a tinted parent reads as "something changed in here"). Untouched
/// nodes are simply absent from the map.
///
/// Robust to an empty change set — returns an empty map, never throws. That is
/// the "empty diff → no error" acceptance case: every mode then renders the
/// plain structure with nothing tinted.
export function deriveChangedNodes(
  map: FolderMap,
  changes: readonly ChangedFile[],
): Map<string, DiffStatus> {
  const statusById = new Map<string, DiffStatus>();
  if (changes.length === 0 || map.nodes.length === 0) return statusById;

  // Index the change set by normalised path for O(1) leaf lookup.
  const changeByPath = new Map<string, DiffStatus>();
  for (const c of changes) {
    changeByPath.set(normPath(c.path), c.status);
  }

  const byParent = new Map<string, FolderMapNode[]>();
  for (const n of map.nodes) {
    if (n.parent === null) continue;
    const arr = byParent.get(n.parent) ?? [];
    arr.push(n);
    byParent.set(n.parent, arr);
  }

  function visit(node: FolderMapNode): DiffStatus | null {
    if (node.kind === 'file') {
      const s = changeByPath.get(normPath(node.id)) ?? null;
      if (s) statusById.set(node.id, s);
      return s;
    }
    let best: DiffStatus | null = null;
    for (const child of byParent.get(node.id) ?? []) {
      const v = visit(child);
      if (v !== null && (best === null || diffPriority(v) > diffPriority(best))) {
        best = v;
      }
    }
    if (best !== null) statusById.set(node.id, best);
    return best;
  }

  const rootNode = map.nodes.find((n) => n.parent === null) ?? map.nodes[0];
  if (rootNode) visit(rootNode);
  return statusById;
}

/// The subset of nodes a given mode renders.
///
/// - **before**: every node — the baseline structure, nothing tinted (the
///   caller passes an empty status map so no fills apply).
/// - **after**: every node — same structure, tinted by change status.
/// - **changed-only**: the changed nodes plus every ancestor needed to keep
///   them connected to the root, so a huge repo stays readable (the issue's
///   "large graphs remain readable via changed-only" criterion). The root is
///   always kept so the tree never renders parentless.
///
/// `changed-only` with an empty status map collapses to just the root — a
/// single node, which reads as "nothing changed here" rather than an error.
export function filterNodesForMode(
  map: FolderMap,
  statusById: ReadonlyMap<string, DiffStatus>,
  mode: DiagramDiffMode,
): FolderMapNode[] {
  if (mode !== 'changed-only') return map.nodes;

  const byId = new Map(map.nodes.map((n) => [n.id, n]));
  const keep = new Set<string>();

  // Always keep the root so the filtered tree has an anchor.
  const rootNode = map.nodes.find((n) => n.parent === null);
  if (rootNode) keep.add(rootNode.id);

  // Keep every changed node and walk up to the root through its ancestors so
  // no kept node is orphaned.
  for (const id of statusById.keys()) {
    let cursor: FolderMapNode | undefined = byId.get(id);
    while (cursor && !keep.has(cursor.id)) {
      keep.add(cursor.id);
      cursor = cursor.parent ? byId.get(cursor.parent) : undefined;
    }
  }

  // Preserve original node order for a stable layout.
  return map.nodes.filter((n) => keep.has(n.id));
}

/// Node ids that should pulse once when the step opens: the changed *leaf*
/// files (not the inherited folder aggregates — pulsing every ancestor would
/// wash the effect out). Empty when nothing changed, so the pulse simply
/// doesn't fire. Only ids present in `visibleIds` are returned, so a
/// changed-only render doesn't try to pulse a node it filtered away.
export function changedPulseIds(
  map: FolderMap,
  statusById: ReadonlyMap<string, DiffStatus>,
  visibleIds: ReadonlySet<string>,
): string[] {
  const out: string[] = [];
  for (const n of map.nodes) {
    if (n.kind !== 'file') continue;
    if (!statusById.has(n.id)) continue;
    if (!visibleIds.has(n.id)) continue;
    out.push(n.id);
  }
  return out;
}

/// Count of distinct changed files (leaf nodes) in this map — drives the
/// header summary ("3 changed"). Counts leaves only so folder aggregates
/// don't inflate the number.
export function changedFileCount(
  map: FolderMap,
  statusById: ReadonlyMap<string, DiffStatus>,
): number {
  let n = 0;
  for (const node of map.nodes) {
    if (node.kind === 'file' && statusById.has(node.id)) n += 1;
  }
  return n;
}

// ---------------------------------------------------------------------------
// SVG renderer
// ---------------------------------------------------------------------------
//
// A compact solar (concentric-ring) folder-map renderer. It intentionally
// mirrors the layout maths of DiagramView's `renderFolderSolar` — the one
// existing folder-map renderer — but adds the two things the diff step needs
// and that renderer can't express: per-mode node filtering and a `pulse` class
// on freshly changed leaves. Kept as a pure string function so it is testable
// and carries no Svelte/DOM dependency.

function esc(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function shortLabel(label: string, max: number): string {
  return label.length <= max ? label : `${label.slice(0, max - 1)}…`;
}

function nodeRadius(n: FolderMapNode): number {
  const base = n.kind === 'root' ? 30 : n.kind === 'folder' ? 18 : 7;
  return Math.min(base + Math.sqrt(n.weight) * 2.5, n.kind === 'file' ? 13 : 46);
}

export interface RenderFolderDiffOptions {
  mode: DiagramDiffMode;
  /// Set of leaf ids to mark with the `pulse` class (added by the caller
  /// after computing `changedPulseIds`). Empty = no pulse.
  pulseIds?: ReadonlySet<string>;
}

/// Render the folder map for one diff mode as an SVG string.
///
/// `statusById` supplies the tint; in `before` mode the caller passes an empty
/// map so nothing is tinted. Nodes are filtered per mode first, then laid out
/// on concentric rings by depth. Empty / root-only results still produce a
/// valid SVG (never throws) so the component can render it unconditionally.
export function renderFolderDiff(
  map: FolderMap,
  statusById: ReadonlyMap<string, DiffStatus>,
  opts: RenderFolderDiffOptions,
): string {
  const nodes = filterNodesForMode(map, statusById, opts.mode);
  const pulseIds = opts.pulseIds ?? new Set<string>();
  const tint = opts.mode !== 'before';

  const width = 1400;
  const height = 900;
  const cx = width / 2;
  const cy = height / 2;

  const placed = new Map<string, { n: FolderMapNode; x: number; y: number }>();
  const rootNode = nodes.find((n) => n.parent === null);
  if (rootNode) placed.set(rootNode.id, { n: rootNode, x: cx, y: cy });

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
      const status = tint ? statusById.get(n.id) ?? null : null;
      const fill =
        status !== null ? ` style="fill:${diffColor(status, n.kind !== 'file')};stroke:color-mix(in srgb, ${diffColor(status, n.kind !== 'file')} 60%, white);"` : '';
      const classes = ['node', n.kind];
      if (status !== null) classes.push('changed');
      else if (tint) classes.push('faded');
      if (pulseIds.has(n.id)) classes.push('pulse');
      const title = status ? `${esc(n.id)}\n${status}` : esc(n.id);
      return `<g class="${classes.join(' ')}" data-id="${esc(n.id)}" transform="translate(${x} ${y})">
        <circle r="${r}"${fill}/>
        <text y="${r + 16}" text-anchor="middle">${esc(shortLabel(n.label, 18))}</text>
        <title>${title}</title>
      </g>`;
    })
    .join('');

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
    <style>
      .edge{stroke:#3d4657;stroke-width:1.4;fill:none;opacity:.75}
      .orbit{stroke:#2a3344;stroke-width:1;fill:none;stroke-dasharray:6 10}
      .node circle{stroke-width:2;filter:drop-shadow(0 8px 14px rgba(0,0,0,.28))}
      .node.root circle{fill:#4f46e5;stroke:#c4b5fd}
      .node.folder circle{fill:#0f766e;stroke:#5eead4}
      .node.file circle{fill:#334155;stroke:#94a3b8}
      /* Unchanged nodes fade back so changed ones read as the signal (#125). */
      .node.faded{opacity:.32}
      .node.changed{opacity:1}
      text{fill:#dce3f0;font:13px ui-sans-serif,system-ui,sans-serif}
      /* One-shot pulse played when the step opens. The class is stripped
         after the animation so re-entering the step replays it. */
      .node.pulse circle{animation:dd-pulse 1.4s ease-out}
      @keyframes dd-pulse{
        0%{transform:scale(1)}
        30%{transform:scale(1.8);filter:drop-shadow(0 0 14px rgba(255,255,255,.7))}
        100%{transform:scale(1)}
      }
    </style>
    <rect width="100%" height="100%" fill="#090d14"/>
    ${rings}
    ${edges}
    ${body}
  </svg>`;
}

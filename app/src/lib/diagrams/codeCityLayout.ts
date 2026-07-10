/// Code-city layout (V4.6a, #66) — pure geometry, no DOM, no WebGL.
///
/// Turns the `code_city_data` payload (walked file tree + risk/recency
/// joins) into world-space boxes: folders become stacked treemap districts
/// (plateaus), files become buildings whose height follows sloc/bytes and
/// whose colour follows the Risk-Atlas score scale (`colorForScore` reuse).
/// `CodeCity.svelte` only maps this model onto Three.js instances — every
/// number that matters is computed (and unit-tested) here, mirroring how
/// `folderMap.ts` keeps the SVG renderers pure.
///
/// The treemap is a classic squarified treemap (Bruls et al. 2000): rows are
/// packed along the shorter side of the remaining rectangle and a row is
/// closed as soon as adding the next item would worsen the worst aspect
/// ratio. Cells are constructed disjoint, so sibling buildings can never
/// collide. The simpler halving `treemap.ts` stays untouched — it lays out a
/// flat `RiskScore[]`, not a folder hierarchy.

import type { CityNode, CodeCityData } from '../api';
import { colorForScore } from '../treemap';

export interface CityLayoutOptions {
  /// Edge length of the ground square in world units.
  world?: number;
  /// Inset applied per district level, so nested districts read as terraces.
  padding?: number;
  /// Gap between a building and its treemap cell edge.
  gap?: number;
  /// Plateau height per depth level — districts stack like terraces.
  plinth?: number;
  /// Minimum building height.
  minH?: number;
  /// Maximum building height.
  maxH?: number;
}

const DEFAULTS: Required<CityLayoutOptions> = {
  world: 200,
  padding: 1.5,
  gap: 0.4,
  plinth: 0.6,
  minH: 0.5,
  maxH: 30,
};

/// One building (= file). Footprint on the XZ plane, y-up.
export interface CityBuilding {
  /// Repo-relative path — drill target + tooltip.
  id: string;
  label: string;
  /// Drill: FQN + module of the hottest class in the file, or null → FileView.
  fqn: string | null;
  module: string | null;
  x: number;
  z: number;
  w: number;
  d: number;
  /// Base elevation (top of the containing district's plateau).
  y: number;
  /// Building height above the base.
  h: number;
  /// hsl() facade colour — risk scale or neutral slate.
  color: string;
  /// 0..1 emissive share from commit recency ("freshly built").
  glow: number;
  score: number | null;
  sloc: number | null;
  bytes: number;
}

/// One district (= folder) plateau.
export interface CityDistrict {
  id: string;
  label: string;
  depth: number;
  x: number;
  z: number;
  w: number;
  d: number;
  /// Plateau top elevation (`depth * plinth`).
  y: number;
}

export interface CityModel {
  buildings: CityBuilding[];
  districts: CityDistrict[];
  world: number;
  truncated: boolean;
}

/// Axis-aligned footprint rectangle on the ground plane.
export interface Rect {
  x: number;
  z: number;
  w: number;
  d: number;
}

/// ~40 bytes per source line: the fallback heuristic that maps raw file
/// size onto the same "lines" scale sloc uses, so parsed and unparsed
/// files get comparable heights.
const BYTES_PER_LINE = 40;
/// Height gain per doubling of the metric. 2.2 · log2(10 001) ≈ 29.5, so a
/// 10 000-line file tops out just under the default maxH of 30.
const HEIGHT_PER_LOG2 = 2.2;

const DAY_SECS = 86_400;
const WEEK_SECS = 604_800;

/// Neutral facade for files without a risk score, leaning on the folder-map
/// `.file` slate palette.
const NEUTRAL_COLOR = 'hsl(215, 15%, 40%)';

/// Building height: log-scaled so a 10k-line outlier towers without
/// flattening everything else. Prefers `sloc` (precise, from parsed
/// classes) over the bytes heuristic.
export function heightFor(
  sloc: number | null,
  bytes: number,
  o: Required<CityLayoutOptions>,
): number {
  const metric = sloc ?? bytes / BYTES_PER_LINE;
  const h = o.minH + HEIGHT_PER_LOG2 * Math.log2(1 + Math.max(metric, 0));
  return Math.min(Math.max(h, o.minH), o.maxH);
}

/// Facade colour: Risk-Atlas scale (green 0 → red 100) when scored,
/// neutral slate otherwise.
export function buildingColor(score: number | null): string {
  return score === null ? NEUTRAL_COLOR : colorForScore(score);
}

/// "Freshly built" glow from commit recency: full inside 24 h, fading
/// linearly to zero at 7 days.
export function glowFor(secsAgo: number | null): number {
  if (secsAgo === null) return 0;
  if (secsAgo <= DAY_SECS) return 1;
  if (secsAgo >= WEEK_SECS) return 0;
  return 1 - (secsAgo - DAY_SECS) / (WEEK_SECS - DAY_SECS);
}

/// Squarified treemap (Bruls et al.): packs `items` into `rect`, cell area
/// proportional to weight. Rows go along the shorter side; a row closes when
/// adding the next item would worsen its worst aspect ratio. Cells are
/// disjoint by construction and exactly tile `rect`.
export function squarify(items: { id: string; weight: number }[], rect: Rect): Map<string, Rect> {
  const out = new Map<string, Rect>();
  if (items.length === 0 || rect.w <= 0 || rect.d <= 0) return out;

  const total = items.reduce((s, it) => s + Math.max(it.weight, 0), 0);
  const area = rect.w * rect.d;
  // Degenerate weights: tile evenly so every item still gets a cell.
  const scaled = items.map((it, i) => ({
    id: it.id,
    area: total > 0 ? (Math.max(it.weight, 0) / total) * area : area / items.length,
    i,
  }));
  // Classic squarify wants descending areas; the index tie-break keeps the
  // sort — and therefore the whole layout — deterministic.
  scaled.sort((a, b) => b.area - a.area || a.i - b.i);

  let free: Rect = { ...rect };
  let row: { id: string; area: number }[] = [];

  const worst = (rowAreas: number[], side: number): number => {
    const sum = rowAreas.reduce((s, a) => s + a, 0);
    if (sum <= 0 || side <= 0) return Infinity;
    const thickness = sum / side;
    let w = 0;
    for (const a of rowAreas) {
      const len = a / thickness;
      w = Math.max(w, Math.max(thickness / len, len / thickness));
    }
    return w;
  };

  const layoutRow = () => {
    const sum = row.reduce((s, it) => s + it.area, 0);
    if (sum <= 0) {
      // Zero-area row (can only happen through rounding): give each item a
      // degenerate cell at the free-rect corner so every id gets an entry.
      for (const it of row) out.set(it.id, { x: free.x, z: free.z, w: 0, d: 0 });
      row = [];
      return;
    }
    const horizontal = free.w >= free.d; // row runs along the shorter side
    const side = horizontal ? free.d : free.w;
    const thickness = sum / side;
    let offset = 0;
    for (const it of row) {
      const len = it.area / thickness;
      out.set(
        it.id,
        horizontal
          ? { x: free.x, z: free.z + offset, w: thickness, d: len }
          : { x: free.x + offset, z: free.z, w: len, d: thickness },
      );
      offset += len;
    }
    free = horizontal
      ? { x: free.x + thickness, z: free.z, w: free.w - thickness, d: free.d }
      : { x: free.x, z: free.z + thickness, w: free.w, d: free.d - thickness };
    row = [];
  };

  for (const it of scaled) {
    const side = Math.min(free.w, free.d);
    const current = row.map((r) => r.area);
    if (row.length === 0 || worst([...current, it.area], side) <= worst(current, side)) {
      row.push({ id: it.id, area: it.area });
    } else {
      layoutRow();
      row.push({ id: it.id, area: it.area });
    }
  }
  if (row.length > 0) layoutRow();
  return out;
}

/// Orbit-camera start pose: outside the city's bounding box, looking at the
/// ground centre from a raised diagonal — the classic "approach from the
/// south-east" establishing shot.
export function cameraFitFor(model: CityModel): {
  position: [number, number, number];
  target: [number, number, number];
} {
  const w = model.world;
  return {
    position: [w * 1.15, w * 0.85, w * 1.15],
    target: [w / 2, 0, w / 2],
  };
}

/// Deterministic sibling order: folders before files, then label — mirrors
/// `folderRank` in folderMap.ts so both views walk the tree alike. Plain
/// `<`-comparison instead of localeCompare keeps it locale-independent.
function siblingRank(n: CityNode): number {
  return n.kind === 'file' ? 1 : 0;
}

function sortSiblings(nodes: CityNode[]): CityNode[] {
  return [...nodes].sort(
    (a, b) => siblingRank(a) - siblingRank(b) || (a.label < b.label ? -1 : a.label > b.label ? 1 : 0),
  );
}

/// Shrink a rect symmetrically by `inset` per side, capping the inset so
/// deep nesting degrades to thin cells instead of negative sizes.
function shrink(rect: Rect, inset: number): Rect {
  const dx = Math.min(inset, rect.w / 4);
  const dz = Math.min(inset, rect.d / 4);
  return { x: rect.x + dx, z: rect.z + dz, w: rect.w - 2 * dx, d: rect.d - 2 * dz };
}

/// Lay the whole payload out: recursive squarified treemap over the folder
/// hierarchy. Every folder (including the root) becomes a district plateau
/// at `depth * plinth`; every file becomes a building sitting on its
/// parent's plateau.
export function codeCityLayout(data: CodeCityData, opts?: CityLayoutOptions): CityModel {
  const o: Required<CityLayoutOptions> = { ...DEFAULTS, ...opts };
  const buildings: CityBuilding[] = [];
  const districts: CityDistrict[] = [];

  const byParent = new Map<string, CityNode[]>();
  let root: CityNode | null = null;
  for (const n of data.nodes) {
    if (n.parent === null) {
      root = n;
      continue;
    }
    const arr = byParent.get(n.parent);
    if (arr) arr.push(n);
    else byParent.set(n.parent, [n]);
  }

  const weightOf = (n: CityNode): number => Math.max(n.bytes, 1);

  const layoutFolder = (folder: CityNode, rect: Rect, depth: number) => {
    const plateauY = depth * o.plinth;
    districts.push({
      id: folder.id,
      label: folder.label,
      depth,
      x: rect.x,
      z: rect.z,
      w: rect.w,
      d: rect.d,
      y: plateauY,
    });
    const children = sortSiblings(byParent.get(folder.id) ?? []);
    if (children.length === 0) return;
    const inner = shrink(rect, o.padding);
    const cells = squarify(
      children.map((c) => ({ id: c.id, weight: weightOf(c) })),
      inner,
    );
    for (const child of children) {
      const cell = cells.get(child.id);
      if (!cell || cell.w <= 0 || cell.d <= 0) continue;
      if (child.kind === 'file') {
        const foot = shrink(cell, o.gap);
        if (foot.w <= 0 || foot.d <= 0) continue;
        buildings.push({
          id: child.id,
          label: child.label,
          fqn: child.fqn,
          module: child.module,
          x: foot.x,
          z: foot.z,
          w: foot.w,
          d: foot.d,
          y: plateauY,
          h: heightFor(child.sloc, child.bytes, o),
          color: buildingColor(child.risk_score),
          glow: glowFor(child.recency_secs_ago),
          score: child.risk_score,
          sloc: child.sloc,
          bytes: child.bytes,
        });
      } else {
        layoutFolder(child, cell, depth + 1);
      }
    }
  };

  if (root) {
    layoutFolder(root, { x: 0, z: 0, w: o.world, d: o.world }, 0);
  }

  return { buildings, districts, world: o.world, truncated: data.truncated };
}

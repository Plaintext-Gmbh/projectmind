/// Tests for the pure code-city layout (V4.6a, #66). No DOM, no WebGL —
/// vitest under happy-dom must stay green on the Linux CI runner, so
/// nothing here (nor in codeCityLayout.ts) may import three.

import { describe, expect, it } from 'vitest';
import type { CityNode, CodeCityData } from '../api';
import { colorForScore } from '../treemap';
import {
  buildingColor,
  cameraFitFor,
  codeCityLayout,
  glowFor,
  heightFor,
  squarify,
  type CityLayoutOptions,
  type Rect,
} from './codeCityLayout';

const OPTS: Required<CityLayoutOptions> = {
  world: 200,
  padding: 1.5,
  gap: 0.4,
  plinth: 0.6,
  minH: 0.5,
  maxH: 30,
};

function node(partial: Partial<CityNode> & { id: string }): CityNode {
  return {
    parent: '.',
    label: partial.id.split('/').pop() ?? partial.id,
    kind: 'file',
    depth: partial.id === '.' ? 0 : partial.id.split('/').length,
    bytes: 1,
    sloc: null,
    risk_score: null,
    churn: null,
    fqn: null,
    module: null,
    recency_secs_ago: null,
    ...partial,
  };
}

function payload(nodes: CityNode[], truncated = false): CodeCityData {
  return {
    root: '/repo',
    max_depth: 5,
    truncated,
    has_risk: false,
    nodes: [node({ id: '.', parent: null, kind: 'root', depth: 0, label: 'repo' }), ...nodes],
  };
}

const overlaps = (a: Rect, b: Rect): boolean =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.z < b.z + b.d && b.z < a.z + a.d;

describe('squarify', () => {
  const rect: Rect = { x: 0, z: 0, w: 100, d: 60 };

  it('areas are proportional to weights and tile the rect', () => {
    const items = [
      { id: 'a', weight: 6 },
      { id: 'b', weight: 3 },
      { id: 'c', weight: 1 },
    ];
    const cells = squarify(items, rect);
    const total = rect.w * rect.d;
    expect(cells.size).toBe(3);
    let sum = 0;
    for (const it of items) {
      const c = cells.get(it.id)!;
      const area = c.w * c.d;
      sum += area;
      expect(area).toBeCloseTo((it.weight / 10) * total, 6);
    }
    expect(sum).toBeCloseTo(total, 6);
  });

  it('sibling cells never overlap and stay inside the rect', () => {
    const items = Array.from({ length: 17 }, (_, i) => ({ id: `n${i}`, weight: i + 1 }));
    const cells = [...squarify(items, rect).values()];
    for (const c of cells) {
      expect(c.x).toBeGreaterThanOrEqual(rect.x - 1e-9);
      expect(c.z).toBeGreaterThanOrEqual(rect.z - 1e-9);
      expect(c.x + c.w).toBeLessThanOrEqual(rect.x + rect.w + 1e-9);
      expect(c.z + c.d).toBeLessThanOrEqual(rect.z + rect.d + 1e-9);
    }
    for (let i = 0; i < cells.length; i++) {
      for (let j = i + 1; j < cells.length; j++) {
        expect(overlaps(cells[i], cells[j]), `cells ${i} and ${j} overlap`).toBe(false);
      }
    }
  });

  it('keeps aspect ratios better than a naive strip layout', () => {
    // 10 equal weights in a square: strips would be 10:1 slivers; the
    // squarified rows must do markedly better.
    const square: Rect = { x: 0, z: 0, w: 100, d: 100 };
    const items = Array.from({ length: 10 }, (_, i) => ({ id: `n${i}`, weight: 1 }));
    const cells = [...squarify(items, square).values()];
    const worst = Math.max(...cells.map((c) => Math.max(c.w / c.d, c.d / c.w)));
    expect(worst).toBeLessThan(10); // naive vertical strips give exactly 10
    expect(worst).toBeLessThanOrEqual(3);
  });

  it('handles empty input and all-zero weights', () => {
    expect(squarify([], rect).size).toBe(0);
    const cells = squarify(
      [
        { id: 'a', weight: 0 },
        { id: 'b', weight: 0 },
      ],
      rect,
    );
    // Zero weights degrade to an even tiling — both still get cells.
    expect(cells.size).toBe(2);
    const areas = [...cells.values()].map((c) => c.w * c.d);
    expect(areas[0]).toBeCloseTo(areas[1], 6);
  });
});

describe('heightFor', () => {
  it('clamps to minH and maxH', () => {
    expect(heightFor(null, 0, OPTS)).toBe(OPTS.minH);
    expect(heightFor(1_000_000, 0, OPTS)).toBe(OPTS.maxH);
  });

  it('is monotonic in the metric', () => {
    let prev = 0;
    for (const sloc of [1, 10, 100, 1000, 5000]) {
      const h = heightFor(sloc, 0, OPTS);
      expect(h).toBeGreaterThan(prev);
      prev = h;
    }
  });

  it('prefers sloc over the bytes heuristic', () => {
    // 400 bytes ≈ 10 lines via the heuristic; explicit sloc 1000 must win.
    expect(heightFor(1000, 400, OPTS)).toBeGreaterThan(heightFor(null, 400, OPTS));
    // And with sloc present, bytes are ignored entirely.
    expect(heightFor(50, 1, OPTS)).toBeCloseTo(heightFor(50, 1_000_000, OPTS), 9);
  });
});

describe('buildingColor', () => {
  it('mirrors the Risk-Atlas colorForScore scale', () => {
    expect(buildingColor(0)).toBe(colorForScore(0));
    expect(buildingColor(100)).toBe(colorForScore(100));
    expect(buildingColor(0)).toContain('hsl(120'); // green
    expect(buildingColor(100)).toContain('hsl(0'); // red
  });

  it('falls back to neutral slate without a score', () => {
    expect(buildingColor(null)).toBe('hsl(215, 15%, 40%)');
  });
});

describe('glowFor', () => {
  it('ramps 1.0 inside 24h down to 0 at 7 days', () => {
    expect(glowFor(null)).toBe(0);
    expect(glowFor(0)).toBe(1);
    expect(glowFor(3600)).toBe(1);
    expect(glowFor(7 * 86_400)).toBe(0);
    expect(glowFor(30 * 86_400)).toBe(0);
    const mid = glowFor(4 * 86_400);
    expect(mid).toBeGreaterThan(0);
    expect(mid).toBeLessThan(1);
    // Linear + monotonic decreasing on the ramp.
    expect(glowFor(2 * 86_400)).toBeGreaterThan(glowFor(5 * 86_400));
    expect(glowFor(4 * 86_400)).toBeCloseTo(0.5, 9);
  });
});

describe('codeCityLayout', () => {
  it('empty repo (only root) → 0 buildings, 1 district', () => {
    const model = codeCityLayout(payload([]));
    expect(model.buildings).toHaveLength(0);
    expect(model.districts).toHaveLength(1);
    expect(model.districts[0].id).toBe('.');
    expect(model.districts[0].y).toBe(0);
    expect(model.world).toBe(200);
    expect(model.truncated).toBe(false);
  });

  it('single file fills the padded root district', () => {
    const model = codeCityLayout(
      payload([node({ id: 'main.rs', bytes: 4000, sloc: 100, risk_score: 80 })]),
    );
    expect(model.buildings).toHaveLength(1);
    const b = model.buildings[0];
    expect(b.id).toBe('main.rs');
    expect(b.y).toBe(0); // sits on the root plateau
    expect(b.color).toBe(colorForScore(80));
    expect(b.h).toBe(heightFor(100, 4000, OPTS));
    // Inside the world square.
    expect(b.x).toBeGreaterThan(0);
    expect(b.z).toBeGreaterThan(0);
    expect(b.x + b.w).toBeLessThan(200);
    expect(b.z + b.d).toBeLessThan(200);
  });

  it('nesting to depth 5 stacks plateaus and keeps children inside parents', () => {
    const nodes: CityNode[] = [];
    let parent = '.';
    let path = '';
    for (let d = 1; d <= 4; d++) {
      path = path === '' ? `f${d}` : `${path}/f${d}`;
      nodes.push(node({ id: path, parent, kind: 'folder', depth: d, bytes: 10 }));
      parent = path;
    }
    nodes.push(node({ id: `${path}/leaf.txt`, parent, depth: 5, bytes: 10 }));
    const model = codeCityLayout(payload(nodes));

    expect(model.districts).toHaveLength(5); // root + 4 folders
    for (const dist of model.districts) {
      expect(dist.y).toBeCloseTo(dist.depth * OPTS.plinth, 9);
    }
    // Each district is contained in its parent's rect.
    const byId = new Map(model.districts.map((d) => [d.id, d]));
    for (const dist of model.districts) {
      if (dist.id === '.') continue;
      const parentId = dist.id.includes('/')
        ? dist.id.slice(0, dist.id.lastIndexOf('/'))
        : '.';
      const p = byId.get(parentId)!;
      expect(dist.x).toBeGreaterThanOrEqual(p.x - 1e-9);
      expect(dist.z).toBeGreaterThanOrEqual(p.z - 1e-9);
      expect(dist.x + dist.w).toBeLessThanOrEqual(p.x + p.w + 1e-9);
      expect(dist.z + dist.d).toBeLessThanOrEqual(p.z + p.d + 1e-9);
    }
    // The deep leaf sits on the deepest folder's plateau.
    expect(model.buildings).toHaveLength(1);
    expect(model.buildings[0].y).toBeCloseTo(4 * OPTS.plinth, 9);
  });

  it('buildings of different folders never collide', () => {
    const nodes: CityNode[] = [
      node({ id: 'a', kind: 'folder', depth: 1, bytes: 100 }),
      node({ id: 'b', kind: 'folder', depth: 1, bytes: 50 }),
      node({ id: 'a/x.rs', parent: 'a', depth: 2, bytes: 60 }),
      node({ id: 'a/y.rs', parent: 'a', depth: 2, bytes: 40 }),
      node({ id: 'b/z.rs', parent: 'b', depth: 2, bytes: 50 }),
      node({ id: 'top.md', depth: 1, bytes: 30 }),
    ];
    const model = codeCityLayout(payload(nodes));
    expect(model.buildings).toHaveLength(4);
    for (let i = 0; i < model.buildings.length; i++) {
      for (let j = i + 1; j < model.buildings.length; j++) {
        const a = model.buildings[i];
        const b = model.buildings[j];
        expect(overlaps(a, b), `${a.id} overlaps ${b.id}`).toBe(false);
      }
    }
  });

  it('passes truncated through and is deterministic', () => {
    const nodes = [
      node({ id: 'a', kind: 'folder', depth: 1, bytes: 10 }),
      node({ id: 'a/f.txt', parent: 'a', depth: 2, bytes: 10 }),
      node({ id: 'z.txt', depth: 1, bytes: 5 }),
    ];
    const p = payload(nodes, true);
    const one = codeCityLayout(p);
    const two = codeCityLayout(p);
    expect(one.truncated).toBe(true);
    expect(two).toEqual(one);
  });
});

describe('cameraFitFor', () => {
  it('positions the camera outside the bounding box, aimed at the centre', () => {
    const model = codeCityLayout(payload([node({ id: 'main.rs', bytes: 400 })]));
    const { position, target } = cameraFitFor(model);
    const [px, py, pz] = position;
    // Outside the world square, above the tallest possible building.
    expect(px).toBeGreaterThan(model.world);
    expect(pz).toBeGreaterThan(model.world);
    expect(py).toBeGreaterThan(OPTS.maxH);
    expect(target).toEqual([model.world / 2, 0, model.world / 2]);
  });
});

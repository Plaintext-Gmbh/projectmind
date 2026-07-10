/// Tests for the pure first-person walk physics (V4.6b, #66). No DOM, no
/// WebGL — vitest under happy-dom must stay green on the Linux CI runner,
/// so nothing here (nor in cityWalk.ts) may import three.

import { describe, expect, it } from 'vitest';
import type { CityBuilding, CityDistrict, CityModel } from './codeCityLayout';
import {
  collides,
  groundHeightAt,
  resolveCollision,
  stepMovement,
  WALK_DEFAULTS,
  type WalkInput,
  type WalkPose,
} from './cityWalk';

function district(
  partial: Partial<CityDistrict> & Pick<CityDistrict, 'id' | 'x' | 'z' | 'w' | 'd' | 'y'>,
): CityDistrict {
  return { label: partial.id, depth: 0, ...partial };
}

function building(
  partial: Partial<CityBuilding> & Pick<CityBuilding, 'id' | 'x' | 'z' | 'w' | 'd'>,
): CityBuilding {
  return {
    label: partial.id,
    fqn: null,
    module: null,
    y: 0,
    h: 10,
    color: 'hsl(215, 15%, 40%)',
    glow: 0,
    score: null,
    sloc: null,
    bytes: 100,
    ...partial,
  };
}

/// City fixture: 100×100 root plateau at 0, nested districts terracing to
/// 0.6 and 1.2, one 4×4 building at (30, 30) on the depth-1 plateau.
const model: CityModel = {
  world: 100,
  truncated: false,
  districts: [
    district({ id: '.', x: 0, z: 0, w: 100, d: 100, y: 0, depth: 0 }),
    district({ id: 'a', x: 10, z: 10, w: 40, d: 40, y: 0.6, depth: 1 }),
    district({ id: 'a/b', x: 15, z: 15, w: 12, d: 12, y: 1.2, depth: 2 }),
  ],
  buildings: [building({ id: 'a/f.rs', x: 30, z: 30, w: 4, d: 4, y: 0.6 })],
};

const emptyModel: CityModel = { world: 100, truncated: false, districts: [], buildings: [] };

const pose = (x: number, z: number, yaw = 0): WalkPose => ({
  x,
  y: groundHeightAt(model, x, z) + WALK_DEFAULTS.eyeHeight,
  z,
  yaw,
});

const input = (partial?: Partial<WalkInput>): WalkInput => ({
  forward: 0,
  strafe: 0,
  sprint: false,
  ...partial,
});

describe('groundHeightAt', () => {
  it('returns the plateau top of the deepest containing district', () => {
    expect(groundHeightAt(model, 5, 5)).toBe(0); // root only
    expect(groundHeightAt(model, 12, 12)).toBe(0.6); // depth-1 terrace
    expect(groundHeightAt(model, 20, 20)).toBe(1.2); // nested depth-2 terrace
  });

  it('returns the ground plane outside the city', () => {
    expect(groundHeightAt(model, -5, 50)).toBe(0);
    expect(groundHeightAt(model, 50, 130)).toBe(0);
  });

  it('handles an empty layout', () => {
    expect(groundHeightAt(emptyModel, 50, 50)).toBe(0);
  });
});

describe('collides / resolveCollision', () => {
  it('detects the building footprint expanded by the radius', () => {
    expect(collides(model, 32, 32, 0.35)).toBe(true); // inside
    expect(collides(model, 29.8, 32, 0.35)).toBe(true); // within radius of the wall
    expect(collides(model, 29.8, 32, 0)).toBe(false); // radius matters
    expect(collides(model, 28, 32, 0.35)).toBe(false); // clear of it
  });

  it('blocks head-on movement into a wall', () => {
    const r = resolveCollision(model, { x: 28, z: 32 }, { x: 31, z: 32 }, 0.35);
    expect(r).toEqual({ x: 28, z: 32 });
  });

  it('slides along the wall on a diagonal approach', () => {
    // Moving +x/+z into the south-west corner region: x passes (z is still
    // clear of the footprint), z is then blocked → slide along the wall.
    const r = resolveCollision(model, { x: 28, z: 28 }, { x: 31, z: 31 }, 0.35);
    expect(r).toEqual({ x: 31, z: 28 });
  });

  it('moves freely when no building is in the way', () => {
    const r = resolveCollision(model, { x: 5, z: 5 }, { x: 6, z: 7 }, 0.35);
    expect(r).toEqual({ x: 6, z: 7 });
  });

  it('lets a walker spawned inside a building walk out (escape hatch)', () => {
    const r = resolveCollision(model, { x: 32, z: 32 }, { x: 36, z: 32 }, 0.35);
    expect(r).toEqual({ x: 36, z: 32 });
  });
});

describe('stepMovement', () => {
  it('moves along the yaw-projected forward axis', () => {
    // yaw 0 looks down −z (three.js convention).
    const a = stepMovement(model, pose(50, 90), input({ forward: 1 }), 0.5);
    expect(a.x).toBeCloseTo(50, 9);
    expect(a.z).toBeCloseTo(90 - WALK_DEFAULTS.speed * 0.5, 9);
    // yaw π/2 looks down −x.
    const b = stepMovement(model, pose(50, 90, Math.PI / 2), input({ forward: 1 }), 0.5);
    expect(b.x).toBeCloseTo(50 - WALK_DEFAULTS.speed * 0.5, 9);
    expect(b.z).toBeCloseTo(90, 9);
    // strafe +1 at yaw 0 goes +x.
    const c = stepMovement(model, pose(50, 90), input({ strafe: 1 }), 0.5);
    expect(c.x).toBeCloseTo(50 + WALK_DEFAULTS.speed * 0.5, 9);
    expect(c.z).toBeCloseTo(90, 9);
  });

  it('is dt-independent: two half steps equal one full step', () => {
    const start = pose(50, 90, Math.PI / 5);
    const inp = input({ forward: 1, strafe: -1 });
    const full = stepMovement(model, start, inp, 1);
    const half = stepMovement(model, stepMovement(model, start, inp, 0.5), inp, 0.5);
    expect(half.x).toBeCloseTo(full.x, 9);
    expect(half.z).toBeCloseTo(full.z, 9);
    expect(half.y).toBeCloseTo(full.y, 9);
  });

  it('normalises diagonal input and applies the sprint factor', () => {
    const start = pose(50, 90);
    const diag = stepMovement(model, start, input({ forward: 1, strafe: 1 }), 1);
    const moved = Math.hypot(diag.x - start.x, diag.z - start.z);
    expect(moved).toBeCloseTo(WALK_DEFAULTS.speed, 9); // not ×√2
    const sprint = stepMovement(model, start, input({ forward: 1, sprint: true }), 1);
    expect(Math.abs(sprint.z - start.z)).toBeCloseTo(
      WALK_DEFAULTS.speed * WALK_DEFAULTS.sprintFactor,
      9,
    );
  });

  it('clamps y onto the terraces while walking up and down', () => {
    // Walk from the root plateau across the depth-1 edge at x = 10.
    const onRoot = pose(5, 12);
    expect(onRoot.y).toBeCloseTo(WALK_DEFAULTS.eyeHeight, 9);
    const up = stepMovement(model, { ...onRoot, yaw: -Math.PI / 2 }, input({ forward: 1 }), 0.5);
    expect(up.x).toBeGreaterThan(10);
    expect(up.y).toBeCloseTo(0.6 + WALK_DEFAULTS.eyeHeight, 9);
    const down = stepMovement(model, { ...up, yaw: Math.PI / 2 }, input({ forward: 1 }), 0.5);
    expect(down.x).toBeLessThan(10);
    expect(down.y).toBeCloseTo(WALK_DEFAULTS.eyeHeight, 9);
  });

  it('blocks at buildings and slides along their walls', () => {
    // Head-on into the west face of the building at x = 30 (expanded to
    // 30 − radius = 29.65): x stops short of the wall even though one
    // frame's travel would overshoot the whole building (no tunnelling).
    const blocked = stepMovement(model, pose(29, 32, -Math.PI / 2), input({ forward: 1 }), 1);
    expect(blocked.x).toBeGreaterThanOrEqual(29);
    expect(blocked.x).toBeLessThan(30 - WALK_DEFAULTS.radius);
    expect(blocked.z).toBe(32);
    // Diagonal into the same face: x stays blocked while z slides the full
    // tangential distance along the wall (dt chosen so the walker stays
    // within the wall's z-extent — a longer step would round the corner).
    const slide = stepMovement(
      model,
      pose(29, 30, -Math.PI / 2),
      input({ forward: 1, strafe: 1 }),
      0.2,
    );
    expect(slide.x).toBeLessThan(30 - WALK_DEFAULTS.radius);
    expect(slide.z).toBeCloseTo(30 + (WALK_DEFAULTS.speed * 0.2) / Math.SQRT2, 9);
  });

  it('clamps to the roam bounds around the city', () => {
    const out = stepMovement(model, pose(50, 5), input({ forward: 1 }), 60);
    expect(out.z).toBe(-WALK_DEFAULTS.margin);
    expect(out.y).toBeCloseTo(WALK_DEFAULTS.eyeHeight, 9); // ground plane outside
  });

  it('keeps the pose still on zero input and works on an empty layout', () => {
    const still = stepMovement(model, pose(20, 20), input(), 1);
    expect(still).toEqual(pose(20, 20));
    const empty = stepMovement(
      emptyModel,
      { x: 50, y: WALK_DEFAULTS.eyeHeight, z: 50, yaw: 0 },
      input({ forward: 1 }),
      0.25,
    );
    expect(empty.z).toBeCloseTo(50 - WALK_DEFAULTS.speed * 0.25, 9);
    expect(empty.y).toBeCloseTo(WALK_DEFAULTS.eyeHeight, 9);
  });
});

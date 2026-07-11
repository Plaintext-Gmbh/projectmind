/// Tests for the pure tour-waypoint logic (V4.6c, #66). No DOM, no WebGL —
/// vitest under happy-dom must stay green on the Linux CI runner, so nothing
/// here (nor in cityTour.ts) may import three.

import { describe, expect, it } from 'vitest';
import type { WalkthroughTarget } from '../api';
import type { CityBuilding, CityModel } from './codeCityLayout';
import {
  cameraFlightTo,
  normalizeStepPath,
  resolveTourTarget,
  shouldRefetchTourBody,
  smoothstep,
  TOUR_CAMERA_DEFAULTS,
  tweenPose,
  type CameraPose,
} from './cityTour';

function building(
  partial: Partial<CityBuilding> & Pick<CityBuilding, 'id' | 'x' | 'z' | 'w' | 'd' | 'h'>,
): CityBuilding {
  return {
    label: partial.id,
    fqn: null,
    module: null,
    y: 0,
    color: 'hsl(215, 15%, 40%)',
    glow: 0,
    score: null,
    sloc: null,
    bytes: 100,
    ...partial,
  };
}

/// Two buildings: a parsed Rust tower with an fqn (hottest class) and a
/// plain Svelte file without one.
const tower = building({
  id: 'crates/core/src/engine.rs',
  fqn: 'core::engine::Engine',
  module: 'core',
  x: 30,
  z: 30,
  w: 4,
  d: 4,
  y: 0.6,
  h: 20,
});
const shed = building({ id: 'app/src/App.svelte', x: 80, z: 10, w: 1, d: 1, h: 0.5 });

const model: CityModel = {
  world: 200,
  truncated: false,
  districts: [],
  buildings: [tower, shed],
};

const ROOT = '/home/user/projects/demo';

describe('normalizeStepPath', () => {
  it('strips the repo root off an absolute step path', () => {
    expect(normalizeStepPath(`${ROOT}/app/src/App.svelte`, ROOT)).toBe('app/src/App.svelte');
  });

  it('tolerates a trailing slash on the root', () => {
    expect(normalizeStepPath(`${ROOT}/a.ts`, `${ROOT}/`)).toBe('a.ts');
  });

  it('passes an already repo-relative path through', () => {
    expect(normalizeStepPath('app/src/App.svelte', ROOT)).toBe('app/src/App.svelte');
    expect(normalizeStepPath('./app/src/App.svelte', ROOT)).toBe('app/src/App.svelte');
  });

  it('folds Windows separators', () => {
    expect(normalizeStepPath('C:\\repo\\src\\a.ts', 'C:\\repo')).toBe('src/a.ts');
  });

  it('leaves paths outside the root untouched (they match no building)', () => {
    expect(normalizeStepPath('/elsewhere/x.ts', ROOT)).toBe('/elsewhere/x.ts');
  });

  it('works without a root (null)', () => {
    expect(normalizeStepPath('app/a.ts', null)).toBe('app/a.ts');
  });
});

describe('resolveTourTarget', () => {
  it('maps a class step onto the building carrying that fqn', () => {
    const t: WalkthroughTarget = { kind: 'class', fqn: 'core::engine::Engine' };
    expect(resolveTourTarget(model, t, ROOT)).toEqual({ buildingId: tower.id });
  });

  it('maps a risk step like a class step', () => {
    const t: WalkthroughTarget = { kind: 'risk', fqn: 'core::engine::Engine' };
    expect(resolveTourTarget(model, t, ROOT)).toEqual({ buildingId: tower.id });
  });

  it('misses on an unknown fqn', () => {
    const t: WalkthroughTarget = { kind: 'class', fqn: 'no.such.Class' };
    expect(resolveTourTarget(model, t, ROOT)).toBeNull();
  });

  it('maps an absolute file step onto the repo-relative building id', () => {
    const t: WalkthroughTarget = { kind: 'file', path: `${ROOT}/app/src/App.svelte` };
    expect(resolveTourTarget(model, t, ROOT)).toEqual({ buildingId: shed.id });
  });

  it('maps a repo-relative file step directly', () => {
    const t: WalkthroughTarget = { kind: 'file', path: 'app/src/App.svelte' };
    expect(resolveTourTarget(model, t, null)).toEqual({ buildingId: shed.id });
  });

  it('misses on a file outside the city', () => {
    const t: WalkthroughTarget = { kind: 'file', path: `${ROOT}/README.adoc` };
    expect(resolveTourTarget(model, t, ROOT)).toBeNull();
  });

  it('returns null for kinds without city geometry', () => {
    const targets: WalkthroughTarget[] = [
      { kind: 'note' },
      { kind: 'diff', reference: 'HEAD~1' },
      { kind: 'artifact', id: 'a1' },
      { kind: 'pattern', pattern: 'layering' },
      { kind: 'atlas' },
      { kind: 'diagram-diff', from: 'HEAD~5' },
    ];
    for (const t of targets) {
      expect(resolveTourTarget(model, t, ROOT)).toBeNull();
    }
  });

  // --- ClassEntry join fallback (R2 fix): the risk join only tags the
  // hottest class per file (and nothing at all without git history) —
  // the classIndex resolves every other class via its source file.
  it('resolves a non-hottest class via the classIndex (fqn → file → building)', () => {
    const idx = new Map([['app::App', `${ROOT}/app/src/App.svelte`]]);
    const t: WalkthroughTarget = { kind: 'class', fqn: 'app::App' };
    expect(resolveTourTarget(model, t, ROOT, idx)).toEqual({ buildingId: shed.id });
  });

  it('prefers the direct fqn match over the classIndex', () => {
    // Index deliberately points the hottest class at the WRONG building —
    // the direct building.fqn hit must keep winning.
    const idx = new Map([['core::engine::Engine', `${ROOT}/app/src/App.svelte`]]);
    const t: WalkthroughTarget = { kind: 'class', fqn: 'core::engine::Engine' };
    expect(resolveTourTarget(model, t, ROOT, idx)).toEqual({ buildingId: tower.id });
  });

  it('still misses when the classIndex maps to a file outside the city', () => {
    const idx = new Map([['app::Ghost', `${ROOT}/generated/Ghost.svelte`]]);
    const t: WalkthroughTarget = { kind: 'class', fqn: 'app::Ghost' };
    expect(resolveTourTarget(model, t, ROOT, idx)).toBeNull();
  });

  it('misses unknown fqns without an index exactly like before', () => {
    const t: WalkthroughTarget = { kind: 'risk', fqn: 'app::App' };
    expect(resolveTourTarget(model, t, ROOT, null)).toBeNull();
  });

  it('folds Windows drive-letter casing when stripping the root', () => {
    const winModel: CityModel = {
      ...model,
      buildings: [building({ id: 'src/a.ts', x: 1, z: 1, w: 1, d: 1, h: 1 })],
    };
    const t: WalkthroughTarget = { kind: 'file', path: 'C:/repo/src/a.ts' };
    expect(resolveTourTarget(winModel, t, 'c:/repo')).toEqual({ buildingId: 'src/a.ts' });
  });
});

describe('shouldRefetchTourBody', () => {
  const cursor = { id: 'tour-1', nonce: 3 };

  it('never refetches without a cursor', () => {
    expect(shouldRefetchTourBody(null, { id: 'tour-1' }, 3)).toBe(false);
  });

  it('fetches when nothing is cached (first cursor or failed fetch)', () => {
    expect(shouldRefetchTourBody(cursor, null, -1)).toBe(true);
  });

  it('fetches when the tour id changed', () => {
    expect(shouldRefetchTourBody(cursor, { id: 'tour-0' }, 3)).toBe(true);
  });

  it('fetches when the nonce moved behind the same id (append/rewrite)', () => {
    expect(shouldRefetchTourBody(cursor, { id: 'tour-1' }, 2)).toBe(true);
  });

  it('keeps the cache when id and nonce both match', () => {
    expect(shouldRefetchTourBody(cursor, { id: 'tour-1' }, 3)).toBe(false);
  });
});

describe('cameraFlightTo', () => {
  const centreOf = (b: CityBuilding): [number, number, number] => [
    b.x + b.w / 2,
    b.y + b.h / 2,
    b.z + b.d / 2,
  ];

  it('looks at the building centre at half height', () => {
    const pose = cameraFlightTo(model, tower, null);
    expect(pose.target).toEqual(centreOf(tower));
  });

  it('approaches from the configured elevation', () => {
    const pose = cameraFlightTo(model, tower, null);
    const dy = pose.position[1] - pose.target[1];
    const horizontal = Math.hypot(
      pose.position[0] - pose.target[0],
      pose.position[2] - pose.target[2],
    );
    expect(Math.atan2(dy, horizontal)).toBeCloseTo(TOUR_CAMERA_DEFAULTS.elevation, 9);
  });

  it('scales the distance with the building size, clamped below by minDistance', () => {
    const dist = (b: CityBuilding): number => {
      const pose = cameraFlightTo(model, b, null);
      return Math.hypot(
        pose.position[0] - pose.target[0],
        pose.position[1] - pose.target[1],
        pose.position[2] - pose.target[2],
      );
    };
    // Tower: height 20 dominates → 20 · 2.4 = 48.
    expect(dist(tower)).toBeCloseTo(20 * TOUR_CAMERA_DEFAULTS.distanceFactor, 9);
    // Shed: everything tiny → clamped to minDistance.
    expect(dist(shed)).toBeCloseTo(TOUR_CAMERA_DEFAULTS.minDistance, 9);
    // Never further out than the world edge length.
    const skyscraper = building({ id: 'big', x: 0, z: 0, w: 100, d: 100, h: 30 });
    expect(dist(skyscraper)).toBeCloseTo(model.world, 9);
  });

  it('ends up outside the building footprint', () => {
    const pose = cameraFlightTo(model, tower, null);
    const [x, , z] = pose.position;
    const inside = x > tower.x && x < tower.x + tower.w && z > tower.z && z < tower.z + tower.d;
    expect(inside).toBe(false);
  });

  it('keeps the approach azimuth of the current pose', () => {
    // Camera due north of the target (−z side) → stays on that side.
    const current: CameraPose = { position: [32, 50, -100], target: [0, 0, 0] };
    const pose = cameraFlightTo(model, tower, current);
    expect(pose.position[2]).toBeLessThan(pose.target[2]);
    expect(pose.position[0]).toBeCloseTo(pose.target[0], 9);
  });

  it('falls back to the establishing-shot diagonal without a usable azimuth', () => {
    const overhead: CameraPose = { position: [32, 90, 32], target: [32, 0, 32] };
    for (const current of [null, overhead]) {
      const pose = cameraFlightTo(model, tower, current);
      const dx = pose.position[0] - pose.target[0];
      const dz = pose.position[2] - pose.target[2];
      expect(Math.atan2(dx, dz)).toBeCloseTo(Math.PI / 4, 9);
    }
  });
});

describe('smoothstep / tweenPose', () => {
  const a: CameraPose = { position: [0, 10, 20], target: [1, 2, 3] };
  const b: CameraPose = { position: [100, 50, -20], target: [7, 8, 9] };

  it('hits the endpoints exactly (including out-of-range t)', () => {
    expect(tweenPose(a, b, 0)).toEqual(a);
    expect(tweenPose(a, b, 1)).toEqual(b);
    expect(tweenPose(a, b, -0.5)).toEqual(a);
    expect(tweenPose(a, b, 1.5)).toEqual(b);
  });

  it('returns copies, never aliases of the input arrays', () => {
    const startPose = tweenPose(a, b, 0);
    expect(startPose.position).not.toBe(a.position);
    startPose.position[0] = 999;
    expect(a.position[0]).toBe(0);
  });

  it('passes the halfway point at t = 0.5 (smoothstep symmetry)', () => {
    const mid = tweenPose(a, b, 0.5);
    expect(mid.position[0]).toBeCloseTo(50, 9);
    expect(mid.target[1]).toBeCloseTo(5, 9);
  });

  it('progresses monotonically with zero slope at the ends', () => {
    let prev = -1;
    for (let i = 0; i <= 20; i++) {
      const s = smoothstep(i / 20);
      expect(s).toBeGreaterThanOrEqual(prev);
      prev = s;
    }
    // Ease-in/out: the first and last 5 % move less than a linear ramp.
    expect(smoothstep(0.05)).toBeLessThan(0.05);
    expect(1 - smoothstep(0.95)).toBeLessThan(0.05);
  });

  it('every position component moves monotonically towards the destination', () => {
    let prevX = a.position[0];
    let prevZ = a.position[2];
    for (let i = 1; i <= 10; i++) {
      const pose = tweenPose(a, b, i / 10);
      expect(pose.position[0]).toBeGreaterThanOrEqual(prevX); // 0 → 100
      expect(pose.position[2]).toBeLessThanOrEqual(prevZ); // 20 → −20
      prevX = pose.position[0];
      prevZ = pose.position[2];
    }
  });
});

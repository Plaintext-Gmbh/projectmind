import { describe, expect, it } from 'vitest';
import {
  bornPaths,
  growthTargets,
  LAPSE_GROW_MS,
  LAPSE_STEP_MS,
  stepScales,
  visibleBuildings,
} from './cityTimelapse';
import type { CityBuilding } from './codeCityLayout';
import type { ChangedFile } from '../api';

/// Minimal building stub — the time-lapse only reads `id`; the rest keeps
/// the type honest against `codeCityLayout.ts`.
function building(id: string): CityBuilding {
  return {
    id,
    label: id.split('/').pop() ?? id,
    fqn: null,
    module: null,
    x: 0,
    z: 0,
    w: 1,
    d: 1,
    y: 0,
    h: 5,
    color: 'hsl(215, 15%, 40%)',
    glow: 0,
    score: null,
    sloc: null,
    bytes: 1000,
  };
}

function change(path: string, status: ChangedFile['status']): ChangedFile {
  return { path, status };
}

describe('bornPaths', () => {
  it('counts added and renamed, ignores every other status', () => {
    const born = bornPaths([
      change('a/new.rs', 'added'),
      change('b/moved.rs', 'renamed'),
      change('c/edited.rs', 'modified'),
      change('d/gone.rs', 'deleted'),
      change('e/perms.rs', 'type_change'),
      change('f/odd.rs', 'other'),
    ]);
    expect(born).toEqual(new Set(['a/new.rs', 'b/moved.rs']));
  });

  it('normalises backslashes and ./ prefixes onto the building-id form', () => {
    const born = bornPaths([change('a\\b\\new.rs', 'added'), change('./c/d/new.rs', 'renamed')]);
    expect(born).toEqual(new Set(['a/b/new.rs', 'c/d/new.rs']));
  });

  it('is empty for an empty change list and never throws', () => {
    expect(bornPaths([])).toEqual(new Set());
  });
});

describe('visibleBuildings', () => {
  const buildings = [building('base/old.rs'), building('src/young.rs')];
  const windowBorn = new Set(['src/young.rs']);

  it('always keeps base-city buildings (not window-born) visible', () => {
    // Baseline step: nothing born yet — only the base city stands.
    expect(visibleBuildings(buildings, windowBorn, new Set())).toEqual(new Set(['base/old.rs']));
  });

  it('reveals a window-born building once the cumulative diff lists it', () => {
    const visible = visibleBuildings(buildings, windowBorn, new Set(['src/young.rs']));
    expect(visible).toEqual(new Set(['base/old.rs', 'src/young.rs']));
  });

  it('hides a born building again when it drops out of the cumulative diff (deleted mid-window)', () => {
    // Step 5: born. Step 20 (file deleted): the cumulative diff no longer
    // reports it as added — the building disappears again.
    const atBirth = visibleBuildings(buildings, windowBorn, new Set(['src/young.rs']));
    const afterDelete = visibleBuildings(buildings, windowBorn, new Set());
    expect(atBirth.has('src/young.rs')).toBe(true);
    expect(afterDelete.has('src/young.rs')).toBe(false);
    expect(afterDelete.has('base/old.rs')).toBe(true);
  });

  it('is deterministic and never throws on empty inputs', () => {
    expect(visibleBuildings([], new Set(), new Set())).toEqual(new Set());
  });
});

describe('growthTargets', () => {
  it('writes 1/0 at the buildings-aligned indices', () => {
    const buildings = [building('a.rs'), building('b.rs'), building('c.rs')];
    const out = new Float32Array(3);
    growthTargets(buildings, new Set(['a.rs', 'c.rs']), out);
    expect([...out]).toEqual([1, 0, 1]);
  });

  it('never writes past the output buffer', () => {
    const buildings = [building('a.rs'), building('b.rs')];
    const out = new Float32Array(1);
    growthTargets(buildings, new Set(['b.rs']), out);
    expect([...out]).toEqual([0]);
  });
});

describe('stepScales', () => {
  it('converges monotonically onto the target without overshooting', () => {
    const current = new Float32Array([0]);
    const targets = new Float32Array([1]);
    let prev = 0;
    // 650 ms tween at 40 ms ticks → done after ⌈650/40⌉ = 17 ticks.
    for (let tick = 0; tick < 17; tick++) {
      stepScales(current, targets, 40, LAPSE_GROW_MS);
      expect(current[0]).toBeGreaterThanOrEqual(prev);
      expect(current[0]).toBeLessThanOrEqual(1);
      prev = current[0];
    }
    expect(current[0]).toBe(1);
  });

  it('shrinks towards 0 with the same ramp (reverse scrub)', () => {
    const current = new Float32Array([1]);
    const targets = new Float32Array([0]);
    stepScales(current, targets, LAPSE_GROW_MS / 2, LAPSE_GROW_MS);
    expect(current[0]).toBeCloseTo(0.5, 5);
    stepScales(current, targets, LAPSE_GROW_MS, LAPSE_GROW_MS);
    expect(current[0]).toBe(0);
  });

  it('snaps to the target on a huge dt (tab was hidden) instead of overshooting', () => {
    const current = new Float32Array([0.2, 0.9]);
    const targets = new Float32Array([1, 0]);
    stepScales(current, targets, 60_000, LAPSE_GROW_MS);
    expect([...current]).toEqual([1, 0]);
  });

  it('returns true while animating and false once settled', () => {
    const current = new Float32Array([0]);
    const targets = new Float32Array([1]);
    // The tick that lands exactly on the target still reports true (the
    // caller must paint that final frame); only the next tick is free.
    expect(stepScales(current, targets, LAPSE_GROW_MS, LAPSE_GROW_MS)).toBe(true);
    expect(current[0]).toBe(1);
    expect(stepScales(current, targets, LAPSE_GROW_MS, LAPSE_GROW_MS)).toBe(false);
  });

  it('dt = 0 is a no-op (values hold, still reports animating)', () => {
    const current = new Float32Array([0.5]);
    const targets = new Float32Array([1]);
    expect(stepScales(current, targets, 0, LAPSE_GROW_MS)).toBe(true);
    expect(current[0]).toBe(0.5);
  });

  it('the grow duration stays under the step cadence (a building finishes rising before the next step)', () => {
    expect(LAPSE_GROW_MS).toBeLessThan(LAPSE_STEP_MS);
  });
});

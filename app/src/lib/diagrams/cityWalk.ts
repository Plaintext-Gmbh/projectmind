/// First-person walk physics for the code city (V4.6b, #66) — pure math,
/// no DOM, no WebGL, no three import (Linux CI runs vitest without WebGL).
///
/// `CodeCity.svelte` drives this from its animation loop: PointerLockControls
/// owns the *look* (mouse → camera quaternion), this module owns the *move* —
/// WASD intent + yaw in, a collision- and terrain-resolved pose out.
///
/// Model of the world (all data straight from `codeCityLayout`):
/// - **Ground** = district plateaus. Districts nest, so the ground height at
///   (x, z) is the plateau top of the *deepest* district containing the
///   point; outside every district it is the ground plane at 0. Plateau
///   edges are small (`plinth` per level), so the walker simply steps up and
///   down — no jump, no gravity, `y` is clamped to ground + eye height.
/// - **Obstacles** = buildings. Collision is a 2D axis-aligned test on the
///   XZ footprint (building heights always exceed the step scale, so a pure
///   2D check is enough). Blocked movement slides along the wall via the
///   classic axis-separated resolve: try x first, then z.

import type { CityModel } from './codeCityLayout';

/// Walker pose. `y` is derived (ground + eye height) but kept in the pose so
/// the caller can copy it straight onto the camera. `yaw` follows the
/// three.js YXZ convention: 0 looks down −z, positive turns towards −x.
export interface WalkPose {
  x: number;
  y: number;
  z: number;
  yaw: number;
}

/// Per-frame movement intent, already resolved from pressed keys:
/// `forward` +1 = ahead / −1 = back, `strafe` +1 = right / −1 = left.
export interface WalkInput {
  forward: number;
  strafe: number;
  sprint: boolean;
}

export interface WalkOptions {
  /// Walking speed in world units per second.
  speed?: number;
  /// Speed multiplier while sprinting (Shift).
  sprintFactor?: number;
  /// Collision radius around the walker. Must stay below the layout's
  /// building gap (0.4 per side → 0.8 corridors) so alleys remain passable.
  radius?: number;
  /// Camera height above the plateau the walker stands on.
  eyeHeight?: number;
  /// How far beyond the city square ([0, world]²) the walker may roam.
  margin?: number;
}

/// Defaults tuned to the codeCityLayout scale (world = 200, buildings
/// 0.5–30 high, plateau steps of 0.6): eye height 1.7 reads as street level,
/// 24 u/s crosses the city in ~8 s.
export const WALK_DEFAULTS: Required<WalkOptions> = {
  speed: 24,
  sprintFactor: 2.5,
  radius: 0.35,
  eyeHeight: 1.7,
  margin: 30,
};

/// Ground height at (x, z): plateau top of the deepest district containing
/// the point, 0 outside the city (districts nest, so "deepest containing"
/// equals the max plateau `y` over all hits).
export function groundHeightAt(model: CityModel, x: number, z: number): number {
  let ground = 0;
  for (const d of model.districts) {
    if (x >= d.x && x <= d.x + d.w && z >= d.z && z <= d.z + d.d && d.y > ground) {
      ground = d.y;
    }
  }
  return ground;
}

/// True when a walker disc of `radius` at (x, z) overlaps any building
/// footprint. Pure 2D — see the module header for why height is ignored.
export function collides(model: CityModel, x: number, z: number, radius: number): boolean {
  for (const b of model.buildings) {
    if (x > b.x - radius && x < b.x + b.w + radius && z > b.z - radius && z < b.z + b.d + radius) {
      return true;
    }
  }
  return false;
}

/// Resolve a move from → to against the building AABBs with wall slide:
/// each axis is applied independently, so hitting a wall head-on stops that
/// axis while the tangential axis keeps moving (classic slide-along-wall).
/// Escape hatch: a walker that already starts inside a building (bad spawn)
/// may move freely so it can walk out instead of being stuck.
export function resolveCollision(
  model: CityModel,
  from: { x: number; z: number },
  to: { x: number; z: number },
  radius: number,
): { x: number; z: number } {
  if (collides(model, from.x, from.z, radius)) return { x: to.x, z: to.z };
  const x = collides(model, to.x, from.z, radius) ? from.x : to.x;
  const z = collides(model, x, to.z, radius) ? from.z : to.z;
  return { x, z };
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

/// One integration step: input + yaw → world-space delta (scaled by `dt`
/// seconds — pure linear motion, so N small steps equal one big step and the
/// walker is framerate-independent), clamped to the roam bounds, resolved
/// against buildings, then snapped onto the ground. Returns a new pose;
/// never mutates its inputs.
///
/// The collision resolve is applied in substeps no longer than the walker
/// radius: a single point test at the destination would tunnel straight
/// through a building whenever one frame's travel exceeds its footprint
/// (sprint + a slow frame is enough). Every building is at least 2·radius
/// wide once expanded by the radius, so radius-length substeps cannot skip
/// one. Free movement stays exactly linear, preserving dt-independence.
export function stepMovement(
  model: CityModel,
  pose: WalkPose,
  input: WalkInput,
  dt: number,
  opts?: WalkOptions,
): WalkPose {
  const o: Required<WalkOptions> = { ...WALK_DEFAULTS, ...opts };

  // Normalise the intent so diagonals are not √2 faster.
  let fwd = input.forward;
  let strafe = input.strafe;
  const len = Math.hypot(fwd, strafe);
  if (len > 1) {
    fwd /= len;
    strafe /= len;
  }

  // Yaw → ground-projected forward (−sin, −cos) and right (cos, −sin),
  // matching the three.js YXZ camera convention (yaw 0 looks down −z).
  const dist = o.speed * (input.sprint ? o.sprintFactor : 1) * dt;
  const sin = Math.sin(pose.yaw);
  const cos = Math.cos(pose.yaw);
  const lo = -o.margin;
  const hi = model.world + o.margin;
  // Clamp the target once; the roam box is convex, so every substep along
  // the segment stays inside it as long as the pose already is.
  const to = {
    x: clamp(pose.x + (-sin * fwd + cos * strafe) * dist, lo, hi),
    z: clamp(pose.z + (-cos * fwd - sin * strafe) * dist, lo, hi),
  };

  const travel = Math.hypot(to.x - pose.x, to.z - pose.z);
  const steps = Math.max(1, Math.ceil(travel / o.radius));
  let cur = { x: clamp(pose.x, lo, hi), z: clamp(pose.z, lo, hi) };
  const dx = (to.x - pose.x) / steps;
  const dz = (to.z - pose.z) / steps;
  for (let i = 0; i < steps; i++) {
    cur = resolveCollision(model, cur, { x: cur.x + dx, z: cur.z + dz }, o.radius);
  }
  // Substep accumulation drifts by a few ulps — re-clamp so the roam bounds
  // hold exactly.
  cur = { x: clamp(cur.x, lo, hi), z: clamp(cur.z, lo, hi) };

  return {
    x: cur.x,
    y: groundHeightAt(model, cur.x, cur.z) + o.eyeHeight,
    z: cur.z,
    yaw: pose.yaw,
  };
}

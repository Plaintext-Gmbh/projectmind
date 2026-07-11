/// Tour waypoints for the code city (V4.6c, #66) — pure math + string
/// mapping, no DOM, no WebGL, no three import (Linux CI runs vitest without
/// WebGL). Stage 3/3 of the #66 flythrough.
///
/// `CodeCity.svelte` drives this from its walkthrough-store subscription:
/// while a tour runs, the active step's target is mapped onto a building
/// (`resolveTourTarget`), the orbit camera gets a destination pose
/// (`cameraFlightTo`) and the animation loop eases towards it
/// (`tweenPose`). Deliberately **frontend-only**: the Rust
/// `WalkthroughTarget` enum stays untouched — the city reads the existing
/// class/file/risk targets and maps them itself.
///
/// Mapping rules (see `resolveTourTarget`):
/// - `class` / `risk` → the building whose `fqn` (the file's hottest class,
///   from the risk join) equals the step's fqn.
/// - `file` → the building whose repo-relative `id` equals the step path.
///   Step paths are absolute (the MCP schema wants them that way), building
///   ids are repo-relative — the repo root is stripped first, the exact
///   inverse of the drill's `${root}/${b.id}` join in `CodeCity.svelte`.
/// - every other kind (diff / artifact / pattern / atlas / diagram-diff /
///   note) has no city geometry → `null`; the camera holds its position and
///   the step is a stopover in the flyover.

import type { WalkthroughTarget } from '../api';
import type { CityBuilding, CityModel } from './codeCityLayout';

/// Orbit camera pose: eye position + look-at target, both world-space
/// `[x, y, z]` — exactly the shape `cameraFitFor` returns, so a flight can
/// start from (and end on) any pose the component already knows.
export interface CameraPose {
  position: [number, number, number];
  target: [number, number, number];
}

export interface TourCameraOptions {
  /// Elevation of the approach above the horizon, in radians.
  elevation?: number;
  /// Camera distance as a multiple of the building's dominant dimension.
  distanceFactor?: number;
  /// Lower distance clamp — a 0.5-unit shed must not fill the screen.
  minDistance?: number;
}

/// Camera heuristic (default 50° FOV): an object of size S fills the frame
/// at ≈ 1.07·S, so `distanceFactor` 2.4 shows the building at roughly 40 %
/// of the frame height with its district as context. 35° elevation keeps
/// both the facade colour (risk) and the roof footprint readable — flatter
/// hides the layout, steeper hides the height. `minDistance` 12 stops tiny
/// buildings from becoming wall-filling close-ups.
export const TOUR_CAMERA_DEFAULTS: Required<TourCameraOptions> = {
  elevation: (35 * Math.PI) / 180,
  distanceFactor: 2.4,
  minDistance: 12,
};

/// Normalise a walkthrough file path onto the building-id scale: absolute
/// step path → repo-relative forward-slashed path (the inverse of the
/// drill's `${root}/${b.id}` join). Windows separators are folded, an
/// already-relative path passes through, and a path outside the repo root
/// is returned as-is (it will simply match no building).
export function normalizeStepPath(path: string, repoRoot: string | null): string {
  const slashed = path.replace(/\\/g, '/');
  if (repoRoot) {
    const root = repoRoot.replace(/\\/g, '/').replace(/\/+$/, '');
    if (root && slashed.startsWith(root + '/')) {
      return slashed.slice(root.length + 1);
    }
  }
  return slashed.replace(/^\.\//, '');
}

/// Map the active step's target onto a city building. Returns `null` for
/// misses and for kinds without city geometry — the caller then holds the
/// camera (stopover) instead of flying. See the module header for the
/// per-kind rules.
export function resolveTourTarget(
  model: CityModel,
  target: WalkthroughTarget,
  repoRoot: string | null,
): { buildingId: string } | null {
  switch (target.kind) {
    case 'class':
    case 'risk': {
      const hit = model.buildings.find((b) => b.fqn === target.fqn);
      return hit ? { buildingId: hit.id } : null;
    }
    case 'file': {
      const rel = normalizeStepPath(target.path, repoRoot);
      const hit = model.buildings.find((b) => b.id === rel);
      return hit ? { buildingId: hit.id } : null;
    }
    default:
      return null;
  }
}

/// Destination pose for a flight to `building`: look at the building's
/// centre (half height — facade and roof share the frame), from
/// `elevation` above the horizon at a distance proportional to the
/// building's dominant dimension (max of footprint diagonal and height —
/// see `TOUR_CAMERA_DEFAULTS` for the numbers). The approach azimuth is
/// kept from `currentPose` — the camera swings around the *shortest* arc
/// instead of always circling to a fixed compass side. Degenerate current
/// poses (no pose yet, or camera directly above the target) fall back to
/// the π/4 diagonal of the establishing shot (`cameraFitFor`).
export function cameraFlightTo(
  model: CityModel,
  building: CityBuilding,
  currentPose: CameraPose | null,
  opts?: TourCameraOptions,
): CameraPose {
  const o: Required<TourCameraOptions> = { ...TOUR_CAMERA_DEFAULTS, ...opts };
  const target: [number, number, number] = [
    building.x + building.w / 2,
    building.y + building.h / 2,
    building.z + building.d / 2,
  ];

  const size = Math.max(Math.hypot(building.w, building.d), building.h);
  const distance = Math.min(Math.max(size * o.distanceFactor, o.minDistance), model.world);

  // Approach azimuth: keep the camera on its current side of the target.
  let azimuth = Math.PI / 4;
  if (currentPose) {
    const dx = currentPose.position[0] - target[0];
    const dz = currentPose.position[2] - target[2];
    // Below ~1 world unit of horizontal offset the direction is noise
    // (e.g. hovering directly above) — use the establishing-shot diagonal.
    if (Math.hypot(dx, dz) > 1) azimuth = Math.atan2(dx, dz);
  }

  const horizontal = Math.cos(o.elevation) * distance;
  return {
    position: [
      target[0] + Math.sin(azimuth) * horizontal,
      target[1] + Math.sin(o.elevation) * distance,
      target[2] + Math.cos(azimuth) * horizontal,
    ],
    target,
  };
}

/// Smoothstep 3t² − 2t³ on the clamped unit interval: zero slope at both
/// ends (the camera departs and arrives gently), strictly monotonic in
/// between.
export function smoothstep(t: number): number {
  const x = Math.min(Math.max(t, 0), 1);
  return x * x * (3 - 2 * x);
}

/// Interpolate between two camera poses with smoothstep easing. `t` ≤ 0
/// returns exactly `a`, `t` ≥ 1 exactly `b` (no floating-point drift at the
/// endpoints — the flight must land on the computed pose). Positions and
/// look-at targets are lerped component-wise; the orbit camera derives its
/// orientation from `lookAt(target)`, so no yaw wrapping is needed here.
export function tweenPose(a: CameraPose, b: CameraPose, t: number): CameraPose {
  if (t <= 0) return { position: [...a.position], target: [...a.target] };
  if (t >= 1) return { position: [...b.position], target: [...b.target] };
  const s = smoothstep(t);
  const lerp3 = (
    from: [number, number, number],
    to: [number, number, number],
  ): [number, number, number] => [
    from[0] + (to[0] - from[0]) * s,
    from[1] + (to[1] - from[1]) * s,
    from[2] + (to[2] - from[2]) * s,
  ];
  return { position: lerp3(a.position, b.position), target: lerp3(a.target, b.target) };
}

/// Pure visibility + growth-scale logic for the code-city *time-lapse*
/// (`code-city`, V5 — "watch the city grow along the commit history"). The
/// stateful Three.js mount, the 🎬 player row and the per-step fetch live in
/// `app/src/components/CodeCity.svelte`; this module holds the part that can
/// be unit-tested without a DOM or WebGL — same split as `cityWalk.ts` /
/// `cityTour.ts` (deliberately no `three` import).
///
/// ## The "born in the window" model (honest current-state semantics)
///
/// The timeline comes from `commitTimeline.ts` and every step *k* diffs the
/// cumulative range `timeline[0].sha .. timeline[k].sha` via the existing
/// `list_changes_since`. A file whose status in that diff is `added` (or
/// `renamed` — new path, so it did not exist at the window start either) is
/// **born in the window**: it did not exist at `timeline[0]` and does exist
/// at `timeline[k]`. Everything else is the **base city**, visible from
/// step 0:
///
///  - `modified` files existed before the window — never "born".
///  - Files older than the 24-month activity window, and untracked /
///    working-tree-only files (they have buildings but never appear in a
///    tree-to-tree diff), are base city. Documented limitation.
///  - A file added at step 5 and deleted at step 20 drops out of the
///    cumulative diff again — its building honestly disappears (and only
///    exists at all if the file exists *today*, since the city is built
///    from the working tree).
///  - `timeline[0]`'s own changes are exclusive (step 0 is the baseline
///    `from === to` empty diff), identical to the cinematics contract.
///
/// The city itself stays *today's* city: footprints, heights and the treemap
/// layout are never recomputed per step (historical sizes are stage 2). The
/// time-lapse only scales buildings in/out along Y.

import type { ChangedFile } from '../api';
import type { CityBuilding } from './codeCityLayout';

/// Auto-play cadence — one timeline step every 1.2 s (same as CINE_STEP_MS,
/// so both players feel like the same instrument).
export const LAPSE_STEP_MS = 1200;
/// Growth-tween duration; deliberately < LAPSE_STEP_MS so a building has
/// fully risen before the next step lands.
export const LAPSE_GROW_MS = 650;

/// Normalise a path to the forward-slash, no-`./`-prefix form the building
/// `id` fields use (mirrors the private `normPath` in `beanGraphDiff.ts`).
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '');
}

/// The paths a (cumulative) diff counts as born: status `added` or `renamed`
/// (see the module header for why exactly those two). Pure: empty change
/// list → empty set; never throws.
export function bornPaths(changes: readonly ChangedFile[]): Set<string> {
  const born = new Set<string>();
  for (const c of changes) {
    if (c.status === 'added' || c.status === 'renamed') born.add(normPath(c.path));
  }
  return born;
}

/// The buildings visible at one step: base-city buildings (not born anywhere
/// in the window) are always visible; window-born buildings are visible once
/// the cumulative diff up to this step lists them.
///
///   visible ⇔ !windowBorn.has(id) || bornSoFar.has(id)
///
/// `windowBorn` is `bornPaths` of the full-window diff (step 0 .. last),
/// `bornSoFar` is `bornPaths` of the cumulative diff up to the shown step.
export function visibleBuildings(
  buildings: readonly CityBuilding[],
  windowBorn: ReadonlySet<string>,
  bornSoFar: ReadonlySet<string>,
): Set<string> {
  const visible = new Set<string>();
  for (const b of buildings) {
    if (!windowBorn.has(b.id) || bornSoFar.has(b.id)) visible.add(b.id);
  }
  return visible;
}

/// Write the target scale per building into `out` (aligned with `buildings`
/// by index, exactly like the InstancedMesh instances): 1 = fully grown,
/// 0 = hidden. Indices beyond `out.length` are ignored so a stale buffer
/// can never throw.
export function growthTargets(
  buildings: readonly CityBuilding[],
  visible: ReadonlySet<string>,
  out: Float32Array,
): void {
  const n = Math.min(buildings.length, out.length);
  for (let i = 0; i < n; i++) {
    out[i] = visible.has(buildings[i].id) ? 1 : 0;
  }
}

/// One animation tick: move every `current[i]` towards `targets[i]` by a
/// linear ramp of `dtMs / growMs`, clamped so it lands exactly on the target
/// and never leaves [0, 1]. Returns `true` while anything still had to move
/// when the tick started (the caller should rewrite matrices), `false` once
/// fully settled (the caller can skip all work — the steady state is free).
///
/// The ramp is deliberately linear so this stepper stays trivially testable;
/// the smoothstep easing is applied at matrix-write time
/// (`s_eased = smoothstep(current[i])`, see `cityTour.ts`).
export function stepScales(
  current: Float32Array,
  targets: Float32Array,
  dtMs: number,
  growMs: number = LAPSE_GROW_MS,
): boolean {
  const step = growMs > 0 ? Math.max(dtMs, 0) / growMs : 1;
  const n = Math.min(current.length, targets.length);
  let animating = false;
  for (let i = 0; i < n; i++) {
    const cur = current[i];
    const tgt = Math.min(Math.max(targets[i], 0), 1);
    if (cur === tgt) continue;
    animating = true;
    current[i] = cur < tgt ? Math.min(cur + step, tgt) : Math.max(cur - step, tgt);
  }
  return animating;
}

/// Re-export shim: the pure timeline/range logic that used to live here was
/// extracted verbatim into `commitTimeline.ts` when the code-city time-lapse
/// (V5) started sharing it. Existing cinematics call sites and the colocated
/// test import from this path unchanged — their staying green proves the
/// extraction was behaviour-preserving. New code should import from
/// `./commitTimeline` directly.
export { buildCommitTimeline, stepRange, DEFAULT_CINEMATICS_STEPS } from './commitTimeline';
export type { CinematicsStep, CinematicsRange } from './commitTimeline';

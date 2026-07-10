/// Pure timeline + range logic for the bean-graph *cinematics* player
/// (`bean-graph-live`, V4.3 / #66 concept 2 — "press play over a commit
/// range"). The stateful Cytoscape mount, the player toolbar and the actual
/// per-step fetch/paint live in `app/src/components/BeanGraphLive.svelte`;
/// this module holds the part that can be unit-tested without a DOM: turning
/// the per-module `commit_activity` payload into one global, evenly sampled
/// commit timeline, and mapping a player step onto the `{from, to}` ref pair
/// the existing `listChangesSince(from, to)` API diffs.
///
/// ## Timeline construction
///
/// `commit_activity` groups commits *per module* and a commit touching N
/// modules appears N times (documented on `CommitActivity.total_commits`).
/// The player wants one global reel, so:
///
///  1. **Dedupe by SHA** across modules (first occurrence wins — the same
///     commit carries the same summary/age on every module band).
///  2. **Sort old → new** (descending `secs_ago`; ties break on SHA so the
///     order is deterministic).
///  3. **Downsample evenly** to at most `maxSteps` frames, always keeping the
///     first (oldest) and last (newest) commit so the reel spans the whole
///     window. Between them the picks are evenly spaced index samples — a
///     40-frame movie over 4,000 commits still starts at the window's start
///     and ends at HEAD.
///
/// ## Cumulative ranges (the design choice)
///
/// `stepRange` is **cumulative from the timeline start**: step *k* diffs
/// `timeline[0].sha .. timeline[k].sha`, so every class touched anywhere in
/// the window so far stays accented and each step only *adds* highlights —
/// a much calmer picture than per-commit flashes (the plan's recommendation).
/// Step 0 is the **baseline**: `from === to`, an empty diff by construction —
/// the movie starts on the plain graph and the first advance brings the first
/// changes in. That also sidesteps `timeline[0].sha~1`, which would explode
/// on a root commit.
///
/// ## Honest limitation (same contract as the morph)
///
/// The graph shows *today's* classes. A cinematics step therefore highlights
/// "which of the current classes were touched up to this commit", never a
/// historical intermediate state — classes added or removed along the way are
/// not reconstructed. This is the same documented design decision as
/// `beanGraphMorph.ts` ("morph never shows removed nodes"); the toolbar
/// tooltip says so.
///
/// Kept dependency-free (no `cytoscape` import) so everything here is a plain
/// function — same pattern as `beanGraphFlow.ts` / `activityPulse.ts`.

import type { CommitActivity } from '../api';

/// One frame of the cinematics reel: a commit the player can step onto.
export interface CinematicsStep {
  /// Short (7-char) commit SHA — `listChangesSince` resolves it via revparse.
  sha: string;
  /// First line of the commit message, shown next to the scrubber.
  summary: string;
  /// Seconds between the commit and the activity walk; larger = older.
  secsAgo: number;
}

/// The ref pair one player step diffs, fed straight into
/// `listChangesSince(from, to)`. `from === to` marks the baseline step
/// (empty diff, no fetch needed).
export interface CinematicsRange {
  from: string;
  to: string;
}

/// Default frame cap: a full playthrough at ~1.2 s per step stays under a
/// minute while still sampling the whole 24-month window.
export const DEFAULT_CINEMATICS_STEPS = 40;

/// Build the global commit timeline from the per-module activity payload:
/// dedupe by SHA, sort old → new, downsample evenly to `maxSteps` frames
/// (first and last commit always kept).
///
/// Pure: empty/`no_git` activity or `maxSteps < 1` → `[]`; deterministic;
/// never throws; does not mutate its input.
export function buildCommitTimeline(
  activity: CommitActivity,
  maxSteps: number = DEFAULT_CINEMATICS_STEPS,
): CinematicsStep[] {
  if (maxSteps < 1) return [];

  // 1. Dedupe across module bands — first occurrence wins.
  const bySha = new Map<string, CinematicsStep>();
  for (const m of activity.modules) {
    for (const c of m.commits) {
      if (!bySha.has(c.sha)) {
        bySha.set(c.sha, { sha: c.sha, summary: c.summary, secsAgo: c.secs_ago });
      }
    }
  }

  // 2. Old → new: descending secs_ago, SHA tie-break for determinism.
  const all = [...bySha.values()].sort(
    (a, b) => b.secsAgo - a.secsAgo || a.sha.localeCompare(b.sha),
  );

  // 3. Even downsampling. `maxSteps === 1` degenerates to "the newest commit"
  // (the only frame that can represent the whole range's end state).
  const n = all.length;
  if (n <= maxSteps) return all;
  if (maxSteps === 1) return [all[n - 1]];

  const sampled: CinematicsStep[] = [];
  for (let k = 0; k < maxSteps; k++) {
    sampled.push(all[Math.round((k * (n - 1)) / (maxSteps - 1))]);
  }
  return sampled;
}

/// Map player step `k` onto the cumulative `{from, to}` ref pair (see the
/// module header for why cumulative). `k` is clamped into the timeline, so a
/// scrubber that briefly overshoots never produces an invalid range; an empty
/// timeline has no ranges at all → `null`.
export function stepRange(
  timeline: readonly CinematicsStep[],
  k: number,
): CinematicsRange | null {
  if (timeline.length === 0) return null;
  const clamped = Math.max(0, Math.min(k, timeline.length - 1));
  return { from: timeline[0].sha, to: timeline[clamped].sha };
}

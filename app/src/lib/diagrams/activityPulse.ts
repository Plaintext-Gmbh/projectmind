/// Pure planner for the bean-graph *activity pulse* overlay
/// (`bean-graph-live`, V4.2 / #66 "living architecture", concept 1c). The
/// stateful Cytoscape mount and the actual heartbeat animation live in
/// `app/src/components/BeanGraphLive.svelte`; this module holds the part that
/// can be unit-tested without a DOM: joining the graph's nodes against the
/// repo's `commit_activity` and bucketing them by commit freshness, so the
/// component just paints the buckets.
///
/// ## What "pulse" means (honesty rule #61)
///
/// Unlike the *simulated* flow wave (V4.1), the pulse is driven by REAL
/// repository data: `commit_activity` walks HEAD's history (24-month window)
/// and attributes each commit to the modules whose files it touched. Modules
/// with fresh commits "beat" — the fresher, the faster and brighter. It
/// answers "which parts of this system are alive *in the repo* right now"
/// (commit frequency), not "what code is executing" — the toolbar tooltip
/// says exactly that.
///
/// ## Intensity buckets (the weighting)
///
/// A module's intensity is decided by its **freshest** commit alone — a
/// heartbeat shows recency, and a single fresh commit means someone is in
/// that code *now*, regardless of how quiet the module was before:
///
/// - **hot**  — freshest commit strictly less than 7 days old. Fast, bright
///   beat: this is where the repo is being worked on this week.
/// - **warm** — freshest commit strictly less than 30 days old. Slower,
///   dimmer beat: recently touched, cooling off.
/// - **cool** — everything else. No pulse; deliberately NOT a third visual
///   class, so the resting graph stays calm and the beats read as the signal.
///
/// Bucket edges are half-open `[0, limit)`: a commit exactly 7 days old is
/// warm, exactly 30 days old is cool. The limits are injectable (`buckets`
/// param) so tests can pin the boundaries without faking 7-day timestamps.
///
/// ## The join (and its trap)
///
/// The two sides name modules differently:
/// - `BeanNode.module` is `groupId:artifactId` for Maven (or the bare crate
///   name for Cargo) — see `crates/core/src/diagram.rs` `BeanNode`.
/// - `ModuleActivity.module` is the bare `artifactId` / crate name, or a
///   top-level directory as fallback — see `crates/core/src/git.rs`
///   `ModuleActivity`.
///
/// So the join is a **suffix match**: `beanModule.split(':').pop()` must
/// equal the activity module id. For Cargo (no colon) that is the identity,
/// for Maven it strips the groupId. A bean module with no matching activity
/// entry simply gets no pulse — silent, never an error (e.g. the activity
/// walker attributed the files to a top-level dir instead, or the module had
/// no commits in the window).
///
/// Kept dependency-free (no `cytoscape` import) so the plan stays a plain
/// function — same pattern as `beanGraphFlow.ts` / `beanGraphMorph.ts`.

import type { CommitActivity } from '../api';
import type { BeanGraphElements } from './beanGraphElements';

/// Pulse intensity levels. `cool` never appears in a plan (cool modules are
/// simply absent) but is part of the vocabulary so callers can exhaustively
/// switch over it.
export type PulseIntensity = 'hot' | 'warm' | 'cool';

/// Default bucket limits: hot < 7 days, warm < 30 days (half-open).
export const DEFAULT_PULSE_BUCKETS = {
  hotSecs: 7 * 86_400,
  warmSecs: 30 * 86_400,
} as const;

export interface PulseBuckets {
  /// Upper bound (exclusive) in seconds for the `hot` bucket.
  hotSecs: number;
  /// Upper bound (exclusive) in seconds for the `warm` bucket.
  warmSecs: number;
}

/// One intensity bucket of the plan: the node ids to halo and how strongly.
/// Node ids are in graph element order so a repaint is deterministic.
export interface ActivityPulse {
  intensity: PulseIntensity;
  nodeIds: string[];
}

/// The plan the component paints: at most one `hot` and one `warm` bucket
/// (empty buckets are omitted), the matched activity module ids per bucket
/// (echoed for the toolbar / debugging), and whether there is anything to
/// animate at all. `animate` is false when no node joined a fresh-enough
/// module — including the `no_git` / empty-activity cases.
export interface ActivityPulsePlan {
  pulses: ActivityPulse[];
  hotModules: string[];
  warmModules: string[];
  animate: boolean;
}

/// The bean side of the join: `groupId:artifactId` → `artifactId`; a bare
/// crate name (no colon) passes through unchanged.
function beanModuleSuffix(beanModule: string): string {
  const parts = beanModule.split(':');
  return parts[parts.length - 1];
}

/// Bucket one module by the age of its freshest commit. The backend sorts
/// commits newest-first, but we take the minimum defensively so the plan
/// never depends on that ordering. Empty commit lists are `cool`.
function bucketOf(secsAgoList: number[], buckets: PulseBuckets): PulseIntensity {
  if (secsAgoList.length === 0) return 'cool';
  const freshest = Math.min(...secsAgoList);
  if (freshest < buckets.hotSecs) return 'hot';
  if (freshest < buckets.warmSecs) return 'warm';
  return 'cool';
}

/// Build the activity-pulse plan: join every graph node against the repo's
/// commit activity (suffix match, see module header) and group the matched
/// nodes into `hot` / `warm` buckets by their module's freshest commit.
///
/// Pure: empty graph or empty/`no_git` activity → `{ pulses: [], animate:
/// false }`, deterministic, never throws, does not mutate its inputs.
export function planActivityPulse(
  els: BeanGraphElements,
  activity: CommitActivity,
  buckets: PulseBuckets = DEFAULT_PULSE_BUCKETS,
): ActivityPulsePlan {
  // Activity module id → intensity, resolved once per module.
  const intensityOf = new Map<string, PulseIntensity>();
  for (const m of activity.modules) {
    intensityOf.set(
      m.module,
      bucketOf(m.commits.map((c) => c.secs_ago), buckets),
    );
  }

  const hotNodeIds: string[] = [];
  const warmNodeIds: string[] = [];
  const hotModules = new Set<string>();
  const warmModules = new Set<string>();

  for (const n of els.nodes) {
    const suffix = beanModuleSuffix(n.data.module);
    const intensity = intensityOf.get(suffix);
    // No activity entry (join miss) or a cool module → no pulse, silently.
    if (intensity === 'hot') {
      hotNodeIds.push(n.data.id);
      hotModules.add(suffix);
    } else if (intensity === 'warm') {
      warmNodeIds.push(n.data.id);
      warmModules.add(suffix);
    }
  }

  const pulses: ActivityPulse[] = [];
  if (hotNodeIds.length > 0) pulses.push({ intensity: 'hot', nodeIds: hotNodeIds });
  if (warmNodeIds.length > 0) pulses.push({ intensity: 'warm', nodeIds: warmNodeIds });

  return {
    pulses,
    hotModules: [...hotModules].sort(),
    warmModules: [...warmModules].sort(),
    animate: pulses.length > 0,
  };
}

import { describe, expect, it } from 'vitest';
import {
  buildCommitTimeline,
  stepRange,
  DEFAULT_CINEMATICS_STEPS,
} from './beanGraphCinematics';
import type { CinematicsStep } from './beanGraphCinematics';
import type { CommitActivity, ModuleActivity } from '../api';

/// Build a `CommitActivity` stub around per-module commit drops — the
/// timeline builder only reads `modules[].commits[]`, but the full payload
/// shape keeps the type honest against `api.ts`.
function activity(modules: ModuleActivity[]): CommitActivity {
  return {
    root: '/repo',
    now_secs: 1_700_000_000,
    window_secs: 730 * 86_400,
    modules,
    total_commits: modules.reduce((n, m) => n + m.commits.length, 0),
    truncated: false,
    no_git: modules.length === 0,
  };
}

function drop(sha: string, secsAgo: number, summary = `msg ${sha}`) {
  return { secs_ago: secsAgo, sha, summary };
}

/// `n` synthetic commits, oldest first (`c000` oldest … newest last), spread
/// over one module.
function linearActivity(n: number): CommitActivity {
  const commits = [];
  for (let i = 0; i < n; i++) {
    // Backend order is newest-first; the builder must not depend on it, so we
    // hand the drops newest-first here (i = 0 → newest).
    commits.push(drop(`c${String(n - 1 - i).padStart(3, '0')}`, (i + 1) * 100));
  }
  return activity([{ module: 'core', commits }]);
}

describe('buildCommitTimeline', () => {
  it('returns [] for an empty / no_git activity payload', () => {
    expect(buildCommitTimeline(activity([]))).toEqual([]);
  });

  it('returns [] when maxSteps is below 1', () => {
    expect(buildCommitTimeline(linearActivity(5), 0)).toEqual([]);
    expect(buildCommitTimeline(linearActivity(5), -3)).toEqual([]);
  });

  it('handles a single commit', () => {
    const timeline = buildCommitTimeline(
      activity([{ module: 'core', commits: [drop('abc1234', 42, 'feat: x')] }]),
    );
    expect(timeline).toEqual([{ sha: 'abc1234', summary: 'feat: x', secsAgo: 42 }]);
  });

  it('dedupes the same SHA across module bands (first occurrence wins)', () => {
    const timeline = buildCommitTimeline(
      activity([
        { module: 'core', commits: [drop('aaa1111', 200, 'touches both')] },
        { module: 'web', commits: [drop('aaa1111', 200, 'touches both'), drop('bbb2222', 100)] },
      ]),
    );
    expect(timeline.map((s) => s.sha)).toEqual(['aaa1111', 'bbb2222']);
  });

  it('sorts old → new (descending secs_ago) regardless of input order', () => {
    const timeline = buildCommitTimeline(
      activity([
        {
          module: 'core',
          commits: [drop('new0001', 10), drop('old0001', 300), drop('mid0001', 150)],
        },
      ]),
    );
    expect(timeline.map((s) => s.sha)).toEqual(['old0001', 'mid0001', 'new0001']);
    expect(timeline[0].secsAgo).toBeGreaterThan(timeline[2].secsAgo);
  });

  it('keeps every commit when there are exactly maxSteps', () => {
    const timeline = buildCommitTimeline(linearActivity(8), 8);
    expect(timeline).toHaveLength(8);
    expect(timeline[0].sha).toBe('c000');
    expect(timeline[7].sha).toBe('c007');
  });

  it('downsamples > maxSteps evenly, always keeping the oldest and newest', () => {
    const timeline = buildCommitTimeline(linearActivity(100), 10);
    expect(timeline).toHaveLength(10);
    // The reel spans the whole window: first frame = oldest, last = newest.
    expect(timeline[0].sha).toBe('c000');
    expect(timeline[9].sha).toBe('c099');
    // Strictly old → new with no duplicate frames.
    for (let i = 1; i < timeline.length; i++) {
      expect(timeline[i].secsAgo).toBeLessThan(timeline[i - 1].secsAgo);
    }
    expect(new Set(timeline.map((s) => s.sha)).size).toBe(10);
  });

  it('degenerates maxSteps=1 to the newest commit (the range end state)', () => {
    const timeline = buildCommitTimeline(linearActivity(5), 1);
    expect(timeline.map((s) => s.sha)).toEqual(['c004']);
  });

  it('defaults to 40 frames', () => {
    expect(DEFAULT_CINEMATICS_STEPS).toBe(40);
    expect(buildCommitTimeline(linearActivity(500))).toHaveLength(40);
  });

  it('does not mutate the activity payload', () => {
    const a = activity([
      { module: 'core', commits: [drop('new0001', 10), drop('old0001', 300)] },
    ]);
    buildCommitTimeline(a);
    // Input order (newest-first, as the backend ships it) is untouched.
    expect(a.modules[0].commits.map((c) => c.sha)).toEqual(['new0001', 'old0001']);
  });
});

describe('stepRange', () => {
  const timeline: CinematicsStep[] = [
    { sha: 'aaa0000', summary: 'oldest', secsAgo: 300 },
    { sha: 'bbb1111', summary: 'middle', secsAgo: 200 },
    { sha: 'ccc2222', summary: 'newest', secsAgo: 100 },
  ];

  it('is null for an empty timeline', () => {
    expect(stepRange([], 0)).toBeNull();
  });

  it('step 0 is the baseline: from === to (an empty diff by construction)', () => {
    expect(stepRange(timeline, 0)).toEqual({ from: 'aaa0000', to: 'aaa0000' });
  });

  it('is cumulative from the timeline start, not per-commit', () => {
    expect(stepRange(timeline, 1)).toEqual({ from: 'aaa0000', to: 'bbb1111' });
    expect(stepRange(timeline, 2)).toEqual({ from: 'aaa0000', to: 'ccc2222' });
  });

  it('clamps an overshooting step into the timeline', () => {
    expect(stepRange(timeline, 99)).toEqual({ from: 'aaa0000', to: 'ccc2222' });
    expect(stepRange(timeline, -1)).toEqual({ from: 'aaa0000', to: 'aaa0000' });
  });
});

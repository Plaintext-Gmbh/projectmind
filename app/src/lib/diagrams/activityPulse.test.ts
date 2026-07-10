import { describe, expect, it } from 'vitest';
import { DEFAULT_PULSE_BUCKETS, planActivityPulse } from './activityPulse';
import { beanGraphElements } from './beanGraphElements';
import type { BeanGraphData, CommitActivity, ModuleActivity } from '../api';

/// Build elements from a compact payload, reusing the real mapping so the plan
/// is exercised against exactly the `{ nodes, edges }` shape the component
/// feeds it — including the `module` field the join reads.
function els(payload: BeanGraphData) {
  return beanGraphElements(payload);
}

/// A two-module Maven-style graph: `com.acme:web` (2 classes) and
/// `com.acme:core` (1 class). Edges are irrelevant to the pulse.
function twoModules(): BeanGraphData {
  return {
    nodes: [
      { id: 'a.Ctrl', label: 'Ctrl', module: 'com.acme:web', stereotype: 'rest-controller', path: 'Ctrl.java' },
      { id: 'a.View', label: 'View', module: 'com.acme:web', stereotype: 'controller', path: 'View.java' },
      { id: 'a.Repo', label: 'Repo', module: 'com.acme:core', stereotype: 'repository', path: 'Repo.java' },
    ],
    edges: [],
  };
}

/// Wrap per-module drops into the full `CommitActivity` envelope. Only
/// `modules` matters to the planner; the rest is realistic filler.
function activity(modules: ModuleActivity[]): CommitActivity {
  return {
    root: '/repo',
    now_secs: 1_700_000_000,
    window_secs: 63_072_000,
    modules,
    total_commits: modules.reduce((n, m) => n + m.commits.length, 0),
    truncated: false,
    no_git: false,
  };
}

function drop(secs_ago: number): { secs_ago: number; sha: string; summary: string } {
  return { secs_ago, sha: 'abc1234', summary: 'a commit' };
}

const DAY = 86_400;

describe('planActivityPulse — join (suffix match)', () => {
  it('joins Maven groupId:artifactId nodes against bare artifactId activity', () => {
    const plan = planActivityPulse(
      els(twoModules()),
      activity([{ module: 'web', commits: [drop(1 * DAY)] }]),
    );
    // Both `com.acme:web` classes pulse; `com.acme:core` has no activity entry.
    expect(plan.pulses).toEqual([{ intensity: 'hot', nodeIds: ['a.Ctrl', 'a.View'] }]);
    expect(plan.hotModules).toEqual(['web']);
    expect(plan.animate).toBe(true);
  });

  it('joins colon-free (Cargo crate) module ids by identity', () => {
    const plan = planActivityPulse(
      els({
        nodes: [{ id: 'core::Engine', label: 'Engine', module: 'core', stereotype: null, path: 'engine.rs' }],
        edges: [],
      }),
      activity([{ module: 'core', commits: [drop(2 * DAY)] }]),
    );
    expect(plan.pulses).toEqual([{ intensity: 'hot', nodeIds: ['core::Engine'] }]);
  });

  it('a join miss yields no pulse and no error (silent fallback)', () => {
    // Activity attributed the commits to a top-level dir the graph never uses.
    const plan = planActivityPulse(
      els(twoModules()),
      activity([{ module: 'docs', commits: [drop(1 * DAY)] }]),
    );
    expect(plan.pulses).toEqual([]);
    expect(plan.animate).toBe(false);
  });

  it('does not join on the groupId half of the coordinate', () => {
    // `com.acme` is the groupId, not the artifactId — must NOT match.
    const plan = planActivityPulse(
      els(twoModules()),
      activity([{ module: 'com.acme', commits: [drop(1 * DAY)] }]),
    );
    expect(plan.pulses).toEqual([]);
  });
});

describe('planActivityPulse — bucket boundaries', () => {
  const buckets = { hotSecs: 100, warmSecs: 200 };
  const oneNode = () =>
    els({
      nodes: [{ id: 'N', label: 'N', module: 'g:m', stereotype: null, path: 'N.java' }],
      edges: [],
    });

  it('strictly below hotSecs is hot', () => {
    const plan = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(99)] }]), buckets);
    expect(plan.pulses).toEqual([{ intensity: 'hot', nodeIds: ['N'] }]);
  });

  it('exactly hotSecs falls into warm (half-open buckets)', () => {
    const plan = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(100)] }]), buckets);
    expect(plan.pulses).toEqual([{ intensity: 'warm', nodeIds: ['N'] }]);
    expect(plan.warmModules).toEqual(['m']);
  });

  it('exactly warmSecs is cool — no pulse', () => {
    const plan = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(200)] }]), buckets);
    expect(plan.pulses).toEqual([]);
    expect(plan.animate).toBe(false);
  });

  it('the freshest commit decides — one fresh commit outweighs old history', () => {
    const plan = planActivityPulse(
      oneNode(),
      activity([{ module: 'm', commits: [drop(9_999), drop(50), drop(150)] }]),
      buckets,
    );
    expect(plan.pulses).toEqual([{ intensity: 'hot', nodeIds: ['N'] }]);
  });

  it('defaults are 7 / 30 days', () => {
    expect(DEFAULT_PULSE_BUCKETS.hotSecs).toBe(7 * DAY);
    expect(DEFAULT_PULSE_BUCKETS.warmSecs).toBe(30 * DAY);
    const hot = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(6 * DAY)] }]));
    const warm = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(8 * DAY)] }]));
    const cool = planActivityPulse(oneNode(), activity([{ module: 'm', commits: [drop(31 * DAY)] }]));
    expect(hot.pulses[0]?.intensity).toBe('hot');
    expect(warm.pulses[0]?.intensity).toBe('warm');
    expect(cool.pulses).toEqual([]);
  });
});

describe('planActivityPulse — bucket grouping', () => {
  it('splits modules into hot and warm buckets in one plan', () => {
    const plan = planActivityPulse(
      els(twoModules()),
      activity([
        { module: 'web', commits: [drop(1 * DAY)] },
        { module: 'core', commits: [drop(10 * DAY)] },
      ]),
    );
    expect(plan.pulses).toEqual([
      { intensity: 'hot', nodeIds: ['a.Ctrl', 'a.View'] },
      { intensity: 'warm', nodeIds: ['a.Repo'] },
    ]);
    expect(plan.hotModules).toEqual(['web']);
    expect(plan.warmModules).toEqual(['core']);
  });

  it('omits empty buckets instead of emitting them empty', () => {
    const plan = planActivityPulse(
      els(twoModules()),
      activity([{ module: 'core', commits: [drop(10 * DAY)] }]),
    );
    expect(plan.pulses).toEqual([{ intensity: 'warm', nodeIds: ['a.Repo'] }]);
    expect(plan.hotModules).toEqual([]);
  });
});

describe('planActivityPulse — degenerate inputs', () => {
  it('empty activity (no modules) plans no animation and does not throw', () => {
    const plan = planActivityPulse(els(twoModules()), activity([]));
    expect(plan).toEqual({ pulses: [], hotModules: [], warmModules: [], animate: false });
  });

  it('a no_git activity payload is just an empty plan', () => {
    const noGit = { ...activity([]), no_git: true };
    const plan = planActivityPulse(els(twoModules()), noGit);
    expect(plan.animate).toBe(false);
  });

  it('an empty graph plans no animation even with hot activity', () => {
    const plan = planActivityPulse(
      els({ nodes: [], edges: [] }),
      activity([{ module: 'web', commits: [drop(1 * DAY)] }]),
    );
    expect(plan).toEqual({ pulses: [], hotModules: [], warmModules: [], animate: false });
  });

  it('a module with an empty commit list is cool', () => {
    const plan = planActivityPulse(els(twoModules()), activity([{ module: 'web', commits: [] }]));
    expect(plan.pulses).toEqual([]);
  });

  it('does not mutate the inputs it is handed', () => {
    const elements = els(twoModules());
    const act = activity([{ module: 'web', commits: [drop(9_999), drop(50)] }]);
    const commitsBefore = [...act.modules[0].commits];
    planActivityPulse(elements, act);
    expect(elements.nodes.length).toBe(3);
    expect(act.modules[0].commits).toEqual(commitsBefore);
  });
});

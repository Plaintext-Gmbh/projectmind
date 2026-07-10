import { describe, expect, it } from 'vitest';
import { planBeanGraphFlow } from './beanGraphFlow';
import { beanGraphElements } from './beanGraphElements';
import type { BeanGraphData } from '../api';

/// Build elements from a compact payload, reusing the real mapping so the plan
/// is exercised against exactly the `{ nodes, edges }` shape the component
/// feeds it (stereoClass derivation, edge ids `e0`/`e1`/…, dropped danglers).
function els(payload: BeanGraphData) {
  return beanGraphElements(payload);
}

/// A textbook Controller → Service → Repository chain, plus a second service the
/// controller also calls, so a wave can have more than one node.
///
///   Ctrl ──► Svc ──► Repo
///        └──► Svc2
function layered(): BeanGraphData {
  return {
    nodes: [
      { id: 'Ctrl', label: 'Ctrl', module: 'g:m', stereotype: 'rest-controller', path: 'Ctrl.java' },
      { id: 'Svc', label: 'Svc', module: 'g:m', stereotype: 'service', path: 'Svc.java' },
      { id: 'Svc2', label: 'Svc2', module: 'g:m', stereotype: 'service', path: 'Svc2.java' },
      { id: 'Repo', label: 'Repo', module: 'g:m', stereotype: 'repository', path: 'Repo.java' },
    ],
    edges: [
      { from: 'Ctrl', to: 'Svc', kind: 'injects' },
      { from: 'Ctrl', to: 'Svc2', kind: 'injects' },
      { from: 'Svc', to: 'Repo', kind: 'calls' },
    ],
  };
}

describe('planBeanGraphFlow — entry detection', () => {
  it('seeds the flow from controller stereotypes', () => {
    const plan = planBeanGraphFlow(els(layered()));
    expect(plan.animate).toBe(true);
    expect(plan.entryNodeIds).toEqual(['Ctrl']);
    // Wave 0 = the controller frontier, no carrying edges.
    expect(plan.waves[0]).toEqual({ nodeIds: ['Ctrl'], edgeIds: [] });
  });

  it('groups nodes into waves by BFS depth, controller → service → repo', () => {
    const plan = planBeanGraphFlow(els(layered()));
    expect(plan.waves.map((w) => w.nodeIds)).toEqual([
      ['Ctrl'],
      ['Svc', 'Svc2'],
      ['Repo'],
    ]);
  });

  it('picks up plain (non-rest) controllers too', () => {
    const plan = planBeanGraphFlow(
      els({
        nodes: [
          { id: 'C', label: 'C', module: 'g:m', stereotype: 'controller', path: 'C.java' },
          { id: 'S', label: 'S', module: 'g:m', stereotype: 'service', path: 'S.java' },
        ],
        edges: [{ from: 'C', to: 'S', kind: 'injects' }],
      }),
    );
    expect(plan.entryNodeIds).toEqual(['C']);
  });
});

describe('planBeanGraphFlow — edge → wave assignment', () => {
  it('assigns each edge to the wave in which its target is first reached', () => {
    const plan = planBeanGraphFlow(els(layered()));
    const elements = els(layered());
    // Resolve ids for readability: which edge is Ctrl→Svc etc.
    const idOf = (from: string, to: string) =>
      elements.edges.find((e) => e.data.source === from && e.data.target === to)!.data.id;

    // Wave 0 carries no edge (the entry frontier).
    expect(plan.waves[0].edgeIds).toEqual([]);
    // Wave 1 carries the two controller→service edges (Svc, Svc2 first reached).
    expect(plan.waves[1].edgeIds.sort()).toEqual([idOf('Ctrl', 'Svc'), idOf('Ctrl', 'Svc2')].sort());
    // Wave 2 carries the service→repo edge (Repo first reached).
    expect(plan.waves[2].edgeIds).toEqual([idOf('Svc', 'Repo')]);
  });

  it('emits each reachable edge in at most one wave', () => {
    const plan = planBeanGraphFlow(els(layered()));
    const all = plan.waves.flatMap((w) => w.edgeIds);
    expect(new Set(all).size).toBe(all.length);
  });
});

describe('planBeanGraphFlow — fallbacks', () => {
  it('falls back to fan-in-0 sources when there is no controller', () => {
    // No controller stereotype anywhere; Root has no incoming edge → the source.
    const plan = planBeanGraphFlow(
      els({
        nodes: [
          { id: 'Root', label: 'Root', module: 'g:m', stereotype: 'service', path: 'Root.java' },
          { id: 'Leaf', label: 'Leaf', module: 'g:m', stereotype: 'repository', path: 'Leaf.java' },
        ],
        edges: [{ from: 'Root', to: 'Leaf', kind: 'calls' }],
      }),
    );
    expect(plan.entryNodeIds).toEqual(['Root']);
    expect(plan.waves.map((w) => w.nodeIds)).toEqual([['Root'], ['Leaf']]);
  });

  it('falls back to the sorted-first node when every node is in a cycle', () => {
    // A ↔ B ring: neither is a controller, both have an incoming edge, so the
    // only tiebreaker left is sorted-first id.
    const plan = planBeanGraphFlow(
      els({
        nodes: [
          { id: 'B', label: 'B', module: 'g:m', stereotype: 'service', path: 'B.java' },
          { id: 'A', label: 'A', module: 'g:m', stereotype: 'service', path: 'A.java' },
        ],
        edges: [
          { from: 'A', to: 'B', kind: 'calls' },
          { from: 'B', to: 'A', kind: 'calls' },
        ],
      }),
    );
    expect(plan.entryNodeIds).toEqual(['A']);
  });
});

describe('planBeanGraphFlow — cycles & reachability', () => {
  it('terminates on a cycle without revisiting nodes', () => {
    // Ctrl → A → B → A (back-edge). BFS must not loop.
    const plan = planBeanGraphFlow(
      els({
        nodes: [
          { id: 'Ctrl', label: 'Ctrl', module: 'g:m', stereotype: 'controller', path: 'Ctrl.java' },
          { id: 'A', label: 'A', module: 'g:m', stereotype: 'service', path: 'A.java' },
          { id: 'B', label: 'B', module: 'g:m', stereotype: 'service', path: 'B.java' },
        ],
        edges: [
          { from: 'Ctrl', to: 'A', kind: 'injects' },
          { from: 'A', to: 'B', kind: 'calls' },
          { from: 'B', to: 'A', kind: 'calls' }, // back-edge closes the cycle
        ],
      }),
    );
    // Every node visited exactly once, across the whole plan.
    const visited = plan.waves.flatMap((w) => w.nodeIds);
    expect(visited.sort()).toEqual(['A', 'B', 'Ctrl']);
    expect(new Set(visited).size).toBe(visited.length);
    // The back-edge B→A does not appear (A already reached) — each node's
    // carrying edge is emitted once.
    const edgeCount = plan.waves.flatMap((w) => w.edgeIds).length;
    expect(edgeCount).toBe(2); // Ctrl→A and A→B only
  });

  it('leaves unreachable islands out of the plan', () => {
    // Ctrl → Svc is one component; Island1 → Island2 is disconnected and has no
    // controller, so it is never reached from the entry frontier.
    const plan = planBeanGraphFlow(
      els({
        nodes: [
          { id: 'Ctrl', label: 'Ctrl', module: 'g:m', stereotype: 'controller', path: 'Ctrl.java' },
          { id: 'Svc', label: 'Svc', module: 'g:m', stereotype: 'service', path: 'Svc.java' },
          { id: 'Island1', label: 'I1', module: 'g:m', stereotype: 'service', path: 'I1.java' },
          { id: 'Island2', label: 'I2', module: 'g:m', stereotype: 'repository', path: 'I2.java' },
        ],
        edges: [
          { from: 'Ctrl', to: 'Svc', kind: 'injects' },
          { from: 'Island1', to: 'Island2', kind: 'calls' },
        ],
      }),
    );
    const reached = plan.waves.flatMap((w) => w.nodeIds);
    expect(reached.sort()).toEqual(['Ctrl', 'Svc']);
    expect(reached).not.toContain('Island1');
    expect(reached).not.toContain('Island2');
  });
});

describe('planBeanGraphFlow — degenerate graphs', () => {
  it('an empty graph plans no animation', () => {
    const plan = planBeanGraphFlow(els({ nodes: [], edges: [] }));
    expect(plan.animate).toBe(false);
    expect(plan.waves).toEqual([]);
    expect(plan.entryNodeIds).toEqual([]);
  });

  it('a single isolated node is one wave and animates', () => {
    const plan = planBeanGraphFlow(
      els({
        nodes: [{ id: 'Solo', label: 'Solo', module: 'g:m', stereotype: null, path: 'Solo.java' }],
        edges: [],
      }),
    );
    expect(plan.animate).toBe(true);
    expect(plan.waves).toEqual([{ nodeIds: ['Solo'], edgeIds: [] }]);
  });

  it('does not mutate the elements it is handed', () => {
    const elements = els(layered());
    const nodesBefore = elements.nodes.length;
    const edgesBefore = elements.edges.length;
    planBeanGraphFlow(elements);
    expect(elements.nodes.length).toBe(nodesBefore);
    expect(elements.edges.length).toBe(edgesBefore);
  });
});

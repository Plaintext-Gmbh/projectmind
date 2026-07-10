import { describe, expect, it } from 'vitest';
import {
  resolveOverlayMode,
  planBeanGraphMorph,
  BEAN_GRAPH_OVERLAY_MODES,
} from './beanGraphMorph';
import type { BeanGraphDiff } from './beanGraphDiff';

/// Build a diff classification stub — the morph planner only reads the id sets
/// and `hasDiff`, so we don't need the full element/change plumbing here (that
/// join is already covered by beanGraphDiff.test.ts).
function diff(nodes: string[], edges: string[]): BeanGraphDiff {
  return {
    changedNodeIds: new Set(nodes),
    changedEdgeIds: new Set(edges),
    hasDiff: nodes.length > 0,
  };
}

describe('resolveOverlayMode', () => {
  it('is off when there is no ref, regardless of the morph toggle', () => {
    expect(resolveOverlayMode(false, false)).toBe('off');
    expect(resolveOverlayMode(false, true)).toBe('off');
  });

  it('is diff when a ref is set and morph is not requested', () => {
    expect(resolveOverlayMode(true, false)).toBe('diff');
  });

  it('is morph when a ref is set and morph is requested', () => {
    expect(resolveOverlayMode(true, true)).toBe('morph');
  });

  it('exposes exactly the three overlay states', () => {
    expect([...BEAN_GRAPH_OVERLAY_MODES]).toEqual(['off', 'diff', 'morph']);
  });
});

describe('planBeanGraphMorph', () => {
  const allNodes = ['a.A', 'a.B', 'b.C', 'x.X'];
  const allEdges = ['e0', 'e1', 'e2'];

  it('enters the changed ids and recedes the rest', () => {
    const plan = planBeanGraphMorph(diff(['a.A'], ['e0', 'e1']), allNodes, allEdges);
    expect([...plan.enterNodeIds].sort()).toEqual(['a.A']);
    expect([...plan.enterEdgeIds].sort()).toEqual(['e0', 'e1']);
    // Everything not entering recedes.
    expect([...plan.recedeNodeIds].sort()).toEqual(['a.B', 'b.C', 'x.X']);
    expect([...plan.recedeEdgeIds].sort()).toEqual(['e2']);
    expect(plan.animate).toBe(true);
  });

  it('enter and recede sets are disjoint and cover every current id', () => {
    const plan = planBeanGraphMorph(diff(['a.B', 'b.C'], ['e2']), allNodes, allEdges);
    const nodesCovered = [...plan.enterNodeIds, ...plan.recedeNodeIds].sort();
    expect(nodesCovered).toEqual([...allNodes].sort());
    // No id is both entering and receding.
    for (const id of plan.enterNodeIds) expect(plan.recedeNodeIds.has(id)).toBe(false);
    for (const id of plan.enterEdgeIds) expect(plan.recedeEdgeIds.has(id)).toBe(false);
  });

  it('an empty diff plans no animation and recedes nothing', () => {
    const plan = planBeanGraphMorph(diff([], []), allNodes, allEdges);
    expect(plan.animate).toBe(false);
    expect(plan.enterNodeIds.size).toBe(0);
    expect(plan.enterEdgeIds.size).toBe(0);
    // Nothing dims when there is no signal to contrast against.
    expect(plan.recedeNodeIds.size).toBe(0);
    expect(plan.recedeEdgeIds.size).toBe(0);
  });

  it('does not mutate the diff id sets it is handed', () => {
    const d = diff(['a.A'], ['e0']);
    planBeanGraphMorph(d, allNodes, allEdges);
    // The plan copies; the source classification is untouched.
    expect([...d.changedNodeIds]).toEqual(['a.A']);
    expect([...d.changedEdgeIds]).toEqual(['e0']);
  });

  it('handles a graph with no elements without throwing', () => {
    const plan = planBeanGraphMorph(diff([], []), [], []);
    expect(plan.animate).toBe(false);
    expect(plan.enterNodeIds.size).toBe(0);
    expect(plan.recedeNodeIds.size).toBe(0);
  });

  it('never plans a "removed" role — only enter and recede exist', () => {
    // Current-state browser: a file deleted since the ref is simply absent from
    // the current graph, so it can never appear in any plan set. We assert the
    // plan surface has exactly the two roles and no third bucket.
    const plan = planBeanGraphMorph(diff(['a.A'], ['e0']), allNodes, allEdges);
    expect(Object.keys(plan).sort()).toEqual(
      ['animate', 'enterEdgeIds', 'enterNodeIds', 'recedeEdgeIds', 'recedeNodeIds'].sort(),
    );
  });
});

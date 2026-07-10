import { describe, expect, it } from 'vitest';
import { classifyBeanGraphDiff } from './beanGraphDiff';
import { beanGraphElements } from './beanGraphElements';
import type { BeanGraphData } from '../api';
import type { ChangedFile } from '../api';

/// A small three-node graph: A → B (same module), A → C (cross module).
/// Paths mirror the repo-relative, forward-slashed shape the backend now emits.
function sampleElements() {
  const payload: BeanGraphData = {
    nodes: [
      { id: 'a.A', label: 'A', module: 'g:m1', stereotype: 'service', path: 'm1/a/A.java' },
      { id: 'a.B', label: 'B', module: 'g:m1', stereotype: 'controller', path: 'm1/a/B.java' },
      { id: 'b.C', label: 'C', module: 'g:m2', stereotype: null, path: 'm2/b/C.java' },
      // A node with no resolvable file — must never count as changed.
      { id: 'x.X', label: 'X', module: 'g:m1', stereotype: null, path: null },
    ],
    edges: [
      { from: 'a.A', to: 'a.B', kind: 'injects' },
      { from: 'a.A', to: 'b.C', kind: 'uses' },
      { from: 'a.B', to: 'x.X', kind: 'calls' },
    ],
  };
  return beanGraphElements(payload);
}

function changed(...paths: string[]): ChangedFile[] {
  return paths.map((path) => ({ path, status: 'modified' }));
}

describe('classifyBeanGraphDiff', () => {
  it('marks nodes whose path is in the change set', () => {
    const diff = classifyBeanGraphDiff(sampleElements(), changed('m1/a/A.java'));
    expect([...diff.changedNodeIds]).toEqual(['a.A']);
    expect(diff.hasDiff).toBe(true);
  });

  it('marks an edge changed when either endpoint changed', () => {
    // Only B changed → edge A→B (target) and edge B→X (source) both light up,
    // but edge A→C (neither endpoint) stays unchanged.
    const diff = classifyBeanGraphDiff(sampleElements(), changed('m1/a/B.java'));
    expect([...diff.changedNodeIds]).toEqual(['a.B']);
    // beanGraphElements ids edges by index: e0=A→B, e1=A→C, e2=B→X.
    expect(diff.changedEdgeIds.has('e0')).toBe(true); // A→B, target changed
    expect(diff.changedEdgeIds.has('e2')).toBe(true); // B→X, source changed
    expect(diff.changedEdgeIds.has('e1')).toBe(false); // A→C, neither changed
  });

  it('an empty change set yields no changes and hasDiff=false', () => {
    const diff = classifyBeanGraphDiff(sampleElements(), []);
    expect(diff.changedNodeIds.size).toBe(0);
    expect(diff.changedEdgeIds.size).toBe(0);
    expect(diff.hasDiff).toBe(false);
  });

  it('a change set that matches nothing yields no changes (no throw)', () => {
    const diff = classifyBeanGraphDiff(sampleElements(), changed('unrelated/file.txt'));
    expect(diff.hasDiff).toBe(false);
    expect(diff.changedNodeIds.size).toBe(0);
    expect(diff.changedEdgeIds.size).toBe(0);
  });

  it('never marks a null-path node changed, even if some other file changed', () => {
    // Change B; the null-path node x.X must stay unchanged, but the edge B→X
    // still lights up because its *source* (B) changed.
    const diff = classifyBeanGraphDiff(sampleElements(), changed('m1/a/B.java'));
    expect(diff.changedNodeIds.has('x.X')).toBe(false);
    expect(diff.changedEdgeIds.has('e2')).toBe(true);
  });

  it('normalises backslash and ./-prefixed diff paths before matching', () => {
    const diff = classifyBeanGraphDiff(
      sampleElements(),
      changed('m1\\a\\A.java', './m2/b/C.java'),
    );
    expect([...diff.changedNodeIds].sort()).toEqual(['a.A', 'b.C']);
  });

  it('marks all touched nodes and their incident edges together', () => {
    const diff = classifyBeanGraphDiff(
      sampleElements(),
      changed('m1/a/A.java', 'm2/b/C.java'),
    );
    expect([...diff.changedNodeIds].sort()).toEqual(['a.A', 'b.C']);
    // e0 (A→B) and e1 (A→C) touch A; e1 also touches C. e2 (B→X) touches neither.
    expect(diff.changedEdgeIds.has('e0')).toBe(true);
    expect(diff.changedEdgeIds.has('e1')).toBe(true);
    expect(diff.changedEdgeIds.has('e2')).toBe(false);
  });
});

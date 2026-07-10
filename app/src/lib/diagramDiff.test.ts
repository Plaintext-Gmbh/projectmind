import { describe, expect, it } from 'vitest';
import type { ChangedFile } from './api';
import {
  deriveChangedNodes,
  filterNodesForMode,
  changedPulseIds,
  changedFileCount,
  diffPriority,
  renderFolderDiff,
  DIAGRAM_DIFF_MODES,
  type FolderMap,
  type FolderMapNode,
} from './diagramDiff';

/// Small folder map:
///   .            (root)
///   ├─ src       (folder)
///   │  ├─ a.ts   (file)
///   │  └─ b.ts   (file)
///   └─ docs      (folder)
///      └─ r.md   (file)
function sampleMap(): FolderMap {
  const nodes: FolderMapNode[] = [
    { id: '.', parent: null, label: 'repo', path: '/repo', kind: 'root', depth: 0, weight: 1 },
    { id: 'src', parent: '.', label: 'src', path: '/repo/src', kind: 'folder', depth: 1, weight: 2 },
    { id: 'src/a.ts', parent: 'src', label: 'a.ts', path: '/repo/src/a.ts', kind: 'file', depth: 2, weight: 1 },
    { id: 'src/b.ts', parent: 'src', label: 'b.ts', path: '/repo/src/b.ts', kind: 'file', depth: 2, weight: 1 },
    { id: 'docs', parent: '.', label: 'docs', path: '/repo/docs', kind: 'folder', depth: 1, weight: 1 },
    { id: 'docs/r.md', parent: 'docs', label: 'r.md', path: '/repo/docs/r.md', kind: 'file', depth: 2, weight: 1 },
  ];
  return { root: '/repo', max_depth: 2, truncated: false, nodes };
}

function change(path: string, status: ChangedFile['status']): ChangedFile {
  return { path, status };
}

describe('deriveChangedNodes (#125)', () => {
  it('tags changed leaf files with their status', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    expect(s.get('src/a.ts')).toBe('modified');
    expect(s.has('src/b.ts')).toBe(false);
  });

  it('propagates the most-prominent status up to ancestor folders', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [
      change('src/a.ts', 'modified'),
      change('src/b.ts', 'deleted'),
    ]);
    // deleted outranks modified for the folder + root aggregate.
    expect(s.get('src')).toBe('deleted');
    expect(s.get('.')).toBe('deleted');
    // leaves keep their own status.
    expect(s.get('src/a.ts')).toBe('modified');
    expect(s.get('src/b.ts')).toBe('deleted');
    // untouched subtree stays absent.
    expect(s.has('docs')).toBe(false);
    expect(s.has('docs/r.md')).toBe(false);
  });

  it('normalises backslash + ./ prefixed diff paths', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('.\\src\\a.ts', 'added')]);
    expect(s.get('src/a.ts')).toBe('added');
  });

  it('empty change set → empty map, never throws (empty-diff safety)', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, []);
    expect(s.size).toBe(0);
  });

  it('empty node list → empty map', () => {
    const s = deriveChangedNodes(
      { root: '/x', max_depth: 0, truncated: false, nodes: [] },
      [change('a.ts', 'added')],
    );
    expect(s.size).toBe(0);
  });
});

describe('diffPriority (#125)', () => {
  it('ranks deleted > added > renamed > modified > type_change > other', () => {
    expect(diffPriority('deleted')).toBeGreaterThan(diffPriority('added'));
    expect(diffPriority('added')).toBeGreaterThan(diffPriority('renamed'));
    expect(diffPriority('renamed')).toBeGreaterThan(diffPriority('modified'));
    expect(diffPriority('modified')).toBeGreaterThan(diffPriority('type_change'));
    expect(diffPriority('type_change')).toBeGreaterThan(diffPriority('other'));
  });
});

describe('filterNodesForMode (#125)', () => {
  it('before + after keep every node', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    expect(filterNodesForMode(map, s, 'before')).toHaveLength(map.nodes.length);
    expect(filterNodesForMode(map, s, 'after')).toHaveLength(map.nodes.length);
  });

  it('changed-only keeps changed nodes plus their ancestors and the root', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    const ids = filterNodesForMode(map, s, 'changed-only').map((n) => n.id).sort();
    // . (root) + src (ancestor) + src/a.ts (changed). docs subtree dropped.
    expect(ids).toEqual(['.', 'src', 'src/a.ts']);
  });

  it('changed-only with no changes collapses to just the root (readable, no error)', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, []);
    const kept = filterNodesForMode(map, s, 'changed-only');
    expect(kept).toHaveLength(1);
    expect(kept[0].id).toBe('.');
  });

  it('preserves original node order', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('docs/r.md', 'added')]);
    const ids = filterNodesForMode(map, s, 'changed-only').map((n) => n.id);
    // '.' comes before 'docs' before 'docs/r.md' in the source order.
    expect(ids).toEqual(['.', 'docs', 'docs/r.md']);
  });
});

describe('changedPulseIds (#125)', () => {
  it('returns only changed leaf files, not folder aggregates', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [
      change('src/a.ts', 'modified'),
      change('src/b.ts', 'added'),
    ]);
    const visible = new Set(map.nodes.map((n) => n.id));
    const ids = changedPulseIds(map, s, visible).sort();
    expect(ids).toEqual(['src/a.ts', 'src/b.ts']);
  });

  it('excludes leaves filtered away by changed-only', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    const visible = new Set(filterNodesForMode(map, s, 'changed-only').map((n) => n.id));
    // src/b.ts is unchanged and not visible; only the changed leaf pulses.
    expect(changedPulseIds(map, s, visible)).toEqual(['src/a.ts']);
  });

  it('empty diff → no pulses', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, []);
    const visible = new Set(map.nodes.map((n) => n.id));
    expect(changedPulseIds(map, s, visible)).toEqual([]);
  });
});

describe('changedFileCount (#125)', () => {
  it('counts changed leaves only, not folder aggregates', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [
      change('src/a.ts', 'modified'),
      change('src/b.ts', 'deleted'),
    ]);
    expect(changedFileCount(map, s)).toBe(2);
  });

  it('is 0 for an empty diff', () => {
    const map = sampleMap();
    expect(changedFileCount(map, deriveChangedNodes(map, []))).toBe(0);
  });
});

describe('renderFolderDiff (#125)', () => {
  it('produces a valid SVG for every mode without throwing', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    for (const mode of DIAGRAM_DIFF_MODES) {
      const svg = renderFolderDiff(map, s, { mode });
      expect(svg.startsWith('<svg')).toBe(true);
      expect(svg).toContain('</svg>');
    }
  });

  it('before mode applies no change tint (no changed/faded node classes)', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    const svg = renderFolderDiff(map, s, { mode: 'before' });
    // No node <g> carries the changed/faded classes (the CSS <style> block
    // still names them, so we match the class attribute specifically).
    expect(svg).not.toMatch(/class="node [^"]*changed/);
    expect(svg).not.toMatch(/class="node [^"]*faded/);
  });

  it('after mode tints changed nodes and fades unchanged ones', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'modified')]);
    const svg = renderFolderDiff(map, s, { mode: 'after' });
    expect(svg).toMatch(/class="node [^"]*changed/);
    expect(svg).toMatch(/class="node [^"]*faded/);
  });

  it('marks pulse leaves with the pulse class', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, [change('src/a.ts', 'added')]);
    const svg = renderFolderDiff(map, s, { mode: 'after', pulseIds: new Set(['src/a.ts']) });
    expect(svg).toMatch(/class="node [^"]*pulse/);
    // Without a pulse set, no node <g> carries the pulse class.
    const noPulse = renderFolderDiff(map, s, { mode: 'after' });
    expect(noPulse).not.toMatch(/class="node [^"]*pulse/);
  });

  it('renders an empty-diff after view without error', () => {
    const map = sampleMap();
    const s = deriveChangedNodes(map, []);
    const svg = renderFolderDiff(map, s, { mode: 'after' });
    expect(svg.startsWith('<svg')).toBe(true);
  });

  it('escapes node ids to keep the SVG well-formed', () => {
    const map = sampleMap();
    map.nodes.push({
      id: 'src/<x>&"q".ts',
      parent: 'src',
      label: '<x>&"q".ts',
      path: '/repo/src/x.ts',
      kind: 'file',
      depth: 2,
      weight: 1,
    });
    const svg = renderFolderDiff(map, new Map(), { mode: 'before' });
    expect(svg).not.toContain('<x>&"q"');
    expect(svg).toContain('&lt;x&gt;');
  });
});

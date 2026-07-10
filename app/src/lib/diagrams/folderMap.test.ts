import { describe, expect, it } from 'vitest';
import {
  groupByParent,
  nodeRadius,
  renderFolderHierarchy,
  renderFolderMap,
  renderFolderSolar,
  renderFolderTopDown,
  type FolderMap,
  type FolderMapNode,
} from './folderMap';

function node(
  id: string,
  parent: string | null,
  kind: FolderMapNode['kind'],
  depth: number,
  weight = 1,
  label = id,
): FolderMapNode {
  return { id, parent, label, path: id, kind, depth, weight };
}

/// Tiny tree:  .  →  src(folder) → a.ts, b.ts   and  test(folder) → t.ts
function fixture(): FolderMap {
  return {
    root: '.',
    max_depth: 2,
    truncated: false,
    nodes: [
      node('.', null, 'root', 0, 3, 'repo'),
      node('src', '.', 'folder', 1, 2),
      node('test', '.', 'folder', 1, 1),
      node('src/a.ts', 'src', 'file', 2, 1, 'a.ts'),
      node('src/b.ts', 'src', 'file', 2, 1, 'b.ts'),
      node('test/t.ts', 'test', 'file', 2, 1, 't.ts'),
    ],
  };
}

describe('groupByParent', () => {
  it('buckets children by parent and sorts folders before files', () => {
    const g = groupByParent(fixture().nodes);
    expect(g.get('.')!.map((n) => n.id)).toEqual(['src', 'test']);
    // src's children are both files, sorted by label.
    expect(g.get('src')!.map((n) => n.id)).toEqual(['src/a.ts', 'src/b.ts']);
    // Root node itself has no parent → never a key.
    expect(g.has('')).toBe(false);
  });
});

describe('nodeRadius', () => {
  it('files are small and capped at 13; folders/roots grow with weight', () => {
    expect(nodeRadius(node('f', '.', 'file', 2, 1))).toBeLessThanOrEqual(13);
    expect(nodeRadius(node('f', '.', 'file', 2, 9999))).toBe(13); // clamp
    const root = nodeRadius(node('.', null, 'root', 0, 4));
    const folder = nodeRadius(node('d', '.', 'folder', 1, 1));
    expect(root).toBeGreaterThan(folder);
  });
});

describe('renderFolderMap dispatch', () => {
  it('routes each layout to its renderer (all produce an <svg>)', () => {
    const m = fixture();
    for (const layout of ['solar', 'hierarchy', 'td'] as const) {
      const svg = renderFolderMap(m, layout);
      expect(svg.startsWith('<svg')).toBe(true);
      // One <g class="node …"> per node.
      expect((svg.match(/class="node /g) ?? []).length).toBe(m.nodes.length);
    }
  });

  it('solar layout places the root at the canvas centre (700,450)', () => {
    const svg = renderFolderSolar(fixture());
    // root group carries data-path="." and a translate to the centre.
    expect(svg).toContain('data-path="." data-kind="root" transform="translate(700 450)"');
  });

  it('hierarchy + top-down layouts emit edges for every non-root node', () => {
    const m = fixture();
    const edgesInHierarchy = (renderFolderHierarchy(m).match(/class="edge"/g) ?? []).length;
    const edgesInTd = (renderFolderTopDown(m).match(/class="edge"/g) ?? []).length;
    // 5 non-root nodes → 5 parent→child edges.
    expect(edgesInHierarchy).toBe(5);
    expect(edgesInTd).toBe(5);
  });
});

describe('fillFor pass-through', () => {
  it('applies an inline fill + mixed stroke when the resolver returns a colour', () => {
    const svg = renderFolderSolar(fixture(), (id) =>
      id === 'src/a.ts' ? 'hsl(10, 50%, 50%)' : null,
    );
    expect(svg).toContain('style="fill:hsl(10, 50%, 50%);stroke:color-mix');
    // Only the one tinted node carries an inline style.
    expect((svg.match(/style="fill:/g) ?? []).length).toBe(1);
  });

  it('emits plain circles (no inline fill) in structure mode', () => {
    const svg = renderFolderSolar(fixture());
    expect(svg).not.toContain('style="fill:');
  });
});

describe('escaping + truncation notice', () => {
  it('escapes angle brackets in labels', () => {
    const m = fixture();
    m.nodes[3].label = '<script>';
    const svg = renderFolderSolar(m);
    expect(svg).toContain('&lt;script&gt;');
    expect(svg).not.toContain('<script>');
  });

  it('shows a truncation caption only when truncated', () => {
    expect(renderFolderHierarchy(fixture())).not.toContain('truncated at');
    const t = { ...fixture(), truncated: true };
    expect(renderFolderHierarchy(t)).toContain('truncated at 6 nodes / depth 2');
  });
});

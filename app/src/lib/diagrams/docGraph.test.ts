import { describe, expect, it } from 'vitest';
import {
  docNodeRadius,
  placeDocNetwork,
  placeDocOrphans,
  placeDocRadial,
  renderDocGraph,
  type DocGraph,
  type DocNode,
} from './docGraph';

function docNode(
  id: string,
  rel: string,
  opts: Partial<DocNode> = {},
): DocNode {
  return {
    id,
    abs: `/abs/${rel}`,
    rel,
    title: rel,
    inbound: 0,
    outbound: 0,
    external: 0,
    orphan: false,
    ...opts,
  };
}

function graph(nodes: DocNode[], edges: DocGraph['edges'] = []): DocGraph {
  return {
    root: '.',
    nodes,
    edges,
    dangling: [],
    orphan_count: nodes.filter((n) => n.orphan).length,
    dangling_count: 0,
    external_count: 0,
  };
}

describe('renderDocGraph', () => {
  it('renders a placeholder when there are no docs', () => {
    const svg = renderDocGraph(graph([]), 'network');
    expect(svg).toContain('No markdown documents found');
  });

  it('emits one node group per doc and honours the selected id', () => {
    const g = graph([docNode('a', 'a.md'), docNode('b', 'b.md')]);
    const svg = renderDocGraph(g, 'network', 'b');
    expect((svg.match(/class="node doc-node/g) ?? []).length).toBe(2);
    // The selected node picks up the `selected` class.
    expect(svg).toContain('doc-node selected');
  });

  it('marks orphan nodes with the orphan class', () => {
    const g = graph([docNode('o', 'orphan.md', { orphan: true })]);
    expect(renderDocGraph(g, 'network')).toContain('doc-node orphan');
  });
});

describe('doc-graph placement', () => {
  it('network layout puts the highest-degree node at the centre (700,450)', () => {
    const g = graph([
      docNode('hub', 'hub.md', { inbound: 5, outbound: 5 }),
      docNode('leaf', 'leaf.md', { inbound: 1 }),
    ]);
    const placed = placeDocNetwork(g);
    expect(placed.get('hub')).toMatchObject({ x: 700, y: 450 });
  });

  it('radial layout centres the README', () => {
    const g = graph([
      docNode('readme', 'README.md', { outbound: 1 }),
      docNode('other', 'other.md'),
    ]);
    const placed = placeDocRadial(g);
    expect(placed.get('readme')).toMatchObject({ x: 700, y: 450 });
  });

  it('orphans layout splits into orphan (left) and connected (right) columns', () => {
    const g = graph([
      docNode('orph', 'orph.md', { orphan: true }),
      docNode('conn', 'conn.md', { inbound: 2 }),
    ]);
    const placed = placeDocOrphans(g);
    expect(placed.get('orph')!.x).toBe(360);
    expect(placed.get('conn')!.x).toBe(980);
  });
});

describe('docNodeRadius', () => {
  it('grows with degree and caps at 46', () => {
    expect(docNodeRadius(docNode('a', 'a.md'))).toBeGreaterThan(0);
    const huge = docNodeRadius(
      docNode('a', 'a.md', { inbound: 1000, outbound: 1000, external: 1000 }),
    );
    expect(huge).toBe(46);
  });
});

import { describe, expect, it } from 'vitest';
import {
  c4ComponentMermaid,
  c4Label,
  DEFAULT_COMPONENT_CAP,
  escapeId,
  hiddenComponentCount,
  moduleList,
  shortModule,
} from './c4Component';
import type { BeanEdge, BeanGraphData, BeanNode } from '../api';

function node(id: string, module: string, stereotype: string | null = null): BeanNode {
  return { id, label: id.includes('.') ? id.slice(id.lastIndexOf('.') + 1) : id, module, stereotype, path: null };
}
function edge(from: string, to: string, kind = 'uses'): BeanEdge {
  return { from, to, kind };
}

describe('escapeId', () => {
  it('collapses every non-alphanumeric char to underscore', () => {
    expect(escapeId('com.foo.BarService')).toBe('com_foo_BarService');
    expect(escapeId('group:artifact-id')).toBe('group_artifact_id');
    expect(escapeId('Already_Safe1')).toBe('Already_Safe1');
  });
});

describe('c4Label', () => {
  it('replaces embedded double quotes with single quotes', () => {
    expect(c4Label('a "quoted" name')).toBe("a 'quoted' name");
    expect(c4Label('plain')).toBe('plain');
  });
});

describe('shortModule', () => {
  it('reduces groupId:artifactId to the artifactId', () => {
    expect(shortModule('com.plaintext:plaintext-app-web')).toBe('plaintext-app-web');
  });
  it('reduces a path-style id to its last segment', () => {
    expect(shortModule('crates/core')).toBe('core');
  });
  it('returns the id unchanged when it has no separator', () => {
    expect(shortModule('core')).toBe('core');
  });
});

describe('moduleList', () => {
  it('is empty for an empty payload', () => {
    expect(moduleList({ nodes: [], edges: [] })).toEqual([]);
  });

  it('counts classes per module and sorts by count desc then label asc', () => {
    const data: BeanGraphData = {
      nodes: [
        node('a.A', 'g:beta'),
        node('a.B', 'g:beta'),
        node('a.C', 'g:beta'),
        node('x.X', 'g:alpha'),
        node('x.Y', 'g:alpha'),
        node('z.Z', 'g:gamma'),
      ],
      edges: [],
    };
    const rows = moduleList(data);
    expect(rows).toEqual([
      { id: 'g:beta', label: 'beta', classCount: 3 },
      { id: 'g:alpha', label: 'alpha', classCount: 2 },
      { id: 'g:gamma', label: 'gamma', classCount: 1 },
    ]);
  });

  it('breaks count ties alphabetically by label', () => {
    const data: BeanGraphData = {
      nodes: [node('a.A', 'g:zeta'), node('b.B', 'g:alpha')],
      edges: [],
    };
    expect(moduleList(data).map((r) => r.label)).toEqual(['alpha', 'zeta']);
  });
});

describe('c4ComponentMermaid', () => {
  it('renders a minimal valid document for an empty payload', () => {
    const out = c4ComponentMermaid({ nodes: [], edges: [] }, 'g:missing');
    expect(out.startsWith('C4Component\n')).toBe(true);
    expect(out).toContain('title Component view of missing');
    expect(out).toContain('Component(empty, "empty", "no components")');
  });

  it('renders a minimal valid document for an unknown module', () => {
    const data: BeanGraphData = { nodes: [node('a.A', 'g:real')], edges: [] };
    const out = c4ComponentMermaid(data, 'g:ghost');
    expect(out.startsWith('C4Component\n')).toBe(true);
    expect(out).toContain('Component(empty, "empty", "no components")');
  });

  it('emits a Component per in-module class with its stereotype, in a boundary', () => {
    const data: BeanGraphData = {
      nodes: [
        node('a.UserService', 'g:app', 'service'),
        node('a.UserRepo', 'g:app', 'repository'),
        node('other.Thing', 'g:lib'),
      ],
      edges: [],
    };
    const out = c4ComponentMermaid(data, 'g:app');
    expect(out).toContain('Container_Boundary(g_app, "app") {');
    expect(out).toContain('Component(c_a_UserService, "UserService", "service")');
    expect(out).toContain('Component(c_a_UserRepo, "UserRepo", "repository")');
    // A class from another module is never drawn as a component here.
    expect(out).not.toContain('Thing');
  });

  it('defaults a null stereotype to "class"', () => {
    const data: BeanGraphData = { nodes: [node('a.Plain', 'g:app', null)], edges: [] };
    const out = c4ComponentMermaid(data, 'g:app');
    expect(out).toContain('Component(c_a_Plain, "Plain", "class")');
  });

  it('renders intra-module edges as Rel between components', () => {
    const data: BeanGraphData = {
      nodes: [node('a.A', 'g:app'), node('a.B', 'g:app')],
      edges: [edge('a.A', 'a.B', 'injects')],
    };
    const out = c4ComponentMermaid(data, 'g:app');
    expect(out).toContain('Rel(c_a_A, c_a_B, "injects")');
  });

  it('collapses cross-module edges to a Container_Ext + a single uses Rel', () => {
    const data: BeanGraphData = {
      nodes: [
        node('a.A', 'g:app'),
        node('lib.L1', 'g:lib'),
        node('lib.L2', 'g:lib'),
      ],
      edges: [edge('a.A', 'lib.L1'), edge('a.A', 'lib.L2')],
    };
    const out = c4ComponentMermaid(data, 'g:app');
    expect(out).toContain('Container_Ext(ext_g_lib, "lib")');
    // Deduped: one arrow to the module even though two classes are referenced.
    const relCount = (out.match(/Rel\(c_a_A, ext_g_lib, "uses"\)/g) ?? []).length;
    expect(relCount).toBe(1);
  });

  it('does not treat a capped-out sibling as an external dependency', () => {
    // Two classes, cap 1: B is hidden. An edge A→B must NOT become a cross-
    // module ext edge — B is in the same module, just not drawn.
    const data: BeanGraphData = {
      nodes: [node('a.A', 'g:app'), node('a.B', 'g:app')],
      edges: [edge('a.A', 'a.B')],
    };
    const out = c4ComponentMermaid(data, 'g:app', { cap: 1 });
    expect(out).not.toContain('Container_Ext');
    expect(out).not.toContain('ext_g_app');
  });

  it('caps components by in-module fan-in and reports the hidden count', () => {
    // Hub is referenced by everyone → highest fan-in → survives a cap of 2.
    const nodes: BeanNode[] = [
      node('a.Hub', 'g:app'),
      node('a.N1', 'g:app'),
      node('a.N2', 'g:app'),
      node('a.N3', 'g:app'),
    ];
    const edges: BeanEdge[] = [
      edge('a.N1', 'a.Hub'),
      edge('a.N2', 'a.Hub'),
      edge('a.N3', 'a.Hub'),
    ];
    const out = c4ComponentMermaid({ nodes, edges }, 'g:app', { cap: 2 });
    expect(out).toContain('Component(c_a_Hub,');
    // 4 classes, cap 2 → 2 hidden.
    expect(hiddenComponentCount(out)).toBe(2);
    expect(out).toContain('%% +2 more component(s) not shown (cap 2)');
    // Only 2 Component lines drawn.
    const drawn = (out.match(/^\s*Component\(/gm) ?? []).length;
    expect(drawn).toBe(2);
  });

  it('adds no hidden marker when under the cap', () => {
    const data: BeanGraphData = { nodes: [node('a.A', 'g:app')], edges: [] };
    const out = c4ComponentMermaid(data, 'g:app');
    expect(hiddenComponentCount(out)).toBe(0);
    expect(out).not.toContain('more component');
  });

  it('respects the documented default cap constant', () => {
    const nodes: BeanNode[] = Array.from({ length: DEFAULT_COMPONENT_CAP + 5 }, (_, i) =>
      node(`a.C${String(i).padStart(2, '0')}`, 'g:app'),
    );
    const out = c4ComponentMermaid({ nodes, edges: [] }, 'g:app');
    const drawn = (out.match(/^\s*Component\(/gm) ?? []).length;
    expect(drawn).toBe(DEFAULT_COMPONENT_CAP);
    expect(hiddenComponentCount(out)).toBe(5);
  });

  it('escapes ids and labels safely', () => {
    const data: BeanGraphData = {
      nodes: [{ id: 'a.Weird', label: 'Weird "Name"', module: 'g:app', stereotype: null, path: null }],
      edges: [],
    };
    const out = c4ComponentMermaid(data, 'g:app');
    // Label double-quotes are neutralised to single quotes.
    expect(out).toContain('Component(c_a_Weird, "Weird \'Name\'", "class")');
    // Id carries no dots.
    expect(out).not.toContain('a.Weird');
  });

  it('dedupes identical intra-module from/to/kind triples but keeps distinct kinds', () => {
    const data: BeanGraphData = {
      nodes: [node('a.A', 'g:app'), node('a.B', 'g:app')],
      edges: [edge('a.A', 'a.B', 'uses'), edge('a.A', 'a.B', 'uses'), edge('a.A', 'a.B', 'calls')],
    };
    const out = c4ComponentMermaid(data, 'g:app');
    expect((out.match(/Rel\(c_a_A, c_a_B, "uses"\)/g) ?? []).length).toBe(1);
    expect((out.match(/Rel\(c_a_A, c_a_B, "calls"\)/g) ?? []).length).toBe(1);
  });
});

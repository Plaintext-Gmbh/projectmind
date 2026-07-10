import { describe, expect, it } from 'vitest';
import {
  beanGraphElements,
  relationClass,
  stereotypeClass,
} from './beanGraphElements';
import type { BeanGraphData } from '../api';

describe('stereotypeClass', () => {
  it('maps known stereotypes to a stereo- class', () => {
    expect(stereotypeClass('service')).toBe('stereo-service');
    expect(stereotypeClass('rest-controller')).toBe('stereo-rest-controller');
    expect(stereotypeClass('repository')).toBe('stereo-repository');
  });

  it('falls back to stereo-default for null and unknown', () => {
    expect(stereotypeClass(null)).toBe('stereo-default');
    expect(stereotypeClass('made-up')).toBe('stereo-default');
  });
});

describe('relationClass', () => {
  it('maps known RelationKinds to a rel- class', () => {
    expect(relationClass('injects')).toBe('rel-injects');
    expect(relationClass('extends')).toBe('rel-extends');
    expect(relationClass('annotated')).toBe('rel-annotated');
  });

  it('falls back to rel-other for unknown kinds', () => {
    expect(relationClass('teleports')).toBe('rel-other');
  });
});

describe('beanGraphElements', () => {
  it('maps an empty payload to empty element lists', () => {
    const out = beanGraphElements({ nodes: [], edges: [] });
    expect(out.nodes).toEqual([]);
    expect(out.edges).toEqual([]);
  });

  it('maps nodes with label, module and stereotype class', () => {
    const payload: BeanGraphData = {
      nodes: [
        { id: 'a.A', label: 'A', module: 'g:m1', stereotype: 'service' },
        { id: 'a.B', label: 'B', module: 'g:m1', stereotype: null },
      ],
      edges: [],
    };
    const out = beanGraphElements(payload);
    expect(out.nodes).toHaveLength(2);
    expect(out.nodes[0]).toEqual({
      group: 'nodes',
      data: {
        id: 'a.A',
        label: 'A',
        module: 'g:m1',
        stereoClass: 'stereo-service',
        stereotype: 'service',
      },
    });
    // null stereotype → default class, raw value preserved.
    expect(out.nodes[1].data.stereoClass).toBe('stereo-default');
    expect(out.nodes[1].data.stereotype).toBeNull();
  });

  it('maps edges to Cytoscape source/target with a rel class and unique ids', () => {
    const payload: BeanGraphData = {
      nodes: [
        { id: 'a.A', label: 'A', module: 'g:m1', stereotype: 'service' },
        { id: 'a.B', label: 'B', module: 'g:m1', stereotype: 'controller' },
      ],
      edges: [
        { from: 'a.A', to: 'a.B', kind: 'injects' },
        { from: 'a.B', to: 'a.A', kind: 'calls' },
      ],
    };
    const out = beanGraphElements(payload);
    expect(out.edges).toHaveLength(2);
    expect(out.edges[0]).toMatchObject({
      group: 'edges',
      data: { id: 'e0', source: 'a.A', target: 'a.B', kind: 'injects', relClass: 'rel-injects' },
    });
    // Ids are unique so parallel edges coexist.
    expect(out.edges[0].data.id).not.toBe(out.edges[1].data.id);
  });

  it('flags cross-module edges and keeps same-module edges unflagged', () => {
    const payload: BeanGraphData = {
      nodes: [
        { id: 'a.A', label: 'A', module: 'g:m1', stereotype: null },
        { id: 'b.B', label: 'B', module: 'g:m2', stereotype: null },
        { id: 'a.C', label: 'C', module: 'g:m1', stereotype: null },
      ],
      edges: [
        { from: 'a.A', to: 'b.B', kind: 'uses' }, // cross-module
        { from: 'a.A', to: 'a.C', kind: 'uses' }, // same module
      ],
    };
    const out = beanGraphElements(payload);
    expect(out.edges[0].data.crossModule).toBe(true);
    expect(out.edges[1].data.crossModule).toBe(false);
  });

  it('drops edges whose endpoints are missing from the node set', () => {
    const payload: BeanGraphData = {
      nodes: [{ id: 'a.A', label: 'A', module: 'g:m1', stereotype: null }],
      edges: [{ from: 'a.A', to: 'ghost.X', kind: 'calls' }],
    };
    const out = beanGraphElements(payload);
    expect(out.edges).toEqual([]);
  });
});

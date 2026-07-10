/// Pure payload → Cytoscape-elements mapping for the interactive bean graph
/// (`bean-graph-live`, V3.1 / #61). The stateful Cytoscape mount lives in
/// `app/src/components/BeanGraphLive.svelte`; this module holds the part that
/// can be unit-tested without a DOM: turning the `BeanGraphData` JSON from the
/// backend into the `{ nodes, edges }` element shape Cytoscape consumes,
/// including the stereotype → node-class and RelationKind → edge-class
/// derivation that drives colouring (parity with the Mermaid `classDef`s).
///
/// Kept dependency-free (no `cytoscape` import) so the mapping stays a plain
/// function — the component passes the result straight into `cy.add()`.

import type { BeanGraphData } from '../api';

/// Cytoscape element shapes we emit. Deliberately minimal — just the fields
/// the component's stylesheet and drilldown handler read. Cytoscape accepts
/// extra `data` fields, so this stays forward-compatible with later diff/morph
/// overlays that want to stash more on each element.
export interface BeanNodeElement {
  group: 'nodes';
  data: {
    id: string;
    label: string;
    module: string;
    /// Normalised stereotype class name used by the stylesheet selector
    /// (`stereo-service`, …). Always present; falls back to the default.
    stereoClass: string;
    /// Raw stereotype from the backend (null when the class has none).
    stereotype: string | null;
  };
}

export interface BeanEdgeElement {
  group: 'edges';
  data: {
    id: string;
    source: string;
    target: string;
    /// RelationKind, echoed for tooltips / later overlays.
    kind: string;
    /// Stylesheet selector class (`rel-injects`, …).
    relClass: string;
    /// True when the edge crosses a module boundary (drives a heavier
    /// stroke, parity with the Mermaid cross-module highlight).
    crossModule: boolean;
  };
}

export interface BeanGraphElements {
  nodes: BeanNodeElement[];
  edges: BeanEdgeElement[];
}

/// Stereotypes the Mermaid `classDef`s know about. Anything outside this set
/// maps to `stereo-default`, matching the Rust `DEFAULT_STYLE` fallback.
const KNOWN_STEREOTYPES = new Set([
  'service',
  'rest-controller',
  'controller',
  'repository',
  'component',
  'configuration',
  'lombok',
]);

/// RelationKinds the backend serialises (snake_case `RelationKind`). Anything
/// else collapses to `rel-other`.
const KNOWN_KINDS = new Set([
  'extends',
  'implements',
  'uses',
  'injects',
  'calls',
  'annotated',
  'other',
]);

/// Normalise a stereotype into a stylesheet class name. `null`/unknown →
/// `stereo-default`.
export function stereotypeClass(stereotype: string | null): string {
  if (stereotype && KNOWN_STEREOTYPES.has(stereotype)) {
    return `stereo-${stereotype}`;
  }
  return 'stereo-default';
}

/// Normalise a RelationKind into a stylesheet class name. Unknown → `rel-other`.
export function relationClass(kind: string): string {
  return KNOWN_KINDS.has(kind) ? `rel-${kind}` : 'rel-other';
}

/// Map a backend `BeanGraphData` payload to Cytoscape elements. Pure: empty in
/// → empty out. Edges whose endpoints are missing from the node set are
/// dropped (defensive — the backend only emits touched nodes, so this should
/// not happen, but a dangling edge would make Cytoscape throw). Edge ids are
/// made unique by index so parallel edges of different kinds coexist.
export function beanGraphElements(payload: BeanGraphData): BeanGraphElements {
  const moduleOf = new Map<string, string>();
  for (const n of payload.nodes) moduleOf.set(n.id, n.module);

  const nodes: BeanNodeElement[] = payload.nodes.map((n) => ({
    group: 'nodes',
    data: {
      id: n.id,
      label: n.label,
      module: n.module,
      stereoClass: stereotypeClass(n.stereotype),
      stereotype: n.stereotype,
    },
  }));

  const edges: BeanEdgeElement[] = [];
  payload.edges.forEach((e, i) => {
    if (!moduleOf.has(e.from) || !moduleOf.has(e.to)) return;
    edges.push({
      group: 'edges',
      data: {
        id: `e${i}`,
        source: e.from,
        target: e.to,
        kind: e.kind,
        relClass: relationClass(e.kind),
        crossModule: moduleOf.get(e.from) !== moduleOf.get(e.to),
      },
    });
  });

  return { nodes, edges };
}

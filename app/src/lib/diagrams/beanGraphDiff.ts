/// Pure diff classification for the animated bean-graph overlay
/// (`bean-graph-live`, #63 concept 3). The stateful Cytoscape mount lives in
/// `app/src/components/BeanGraphLive.svelte`; this module holds the part that
/// can be unit-tested without a DOM: given the graph elements and the set of
/// files changed since a git ref (from `list_changes_since`), decide which
/// nodes and edges are "changed" (they pulse + get a thicker stroke) so the
/// component can toggle the `changed` / `faded` Cytoscape classes.
///
/// ## Node → file join
///
/// Every node carries a repo-relative, forward-slashed `path` (added to the
/// backend `BeanNode` payload for exactly this feature). `list_changes_since`
/// reports the same repo-relative, forward-slashed paths, so a node is
/// "changed" iff its `path` is in the change set — a plain string-set
/// intersection, no FQN→path guessing in the frontend. Nodes with a `null`
/// path (an inferred super type with no parsed file) can never be changed.
///
/// ## Edge rule
///
/// An edge is "changed" iff **either** of its endpoint nodes is changed. The
/// rationale: an edge is a relationship between two classes, and a change to
/// either class can add, remove, or alter that relationship — so highlighting
/// the edge whenever a touched class sits on it is the honest, over-inclusive
/// reading. (We can't tell from file-level diffs whether the specific relation
/// changed, so we surface every edge that *might* have.)
///
/// Kept dependency-free (no `cytoscape` import) so the classification stays a
/// plain function the component feeds into `cy.$id(...).addClass(...)`.

import type { ChangedFile } from '../api';
import type { BeanGraphElements } from './beanGraphElements';

/// The outcome of classifying a graph against a change set: the ids that should
/// light up. Everything not in `changedNodeIds` / `changedEdgeIds` is faded by
/// the component. `hasDiff` is false when nothing changed, letting the caller
/// skip the overlay entirely (no fading) so an empty diff reads as "no change"
/// rather than "everything dimmed".
export interface BeanGraphDiff {
  changedNodeIds: Set<string>;
  changedEdgeIds: Set<string>;
  hasDiff: boolean;
}

/// Normalise a path to the forward-slash, no-`./`-prefix form the node `path`
/// fields use, so a Windows delta (`a\b`) and a `./a/b` entry both match.
/// Mirrors the normalisation `diagramDiff.ts` applies to the folder-map diff.
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '');
}

/// Classify graph elements against the set of files changed since a ref.
///
/// Pure: an empty change set (or an element set with no matching paths) yields
/// empty id sets and `hasDiff = false` — never throws. That is the "empty diff
/// → nothing changed" acceptance case.
export function classifyBeanGraphDiff(
  elements: BeanGraphElements,
  changes: readonly ChangedFile[],
): BeanGraphDiff {
  const changedPaths = new Set<string>();
  for (const c of changes) changedPaths.add(normPath(c.path));

  const changedNodeIds = new Set<string>();
  if (changedPaths.size > 0) {
    for (const node of elements.nodes) {
      const p = node.data.path;
      if (p !== null && changedPaths.has(normPath(p))) {
        changedNodeIds.add(node.data.id);
      }
    }
  }

  // Edge rule: changed iff either endpoint node is changed.
  const changedEdgeIds = new Set<string>();
  if (changedNodeIds.size > 0) {
    for (const edge of elements.edges) {
      if (
        changedNodeIds.has(edge.data.source) ||
        changedNodeIds.has(edge.data.target)
      ) {
        changedEdgeIds.add(edge.data.id);
      }
    }
  }

  return {
    changedNodeIds,
    changedEdgeIds,
    hasDiff: changedNodeIds.size > 0,
  };
}

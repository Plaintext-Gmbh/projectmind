/// Pure mode-selection + animation-plan logic for the bean-graph *morph* mode
/// (`bean-graph-live`, V3.3 / #125 fourth mode). The stateful Cytoscape mount
/// and the actual animation calls live in
/// `app/src/components/BeanGraphLive.svelte`; this module holds the part that
/// can be unit-tested without a DOM: which of the graph's three overlay states
/// is active, and — given a diff classification — which node/edge ids animate
/// "in" versus which recede.
///
/// ## The three overlay states
///
/// `bean-graph-live` has one plain graph and two ways to surface a since-ref
/// change set:
///
/// - **`off`** — no ref set: the plain, un-faded graph.
/// - **`diff`** — the V3.2 static overlay: changed elements get a heavier
///   accent stroke, everything else fades to ~50 %, one short pulse. It reads
///   like a highlight and holds its state.
/// - **`morph`** — the V3.3 animated transition (this feature): the changed
///   elements animate *in* (start recessed, then pulse in + thicken while the
///   layout eases into place); the unchanged elements settle to the same ~50 %
///   fade. A one-shot transition, not a persistent state — after it plays the
///   graph rests in exactly the `diff` look, so morph is "diff, but arrived at
///   by an animation".
///
/// ## Why a mode, not a second overlay
///
/// The component reuses the very same `changed` / `faded` Cytoscape classes the
/// V3.2 diff already ships; morph differs only in *how* those classes arrive
/// (animated from a recessed start state) — so the classification stays
/// `classifyBeanGraphDiff` and this module only decides the plan, never
/// re-implements the node→file join.
///
/// Kept dependency-free (no `cytoscape` import) so the whole thing is a plain
/// function the component feeds into `cy` animation calls.

import type { BeanGraphDiff } from './beanGraphDiff';

/// The overlay state the graph is in. Drives which toggle button reads active
/// and, for `morph`, whether the component plays the entry animation.
export type BeanGraphOverlayMode = 'off' | 'diff' | 'morph';

export const BEAN_GRAPH_OVERLAY_MODES: readonly BeanGraphOverlayMode[] = [
  'off',
  'diff',
  'morph',
];

/// Resolve the active overlay state from the two pieces of UI state the
/// component owns: whether a ref is currently applied, and whether the user
/// picked the morph toggle.
///
/// - no ref → `off` regardless of the toggle (nothing to animate).
/// - ref + morph toggle → `morph`.
/// - ref, no morph toggle → `diff` (the V3.2 static overlay).
///
/// Pure so the component's reactive `$:` can derive the mode and the tests can
/// pin every combination.
export function resolveOverlayMode(
  hasRef: boolean,
  morphRequested: boolean,
): BeanGraphOverlayMode {
  if (!hasRef) return 'off';
  return morphRequested ? 'morph' : 'diff';
}

/// A single element's role in a morph transition. `enter` elements are the
/// changed ones that animate in (pulse + stroke); `recede` elements fade to the
/// dim rest-state; nothing is ever "removed" — see the architecture note below.
export type MorphRole = 'enter' | 'recede';

/// The plan the component executes: the ids that animate in, the ids that
/// recede, and whether there's anything to animate at all. Empty `enter` (an
/// empty diff) yields `animate: false` so the component leaves the graph plain
/// rather than dim everything — the same "empty diff → no change" contract the
/// static overlays honour.
export interface BeanGraphMorphPlan {
  /// Changed node ids that animate in.
  enterNodeIds: Set<string>;
  /// Changed edge ids that animate in.
  enterEdgeIds: Set<string>;
  /// Unchanged node ids that recede to the faded rest-state.
  recedeNodeIds: Set<string>;
  /// Unchanged edge ids that recede.
  recedeEdgeIds: Set<string>;
  /// False when nothing changed — the component skips the animation and the
  /// fade so an empty diff reads as "no change" rather than "everything dimmed".
  animate: boolean;
}

/// Build the morph plan from a diff classification and the full id sets of the
/// current graph.
///
/// The changed ids come straight from `classifyBeanGraphDiff` (so morph and the
/// static diff always agree on *what* changed); the "recede" sets are the
/// complement — every current id that is not changed. Because ProjectMind is a
/// current-state browser we only ever have the *current* graph's ids, so a file
/// deleted since the ref simply isn't here to plan for (documented limitation:
/// morph never shows "removed" nodes — see the module + component headers).
///
/// Pure: no mutation of the inputs, deterministic, never throws. `allNodeIds`
/// and `allEdgeIds` are typically `cy.nodes().map(id)` / `cy.edges().map(id)`,
/// passed in so this stays DOM-free and testable.
export function planBeanGraphMorph(
  diff: BeanGraphDiff,
  allNodeIds: Iterable<string>,
  allEdgeIds: Iterable<string>,
): BeanGraphMorphPlan {
  const enterNodeIds = new Set(diff.changedNodeIds);
  const enterEdgeIds = new Set(diff.changedEdgeIds);

  const recedeNodeIds = new Set<string>();
  const recedeEdgeIds = new Set<string>();

  // Only fade the rest when there is a signal to contrast it against; an empty
  // diff leaves the whole graph at full opacity (no recede).
  if (diff.hasDiff) {
    for (const id of allNodeIds) {
      if (!enterNodeIds.has(id)) recedeNodeIds.add(id);
    }
    for (const id of allEdgeIds) {
      if (!enterEdgeIds.has(id)) recedeEdgeIds.add(id);
    }
  }

  return {
    enterNodeIds,
    enterEdgeIds,
    recedeNodeIds,
    recedeEdgeIds,
    animate: diff.hasDiff,
  };
}

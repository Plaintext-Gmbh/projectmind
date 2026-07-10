/// Pure BFS-wave planner for the bean-graph *flow* mode
/// (`bean-graph-live`, V4.1 / #200 fourth toolbar mode). The stateful Cytoscape
/// mount and the actual marching-ants / pulse animation live in
/// `app/src/components/BeanGraphLive.svelte`; this module holds the part that
/// can be unit-tested without a DOM: turning the graph elements into the ordered
/// waves a simulated request travels through, so the component just plays them.
///
/// ## What "flow" means (honesty rule #61)
///
/// ProjectMind is a read-only current-state browser — there is no live request
/// stream to animate. "Flow" is a *simulated* request wave: a BFS from the entry
/// stereotypes (rest-controller / controller) along the directed relation edges
/// towards the repositories, surfaced as marching-ants edges + node pulses. It
/// answers "how does a request topologically travel through this system", not
/// "what traffic is happening now" — the toolbar tooltip says exactly that.
///
/// ## Wave construction
///
/// - **Entry nodes** are the ones whose `stereoClass` is a controller
///   (`stereo-rest-controller` / `stereo-controller`). If a graph has none
///   (e.g. a non-Spring repo, or one where controllers weren't parsed), we fall
///   back to the nodes with no incoming edge (the topological sources); if there
///   are none of those either (every node is in a cycle), we fall back to the
///   first node in sorted id order so the animation always has a start. An empty
///   graph yields `{ waves: [], animate: false }`.
/// - **BFS with a visited set** makes cycles (a `calls` edge can point back)
///   terminate: every node is enqueued at most once, at the depth it is first
///   reached. Waves are grouped by that depth — wave 0 is the entry frontier,
///   wave `k` the nodes first reached at distance `k`.
/// - **Edges belong to the wave in which their *target* is first reached** — the
///   edge that "carries" the request into a node lights up together with that
///   node's arrival. An edge into an already-visited node (a back-edge closing a
///   cycle, or a second path to the same node) is not re-emitted, so each edge
///   appears in at most one wave.
/// - **Unreachable nodes** (no directed path from any entry) are left out of the
///   plan entirely; the component fades them for the duration of the flow so the
///   travelled path reads as the signal.
///
/// Kept dependency-free (no `cytoscape` import) so the plan stays a plain
/// function the component feeds into `cy` animation calls — same pattern as
/// `beanGraphMorph.ts` / `beanGraphDiff.ts`.

import type { BeanGraphElements } from './beanGraphElements';

/// The stereotype classes that seed the flow (a request enters the system at a
/// controller). Mirrors the `stereoClass` values `beanGraphElements` emits.
const ENTRY_STEREO_CLASSES = new Set(['stereo-rest-controller', 'stereo-controller']);

/// One BFS frontier: the node ids first reached at this depth and the edge ids
/// that carried the request into them. Both are ordered by first-encounter so a
/// replay is deterministic (tests can pin the exact wave contents).
export interface FlowWave {
  nodeIds: string[];
  edgeIds: string[];
}

/// The plan the component plays: the ordered waves, the entry frontier it seeded
/// from (echoed for the toolbar / debugging), and whether there is anything to
/// animate. `animate` is false only for an empty graph — any non-empty graph has
/// at least one entry node and therefore at least one wave.
export interface FlowPlan {
  waves: FlowWave[];
  entryNodeIds: string[];
  animate: boolean;
}

/// Pick the entry frontier: controllers first, then topological sources (no
/// incoming edge), then the single sorted-first node. Returns [] only for an
/// empty node set.
function pickEntryNodeIds(
  els: BeanGraphElements,
  hasIncoming: Set<string>,
): string[] {
  const controllers = els.nodes
    .filter((n) => ENTRY_STEREO_CLASSES.has(n.data.stereoClass))
    .map((n) => n.data.id);
  if (controllers.length > 0) return controllers;

  const sources = els.nodes
    .filter((n) => !hasIncoming.has(n.data.id))
    .map((n) => n.data.id);
  if (sources.length > 0) return sources;

  // Every node has an incoming edge (fully cyclic) — seed from the sorted-first
  // node so the wave still has somewhere to start.
  const ids = els.nodes.map((n) => n.data.id).sort();
  return ids.length > 0 ? [ids[0]] : [];
}

/// Build the BFS flow plan from the graph elements. Pure: empty in → empty out,
/// deterministic, never throws.
///
/// See the module header for the full contract (entry selection, cycle
/// termination, edge→wave assignment, unreachable-node handling).
export function planBeanGraphFlow(els: BeanGraphElements): FlowPlan {
  if (els.nodes.length === 0) {
    return { waves: [], entryNodeIds: [], animate: false };
  }

  // Adjacency + incoming-edge presence in one pass. `outgoing` maps a source id
  // to its (target, edgeId) pairs, in element order so BFS is deterministic.
  const outgoing = new Map<string, { target: string; edgeId: string }[]>();
  const hasIncoming = new Set<string>();
  const nodeIds = new Set(els.nodes.map((n) => n.data.id));
  for (const e of els.edges) {
    // Defensive: beanGraphElements already drops dangling edges, but guard so a
    // stray endpoint can never seed a phantom node into a wave.
    if (!nodeIds.has(e.data.source) || !nodeIds.has(e.data.target)) continue;
    let list = outgoing.get(e.data.source);
    if (!list) {
      list = [];
      outgoing.set(e.data.source, list);
    }
    list.push({ target: e.data.target, edgeId: e.data.id });
    hasIncoming.add(e.data.target);
  }

  const entryNodeIds = pickEntryNodeIds(els, hasIncoming);

  // Standard level-order BFS. `visited` guards against cycles and re-entry; a
  // node is added to exactly the wave (depth) it is first reached at, and the
  // edge that first reaches it is the one emitted for that wave.
  const visited = new Set<string>();
  const waves: FlowWave[] = [];

  // Frontier of the current wave: node ids reached at this depth, plus the edge
  // ids that carried the request into them. Entry nodes have no carrying edge.
  let frontierNodes: string[] = [];
  const seededEdges: string[] = [];
  for (const id of entryNodeIds) {
    if (!visited.has(id)) {
      visited.add(id);
      frontierNodes.push(id);
    }
  }
  if (frontierNodes.length > 0) {
    waves.push({ nodeIds: frontierNodes, edgeIds: seededEdges });
  }

  while (frontierNodes.length > 0) {
    const nextNodes: string[] = [];
    const nextEdges: string[] = [];
    for (const src of frontierNodes) {
      const outs = outgoing.get(src);
      if (!outs) continue;
      for (const { target, edgeId } of outs) {
        if (visited.has(target)) continue; // cycle / already reached — skip
        visited.add(target);
        nextNodes.push(target);
        nextEdges.push(edgeId);
      }
    }
    if (nextNodes.length === 0) break;
    waves.push({ nodeIds: nextNodes, edgeIds: nextEdges });
    frontierNodes = nextNodes;
  }

  return { waves, entryNodeIds, animate: true };
}

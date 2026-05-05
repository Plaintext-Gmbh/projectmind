# Tour UX — before/after architecture snapshots — sketch (#125)

> Concept: a tour step can show two graph states (a "before" diagram at
> commit A and an "after" diagram at commit B) for the same diagram kind,
> with changed nodes/edges animated between the two.
> Concept owner: ProjectMind core, on top of the existing diagram
> rendering. Question this answers: *"what does this refactor / feature
> branch actually change in the architecture?"*
> Candidate library: Cytoscape.js (replaces the bean-graph's current
> Mermaid path; reuses the renderer for animated graph classes).

## TL;DR

A tour step gets a new optional target shape — `diagram-diff` — that
points at one diagram kind (`bean-graph` first; `folder-map` second)
and two git refs. The viewer renders both states, derives the changed
set from `list_changes_since(from, to)`, and lets the user toggle:

- **before** — graph at `from`
- **after**  — graph at `to`
- **morph**  — interpolate between the two (CSS / Cytoscape transitions)
- **changed only** — fade unchanged nodes to 30%

Changed nodes pulse once when the step opens, mirroring the diff-focus
pulse from #126 so the visual language stays consistent across the
tour.

The work splits into three follow-up PRs spelt out in
[Implementation phases](#implementation-phases) below.

## What the user sees

```
  ┌─ Tour step 4 of 7 — "User flow now goes through the API gateway" ───┐
  │  [ before ] [ after ] [ morph ] [ changed only ]                    │
  │                                                                      │
  │            UserCtrl  ────►  UserSvc  ─────►  UserRepo               │
  │              │                │                                       │
  │              │ (added)        │                                       │
  │              ▼                ▼                                       │
  │          ●   GwCtrl      ●  AuditSvc                                  │
  │            ▲ pulses      ▲ pulses                                     │
  │                                                                      │
  │  Δ 2 added, 1 removed, 4 unchanged                                   │
  └──────────────────────────────────────────────────────────────────────┘
```

- **Toolbar** at the top of the diagram pane offers the four toggles.
  Default state per step is "after with changed pulses" so the eye
  lands on the new shape.
- **Pulses** reuse the same 1.4s `--accent-2` animation #126 added for
  diff-focus rails; cross-feature consistency keeps the visual
  vocabulary tight.
- **Footer counter** (`Δ 2 added, 1 removed, 4 unchanged`) gives the
  user a numeric anchor without having to count nodes.

## Walk-through target shape

```ts
// app/src/lib/api.ts (illustrative — to land in the impl PR, not here)
type WalkthroughTarget =
  // ... existing variants ...
  | {
      kind: 'diagram-diff';
      diagram: 'bean-graph' | 'folder-map'; // first impl supports one
      from: string;          // git ref (HEAD~5, branch name, tag)
      to?: string | null;    // git ref or null = working tree
      mode?: 'before' | 'after' | 'morph' | 'changed-only'; // default: 'after'
    };
```

```rust
// crates/core/src/walkthrough.rs (illustrative)
WalkthroughTarget::DiagramDiff {
    diagram: String, // "bean-graph" | "folder-map" — validated server-side
    from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<DiagramDiffMode>,
}
```

The `mode` field lets the LLM author a tour that opens directly in
"morph" or "changed only" — useful for "watch the architecture change"
narration. Tours that omit it pick the default ("after with pulses").

## Reuse audit

| Piece | Existing component / data | Status |
|---|---|---|
| Per-ref diagram payload | `show_diagram` already accepts a `kind`; needs a second arg `at_ref` so callers can ask for "bean-graph at HEAD~5" | **Add** — small backend change, ~30 LOC + 1 tree-walking helper |
| Changed file set | `git::list_changes_since(from, to)` shipped with #63 | **Reuse** as-is |
| Changed *node* set (FQN-level) | Walking the per-ref repo and intersecting class FQNs | **Add** — 50 LOC in `core/diagram.rs` |
| Animated graph classes | Mermaid renders static SVG every frame; Cytoscape supports per-element class transitions | **Migrate** — bean-graph moves to Cytoscape (also unblocks the dependency wheel from the architecture-maps eval) |
| Pulse animation | `.line.pulse` keyframes in `DiffView.svelte` (#126) | **Lift** to a shared `app/src/lib/pulse.css` so both viewers share one keyframe definition |
| Toggle bar UX | `DiagramView.svelte` already has a layout switcher next to the diagram kind selector | **Mirror** — same component shape, four buttons instead of three |

## Implementation phases

The full feature is M-L; splitting into three PRs keeps each one
shippable on its own and avoids a giant Cytoscape migration in the
same change as the new diff endpoint.

### Phase 1 — `show_diagram` accepts an `at_ref` (S)

Backend only. `show_diagram(type, at_ref?)` walks the repo at the
requested ref into a temporary `Repository`, runs the existing
diagram renderers on it, returns the same payload shape. No frontend
work.

Tests: round-trip a known commit with `at_ref=HEAD~1` against a
two-commit fixture; confirm the diagram payload differs from
`at_ref=HEAD`.

### Phase 2 — Cytoscape migration for `bean-graph` (M)

Frontend only. Replace the Mermaid `flowchart` output with a
Cytoscape graph that consumes the same node/edge data. No behavioural
change for the user; the only observable difference is that the
diagram now supports per-element class toggles for animations.

This unblocks both this issue *and* the dependency wheel from the
architecture-maps evaluation (#62).

### Phase 3 — `diagram-diff` walk-through target (M)

The actual feature. New `WalkthroughTarget::DiagramDiff` variant,
`<DiagramDiffView>` Svelte component that:

1. Calls `show_diagram(kind, at_ref=from)` and
   `show_diagram(kind, at_ref=to)` in parallel.
2. Merges the node/edge sets, tags each item as `added`, `removed`,
   `unchanged`.
3. Renders the merged graph with mode-aware classes:
   `cy.elements('.added').addClass('pulse')`, etc.
4. Wires the toggle bar to `cy.elements()` class toggles.

Estimated frontend LOC: ~350 (component + helper that derives the
tagged set + 2-3 vitest specs for the merge math).

## Trade-offs

- ✅ Single feature, but each phase ships independently — a slipped
  Cytoscape migration doesn't block the `at_ref` endpoint and vice
  versa.
- ✅ Reuses the existing `list_changes_since` and the #126 pulse
  vocabulary.
- ✅ Cytoscape migration unblocks **two** roadmap items (#62
  dependency wheel + #125 before/after).
- ⚠️  Cytoscape adds ~150 KB minified; current bundle is ~1.1 MB so
   we land at ~1.25 MB. Acceptable; well below the 1.7 MB threshold
   the architecture-maps eval (#128) flagged for Code-City.
- ⚠️  Per-ref repo parsing has to walk the full tree once per request.
   Default expectation is sub-second on a 50k-LOC repo; cap with a
   `max_files` similar to `file_recency` if it surprises us.
- ❌ Folder-map is a JSON-rendered SVG today, not a graph; the
  morph/animation modes don't apply directly. First impl ships
  bean-graph only and folder-map gets a follow-up issue with a
  different visual language (e.g. modules pulse, files don't).

## Acceptance criteria mapping

The issue body (#125) lists four:

- ✅ First implementation can support one diagram kind, ideally
  bean-graph or folder-map → bean-graph (Phase 3).
- ✅ Large graphs remain readable via changed-only mode → toggle
  fades unchanged to 30%.
- ✅ Old tour payloads are unaffected → `WalkthroughTarget` enum
  growth is additive.
- ➕ (extra) Cytoscape migration is reusable for the dependency
  wheel (#62 follow-up).

## Out of scope for this sketch

- **Folder-map diff overlay**. Already shipped as a *static* layer in
  PR #129 (different concept — colours files by changed/unchanged
  rather than animating between two states).
- **Diff cinematics** ("press play and watch the architecture morph"
  across a range of commits). That's the #66 wow-factor concept;
  this sketch parks the building block (`at_ref` endpoint) that
  cinematics will reuse.
- **Cross-repo before/after**. We only support intra-repo refs.

## Recommendation

Promote to an implementation issue split into the three phases. The
phase boundaries are deliberately small enough that each could be a
standalone PR a reviewer can read in one sitting.

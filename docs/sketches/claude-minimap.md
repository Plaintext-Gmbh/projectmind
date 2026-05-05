# Mini-map (Cities-Skylines style) — sketch

> Concept: a constantly-visible mini-map of whatever the active visualisation is showing.
> Concept owner: ProjectMind core. Question this answers: *"where am I in the bigger
> picture?"* — keeps spatial orientation when the user has zoomed deep into a folder
> map, bean graph, or doc graph.
> Candidate library: none. Pure SVG, ~150 lines of Svelte.

## What the user sees

A small fixed-position card in the bottom-right of the Diagrams stage:

```
┌─────────────────────────── Diagrams ──────────────────────────┐
│  ┌────────────────────────────────────────────────────────┐   │
│  │                                                        │   │
│  │   .  .  .  .  .  .   ┌─ src ─┐  .  .  .  .  .  .       │   │
│  │   .  .  .  .  .  .   │  ○ ●  │  .  .  .  .  .  .       │   │
│  │   .  .  ┌─ test ─┐    │ ○●○●  │  .  .  .  .  .          │   │
│  │   .  .  │  ●  ●  │    │ ●●●○  │  .  .  .  .  .          │   │
│  │   .  .  └────────┘    └───────┘  .  .  .  .  .          │   │
│  │                                                        │   │
│  └────────────────────────────────────────────────────────┘   │
│                                                                │
│                                              ┌─────────────┐  │
│                                              │  ░░██░░░░░  │  │
│                                              │  ░░██░░░░░  │  │
│                                              │  ░██████░░  │  │   ← mini-map
│                                              │  ░░░██░░░░  │  │     viewport
│                                              │             │  │     rectangle
│                                              │  ┌─░──░─┐   │  │     marks the
│                                              │  │ ▓▓▓▓ │   │  │     visible
│                                              │  │ ▓▓▓▓ │   │  │     region
│                                              │  └──────┘   │  │
│                                              └─────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

- **Mini-map** = SVG snapshot of the current diagram, scaled to ~12% of the stage.
- **Viewport rectangle** overlays the portion of the diagram currently visible at the
  active zoom + pan.
- **Drag** the rectangle to pan; **click** outside it to centre on that point;
  **scroll wheel over the mini-map** to zoom the main view (preserving cursor focus).

The card collapses to a tiny "▢ map" pill when the user dismisses it; preference stored
in `localStorage` as `projectmind.diagram.miniMap.visible`.

## What question does it answer?

The folder map and bean graph already let the user pan/zoom freely. At >2× zoom they
lose orientation — *"is this the auth corner of the codebase or the analytics corner?"*
The mini-map answers that without forcing a reset-zoom round-trip.

The recency / author overlays (#63) make this even more useful: the mini-map keeps the
hotspot pattern visible while the user drills into a single hot file.

## Why pure SVG, no library

The active diagram is already an SVG node in the DOM (`stage > svg`). To render the
mini-map we:

1. Clone the SVG node (cheap — Svelte already renders it once).
2. Strip interactivity (`pointer-events: none`, no event handlers).
3. Apply a fixed-size CSS transform (`width: 192px; height: 144px`) and let the browser
   re-rasterise.
4. Overlay one `<rect>` for the viewport, computed from `(scale, tx, ty, baseW, baseH)`
   already exported by `<DiagramView>`.

Total cost: zero new bundle weight, no `requestAnimationFrame`, just one extra DOM
subtree per active diagram.

## Implementation outline

- Add `<DiagramMiniMap>` Svelte component to `app/src/components/`. Props: the cloned
  SVG element + the viewport state (scale / tx / ty / baseW / baseH).
- Hoist the viewport state out of `<DiagramView>` into the existing diagram store
  (`app/src/lib/store.ts`) so the mini-map and the main stage stay in sync without
  prop-drilling.
- Add a "minimap" toggle button in the Diagrams toolbar (next to the colorBy buttons).
  Persist in `localStorage` per the existing `projectmind.diagram.*` namespace.

Estimate: ~150 LOC + 1 vitest spec for the viewport-rect math (the only piece with
non-trivial logic — the rest is pass-through rendering).

## Trade-offs

- ✅ Fits inside the existing render pipeline. No new diagram-kind plumbing, no MCP
  surface change.
- ✅ Solves a real navigation pain at zoom >2×.
- ⚠️  At very high zoom the viewport rectangle becomes a single pixel; needs a minimum-
  size clamp (e.g. always at least 8×8 px) so the user can still grab it.
- ⚠️  Cloning the live SVG every render flickers if we're not careful — debounce via
  the same `tick()` the main stage uses, then double-buffer the mini-map's `<g>`.
- ❌ Doesn't help for `doc-graph` view in `network` layout (force-directed never settles
  on a stable layout); skip the mini-map there or render only the static skeleton.

## Out of scope for this sketch

- The Cities-Skylines-style "fog of war" effect (un-visited regions dimmed). Cute but
  needs tracking which nodes the user has actually opened — separate concept.
- Touch / pinch interactions. Desktop-first ships first.

## Recommendation

Promote to a real implementation issue when:

1. Either #63's diff overlay or the C4 work from #62 lands a diagram bigger than the
   current bean graph (i.e. there is a real "I'm lost in this view" pain to solve).
2. The viewport state in `<DiagramView>` is already store-backed — currently it's
   component-local and lifting it is a small refactor that should pre-date this
   feature, not co-land with it.

Until then, this sketch parks the concept under #66 with enough detail that anyone
picking it up doesn't have to rediscover the rendering trick.

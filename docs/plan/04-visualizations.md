# Visualization Catalogue & Sketches

> **Status:** 2026-04-28 — open catalogue. Goal: collect visualization concepts, candidate libraries, and sketches that we want to land in `plaintext-ide` over time. Submissions welcome — drop a sketch in this folder as `04-viz-<short-name>.md`.

## North Star

> The user opens `plaintext-ide`, sees a map of their software they've never seen this clearly before, and *enjoys* using it. Every visualization should pull its weight in **clarity, density, or delight**.

Gamification is **low priority** but allowed where it doesn't get in the way (XP for documenting modules, badges for first contributors per area, etc.).

## Visualization Categories

### 1. Architecture Maps

| Concept | Description | Candidate libraries |
|---|---|---|
| **Force-directed bean graph** | Springs and gravity, Spring stereotypes coloured | [Cytoscape.js](https://js.cytoscape.org/), [d3-force](https://d3js.org/d3-force), [sigma.js](https://www.sigmajs.org/) |
| **Hierarchical package tree** | Module → package → class — Sunburst, Treemap or Icicle | [d3-hierarchy](https://d3js.org/d3-hierarchy), [Recharts](https://recharts.org/), [visx](https://airbnb.io/visx/) |
| **Code-City** | 3D city: classes as buildings (height = LOC, footprint = methods), packages as districts | [three.js](https://threejs.org/), [react-three-fiber](https://docs.pmnd.rs/react-three-fiber) |
| **Dependency wheel** | Circular layout, chord diagram for dependencies | [d3-chord](https://d3js.org/d3-chord) |
| **C4 diagrams** (Context / Container / Component / Code) | Generated from code annotations | [Structurizr DSL](https://structurizr.com/), [PlantUML C4](https://github.com/plantuml-stdlib/C4-PlantUML) |

### 2. Change Maps (delta visualisation)

| Concept | Description | Notes |
|---|---|---|
| **Heatmap by recency** | Treemap whose colour intensity is "how recently this file changed" | Pull from `git log`; great for "where is the team active?" |
| **Diff overlay on bean graph** | Existing graph, edges/nodes touched by latest commit pulse | Mermaid is too static — d3 with transitions wins here |
| **Timeline river** | Horizontal time axis, each module a band; commits as drops in their band | [d3-time](https://d3js.org/d3-time) |
| **Author overlay** | Same as heatmap but coloured by primary author | Honest about ownership |

### 3. Code-Level Maps

| Concept | Description | Notes |
|---|---|---|
| **Call graph (per method)** | Open a method, see who calls it and what it calls | Tree-sitter for Java, recursive walk |
| **Annotated source** | Source file with badge column showing stereotypes, doc links, annotations | Custom Svelte component |
| **Inheritance tree** | Class hierarchy, interactive expand | [d3-tree](https://d3js.org/d3-hierarchy/tree) |
| **DTO map** | DTOs and their fields, with arrows to where they're produced/consumed | Useful for refactoring boundaries |

### 4. Documentation Maps

| Concept | Description | Notes |
|---|---|---|
| **Markdown reader** | With Mermaid + draw.io rendering inline | Required for Phase 1 |
| **Code ↔ doc bridge** | Lines in code with a side-bar that shows linked Confluence/Jira pages | Confluence MCP bridge |
| **Doc graph** | Nodes = docs (and sections), edges = links — see what's connected | Cytoscape |

### 5. "Wow factor" / experimental

These are the ones we want to surprise users with. Sketch first, discuss, decide which to land.

| Concept | Idea |
|---|---|
| **Living architecture** | Real-time animation of incoming requests / events through the bean graph |
| **Mini-map** | A constantly visible mini-map of the active visualization, like Cities: Skylines |
| **First-person flythrough** | In Code-City: walk through your codebase as a 3D space |
| **Auto-narrated tour** | "Welcome to this repo — here are the 5 most important modules." (LLM-driven) |
| **Diff cinematics** | "Press play" on a range of commits, watch the architecture morph |

## Candidate Library Stack

| Concern | Top pick | Backup |
|---|---|---|
| 2D graphs | **Cytoscape.js** | sigma.js, d3-force |
| Hierarchies | **d3-hierarchy** | visx |
| Charts | **visx** or **Recharts** | Chart.js |
| 3D | **three.js** (+ Threlte for Svelte) | Babylon.js |
| Static diagrams | **Mermaid** | Vega-Lite |
| Markdown | **unified / remark** | marked |
| draw.io | embedded iframe | n/a |
| Code highlighting | **Shiki** (read-only) | Monaco |

The plugin API allows **multiple visualizer plugins per data shape** — so a `bean-graph` payload can be rendered by a Cytoscape plugin, a d3-force plugin, and a Code-City plugin, and the user picks per session.

## Visual Identity (rough)

- **Density first** — show a lot, but with hierarchy of attention (one strong colour, three accents)
- **Animations are honest** — they show *something happening* (incoming change, new node), never decoration
- **Keyboard-first** — every navigation has a shortcut; mouse is for hovering / pinning
- **Minimap + breadcrumbs** so the user never gets lost
- **Themes** — light, dark, plus a "presentation" mode (high contrast, larger labels) when the user wants to share the screen

## Process for New Sketches

1. Drop a Markdown sketch in `docs/plan/sketches/<your-name>-<topic>.md`
2. Include: concept, what it answers, candidate library, rough mock (ASCII / image / Mermaid)
3. Open a [Discussion](https://github.com/daniel-marthaler/plaintext-ide/discussions) linking the sketch
4. Promising sketches graduate to `docs/plan/04-viz-<short-name>.md` with a more detailed design
5. Adopted sketches become a `viz-*` plugin in `plugins/`

## Sources

- [Cytoscape.js](https://js.cytoscape.org/)
- [d3.js](https://d3js.org/)
- [three.js](https://threejs.org/)
- [Mermaid](https://mermaid.js.org/)
- [Code City research / Wettel & Lanza](https://www.inf.usi.ch/lanza/Downloads/Wett2008a.pdf)
- [Structurizr / Simon Brown C4 model](https://c4model.com/)

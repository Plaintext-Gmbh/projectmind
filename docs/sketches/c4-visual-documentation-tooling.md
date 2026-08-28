# C4 visual documentation tooling — evaluation & sketch

> Idea: add a stronger visual documentation path for ProjectMind by supporting C4
> model creation and editing. The goal is not only to render generated diagrams, but
> to evaluate which graphics tools make sense for creating maintainable C4 models.
> draw.io / diagrams.net integration is a first-class candidate because ProjectMind
> already has a `.drawio` viewer surface.

## Status (August 2026) — evaluation concluded

The layered workflow below is implemented; this section records the outcome per
candidate so the table underneath reads as history, not as an open question.

| Candidate | Verdict | What shipped / why not |
|---|---|---|
| **Structurizr DSL** | ✅ **adopted** (subset, Rust-native) | `docs/architecture.dsl` is the canonical, versioned model: `scaffold_c4_model` writes it once, `parse_c4_dsl` reads it back, `merge_c4_model` folds new code structure in additively. No JVM, no Structurizr CLI — the parser/renderer is `core::c4_dsl` (PRs #223–#225). |
| **Mermaid C4** | ✅ **adopted** for generated views | `c4-container` (generated), `c4-model` (the DSL rendered back), `c4-component` (container zoom, PR #226). Exactly the "generated output target" role this sketch proposed. |
| **draw.io / diagrams.net** | ✅ **adopted** — Phase A, B **and** C | A: `.drawio` files render in the embedded viewer. B: the viewer's **Edit** mode is the full embedded editor with save-back into the repo (`save_drawio`). C: `export_c4_drawio` writes `docs/architecture.drawio` from the model with draw.io's C4 shapes. The semantic-drift risk named below is handled exactly as proposed: every cell carries `pmId` / `pmKind` metadata, so a re-export is a layout-preserving merge that never destroys human layout (`core::c4_drawio`). |
| **LikeC4** | ❌ **evaluated, not adopted** | Its strengths — a repo-local DSL, a merged model across files, embeddable web views — are the capabilities the Rust-native Structurizr subset already covers for ProjectMind's needs, and its cost is real: a Node/TypeScript toolchain in the Rust analysis path plus a *second* canonical model format competing with `architecture.dsl`. A LikeC4 *export* would be cheap but would only add a format nobody asked for. Revisit if a multi-repo architecture model or LikeC4's view DSL becomes a concrete requirement. |
| **D2** | ❌ not adopted | Attractive rendering, but a general-purpose diagram language; Mermaid already fills the generated-preview role and draw.io the polished-output role. |
| **PlantUML + C4-PlantUML** | ❌ not adopted | Would add a second rendering runtime and overlaps Structurizr; no ProjectMind repo currently runs a PlantUML pipeline. |
| **Ilograph / IcePanel / Lucid / Miro** | ⏸ deferred | SaaS workshop tools stay outside the repo-first model. |

Remaining ideas that were *not* part of #142 and stay open as separate topics:
a self-hosted diagrams.net build for repos that must not send diagram XML to
`embed.diagrams.net`, and per-relationship `c4Technology` labels in the DSL.

## TL;DR

| Tooling path | Recommendation | Best fit | Main trade-off |
|---|---|---|---|
| **Structurizr DSL** | **Use as canonical model candidate** | Versioned C4 model-as-code, workspace export, repeatable generation | Requires a new import/render path and likely a JVM or CLI workflow |
| **draw.io / diagrams.net** | **Use as manual editing and review candidate** | Human-edited diagrams, stakeholder-friendly visual polish, existing `.drawio` integration | XML diagrams are harder to keep semantically aligned with code |
| **Mermaid C4** | **Use for generated lightweight views** | Fast in-app previews from repository data, no new renderer | C4 support is weaker and less stable than dedicated C4 tools |
| **LikeC4** | **Evaluate as modern DSL candidate** | Repo-local architecture DSL, model API, embeddable modern web output | Adds a TypeScript toolchain dependency and is less established than Structurizr |
| **D2** | **Evaluate as export target** | Clean text diagrams, good visual output, possible Structurizr export path | General diagram language, not a canonical C4 model by itself |
| **PlantUML + C4-PlantUML** | **Evaluate only if teams already use PlantUML** | Text diagrams in existing PlantUML documentation pipelines | Adds a separate rendering runtime and overlaps Structurizr |
| **Ilograph / IcePanel / Lucid / Miro** | **Defer for Phase 1** | Collaborative SaaS modelling workshops | Harder to integrate locally and less aligned with ProjectMind's repo-first model |

## Product Shape

ProjectMind should treat C4 documentation as a layered workflow:

1. **Generated baseline** — ProjectMind derives a C4 Container or Component view from
   repository metadata, modules, classes, and dependency relations.
2. **Editable architecture model** — the team refines missing context, actors, external
   systems, and intentional boundaries in a maintained source format.
3. **Visual review surface** — ProjectMind renders the model alongside code and docs,
   with optional hand-edited diagrams for meetings, architecture reviews, or onboarding.

This keeps generated views honest while still allowing architects to document the
information that source code cannot infer: users, external systems, deployment context,
trust boundaries, and intentional design decisions.

## Evaluation Criteria

The right C4 tooling for ProjectMind should score well on:

- **Repo compatibility** — can the model live in the repository and be reviewed in Git?
- **Semantic model quality** — does the tool understand C4 concepts or only draw boxes?
- **Round-trip potential** — can ProjectMind generate a draft and preserve user edits?
- **Offline/local viability** — does it work in a Tauri desktop app without depending
  on a SaaS service?
- **Visual quality** — can the output be shown to humans without extensive cleanup?
- **Integration cost** — does it fit the current Mermaid / Markdown / `.drawio` surfaces?

## Candidate Tools

### Structurizr DSL

Structurizr is the strongest candidate for the canonical C4 model because it stores
software architecture as a text workspace rather than as a drawing. That gives
ProjectMind a practical path to:

- generate an initial `workspace.dsl` from repository structure;
- keep actors, external systems, containers, components, and relationships explicit;
- version C4 changes through normal code review;
- export or render diagrams later through Structurizr tooling.

The main cost is runtime and workflow integration. A pure in-app preview would need
either a compatible parser/renderer path or a CLI bridge. This is still worth evaluating
because the model quality is much higher than freeform drawing.

### draw.io / diagrams.net

draw.io is the best candidate for manual visual editing. ProjectMind already renders
`.drawio` files through `DrawIoView.svelte`, and diagrams.net supports an iframe embed
protocol with `postMessage`-based communication. That makes two integration levels
realistic:

- **Phase A: view existing `.drawio` files** — already aligned with the current app
  shape. Architecture documents can link to hand-authored C4 diagrams.
- **Phase B: open/edit `.drawio` files** — ProjectMind can pass XML into the embedded
  diagrams.net editor and receive changed XML back, subject to security and privacy
  constraints.
- **Phase C: generated draft export** — ProjectMind can create a starter `.drawio`
  diagram from the repository model, then let humans refine it.

The risk is semantic drift. A `.drawio` file knows shapes and connectors, but it does not
necessarily know that a box is a C4 Container or that an arrow is a specific relationship
type unless ProjectMind imposes conventions in metadata. If draw.io becomes the editing
surface, ProjectMind should store C4 element IDs in shape metadata so future generation
can update the diagram without destroying human layout.

### Mermaid C4

Mermaid remains the cheapest generated preview path because ProjectMind already renders
Mermaid diagrams. It is useful for quick C4 Context / Container / Component diagrams,
especially in Markdown and the Diagrams tab.

Mermaid should not be the only long-term C4 model format. It is best treated as a
generated output target, not as the canonical architecture model, because it is
diagram-source text rather than a complete architecture workspace.

### LikeC4

LikeC4 is worth evaluating because it sits close to ProjectMind's product shape: a
repo-local DSL, a merged architecture model across multiple files, and tooling APIs that
can be consumed from a TypeScript frontend/backend workflow. It also has a more modern
web-output story than PlantUML-style renderers.

This could be a strong fit if ProjectMind wants a C4-aware model that is easier to embed
than Structurizr. The trade-off is maturity and ecosystem risk: Structurizr remains the
reference point for C4 modelling, while LikeC4 would introduce another Node-side toolchain
into an app that currently keeps most repository analysis in Rust.

### D2

D2 is a good export candidate, not a canonical ProjectMind model candidate. It is useful
when the goal is attractive generated diagrams from text, and recent D2 C4 support makes
it relevant for ProjectMind's C4 output pipeline.

The strongest use would be:

- Structurizr or ProjectMind model -> D2 export -> SVG preview;
- generated C4-ish documentation snippets in Markdown;
- comparison point for Mermaid visual quality.

It should not replace a semantic C4 model because D2 remains a general-purpose diagram
language.

### PlantUML + C4-PlantUML

C4-PlantUML is a credible option for teams already invested in PlantUML. It gives
text-based diagrams and established documentation workflows, but ProjectMind would need
to add a renderer path that it does not currently need for other diagrams.

Because Structurizr DSL is closer to a real model and Mermaid is already available for
lightweight rendering, PlantUML should be evaluated only when a target user repository
already contains PlantUML documentation.

### SaaS Modelling Tools

Tools such as Ilograph, IcePanel, Lucid, and Miro can be useful for collaborative
workshops and stakeholder-facing diagrams, but they are less attractive as ProjectMind's
first C4 integration target:

- they push the source of truth out of the repository;
- they require account, auth, or export workflows;
- they are harder to make reliable inside a local-first desktop app.

They should remain reference points for visual quality and collaboration patterns, not
initial implementation dependencies.

## ProjectMind Fit Matrix

| Capability we need | Best candidate | Why |
|---|---|---|
| In-app generated C4 preview | Mermaid C4 first, D2 later | Mermaid already ships; D2 can be a higher-quality export experiment |
| Canonical C4 model in Git | Structurizr DSL, LikeC4 as challenger | Both are repo-friendly and semantic; Structurizr is more established |
| Human editing surface | draw.io / diagrams.net | Existing `.drawio` viewer and familiar manual editor |
| Round-trip generated-to-edited diagrams | draw.io with stable metadata, or Structurizr layout files | Needs stable element IDs and preservation of manual layout |
| Architecture documentation bundle | Structurizr Lite/static site, ProjectMind Markdown integration | Structurizr handles model/docs/ADRs; ProjectMind can link code/docs/diagrams |
| User repos with existing diagram-as-code | PlantUML, Mermaid, D2 discovery | Detect and render what the repo already uses instead of forcing migration |
| Collaborative workshops | SaaS tools as import/export references only | Useful for teams, but not ideal as a local-first dependency |

## Integration Options In Our Context

ProjectMind already has useful primitives:

- Mermaid rendering in `DiagramView.svelte`;
- `.drawio` file display via `DrawIoView.svelte`;
- Markdown documentation indexing through the doc graph work;
- repository/module/class/relation data in Rust core;
- `.projectmind/annotations.json` as a plausible place for inferred-vs-curated
  architecture metadata.

That means the lowest-risk implementation path is additive:

1. **Discovery first** — scan repos for C4-adjacent files:
   `workspace.dsl`, `*.likec4`, `*.c4`, `*.drawio`, `*.mmd`, `*.puml`, `*.d2`.
2. **Generated preview second** — create a ProjectMind-generated C4 Container view from
   existing module and relation data, rendered as Mermaid initially.
3. **Editable export third** — export the same model to `.drawio` with stable element
   IDs, then render/edit it through the existing draw.io surface.
4. **Canonical model prototype fourth** — compare Structurizr DSL and LikeC4 on one real
   repository and decide which model format ProjectMind should recommend.

## Proposed ProjectMind Workflow

1. Add a **C4 model discovery** pass:
   - detect `workspace.dsl`, `.drawio`, `.mmd`, `.puml`, and C4-labelled Markdown;
   - list discovered architecture models in the existing diagram/document navigation.
2. Add a **generated C4 Container draft**:
   - map Maven modules, Cargo crates, packages, or top-level services to C4 Containers;
   - derive relations from existing ProjectMind relation data;
   - mark inferred elements clearly in the UI.
3. Add a **draw.io export target**:
   - generate a `.drawio` draft with stable C4 element IDs in shape metadata;
   - preserve human layout where IDs match existing shapes;
   - keep the exported file inside the repository.
4. Evaluate **Structurizr DSL as canonical source**:
   - prototype import of a small `workspace.dsl`;
   - compare generated Mermaid and draw.io outputs from the same model;
   - decide whether ProjectMind stores C4 annotations in `.projectmind/annotations.json`
     or directly in Structurizr DSL.

## Open Questions

- Should ProjectMind own a `.projectmind/c4.json` model, or should it adopt
  Structurizr DSL as the model source?
- Can draw.io shape metadata carry stable C4 IDs reliably enough for round-tripping?
- Should generated diagrams overwrite files, create drafts, or open as unsaved previews?
- How should private repositories handle the hosted `embed.diagrams.net` iframe path?
  The current viewer already notes that sensitive diagrams may require a self-hosted
  draw.io viewer or local conversion.

## Recommendation

Use a two-track strategy:

1. **Canonical model track:** evaluate Structurizr DSL with a minimal import/export
   prototype. This is the most credible path for maintainable C4 documentation.
2. **Visual editing track:** deepen draw.io integration because it fits the existing
   `.drawio` viewer and gives teams a practical manual editing surface.

Mermaid C4 should remain the quick generated preview path. PlantUML and SaaS tools are
secondary candidates unless a real target repository already depends on them.

## References

- [C4 model — official site](https://c4model.com/)
- [Structurizr documentation](https://docs.structurizr.com/)
- [diagrams.net embed mode](https://www.drawio.com/doc/faq/embed-mode)
- [diagrams.net integrations](https://www.drawio.com/integrations)
- [Mermaid C4 syntax](https://mermaid.js.org/syntax/c4.html)
- [C4-PlantUML](https://github.com/plantuml-stdlib/C4-PlantUML)

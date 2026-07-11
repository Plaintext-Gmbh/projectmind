// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Editable C4 model as a Structurizr-DSL subset — the JVM-free half of
//! [#142](https://github.com/Plaintext-Gmbh/projectmind/issues/142).
//!
//! The generated architecture (Mermaid `c4-container`) is derived fresh from
//! repository data on every render, so an architect cannot change it. This
//! module closes that loop with an *editable* model:
//!
//! 1. [`generate_c4_dsl`] emits a Structurizr-DSL subset from the same data as
//!    [`crate::diagram::render_c4_container`] (modules → containers, cross-module
//!    relations → relationships, a `developer` person on the busiest module).
//!    Each container also lists the module's **top-3 components** — its most
//!    important classes — so the model carries the C4 component level too (see
//!    [`generate_c4_dsl`] for the importance heuristic and the deliberate cap).
//! 2. [`scaffold_c4_model`] writes that DSL to `docs/architecture.dsl` **once**
//!    — it never clobbers an existing file, so after the first scaffold the file
//!    is owned by the user and versioned in Git.
//! 3. [`parse_c4_dsl`] reads the (possibly hand-edited) DSL back into a
//!    [`C4Model`], and [`c4_model_to_mermaid`] renders that model as the exact
//!    same Mermaid `C4Container` text the frontend already knows how to draw.
//! 4. [`merge_c4_dsl`] closes the round-trip loop of
//!    [#142](https://github.com/Plaintext-Gmbh/projectmind/issues/142): it takes
//!    the *existing* (possibly hand-edited) DSL and the *current* code structure
//!    and folds in **only what the code has that the DSL is missing** — new
//!    containers (modules), new components (top classes), new cross-module
//!    relationships. It is strictly **additive**: nothing that already exists in
//!    the file is deleted or altered — descriptions the user rewrote, external
//!    systems / actors / relationships they added by hand, and comments all
//!    survive **byte-identically** (see [`merge_c4_dsl`] for the exact
//!    preservation guarantee and why text-insertion, not re-emission, is used).
//!
//! Round-trip honesty: the scaffold generates the DSL **once**; from then on the
//! file is the source of truth and ProjectMind never *rewrites* it. When the
//! architect explicitly asks to pull new structure in (the `merge_c4_model` tool
//! / "Update model" button), [`merge_c4_dsl`] adds the missing elements without
//! touching anything the user wrote. To regenerate from scratch instead, delete
//! the file and scaffold again.
//!
//! The parser is intentionally tolerant and line-based: it understands the
//! subset this module emits (`person`, `softwareSystem`, `container`,
//! `component`, and `<id> -> <id> "desc"` relationships inside nested `{}`
//! blocks) and silently ignores anything it does not recognise — comments
//! (`#`, `//`), blank lines, and unknown Structurizr constructs. It never
//! panics on malformed input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use projectmind_plugin_api::FrameworkPlugin;
use serde::{Deserialize, Serialize};

use crate::Repository;

/// A parsed C4 model: the persons, the software systems (each with its
/// containers and components) and the relationships between elements.
///
/// This mirrors the small slice of the Structurizr metamodel that
/// [`generate_c4_dsl`] emits and [`parse_c4_dsl`] understands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4Model {
    /// People that interact with the systems (typically just `developer`).
    pub persons: Vec<C4Person>,
    /// Software systems, each holding its containers.
    pub systems: Vec<C4System>,
    /// Relationships between any two elements, by id.
    pub relationships: Vec<C4Relationship>,
}

/// A person / actor in the model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4Person {
    /// Stable identifier used on the left of relationships.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Free-form description.
    pub description: String,
}

/// A software system, the outer boundary of the container view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4System {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Free-form description.
    pub description: String,
    /// Containers inside this system.
    pub containers: Vec<C4Container>,
}

/// A container (a module, in ProjectMind's mapping).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4Container {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Free-form description (e.g. `"12 classes"`).
    pub description: String,
    /// Components inside this container (unused by the generator, but parsed
    /// so hand-edited component blocks survive the round-trip).
    pub components: Vec<C4Component>,
}

/// A component inside a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4Component {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Free-form description.
    pub description: String,
}

/// A directed relationship between two elements, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct C4Relationship {
    /// Source element id.
    pub from: String,
    /// Target element id.
    pub to: String,
    /// Relationship description (e.g. `"uses"`, `"explores"`).
    pub description: String,
}

/// Result of a scaffold attempt: where the file is and whether we created it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaffoldResult {
    /// Absolute path to `docs/architecture.dsl`.
    pub path: PathBuf,
    /// `true` if this call wrote the file, `false` if it already existed and
    /// was left untouched (no clobber).
    pub created: bool,
}

/// Result of a [`merge_c4_model`] round-trip: where the file is, whether this
/// call had to create it from scratch (the file did not exist yet), and how many
/// new elements were folded in from the current code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeModelResult {
    /// Absolute path to `docs/architecture.dsl`.
    pub path: PathBuf,
    /// `true` if the file did not exist and was scaffolded fresh (like
    /// [`scaffold_c4_model`]); `false` if it existed and was merged in place.
    pub created: bool,
    /// New `container` blocks added from the code (0 when created fresh).
    pub added_containers: usize,
    /// New `component` lines added from the code (0 when created fresh).
    pub added_components: usize,
    /// New cross-module `relationship` lines added (0 when created fresh).
    pub added_relationships: usize,
}

/// Repo-relative location of the editable C4 model.
pub const C4_MODEL_REL_PATH: &str = "docs/architecture.dsl";

/// Sentinel the `c4-model` diagram returns when `docs/architecture.dsl` does
/// not exist yet. The frontend matches on this exact string to show the
/// "scaffold the C4 model" empty state instead of trying to render it.
pub const C4_MODEL_ABSENT: &str = "%% c4-model: no docs/architecture.dsl";

/// Render the editable C4 model as Mermaid `C4Container` text.
///
/// Reads `docs/architecture.dsl` from `repo`; if it exists, the file is parsed
/// with [`parse_c4_dsl`] and rendered with [`c4_model_to_mermaid`]. If it does
/// not exist — or cannot be read — the [`C4_MODEL_ABSENT`] sentinel is
/// returned so the frontend can offer to scaffold it. ProjectMind never
/// derives this from repo data at render time: the file *is* the model, so a
/// hand-edited `.dsl` renders exactly as written.
#[must_use]
pub fn render_c4_model(repo: &Repository) -> String {
    let path = c4_model_path(repo);
    match std::fs::read_to_string(&path) {
        Ok(text) => c4_model_to_mermaid(&parse_c4_dsl(&text)),
        Err(_) => C4_MODEL_ABSENT.to_string(),
    }
}

/// Resolve the absolute path to the editable C4 model for `repo`.
#[must_use]
pub fn c4_model_path(repo: &Repository) -> PathBuf {
    repo.root.join(C4_MODEL_REL_PATH)
}

/// Number of components emitted per container. Deliberately small: at ~19
/// modules a higher cap makes the combined container+component view unreadable,
/// and the model is *editable* — the architect can add more `component` lines
/// to any `container` block by hand and ProjectMind never overwrites them.
pub const COMPONENTS_PER_CONTAINER: usize = 3;

/// Generate a Structurizr-DSL subset describing `repo`'s container view.
///
/// The mapping is identical to [`crate::diagram::render_c4_container`]: one
/// `container` per module, cross-module framework relations become
/// relationships, and a `developer` person is anchored on the busiest module
/// (highest cross-module in-degree). Output is deterministic — modules and
/// relationships are sorted — so regenerating an unchanged repo yields byte-
/// identical DSL.
///
/// Each `container` block additionally carries the module's top
/// [`COMPONENTS_PER_CONTAINER`] **components** — its most important classes — so
/// the model reaches the C4 component level. Importance is the class's
/// **fan-in**: how many *distinct* other classes reference it through the
/// framework relations graph (`framework.relations`). This is exactly the
/// `deps` signal [`crate::risk::compute`] uses, computed directly here because
/// `risk::compute` additionally needs a live git repo (churn) and a coverage
/// report and is far too heavy for a pure DSL generation. Ties — including the
/// common all-zero case for a repo with no relations — are broken by
/// **stereotype priority** (`rest-controller` > `controller` > `service` >
/// `repository` > `configuration` > `component` > anything else), then by class
/// name, so the output is fully deterministic. The component `description` is
/// the chosen stereotype (or `"class"` when the class has none).
#[must_use]
pub fn generate_c4_dsl(repo: &Repository, framework: &dyn FrameworkPlugin) -> String {
    let title = repo
        .root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repository");
    let system_id = dsl_id(title);

    let mut out = String::new();
    out.push_str("# ProjectMind C4 model (Structurizr DSL subset).\n");
    out.push_str("# Generated once by `scaffold_c4_model`; this file is now yours to edit.\n");
    out.push_str("# ProjectMind never overwrites it. Render it via the `c4-model` diagram.\n");
    out.push_str("# See https://github.com/Plaintext-Gmbh/projectmind/issues/142\n");
    out.push_str("workspace {\n");
    out.push_str("    model {\n");
    let _ = writeln!(
        out,
        "        developer = person \"Developer\" \"Browses architecture via ProjectMind\""
    );
    let _ = writeln!(
        out,
        "        {system_id} = softwareSystem \"{}\" \"Architecture of the opened repository\" {{",
        dsl_str(title)
    );

    // Per-class fan-in across the whole repo, from the framework relations
    // graph — the importance signal for component selection.
    let fan_in = fan_in_by_fqn(repo, framework);

    // Stable order — BTreeMap keys are already sorted.
    let mut module_ids: Vec<&String> = repo.modules.keys().collect();
    module_ids.sort();
    for mod_id in &module_ids {
        let module = &repo.modules[*mod_id];
        let id = dsl_id(mod_id);
        let label = short_module(mod_id);
        let class_count = module.classes.len();
        let descr = if class_count == 1 {
            "1 class".to_string()
        } else {
            format!("{class_count} classes")
        };
        let components = top_components(module, &fan_in);
        if components.is_empty() {
            let _ = writeln!(
                out,
                "            {id} = container \"{}\" \"{}\"",
                dsl_str(label),
                dsl_str(&descr)
            );
        } else {
            let _ = writeln!(
                out,
                "            {id} = container \"{}\" \"{}\" {{",
                dsl_str(label),
                dsl_str(&descr)
            );
            for comp in &components {
                // Prefix the component id with the module id so ids stay
                // collision-free across modules (two modules may both hold a
                // class named `Config`).
                let comp_id = format!("{id}_{}", dsl_id(&comp.name));
                let _ = writeln!(
                    out,
                    "                {comp_id} = component \"{}\" \"{}\"",
                    dsl_str(&comp.name),
                    dsl_str(&comp.stereotype)
                );
            }
            out.push_str("            }\n");
        }
    }
    out.push_str("        }\n");

    // Cross-module edges, aggregated exactly like render_c4_container.
    let edges = cross_module_edges(repo, framework);
    if edges.is_empty() {
        if let Some(first) = module_ids.first() {
            let _ = writeln!(out, "        developer -> {} \"explores\"", dsl_id(first));
        }
    } else {
        let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
        for (_, to) in &edges {
            *indegree.entry(to.clone()).or_default() += 1;
        }
        let entry = indegree
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(k, _)| k.clone())
            .or_else(|| module_ids.first().map(|s| (*s).clone()));
        if let Some(entry) = entry {
            let _ = writeln!(out, "        developer -> {} \"explores\"", dsl_id(&entry));
        }
        for (from, to) in &edges {
            let _ = writeln!(out, "        {} -> {} \"uses\"", dsl_id(from), dsl_id(to));
        }
    }

    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

/// Result of [`merge_c4_dsl`]: the merged DSL text plus how many new elements
/// were folded in from the code. All three counts are `0` when the existing
/// model already covers the current code, in which case `text` equals the input
/// byte-for-byte ("already up to date").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeResult {
    /// The merged DSL. Existing content — including user edits and comments — is
    /// preserved byte-identically; new elements are inserted in place.
    pub text: String,
    /// Number of new `container` blocks added (modules present in the code but
    /// absent from the DSL).
    pub added_containers: usize,
    /// Number of new `component` lines added inside existing/new containers.
    pub added_components: usize,
    /// Number of new cross-module `relationship` lines added.
    pub added_relationships: usize,
}

impl MergeResult {
    /// `true` when nothing new was added (the model already covers the code).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_containers == 0 && self.added_components == 0 && self.added_relationships == 0
    }
}

/// A container the current code says should exist, with its top components —
/// the "desired" side of the merge diff.
struct DesiredContainer {
    /// DSL id (module-derived, stable — see [`dsl_id`]).
    id: String,
    /// Human-readable label (`short_module`).
    label: String,
    /// Description (`"N classes"`).
    descr: String,
    /// Component ids + the class/stereotype behind each, in emission order.
    components: Vec<DesiredComponent>,
}

/// A component the current code says should exist inside its container.
struct DesiredComponent {
    /// DSL id (`{container_id}_{ClassName}`), globally unique.
    id: String,
    /// Class name.
    name: String,
    /// Stereotype label (or `"class"`).
    stereotype: String,
}

/// Compute the containers the current code implies, in the same order and with
/// the same ids/labels/descriptions/components as [`generate_c4_dsl`] emits.
/// Factored out so generate and merge agree on the "desired" structure.
fn desired_containers(repo: &Repository, framework: &dyn FrameworkPlugin) -> Vec<DesiredContainer> {
    let fan_in = fan_in_by_fqn(repo, framework);
    let mut module_ids: Vec<&String> = repo.modules.keys().collect();
    module_ids.sort();
    module_ids
        .into_iter()
        .map(|mod_id| {
            let module = &repo.modules[mod_id];
            let id = dsl_id(mod_id);
            let label = short_module(mod_id).to_string();
            let class_count = module.classes.len();
            let descr = if class_count == 1 {
                "1 class".to_string()
            } else {
                format!("{class_count} classes")
            };
            let components = top_components(module, &fan_in)
                .into_iter()
                .map(|comp| DesiredComponent {
                    id: format!("{id}_{}", dsl_id(&comp.name)),
                    name: comp.name,
                    stereotype: comp.stereotype,
                })
                .collect();
            DesiredContainer {
                id,
                label,
                descr,
                components,
            }
        })
        .collect()
}

/// Fold the current code structure into an existing (possibly hand-edited) C4
/// DSL, **additively**. This is the round-trip half of
/// [#142](https://github.com/Plaintext-Gmbh/projectmind/issues/142): the code
/// evolves, and the architect wants the new modules / classes / cross-module
/// edges to appear in their model *without losing a single thing they wrote*.
///
/// # What it adds
/// Exactly the elements the current code implies (via [`desired_containers`] and
/// [`cross_module_edges`] — the same derivation [`generate_c4_dsl`] uses) that
/// are **not already present** in `existing_text`, keyed by DSL `id`:
/// - new **containers** (a module with no matching `container` id),
/// - new **components** (a top class with no matching `component` id, inserted
///   into its container — existing *or* freshly added),
/// - new **relationships** (a cross-module `from -> to` pair not already an
///   edge between those two ids, in either direction-insensitive… no: matched
///   on the exact ordered `(from, to)` pair).
///
/// # What it never touches — the preservation guarantee
/// Every byte of `existing_text` that is not a fresh insertion is left
/// **exactly** as-is: comments (`#`, `//`), blank lines, whitespace/indent,
/// element **descriptions the user rewrote**, and any elements the user added by
/// hand that the code knows nothing about — external `softwareSystem`s, extra
/// `person` actors, custom `relationship`s, hand-authored `container`s /
/// `component`s. Elements that exist in the DSL but *no longer* in the code are
/// **kept** too (the architect prunes those manually). Merge is therefore a
/// pure superset operation on element ids.
///
/// # Why text-insertion (not re-emission)
/// The obvious alternative — parse both sides, union them, re-emit
/// deterministically — is simpler but **lossy**: [`parse_c4_dsl`] is a tolerant
/// subset parser that drops comments, unknown Structurizr constructs (`views`,
/// `styles`, `!identifiers`, tags, technology strings…), and all original
/// formatting, so re-emission would silently rewrite the user's file and
/// normalise away exactly the hand-authored context #142 is about preserving.
/// Instead we keep `existing_text` as the spine and splice new lines in at the
/// right brace-depth: a new component just before its container's closing `}`,
/// a new container just before the enclosing `softwareSystem`'s closing `}`, a
/// new relationship on its own line just after the last recognised relationship
/// in the `model` block. Everything else flows through verbatim. The only cost
/// is that a container that was written as a single-line `container "x" "y"`
/// (no `{}` block) must be reopened into a block to receive its first
/// component; we do that by rewriting *only that one line* (see
/// `reopen_container_line`), preserving its id/name/description text.
///
/// Idempotent: merging an already-merged model adds nothing and returns the
/// input unchanged (all counts `0`).
#[must_use]
pub fn merge_c4_dsl(
    existing_text: &str,
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> MergeResult {
    let existing = parse_c4_dsl(existing_text);

    // Index everything the DSL already has, by id.
    let mut existing_container_ids: BTreeSet<String> = BTreeSet::new();
    let mut existing_component_ids: BTreeSet<String> = BTreeSet::new();
    for sys in &existing.systems {
        for c in &sys.containers {
            existing_container_ids.insert(c.id.clone());
            for comp in &c.components {
                existing_component_ids.insert(comp.id.clone());
            }
        }
    }
    let existing_rels: BTreeSet<(String, String)> = existing
        .relationships
        .iter()
        .map(|r| (r.from.clone(), r.to.clone()))
        .collect();

    let desired = desired_containers(repo, framework);
    let desired_edges = cross_module_edges(repo, framework);

    // Plan the additions. `new_components_by_container` maps an existing (or
    // newly added) container id → the component lines it still lacks.
    let mut new_containers: Vec<&DesiredContainer> = Vec::new();
    let mut new_components_by_container: BTreeMap<String, Vec<&DesiredComponent>> = BTreeMap::new();
    for dc in &desired {
        if existing_container_ids.contains(&dc.id) {
            let missing: Vec<&DesiredComponent> = dc
                .components
                .iter()
                .filter(|comp| !existing_component_ids.contains(&comp.id))
                .collect();
            if !missing.is_empty() {
                new_components_by_container.insert(dc.id.clone(), missing);
            }
        } else {
            new_containers.push(dc);
        }
    }
    // Cross-module relationships the DSL is missing (exact ordered id pair).
    let new_edges: Vec<(String, String)> = desired_edges
        .iter()
        .map(|(from, to)| (dsl_id(from), dsl_id(to)))
        .filter(|pair| pair.0 != pair.1 && !existing_rels.contains(pair))
        .collect();

    let added_containers = new_containers.len();
    let added_components: usize = new_components_by_container
        .values()
        .map(Vec::len)
        .sum::<usize>()
        + new_containers
            .iter()
            .map(|c| c.components.len())
            .sum::<usize>();
    let added_relationships = new_edges.len();

    // Nothing to do → return the input byte-identically.
    if added_containers == 0 && added_components == 0 && added_relationships == 0 {
        return MergeResult {
            text: existing_text.to_string(),
            added_containers: 0,
            added_components: 0,
            added_relationships: 0,
        };
    }

    let text = splice_additions(
        existing_text,
        &new_components_by_container,
        &new_containers,
        &new_edges,
    );

    MergeResult {
        text,
        added_containers,
        added_components,
        added_relationships,
    }
}

/// Splice the planned additions into `existing_text` at the right brace depths,
/// leaving every other byte untouched. See [`merge_c4_dsl`] for the strategy.
///
/// Walks the text line by line tracking brace depth and the id of the currently
/// open `softwareSystem` / `container` blocks, so it can decide where each
/// closing `}` belongs. New component lines are emitted just before the closing
/// `}` of their container; new containers just before the closing `}` of the
/// first `softwareSystem`; new relationships just before the closing `}` of the
/// `model` block (after the existing relationships). Indentation mirrors the
/// generator's four-space steps.
fn splice_additions(
    existing_text: &str,
    new_components_by_container: &BTreeMap<String, Vec<&DesiredComponent>>,
    new_containers: &[&DesiredContainer],
    new_edges: &[(String, String)],
) -> String {
    // A lightweight scan of the block structure, mirroring parse_c4_dsl's
    // brace/keyword handling but tracking the *open* element id at each level
    // and the model-block depth so we know where to inject.
    #[derive(Clone)]
    enum Frame {
        /// A block we don't need to target (workspace, views, person, …).
        Other,
        /// The `model { … }` block.
        Model,
        /// A `softwareSystem { … }` block.
        System,
        /// A `container { … }` block, by its id.
        Container(String),
    }

    let mut out = String::new();
    let mut stack: Vec<Frame> = Vec::new();
    // Remember whether we have already injected containers/relationships so we
    // only do so once (at the first system / model close).
    let mut containers_injected = false;
    let mut relationships_injected = false;

    for raw in existing_text.lines() {
        let stripped = strip_comment(raw).trim();
        let opens = stripped.ends_with('{');
        let body = stripped.trim_end_matches('{').trim();
        let is_close = body == "}" || (body.is_empty() && stripped.starts_with('}'));

        // A closing brace: inject before it if this frame is a target, then pop.
        if is_close {
            match stack.last().cloned() {
                Some(Frame::Container(id)) if new_components_by_container.contains_key(&id) => {
                    for comp in &new_components_by_container[&id] {
                        let _ = writeln!(
                            out,
                            "                {} = component \"{}\" \"{}\"",
                            comp.id,
                            dsl_str(&comp.name),
                            dsl_str(&comp.stereotype)
                        );
                    }
                }
                Some(Frame::System) if !containers_injected => {
                    for dc in new_containers {
                        emit_container_block(&mut out, dc);
                    }
                    containers_injected = true;
                }
                Some(Frame::Model) if !relationships_injected => {
                    for (from, to) in new_edges {
                        let _ = writeln!(out, "        {from} -> {to} \"uses\"");
                    }
                    relationships_injected = true;
                }
                _ => {}
            }
            stack.pop();
            out.push_str(raw);
            out.push('\n');
            continue;
        }

        // Classify an opening declaration so we can label the frame.
        if opens {
            let (assign_id, keyword, _rest) = split_declaration(body);
            let frame = match keyword {
                Some("softwaresystem") => Frame::System,
                Some("container") => {
                    let id = assign_id.unwrap_or_default();
                    Frame::Container(id)
                }
                _ => {
                    // `model {` is the one unkeyed block we must target; detect
                    // it by its leading token.
                    if body
                        .split_whitespace()
                        .next()
                        .is_some_and(|w| w.eq_ignore_ascii_case("model"))
                    {
                        Frame::Model
                    } else {
                        Frame::Other
                    }
                }
            };
            out.push_str(raw);
            out.push('\n');
            stack.push(frame);
            continue;
        }

        // A single-line container that must be reopened to hold new components.
        // The original line has no matching `}` on a later line, so we open the
        // block, emit the new components, and close it right here — all in one
        // self-contained splice, without pushing a frame.
        if let (Some(id), Some("container"), _) = split_declaration(body) {
            // A single-line target is exactly a container id that still needs
            // components (the keys of `new_components_by_container`).
            if let Some(comps) = new_components_by_container.get(&id) {
                out.push_str(&reopen_container_line(raw));
                out.push('\n');
                for comp in comps {
                    let _ = writeln!(
                        out,
                        "                {} = component \"{}\" \"{}\"",
                        comp.id,
                        dsl_str(&comp.name),
                        dsl_str(&comp.stereotype)
                    );
                }
                out.push_str("            }\n");
                continue;
            }
        }

        // Any other line flows through verbatim.
        out.push_str(raw);
        out.push('\n');
    }

    // Preserve a missing trailing newline: `lines()` drops it, so re-add one
    // only if the original ended with a newline. Do this *before* the defensive
    // fallbacks below so appended fallback lines keep their own terminators.
    if !existing_text.ends_with('\n') && existing_text.contains('\n') {
        out.pop();
    } else if existing_text.is_empty() {
        // No lines at all: `out` is empty; nothing to trim.
    }

    // Defensive fallbacks: if the file had no recognisable system / model block
    // to inject into (e.g. a heavily non-standard hand-authored file), append
    // the leftover additions so nothing is silently dropped. In practice this
    // never fires for scaffolded files; it keeps merge total on odd input.
    if !containers_injected && !new_containers.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for dc in new_containers {
            emit_container_block(&mut out, dc);
        }
    }
    if !relationships_injected && !new_edges.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for (from, to) in new_edges {
            let _ = writeln!(out, "        {from} -> {to} \"uses\"");
        }
    }

    out
}

/// Emit a full `container` block (with its components) at the generator's
/// indentation, for a container the DSL was missing entirely.
fn emit_container_block(out: &mut String, dc: &DesiredContainer) {
    if dc.components.is_empty() {
        let _ = writeln!(
            out,
            "            {} = container \"{}\" \"{}\"",
            dc.id,
            dsl_str(&dc.label),
            dsl_str(&dc.descr)
        );
    } else {
        let _ = writeln!(
            out,
            "            {} = container \"{}\" \"{}\" {{",
            dc.id,
            dsl_str(&dc.label),
            dsl_str(&dc.descr)
        );
        for comp in &dc.components {
            let _ = writeln!(
                out,
                "                {} = component \"{}\" \"{}\"",
                comp.id,
                dsl_str(&comp.name),
                dsl_str(&comp.stereotype)
            );
        }
        out.push_str("            }\n");
    }
}

/// Rewrite a single-line `id = container "x" "y"` into an opening
/// `id = container "x" "y" {` line, preserving the original leading whitespace,
/// the id/name/description text, and any trailing comment. Only ` {` is spliced
/// in — inserted *before* an inline comment (`#`/`//`) so the brace stays live
/// code rather than being swallowed by the comment.
fn reopen_container_line(raw: &str) -> String {
    let code_len = strip_comment(raw).len();
    let code = &raw[..code_len];
    let comment = &raw[code_len..];
    let code_trimmed = code.trim_end();
    if comment.is_empty() {
        format!("{code_trimmed} {{")
    } else {
        // e.g. `  db = container "DB" "x"  // note`  →  `  db = container … {  // note`
        format!("{code_trimmed} {{ {}", comment.trim_start())
    }
}

/// Aggregate the set of cross-module `(from_mod, to_mod)` edges — the same
/// derivation `render_c4_container` uses, factored out so both stay in sync.
fn cross_module_edges(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> BTreeSet<(String, String)> {
    let mut node_modules: BTreeMap<String, String> = BTreeMap::new(); // fqn → mod_id
    for (mod_id, module) in &repo.modules {
        for class in module.classes.values() {
            node_modules.insert(class.fqn.clone(), mod_id.clone());
        }
    }
    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for (mod_id, module) in &repo.modules {
        for rel in framework.relations(module) {
            if let Some(to_mod) = node_modules.get(&rel.to) {
                if to_mod != mod_id {
                    edges.insert((mod_id.clone(), to_mod.clone()));
                }
            }
        }
    }
    edges
}

/// Per-class fan-in (`fqn` → number of *distinct* classes referencing it) from
/// the framework relations graph. This mirrors `risk::FanCounts`' in-degree —
/// the same signal [`crate::risk::compute`] weights as `deps` — but stays cheap
/// (no git, no coverage): `(from, to)` pairs are deduplicated and self-edges
/// dropped so a class that both injects and calls another counts once.
fn fan_in_by_fqn(repo: &Repository, framework: &dyn FrameworkPlugin) -> BTreeMap<String, u32> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut fan_in: BTreeMap<String, u32> = BTreeMap::new();
    for module in repo.modules.values() {
        for rel in framework.relations(module) {
            if rel.from == rel.to {
                continue;
            }
            if seen.insert((rel.from.clone(), rel.to.clone())) {
                *fan_in.entry(rel.to.clone()).or_default() += 1;
            }
        }
    }
    fan_in
}

/// A class chosen to represent a container at the component level.
struct ChosenComponent {
    /// Simple class name.
    name: String,
    /// The class's most significant stereotype, or `"class"` when it has none.
    stereotype: String,
}

/// Pick the [`COMPONENTS_PER_CONTAINER`] most important classes of `module`.
///
/// Ranking key (highest first): fan-in, then stereotype priority, then class
/// name. Every tie-breaker is total and deterministic, so a repo with no
/// relations (all fan-in 0) still yields a stable, meaningful choice driven by
/// stereotype and name.
fn top_components(
    module: &projectmind_plugin_api::Module,
    fan_in: &BTreeMap<String, u32>,
) -> Vec<ChosenComponent> {
    let mut ranked: Vec<(u32, usize, &str, &str)> = module
        .classes
        .values()
        .map(|class| {
            let fi = fan_in.get(&class.fqn).copied().unwrap_or(0);
            let (rank, stereotype) = stereotype_rank(class);
            (fi, rank, stereotype, class.name.as_str())
        })
        .collect();
    // Higher fan-in first; lower stereotype rank (0 = most important) first;
    // then name ascending — all applied on the negated/ordered keys below.
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0) // fan-in desc
            .then(a.1.cmp(&b.1)) // stereotype rank asc (0 wins)
            .then(a.3.cmp(b.3)) // name asc
    });
    ranked
        .into_iter()
        .take(COMPONENTS_PER_CONTAINER)
        .map(|(_, _, stereotype, name)| ChosenComponent {
            name: name.to_string(),
            stereotype: stereotype.to_string(),
        })
        .collect()
}

/// Map a class to `(priority_rank, stereotype_label)`. Rank 0 is the most
/// important; classes without a known stereotype get the largest rank so they
/// sort last, labelled `"class"`. The priority list matches
/// `diagram::stereotype_lookup` so both views agree on what "important" means.
fn stereotype_rank(class: &projectmind_plugin_api::Class) -> (usize, &str) {
    const PRIORITY: [&str; 7] = [
        "rest-controller",
        "controller",
        "service",
        "repository",
        "configuration",
        "component",
        "rest",
    ];
    for (rank, name) in PRIORITY.iter().enumerate() {
        if class.stereotypes.iter().any(|s| s == name) {
            return (rank, name);
        }
    }
    // Any other stereotype ranks just after the known ones (but before none).
    if let Some(first) = class.stereotypes.first() {
        return (PRIORITY.len(), first.as_str());
    }
    (PRIORITY.len() + 1, "class")
}

/// Parse a Structurizr-DSL subset into a [`C4Model`].
///
/// The parser is line-based and forgiving: it recognises `person`,
/// `softwareSystem`, `container`, `component` element declarations and
/// `<from> -> <to> "desc"` relationships, tracking nesting via `{`/`}`. Any
/// line it does not understand — comments (`#`, `//`), blank lines, `workspace`
/// / `model` / `views` scaffolding, unknown properties — is ignored. It never
/// panics.
///
/// Recognised element forms (the assignment prefix `id =` is optional):
/// - `developer = person "Developer" "desc"`
/// - `person "Developer" "desc"`
/// - `sys = softwareSystem "Name" "desc" {`
/// - `api = container "Name" "12 classes"`
/// - `svc = component "Name" "desc"`
#[must_use]
pub fn parse_c4_dsl(text: &str) -> C4Model {
    let mut model = C4Model::default();
    // Nesting context by innermost open block: which element (if any) new
    // children attach to.
    #[derive(Clone, Copy)]
    enum Ctx {
        /// Not a system/container/component block (workspace, model, views, …).
        Other,
        /// Inside a `softwareSystem { … }`: index into `model.systems`.
        System(usize),
        /// Inside a `container { … }`: (system index, container index).
        Container(usize, usize),
    }
    let mut stack: Vec<Ctx> = Vec::new();

    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        // A line may both declare an element and open/close braces; handle the
        // opening brace after we've classified the declaration.
        let opens = line.ends_with('{');
        let body = line.trim_end_matches('{').trim();

        // A bare closing brace pops the nesting.
        if body == "}" || body.is_empty() && line.starts_with('}') {
            stack.pop();
            continue;
        }
        // Trailing `}` on the same line as content is rare in our subset; still
        // handle a standalone `}` reliably above and ignore inline trailers.

        let (assign_id, keyword, rest) = split_declaration(body);

        match keyword {
            Some("person") => {
                let (name, desc) = two_strings(rest);
                let id = assign_id.unwrap_or_else(|| dsl_id(&name));
                model.persons.push(C4Person {
                    id,
                    name,
                    description: desc,
                });
                if opens {
                    stack.push(Ctx::Other);
                }
            }
            Some("softwaresystem") => {
                let (name, desc) = two_strings(rest);
                let id = assign_id.unwrap_or_else(|| dsl_id(&name));
                let idx = model.systems.len();
                model.systems.push(C4System {
                    id,
                    name,
                    description: desc,
                    containers: Vec::new(),
                });
                if opens {
                    stack.push(Ctx::System(idx));
                }
            }
            Some("container") => {
                let (name, desc) = two_strings(rest);
                let id = assign_id.unwrap_or_else(|| dsl_id(&name));
                // Attach to the nearest enclosing system; if there is none,
                // start an implicit system so nothing is lost.
                let enclosing_system = stack.iter().rev().find_map(|c| match c {
                    Ctx::System(i) => Some(*i),
                    Ctx::Container(s, _) => Some(*s),
                    Ctx::Other => None,
                });
                let sys_idx = enclosing_system.unwrap_or_else(|| {
                    let idx = model.systems.len();
                    model.systems.push(C4System {
                        id: "system".to_string(),
                        name: "System".to_string(),
                        description: String::new(),
                        containers: Vec::new(),
                    });
                    idx
                });
                let c_idx = model.systems[sys_idx].containers.len();
                model.systems[sys_idx].containers.push(C4Container {
                    id,
                    name,
                    description: desc,
                    components: Vec::new(),
                });
                if opens {
                    stack.push(Ctx::Container(sys_idx, c_idx));
                }
            }
            Some("component") => {
                let (name, desc) = two_strings(rest);
                let id = assign_id.unwrap_or_else(|| dsl_id(&name));
                let enclosing_container = stack.iter().rev().find_map(|c| match c {
                    Ctx::Container(s, i) => Some((*s, *i)),
                    _ => None,
                });
                if let Some((s, c)) = enclosing_container {
                    model.systems[s].containers[c].components.push(C4Component {
                        id,
                        name,
                        description: desc,
                    });
                }
                if opens {
                    stack.push(Ctx::Other);
                }
            }
            _ => {
                // Not a known element. Is it a relationship `a -> b "desc"`?
                if let Some(rel) = parse_relationship(body) {
                    model.relationships.push(rel);
                } else if opens {
                    // Unknown block (workspace, model, views, styles, …).
                    stack.push(Ctx::Other);
                }
            }
        }
    }

    model
}

/// Render a [`C4Model`] as Mermaid `C4Container` text.
///
/// The output format mirrors [`crate::diagram::render_c4_container`] (same
/// `C4Container` header, `title`, `Person(...)`, `System_Boundary` block and
/// `Rel(...)` edges) so the frontend's existing Mermaid renderer draws it
/// unchanged. A container **with** components is rendered as a Mermaid-C4
/// `Container_Boundary(id, "label") { Component(...) … }` nesting (still native
/// `C4Container` syntax, drawn by the same renderer); a container **without**
/// components stays a plain `Container(...)` line. Cross-module `Rel(...)` edges
/// remain at module level.
#[must_use]
pub fn c4_model_to_mermaid(model: &C4Model) -> String {
    let mut out = String::from("C4Container\n");

    // Title from the first system, falling back to a generic label.
    let title = model
        .systems
        .first()
        .map_or("C4 model", |s| s.name.as_str());
    let _ = writeln!(out, "    title Container view of {}", c4_label(title));

    // People.
    if model.persons.is_empty() {
        out.push_str(
            "    Person(developer, \"Developer\", \"Browses architecture via ProjectMind\")\n",
        );
    } else {
        for p in &model.persons {
            let _ = writeln!(
                out,
                "    Person({}, \"{}\", \"{}\")",
                escape_id(&p.id),
                c4_label(&p.name),
                c4_label(&p.description)
            );
        }
    }

    let total_containers: usize = model.systems.iter().map(|s| s.containers.len()).sum();
    if total_containers == 0 {
        out.push_str("    Container(empty, \"empty\", \"no modules detected\")\n");
        return out;
    }

    for system in &model.systems {
        if system.containers.is_empty() {
            continue;
        }
        let _ = writeln!(
            out,
            "    System_Boundary({}, \"{}\") {{",
            escape_id(&system.id),
            c4_label(&system.name)
        );
        for c in &system.containers {
            if c.components.is_empty() {
                let _ = writeln!(
                    out,
                    "        Container({}, \"{}\", \"{}\")",
                    escape_id(&c.id),
                    c4_label(&c.name),
                    c4_label(&c.description)
                );
            } else {
                // A container that owns components becomes a boundary so the
                // components nest visibly inside it (Mermaid-C4 native syntax).
                let _ = writeln!(
                    out,
                    "        Container_Boundary({}, \"{}\") {{",
                    escape_id(&c.id),
                    c4_label(&c.name)
                );
                for comp in &c.components {
                    let _ = writeln!(
                        out,
                        "            Component({}, \"{}\", \"{}\")",
                        escape_id(&comp.id),
                        c4_label(&comp.name),
                        c4_label(&comp.description)
                    );
                }
                out.push_str("        }\n");
            }
        }
        out.push_str("    }\n");
    }

    for rel in &model.relationships {
        let _ = writeln!(
            out,
            "    Rel({}, {}, \"{}\")",
            escape_id(&rel.from),
            escape_id(&rel.to),
            c4_label(&rel.description)
        );
    }

    out
}

/// Scaffold `docs/architecture.dsl` for `repo` — write it once, never clobber.
///
/// If the file already exists it is left completely untouched
/// (`created: false`); the user owns it after the first scaffold. Otherwise
/// the `docs/` directory is ensured and [`generate_c4_dsl`] is written
/// atomically (via a temp file + rename), returning `created: true`.
///
/// # Errors
/// Returns an [`std::io::Error`] if the directory or file cannot be created or
/// written.
pub fn scaffold_c4_model(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> std::io::Result<ScaffoldResult> {
    let path = c4_model_path(repo);
    if path.exists() {
        return Ok(ScaffoldResult {
            path,
            created: false,
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dsl = generate_c4_dsl(repo, framework);
    // Atomic write: temp file then rename, so a concurrent reader never sees a
    // half-written model.
    let tmp = path.with_extension("dsl.tmp");
    std::fs::write(&tmp, dsl)?;
    std::fs::rename(&tmp, &path)?;
    Ok(ScaffoldResult {
        path,
        created: true,
    })
}

/// Merge the current code structure into `docs/architecture.dsl` for `repo`,
/// **additively** — the round-trip of
/// [#142](https://github.com/Plaintext-Gmbh/projectmind/issues/142).
///
/// - If the file **does not exist**, this behaves exactly like
///   [`scaffold_c4_model`]: it generates a fresh model and writes it, returning
///   `created: true` with all `added_*` counts `0`.
/// - If the file **exists**, it is read, run through [`merge_c4_dsl`] (which
///   preserves every user edit and comment byte-for-byte and only *adds* the
///   containers / components / relationships the code has but the DSL lacks),
///   and written back **only when something was actually added** — an
///   already-up-to-date model is left untouched on disk. Returns `created:
///   false` with the per-kind added counts.
///
/// Writes are atomic (temp file + rename) so a concurrent reader never sees a
/// half-written model.
///
/// # Errors
/// Returns an [`std::io::Error`] if the directory or file cannot be created,
/// read or written.
pub fn merge_c4_model(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> std::io::Result<MergeModelResult> {
    let path = c4_model_path(repo);
    if !path.exists() {
        // No model yet → scaffold one, mirroring scaffold_c4_model.
        let scaffolded = scaffold_c4_model(repo, framework)?;
        return Ok(MergeModelResult {
            path: scaffolded.path,
            created: scaffolded.created,
            added_containers: 0,
            added_components: 0,
            added_relationships: 0,
        });
    }

    let existing = std::fs::read_to_string(&path)?;
    let merged = merge_c4_dsl(&existing, repo, framework);

    // Only touch the file when the merge actually changed something; an
    // already-current model stays byte-identical on disk (no needless churn).
    if !merged.is_empty() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("dsl.tmp");
        std::fs::write(&tmp, &merged.text)?;
        std::fs::rename(&tmp, &path)?;
    }

    Ok(MergeModelResult {
        path,
        created: false,
        added_containers: merged.added_containers,
        added_components: merged.added_components,
        added_relationships: merged.added_relationships,
    })
}

// ---------------------------------------------------------------------------
// DSL helpers
// ---------------------------------------------------------------------------

/// Escape a string for a double-quoted DSL literal.
fn dsl_str(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Turn an arbitrary id (module coordinate, title) into a DSL-safe identifier.
/// Structurizr identifiers are alphanumeric + underscore; we map anything else
/// to `_`, matching [`escape_id`] so ids stay consistent through the round-trip.
fn dsl_id(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    // An identifier must not start with a digit for readability; prefix if so.
    match s.chars().next() {
        Some(c) if c.is_ascii_digit() => format!("_{s}"),
        None => "_".to_string(),
        _ => s,
    }
}

/// Quote-safe label for Mermaid C4 — mirrors `diagram::c4_label`.
fn c4_label(raw: &str) -> String {
    raw.replace('"', "'")
}

/// Escape an id for Mermaid — mirrors `diagram::escape_id`.
fn escape_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// `groupId:artifactId` → `artifactId`; mirrors `diagram::short_module`.
fn short_module(mod_id: &str) -> &str {
    mod_id.rsplit_once(':').map_or(mod_id, |(_, s)| s)
}

/// Strip a line comment (`#` or `//`) that lies outside a quoted string.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => in_str = !in_str,
            b'\\' if in_str => {
                i += 1; // skip escaped char
            }
            b'#' if !in_str => return &line[..i],
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                return &line[..i];
            }
            _ => {}
        }
        i += 1;
    }
    line
}

/// Split a declaration into an optional `id = ` prefix, a lowercased keyword,
/// and the remainder. Returns `(None, None, line)` when there is no keyword.
fn split_declaration(body: &str) -> (Option<String>, Option<&str>, &str) {
    // Optional `id = ` assignment prefix.
    let (assign_id, rest) = if let Some(eq) = body.find('=') {
        let lhs = body[..eq].trim();
        let rhs = body[eq + 1..].trim();
        // Only treat as assignment when the LHS is a single bare identifier
        // (no spaces, no quotes) — avoids misreading `a -> b` or quoted text.
        if !lhs.is_empty()
            && !lhs.contains(' ')
            && !lhs.contains('"')
            && !lhs.contains('-')
            && lhs.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            (Some(lhs.to_string()), rhs)
        } else {
            (None, body)
        }
    } else {
        (None, body)
    };

    // First whitespace-delimited token is the keyword candidate.
    let mut it = rest.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("");
    let remainder = it.next().unwrap_or("").trim();
    let lower = match first.to_ascii_lowercase().as_str() {
        "person" => Some("person"),
        "softwaresystem" => Some("softwaresystem"),
        "container" => Some("container"),
        "component" => Some("component"),
        _ => None,
    };
    (assign_id, lower, remainder)
}

/// Extract up to two double-quoted strings from a declaration remainder.
/// Missing strings become empty. Unquoted leading tokens are tolerated.
fn two_strings(rest: &str) -> (String, String) {
    let strings = quoted_strings(rest);
    let name = strings.first().cloned().unwrap_or_default();
    let desc = strings.get(1).cloned().unwrap_or_default();
    (name, desc)
}

/// Collect every double-quoted string in `s`, un-escaping `\"` and `\\`.
fn quoted_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let mut buf = String::new();
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' if i + 1 < bytes.len() => {
                        buf.push(bytes[i + 1] as char);
                        i += 2;
                    }
                    b'"' => {
                        i += 1;
                        break;
                    }
                    other => {
                        buf.push(other as char);
                        i += 1;
                    }
                }
            }
            out.push(buf);
        } else {
            i += 1;
        }
    }
    out
}

/// Parse a relationship line `a -> b "desc"` (description optional).
fn parse_relationship(body: &str) -> Option<C4Relationship> {
    let arrow = body.find("->")?;
    let from = body[..arrow].trim();
    let after = body[arrow + 2..].trim();
    if from.is_empty() || after.is_empty() {
        return None;
    }
    // The target id is the first token; an optional quoted description follows.
    let mut it = after.splitn(2, char::is_whitespace);
    let to = it.next().unwrap_or("").trim();
    if to.is_empty() {
        return None;
    }
    let desc = quoted_strings(after).into_iter().next().unwrap_or_default();
    // `from` must also be a bare id (no spaces / quotes) to avoid false hits.
    if from.contains(' ') || from.contains('"') {
        return None;
    }
    Some(C4Relationship {
        from: from.to_string(),
        to: to.to_string(),
        description: desc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{
        Class, FrameworkPlugin, Module, PluginInfo, Relation, RelationKind, Result as PiResult,
    };

    struct CrossFw;
    impl FrameworkPlugin for CrossFw {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "cross",
                name: "Cross",
                version: "0.0.1",
            }
        }
        fn supported_languages(&self) -> &[&'static str] {
            &["lang-java"]
        }
        fn enrich(&self, _module: &mut Module) -> PiResult<()> {
            Ok(())
        }
        fn relations(&self, module: &Module) -> Vec<Relation> {
            // Every class emits an edge to core.Heart (in module g:core).
            module
                .classes
                .values()
                .map(|c| Relation {
                    from: c.fqn.clone(),
                    to: "core.Heart".to_string(),
                    kind: RelationKind::Calls,
                })
                .collect()
        }
    }

    struct NoRelFw;
    impl FrameworkPlugin for NoRelFw {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "norel",
                name: "NoRel",
                version: "0.0.1",
            }
        }
        fn supported_languages(&self) -> &[&'static str] {
            &["lang-java"]
        }
        fn enrich(&self, _module: &mut Module) -> PiResult<()> {
            Ok(())
        }
        fn relations(&self, _module: &Module) -> Vec<Relation> {
            Vec::new()
        }
    }

    fn class(fqn: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
            ..Default::default()
        }
    }

    fn class_with_stereotype(fqn: &str, stereotype: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
            stereotypes: vec![stereotype.into()],
            ..Default::default()
        }
    }

    fn cross_repo() -> Repository {
        let mut repo = Repository::new(PathBuf::from("/tmp/my-repo"));
        let mut api = Module {
            id: "g:api".into(),
            ..Default::default()
        };
        api.classes.insert("a.A".into(), class("a.A"));
        api.classes.insert("a.B".into(), class("a.B"));
        let mut core = Module {
            id: "g:core".into(),
            ..Default::default()
        };
        core.classes
            .insert("core.Heart".into(), class("core.Heart"));
        repo.insert_module(api);
        repo.insert_module(core);
        repo
    }

    #[test]
    fn generate_emits_workspace_and_containers() {
        let repo = cross_repo();
        let dsl = generate_c4_dsl(&repo, &CrossFw);
        assert!(dsl.contains("workspace {"), "no workspace:\n{dsl}");
        assert!(dsl.contains("model {"), "no model:\n{dsl}");
        assert!(
            dsl.contains("softwareSystem \"my-repo\""),
            "no system:\n{dsl}"
        );
        // Both modules now open a container block because they carry
        // components (top-3 classes).
        assert!(
            dsl.contains("g_api = container \"api\" \"2 classes\" {"),
            "api container (with components) missing:\n{dsl}"
        );
        assert!(
            dsl.contains("g_core = container \"core\" \"1 class\" {"),
            "core container (with components) missing/plural-wrong:\n{dsl}"
        );
        // Components are emitted with a module-prefixed id and the class name.
        assert!(
            dsl.contains("g_api_A = component \"A\""),
            "component A missing:\n{dsl}"
        );
        assert!(
            dsl.contains("g_core_Heart = component \"Heart\""),
            "component Heart missing:\n{dsl}"
        );
        // Cross-module edge + developer anchored on the busiest module.
        assert!(
            dsl.contains("g_api -> g_core \"uses\""),
            "cross-module edge missing:\n{dsl}"
        );
        assert!(
            dsl.contains("developer -> g_core \"explores\""),
            "developer anchor missing:\n{dsl}"
        );
    }

    #[test]
    fn generate_is_deterministic() {
        let repo = cross_repo();
        assert_eq!(
            generate_c4_dsl(&repo, &CrossFw),
            generate_c4_dsl(&repo, &CrossFw)
        );
    }

    #[test]
    fn generate_no_edges_anchors_first_module() {
        let repo = cross_repo();
        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        assert!(
            dsl.contains("developer -> g_api \"explores\""),
            "should anchor developer to first module without edges:\n{dsl}"
        );
    }

    #[test]
    fn round_trip_generate_then_parse_is_consistent() {
        let repo = cross_repo();
        let dsl = generate_c4_dsl(&repo, &CrossFw);
        let model = parse_c4_dsl(&dsl);
        assert_eq!(model.persons.len(), 1, "one developer:\n{model:?}");
        assert_eq!(model.persons[0].id, "developer");
        assert_eq!(model.systems.len(), 1, "one system:\n{model:?}");
        let sys = &model.systems[0];
        assert_eq!(sys.name, "my-repo");
        assert_eq!(sys.containers.len(), 2, "two containers:\n{sys:?}");
        let ids: Vec<&str> = sys.containers.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"g_api") && ids.contains(&"g_core"), "{ids:?}");
        // Components survive the round-trip and attach to their container.
        let api = sys.containers.iter().find(|c| c.id == "g_api").unwrap();
        let comp_names: Vec<&str> = api.components.iter().map(|c| c.name.as_str()).collect();
        assert!(
            comp_names.contains(&"A") && comp_names.contains(&"B"),
            "api components missing: {comp_names:?}"
        );
        let core = sys.containers.iter().find(|c| c.id == "g_core").unwrap();
        assert_eq!(core.components.len(), 1, "{:?}", core.components);
        assert_eq!(core.components[0].name, "Heart");
        // developer->explores + g_api->g_core = 2 relationships.
        assert_eq!(model.relationships.len(), 2, "{:?}", model.relationships);
        assert!(model
            .relationships
            .iter()
            .any(|r| r.from == "g_api" && r.to == "g_core" && r.description == "uses"));
    }

    #[test]
    fn parse_tolerates_user_edits_comments_and_blanks() {
        let text = r#"
            # A hand-written model with comments.
            workspace {
                // free-form Structurizr the parser has never seen
                !identifiers hierarchical
                model {
                    dev = person "Architect" "Owns the model"
                    shop = softwareSystem "Shop" "E-commerce" {
                        web = container "Web" "Storefront"
                        # inline comment
                        db  = container "DB" "Postgres"   // trailing comment
                    }
                    dev -> web "uses"
                    web -> db "reads/writes"   # relationship comment
                }
                views {
                    systemContext shop {
                        include *
                    }
                }
            }
        "#;
        let model = parse_c4_dsl(text);
        assert_eq!(model.persons.len(), 1);
        assert_eq!(model.persons[0].name, "Architect");
        assert_eq!(model.systems.len(), 1);
        assert_eq!(model.systems[0].containers.len(), 2);
        assert_eq!(model.relationships.len(), 2);
        assert!(model
            .relationships
            .iter()
            .any(|r| r.from == "web" && r.to == "db" && r.description == "reads/writes"));
    }

    #[test]
    fn parse_never_panics_on_garbage() {
        for junk in [
            "",
            "}}}}}}",
            "{{{{ unbalanced",
            "container",
            "person \"unterminated",
            "-> -> ->",
            "a = = = b",
            "softwareSystem",
            "   \n\n   # only comments\n\n",
        ] {
            let _ = parse_c4_dsl(junk); // must not panic
        }
    }

    #[test]
    fn parse_component_blocks_are_captured() {
        let text = r#"
            workspace {
                model {
                    sys = softwareSystem "S" "d" {
                        api = container "Api" "n" {
                            ctrl = component "Controller" "handles http"
                        }
                    }
                }
            }
        "#;
        let model = parse_c4_dsl(text);
        assert_eq!(model.systems.len(), 1);
        assert_eq!(model.systems[0].containers.len(), 1);
        assert_eq!(model.systems[0].containers[0].components.len(), 1);
        assert_eq!(
            model.systems[0].containers[0].components[0].name,
            "Controller"
        );
    }

    #[test]
    fn empty_repo_generates_minimal_valid_model() {
        let repo = Repository::new(PathBuf::from("/tmp/void"));
        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        assert!(dsl.contains("workspace {"));
        assert!(dsl.contains("softwareSystem"));
        let model = parse_c4_dsl(&dsl);
        assert_eq!(model.systems.len(), 1);
        assert!(model.systems[0].containers.is_empty());
    }

    #[test]
    fn to_mermaid_mirrors_c4_container_format() {
        let repo = cross_repo();
        let dsl = generate_c4_dsl(&repo, &CrossFw);
        let model = parse_c4_dsl(&dsl);
        let mermaid = c4_model_to_mermaid(&model);
        assert!(mermaid.starts_with("C4Container\n"), "{mermaid}");
        assert!(
            mermaid.contains("title Container view of my-repo"),
            "{mermaid}"
        );
        assert!(
            mermaid.contains("Person(developer, \"Developer\""),
            "{mermaid}"
        );
        assert!(mermaid.contains("System_Boundary(my_repo,"), "{mermaid}");
        // Containers with components render as a Container_Boundary nesting
        // Component(...) lines; the module-level Rel edges stay unchanged.
        assert!(
            mermaid.contains("Container_Boundary(g_api, \"api\") {"),
            "{mermaid}"
        );
        assert!(mermaid.contains("Component(g_api_A, \"A\","), "{mermaid}");
        assert!(
            mermaid.contains("Component(g_core_Heart, \"Heart\","),
            "{mermaid}"
        );
        assert!(
            mermaid.contains("Rel(g_api, g_core, \"uses\")"),
            "{mermaid}"
        );
        assert!(
            mermaid.contains("Rel(developer, g_core, \"explores\")"),
            "{mermaid}"
        );
    }

    #[test]
    fn to_mermaid_empty_model_is_non_empty_with_marker() {
        let model = C4Model::default();
        let mermaid = c4_model_to_mermaid(&model);
        assert!(mermaid.starts_with("C4Container\n"));
        assert!(mermaid.contains("no modules detected"), "{mermaid}");
    }

    #[test]
    fn render_c4_model_absent_returns_sentinel() {
        let repo = Repository::new(PathBuf::from("/tmp/definitely-nonexistent-xyzzy"));
        assert_eq!(render_c4_model(&repo), C4_MODEL_ABSENT);
    }

    #[test]
    fn render_c4_model_reads_and_renders_edited_file() {
        let dir = std::env::temp_dir().join(format!("pm-c4-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join(C4_MODEL_REL_PATH),
            "workspace { model {\n  dev = person \"Architect\" \"x\"\n  s = softwareSystem \"Shop\" \"d\" {\n    web = container \"Web\" \"ui\"\n  }\n  dev -> web \"uses\"\n} }\n",
        )
        .unwrap();
        let repo = Repository::new(dir.clone());
        let mermaid = render_c4_model(&repo);
        assert!(mermaid.starts_with("C4Container\n"), "{mermaid}");
        assert!(
            mermaid.contains("Container(web, \"Web\", \"ui\")"),
            "{mermaid}"
        );
        assert!(mermaid.contains("Rel(dev, web, \"uses\")"), "{mermaid}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A module with more than the cap of classes emits exactly
    /// `COMPONENTS_PER_CONTAINER` components — never more.
    #[test]
    fn generate_caps_components_per_container() {
        let mut repo = Repository::new(PathBuf::from("/tmp/cap-repo"));
        let mut m = Module {
            id: "g:big".into(),
            ..Default::default()
        };
        for n in 0..10 {
            let fqn = format!("p.C{n}");
            m.classes.insert(fqn.clone(), class(&fqn));
        }
        repo.insert_module(m);

        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        let n_components = dsl.matches(" = component ").count();
        assert_eq!(
            n_components, COMPONENTS_PER_CONTAINER,
            "should emit exactly the cap:\n{dsl}"
        );
        // Deterministic pick with no relations: stereotype/name order → the
        // alphabetically-first classes win (C0, C1, C2).
        assert!(dsl.contains("component \"C0\""), "{dsl}");
        assert!(dsl.contains("component \"C1\""), "{dsl}");
        assert!(dsl.contains("component \"C2\""), "{dsl}");
        assert!(!dsl.contains("component \"C3\""), "cap breached:\n{dsl}");
    }

    /// A module with a single class emits exactly one component.
    #[test]
    fn generate_single_class_module_has_one_component() {
        let mut repo = Repository::new(PathBuf::from("/tmp/one-repo"));
        let mut m = Module {
            id: "g:solo".into(),
            ..Default::default()
        };
        m.classes.insert("s.Only".into(), class("s.Only"));
        repo.insert_module(m);

        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        assert_eq!(dsl.matches(" = component ").count(), 1, "{dsl}");
        assert!(dsl.contains("g_solo = container"), "{dsl}");
        assert!(dsl.contains("g_solo_Only = component \"Only\""), "{dsl}");
    }

    /// A module with no classes emits a plain `container` with no components and
    /// no opening brace.
    #[test]
    fn generate_empty_module_has_no_components() {
        let mut repo = Repository::new(PathBuf::from("/tmp/empty-mod-repo"));
        repo.insert_module(Module {
            id: "g:hollow".into(),
            ..Default::default()
        });

        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        assert_eq!(dsl.matches(" = component ").count(), 0, "{dsl}");
        // Plain container line (no trailing `{`).
        assert!(
            dsl.contains("g_hollow = container \"hollow\" \"0 classes\"\n"),
            "empty module should be a plain container:\n{dsl}"
        );
    }

    /// Two modules that each hold a class of the same simple name still produce
    /// collision-free component ids (module-prefixed).
    #[test]
    fn generate_component_ids_are_collision_free_across_modules() {
        let mut repo = Repository::new(PathBuf::from("/tmp/collide-repo"));
        let mut a = Module {
            id: "g:alpha".into(),
            ..Default::default()
        };
        a.classes.insert("a.Config".into(), class("a.Config"));
        let mut b = Module {
            id: "g:beta".into(),
            ..Default::default()
        };
        b.classes.insert("b.Config".into(), class("b.Config"));
        repo.insert_module(a);
        repo.insert_module(b);

        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        assert!(
            dsl.contains("g_alpha_Config = component \"Config\""),
            "{dsl}"
        );
        assert!(
            dsl.contains("g_beta_Config = component \"Config\""),
            "{dsl}"
        );

        // Parsing back gives one component under each container — no clobber.
        let model = parse_c4_dsl(&dsl);
        let sys = &model.systems[0];
        for c in &sys.containers {
            assert_eq!(
                c.components.len(),
                1,
                "container {} :{:?}",
                c.id,
                c.components
            );
        }
        // All component ids across the whole model are distinct.
        let mut ids: Vec<&str> = sys
            .containers
            .iter()
            .flat_map(|c| c.components.iter().map(|comp| comp.id.as_str()))
            .collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate component ids: {ids:?}");
    }

    /// Stereotype priority breaks ties when fan-in is equal (here: all zero),
    /// so a controller outranks a plain class regardless of name order.
    #[test]
    fn generate_orders_components_by_stereotype_then_name() {
        let mut repo = Repository::new(PathBuf::from("/tmp/stereo-repo"));
        let mut m = Module {
            id: "g:web".into(),
            ..Default::default()
        };
        // Name order alone would prefer Aaa/Bbb; stereotype must win.
        m.classes.insert("w.Aaa".into(), class("w.Aaa")); // no stereotype
        m.classes.insert("w.Bbb".into(), class("w.Bbb")); // no stereotype
        m.classes.insert(
            "w.Repo".into(),
            class_with_stereotype("w.Repo", "repository"),
        );
        m.classes.insert(
            "w.Ctrl".into(),
            class_with_stereotype("w.Ctrl", "controller"),
        );
        repo.insert_module(m);

        let dsl = generate_c4_dsl(&repo, &NoRelFw);
        // Cap 3 → controller, repository, then the alphabetically-first plain
        // class (Aaa). Bbb is dropped.
        assert!(dsl.contains("component \"Ctrl\" \"controller\""), "{dsl}");
        assert!(dsl.contains("component \"Repo\" \"repository\""), "{dsl}");
        assert!(dsl.contains("component \"Aaa\" \"class\""), "{dsl}");
        assert!(
            !dsl.contains("component \"Bbb\""),
            "Bbb should be capped:\n{dsl}"
        );
        // Order in the text: Ctrl before Repo before Aaa.
        let ctrl = dsl.find("\"Ctrl\"").unwrap();
        let repo_pos = dsl.find("\"Repo\"").unwrap();
        let aaa = dsl.find("\"Aaa\"").unwrap();
        assert!(ctrl < repo_pos && repo_pos < aaa, "wrong order:\n{dsl}");
    }

    /// Fan-in beats stereotype: a highly-referenced plain class outranks a
    /// controller with no incoming edges.
    #[test]
    fn generate_ranks_high_fan_in_first() {
        let repo = cross_repo();
        // In cross_repo, core.Heart has fan-in 2 (both api classes call it) and
        // is a plain class; the api classes have fan-in 0.
        let dsl = generate_c4_dsl(&repo, &CrossFw);
        assert!(
            dsl.contains("g_core_Heart = component \"Heart\""),
            "high-fan-in class must be a component:\n{dsl}"
        );
    }

    /// generate → parse → to_mermaid keeps the component level end-to-end.
    #[test]
    fn round_trip_preserves_components_to_mermaid() {
        let repo = cross_repo();
        let dsl = generate_c4_dsl(&repo, &CrossFw);
        assert!(
            dsl.contains(" = component "),
            "generate emits components:\n{dsl}"
        );
        let model = parse_c4_dsl(&dsl);
        let mermaid = c4_model_to_mermaid(&model);
        assert!(
            mermaid.contains("Component("),
            "mermaid keeps components:\n{mermaid}"
        );
        assert!(
            mermaid.contains("Container_Boundary("),
            "mermaid nests components in a boundary:\n{mermaid}"
        );
    }

    #[test]
    fn scaffold_creates_then_does_not_clobber() {
        let dir = std::env::temp_dir().join(format!("pm-c4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut repo = Repository::new(dir.clone());
        let mut m = Module {
            id: "g:api".into(),
            ..Default::default()
        };
        m.classes.insert("a.A".into(), class("a.A"));
        repo.insert_module(m);

        let first = scaffold_c4_model(&repo, &NoRelFw).unwrap();
        assert!(first.created, "first scaffold should create");
        assert!(first.path.ends_with("docs/architecture.dsl"));
        assert!(first.path.exists());

        // User edits the file.
        let edited = "workspace { model { x = softwareSystem \"Edited\" \"mine\" } }\n";
        std::fs::write(&first.path, edited).unwrap();

        let second = scaffold_c4_model(&repo, &NoRelFw).unwrap();
        assert!(!second.created, "second scaffold must NOT clobber");
        assert_eq!(
            std::fs::read_to_string(&second.path).unwrap(),
            edited,
            "user edits must survive"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Semantic merge (V6.3, #142)
    // -----------------------------------------------------------------------

    /// Build a repo with an extra module beyond `cross_repo`, so a merge against
    /// a model that only knows `cross_repo` has a new container to add.
    fn grown_repo() -> Repository {
        let mut repo = cross_repo();
        let mut extra = Module {
            id: "g:extra".into(),
            ..Default::default()
        };
        extra.classes.insert("x.New".into(), class("x.New"));
        repo.insert_module(extra);
        repo
    }

    /// A new module in the code (not in the DSL) is added as a whole container,
    /// with its component, and the counts reflect it — while the untouched
    /// containers stay byte-identical.
    #[test]
    fn merge_adds_new_container_and_component() {
        // Start from the model of the smaller repo, then merge the grown repo.
        let base = generate_c4_dsl(&cross_repo(), &CrossFw);
        let result = merge_c4_dsl(&base, &grown_repo(), &CrossFw);

        assert_eq!(result.added_containers, 1, "one new container");
        assert!(
            result.added_components >= 1,
            "new container brings its component(s): {result:?}"
        );
        assert!(
            result.text.contains("g_extra = container \"extra\""),
            "new container spliced in:\n{}",
            result.text
        );
        assert!(
            result.text.contains("g_extra_New = component \"New\""),
            "new component spliced in:\n{}",
            result.text
        );
        // The pre-existing containers survive verbatim.
        assert!(result.text.contains("g_api = container \"api\""));
        assert!(result.text.contains("g_core = container \"core\""));
    }

    /// The generator's own header comments (the `# ProjectMind …` banner) and
    /// full indentation survive a merge of the real scaffolded format — the
    /// common case: scaffold, let the code grow, then update.
    #[test]
    fn merge_preserves_generated_header_and_indent() {
        let base = generate_c4_dsl(&cross_repo(), &CrossFw);
        assert!(base.starts_with("# ProjectMind C4 model"), "sanity: {base}");
        let result = merge_c4_dsl(&base, &grown_repo(), &CrossFw);
        assert!(!result.is_empty(), "the new module should be added");
        assert!(
            result.text.starts_with("# ProjectMind C4 model"),
            "header banner survives:\n{}",
            result.text
        );
        assert!(
            result
                .text
                .contains("# See https://github.com/Plaintext-Gmbh/projectmind/issues/142"),
            "all header lines survive:\n{}",
            result.text
        );
        // The merged text is still valid, parseable DSL with the new module.
        let model = parse_c4_dsl(&result.text);
        assert!(model.systems[0]
            .containers
            .iter()
            .any(|c| c.id == "g_extra"));
    }

    /// A user-rewritten description on an existing element is NEVER changed by a
    /// merge, even though the generator would emit a different description.
    #[test]
    fn merge_preserves_user_edited_descriptions() {
        let mut base = generate_c4_dsl(&cross_repo(), &CrossFw);
        // Rewrite the api container's description to something the generator
        // would never produce.
        base = base.replace(
            "g_api = container \"api\" \"2 classes\"",
            "g_api = container \"api\" \"MY OWN WORDS\"",
        );
        assert!(base.contains("MY OWN WORDS"));

        let result = merge_c4_dsl(&base, &grown_repo(), &CrossFw);
        assert!(
            result
                .text
                .contains("g_api = container \"api\" \"MY OWN WORDS\""),
            "user description must survive verbatim:\n{}",
            result.text
        );
        assert!(
            !result
                .text
                .contains("g_api = container \"api\" \"2 classes\""),
            "merge must not re-emit the generated description:\n{}",
            result.text
        );
    }

    /// Elements the user ADDED by hand (external system, extra actor, custom
    /// relationship) and their comments survive a merge untouched.
    #[test]
    fn merge_preserves_user_added_elements_and_comments() {
        let base = r#"# my hand-written notes
workspace {
    model {
        developer = person "Developer" "Browses architecture via ProjectMind"
        # an external system I added myself
        stripe = softwareSystem "Stripe" "Payments — external"
        auditor = person "Auditor" "Reviews releases"
        my_repo = softwareSystem "my-repo" "Architecture of the opened repository" {
            g_api = container "api" "2 classes" {
                g_api_A = component "A" "class"
                g_api_B = component "B" "class"
            }
            g_core = container "core" "1 class" {
                g_core_Heart = component "Heart" "class"
            }
        }
        developer -> g_core "explores"
        g_api -> g_core "uses"
        auditor -> stripe "audits"  # my custom relationship
    }
}
"#;
        let result = merge_c4_dsl(base, &grown_repo(), &CrossFw);
        // The hand-authored comment, external system, actor and custom rel all
        // survive verbatim.
        assert!(
            result.text.contains("# my hand-written notes"),
            "{}",
            result.text
        );
        assert!(
            result.text.contains("# an external system I added myself"),
            "{}",
            result.text
        );
        assert!(
            result
                .text
                .contains("stripe = softwareSystem \"Stripe\" \"Payments — external\""),
            "{}",
            result.text
        );
        assert!(
            result
                .text
                .contains("auditor = person \"Auditor\" \"Reviews releases\""),
            "{}",
            result.text
        );
        assert!(
            result
                .text
                .contains("auditor -> stripe \"audits\"  # my custom relationship"),
            "custom rel + its comment survive:\n{}",
            result.text
        );
        // And the new module was still folded in.
        assert!(
            result.text.contains("g_extra = container \"extra\""),
            "{}",
            result.text
        );
    }

    /// Nothing new in the code → the text is returned byte-identical and all
    /// counts are zero.
    #[test]
    fn merge_noop_when_already_current() {
        let base = generate_c4_dsl(&cross_repo(), &CrossFw);
        let result = merge_c4_dsl(&base, &cross_repo(), &CrossFw);
        assert_eq!(result.added_containers, 0);
        assert_eq!(result.added_components, 0);
        assert_eq!(result.added_relationships, 0);
        assert!(result.is_empty());
        assert_eq!(result.text, base, "up-to-date merge must be a no-op");
    }

    /// Merging twice yields the same result as merging once (idempotent): the
    /// second pass finds nothing to add.
    #[test]
    fn merge_is_idempotent() {
        let base = generate_c4_dsl(&cross_repo(), &CrossFw);
        let once = merge_c4_dsl(&base, &grown_repo(), &CrossFw);
        assert!(!once.is_empty(), "first merge should add the new module");
        let twice = merge_c4_dsl(&once.text, &grown_repo(), &CrossFw);
        assert!(twice.is_empty(), "second merge must add nothing: {twice:?}");
        assert_eq!(twice.text, once.text, "idempotent text");
    }

    /// A brand-new cross-module relationship in the code is appended into the
    /// model block, after the existing relationships, without disturbing them.
    #[test]
    fn merge_adds_new_relationship() {
        // Model that has both containers but is MISSING the g_api -> g_core edge.
        let base = r#"workspace {
    model {
        developer = person "Developer" "d"
        my_repo = softwareSystem "my-repo" "d" {
            g_api = container "api" "2 classes" {
                g_api_A = component "A" "class"
                g_api_B = component "B" "class"
            }
            g_core = container "core" "1 class" {
                g_core_Heart = component "Heart" "class"
            }
        }
        developer -> g_core "explores"
    }
}
"#;
        let result = merge_c4_dsl(base, &cross_repo(), &CrossFw);
        assert_eq!(
            result.added_relationships, 1,
            "the cross edge is added: {result:?}"
        );
        assert!(
            result.text.contains("g_api -> g_core \"uses\""),
            "new relationship spliced into the model block:\n{}",
            result.text
        );
        // The developer->explores relationship is still there and unique.
        assert_eq!(
            result
                .text
                .matches("developer -> g_core \"explores\"")
                .count(),
            1
        );
    }

    /// A new component for an EXISTING container is inserted inside that
    /// container's brace block, not as a sibling.
    #[test]
    fn merge_adds_component_into_existing_container_block() {
        // Model where g_api only lists A (missing B); the code has both.
        let base = r#"workspace {
    model {
        developer = person "Developer" "d"
        my_repo = softwareSystem "my-repo" "d" {
            g_api = container "api" "2 classes" {
                g_api_A = component "A" "class"
            }
            g_core = container "core" "1 class" {
                g_core_Heart = component "Heart" "class"
            }
        }
        developer -> g_core "explores"
        g_api -> g_core "uses"
    }
}
"#;
        let result = merge_c4_dsl(base, &cross_repo(), &CrossFw);
        assert_eq!(result.added_components, 1, "B added: {result:?}");
        assert_eq!(result.added_containers, 0);
        // B must appear, and the parse must attach it under g_api (not loose).
        let model = parse_c4_dsl(&result.text);
        let sys = &model.systems[0];
        let api = sys.containers.iter().find(|c| c.id == "g_api").unwrap();
        let names: Vec<&str> = api.components.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"A") && names.contains(&"B"), "{names:?}");
    }

    /// A single-line container (no `{}` block) that gains its first component is
    /// reopened into a block, keeping its id/name/description and any trailing
    /// comment.
    #[test]
    fn merge_reopens_single_line_container_for_new_component() {
        let base = r#"workspace {
    model {
        developer = person "Developer" "d"
        my_repo = softwareSystem "my-repo" "d" {
            g_core = container "core" "1 class"   // I inlined this one
        }
        developer -> g_core "explores"
    }
}
"#;
        // Code has g_core with class Heart → one component to add.
        let mut repo = Repository::new(PathBuf::from("/tmp/my-repo"));
        let mut core = Module {
            id: "g:core".into(),
            ..Default::default()
        };
        core.classes
            .insert("core.Heart".into(), class("core.Heart"));
        repo.insert_module(core);

        let result = merge_c4_dsl(base, &repo, &NoRelFw);
        assert_eq!(result.added_components, 1, "{result:?}");
        // The trailing comment survives and the block now holds the component.
        assert!(
            result.text.contains("// I inlined this one"),
            "trailing comment survives:\n{}",
            result.text
        );
        assert!(
            result.text.contains("g_core_Heart = component \"Heart\""),
            "component added:\n{}",
            result.text
        );
        // Parse-level check: the component attaches under g_core.
        let model = parse_c4_dsl(&result.text);
        let core = model.systems[0]
            .containers
            .iter()
            .find(|c| c.id == "g_core")
            .unwrap();
        assert_eq!(core.components.len(), 1);
        assert_eq!(core.components[0].name, "Heart");
    }

    /// The merged output still parses and renders to Mermaid end-to-end (the
    /// spliced text is valid DSL, not just a string blob).
    #[test]
    fn merge_output_still_round_trips_to_mermaid() {
        let base = generate_c4_dsl(&cross_repo(), &CrossFw);
        let result = merge_c4_dsl(&base, &grown_repo(), &CrossFw);
        let model = parse_c4_dsl(&result.text);
        let mermaid = c4_model_to_mermaid(&model);
        assert!(mermaid.starts_with("C4Container\n"), "{mermaid}");
        assert!(
            mermaid.contains("Container_Boundary(g_extra, \"extra\")"),
            "new module reaches mermaid:\n{mermaid}"
        );
    }

    /// `merge_c4_model` scaffolds when the file is absent, then merges in place
    /// on later calls — never clobbering user edits, only adding.
    #[test]
    fn merge_c4_model_scaffolds_then_merges_in_place() {
        let dir = std::env::temp_dir().join(format!("pm-c4-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // First: file absent → merge behaves like scaffold (created:true).
        let repo1 = {
            let mut r = Repository::new(dir.clone());
            let mut m = Module {
                id: "g:api".into(),
                ..Default::default()
            };
            m.classes.insert("a.A".into(), class("a.A"));
            r.insert_module(m);
            r
        };
        let first = merge_c4_model(&repo1, &NoRelFw).unwrap();
        assert!(first.created, "absent file → scaffold: {first:?}");
        assert_eq!(first.added_containers, 0, "fresh scaffold reports no adds");
        assert!(first.path.exists());

        // User hand-edits the api container's description.
        let path = first.path.clone();
        let edited = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"1 class\"", "\"MY OWN WORDS\"");
        std::fs::write(&path, &edited).unwrap();

        // Second: the code grows a module → merge adds it, keeps the edit.
        let repo2 = {
            let mut r = repo1;
            let mut m = Module {
                id: "g:new".into(),
                ..Default::default()
            };
            m.classes.insert("n.N".into(), class("n.N"));
            r.insert_module(m);
            r
        };
        let second = merge_c4_model(&repo2, &NoRelFw).unwrap();
        assert!(!second.created, "existing file → merge in place");
        assert_eq!(second.added_containers, 1, "new module added: {second:?}");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("MY OWN WORDS"),
            "user edit survived:\n{after}"
        );
        assert!(after.contains("g_new = container \"new\""), "{after}");

        // Third: nothing new → file untouched, counts zero.
        let third = merge_c4_model(&repo2, &NoRelFw).unwrap();
        assert!(third.is_empty_adds(), "no-op merge: {third:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after, "no churn");

        let _ = std::fs::remove_dir_all(&dir);
    }

    impl MergeModelResult {
        /// Test helper: no elements were added.
        fn is_empty_adds(&self) -> bool {
            self.added_containers == 0
                && self.added_components == 0
                && self.added_relationships == 0
        }
    }
}

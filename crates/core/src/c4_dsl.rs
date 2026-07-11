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
//! 2. [`scaffold_c4_model`] writes that DSL to `docs/architecture.dsl` **once**
//!    — it never clobbers an existing file, so after the first scaffold the file
//!    is owned by the user and versioned in Git.
//! 3. [`parse_c4_dsl`] reads the (possibly hand-edited) DSL back into a
//!    [`C4Model`], and [`c4_model_to_mermaid`] renders that model as the exact
//!    same Mermaid `C4Container` text the frontend already knows how to draw.
//!
//! Round-trip honesty: the scaffold generates the DSL **once**. From then on
//! `docs/architecture.dsl` is the source of truth and ProjectMind never
//! rewrites it — there is deliberately no semantic merge of regenerated
//! structure with user edits (a follow-up candidate). To regenerate from
//! scratch, delete the file and scaffold again.
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

/// Generate a Structurizr-DSL subset describing `repo`'s container view.
///
/// The mapping is identical to [`crate::diagram::render_c4_container`]: one
/// `container` per module, cross-module framework relations become
/// relationships, and a `developer` person is anchored on the busiest module
/// (highest cross-module in-degree). Output is deterministic — modules and
/// relationships are sorted — so regenerating an unchanged repo yields byte-
/// identical DSL.
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
        let _ = writeln!(
            out,
            "            {id} = container \"{}\" \"{}\"",
            dsl_str(label),
            dsl_str(&descr)
        );
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
/// The output format mirrors [`crate::diagram::render_c4_container`] byte-for-
/// byte (same `C4Container` header, `title`, `Person(...)`, `System_Boundary`
/// block, `Container(...)` lines and `Rel(...)` edges) so the frontend's
/// existing Mermaid renderer draws it unchanged.
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
            let _ = writeln!(
                out,
                "        Container({}, \"{}\", \"{}\")",
                escape_id(&c.id),
                c4_label(&c.name),
                c4_label(&c.description)
            );
            for comp in &c.components {
                let _ = writeln!(
                    out,
                    "        Component({}, \"{}\", \"{}\")",
                    escape_id(&comp.id),
                    c4_label(&comp.name),
                    c4_label(&comp.description)
                );
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
        assert!(
            dsl.contains("g_api = container \"api\" \"2 classes\""),
            "api container missing:\n{dsl}"
        );
        assert!(
            dsl.contains("g_core = container \"core\" \"1 class\""),
            "core container missing/plural-wrong:\n{dsl}"
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
        assert!(
            mermaid.contains("Container(g_api, \"api\", \"2 classes\")"),
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
}

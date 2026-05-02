// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Mermaid diagram rendering shared between the MCP server and the Tauri shell.
//!
//! The renderer is intentionally pure: in → text → in. Visualizer plugins (frontend) can take
//! the same payload shape and render it differently.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use projectmind_plugin_api::{FrameworkPlugin, Relation, RelationKind};
use serde::Serialize;

use crate::Repository;

/// Mermaid `classDef` for each known stereotype. Unknown stereotypes get the default style.
const STEREOTYPE_STYLES: &[(&str, &str)] = &[
    ("service", "fill:#163a1d,stroke:#7ee787,color:#cdf6cd"),
    (
        "rest-controller",
        "fill:#1a2c4d,stroke:#79c0ff,color:#cfe6ff",
    ),
    ("controller", "fill:#1a2c4d,stroke:#58a6ff,color:#cfe6ff"),
    ("repository", "fill:#3a1d4d,stroke:#d2a8ff,color:#ecdcff"),
    ("component", "fill:#3d2010,stroke:#ffa657,color:#fbe7d3"),
    ("configuration", "fill:#4d1d1d,stroke:#ff7b72,color:#ffd5d2"),
    ("lombok", "fill:#262626,stroke:#a0a0a0,color:#dddddd"),
];

const DEFAULT_STYLE: &str = "fill:#21262d,stroke:#6e7781,color:#c9d1d9";

/// Render the bean graph for the entire repo, grouped by module (subgraphs) and colour-coded
/// by primary stereotype.
#[must_use]
pub fn render_bean_graph(repo: &Repository, framework: &dyn FrameworkPlugin) -> String {
    let mut out = String::from("flowchart LR\n");

    // 1. Collect relations and the set of nodes they touch (so we don't dump every class).
    let mut all_relations: Vec<(String, Relation)> = Vec::new(); // (module_id, rel)
    let mut node_modules: BTreeMap<String, String> = BTreeMap::new(); // fqn → module_id
    for (mod_id, module) in &repo.modules {
        for rel in framework.relations(module) {
            node_modules
                .entry(rel.from.clone())
                .or_insert_with(|| mod_id.clone());
            node_modules
                .entry(rel.to.clone())
                .or_insert_with(|| mod_id.clone());
            all_relations.push((mod_id.clone(), rel));
        }
    }

    if all_relations.is_empty() {
        out.push_str("    empty[(no beans detected)]\n");
        return out;
    }

    // 2. Style classes per stereotype.
    let stereotype_for: BTreeMap<String, String> = stereotype_lookup(repo);

    // 3. Render subgraphs per module.
    let mut nodes_by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (fqn, mod_id) in &node_modules {
        nodes_by_module
            .entry(mod_id.clone())
            .or_default()
            .push(fqn.clone());
    }

    for (mod_id, mut nodes) in nodes_by_module {
        nodes.sort();
        let _ = writeln!(
            out,
            "    subgraph {}[\"{}\"]",
            escape_id(&mod_id),
            short_module(&mod_id)
        );
        for fqn in &nodes {
            let label = simple_name(fqn);
            let _ = writeln!(out, "        {}[\"{}\"]", escape_id(fqn), label);
        }
        out.push_str("    end\n");

        for fqn in &nodes {
            if let Some(stereo) = stereotype_for.get(fqn) {
                let _ = writeln!(
                    out,
                    "    class {} stereo_{}",
                    escape_id(fqn),
                    sanitize(stereo)
                );
            }
            // Drilldown: click a class node → host registers `onNodeClick`.
            let _ = writeln!(
                out,
                "    click {id} call onNodeClick(\"class\",\"{m}\",\"{fqn}\")",
                id = escape_id(fqn),
                m = js_arg(&mod_id),
                fqn = js_arg(fqn)
            );
        }
    }

    // 4. Edges with cross-module styling.
    out.push('\n');
    let mut cross_module_edges: BTreeSet<(String, String)> = BTreeSet::new();
    for (_, rel) in &all_relations {
        let from_mod = node_modules.get(&rel.from);
        let to_mod = node_modules.get(&rel.to);
        let symbol = match rel.kind {
            RelationKind::Extends => "==>",
            RelationKind::Implements => "-.->|impl|",
            RelationKind::Calls => "-.->",
            RelationKind::Annotated => "-.->|@|",
            // Default arrow for Injects, Uses, Other.
            _ => "-->",
        };
        let _ = writeln!(
            out,
            "    {} {symbol} {}",
            escape_id(&rel.from),
            escape_id(&rel.to)
        );
        if from_mod != to_mod {
            cross_module_edges.insert((rel.from.clone(), rel.to.clone()));
        }
    }
    out.push('\n');

    // 5. Highlight cross-module edges with a class.
    for (from, to) in &cross_module_edges {
        let _ = writeln!(out, "    linkStyle default stroke:#6e7781,stroke-width:1px");
        let _ = writeln!(out, "    %% cross-module: {from} -> {to}");
    }

    // 6. Mermaid classDef for each stereotype style.
    for (name, style) in STEREOTYPE_STYLES {
        let _ = writeln!(out, "    classDef stereo_{} {style}", sanitize(name));
    }
    let _ = writeln!(out, "    classDef stereo_default {DEFAULT_STYLE}");

    out
}

/// Render the package tree as Mermaid.
#[must_use]
pub fn render_package_tree(repo: &Repository) -> String {
    let mut tree: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new(); // module → pkg → classes
    for module in repo.modules.values() {
        for class in module.classes.values() {
            let pkg = class
                .fqn
                .rsplit_once('.')
                .map_or(String::from("<default>"), |(p, _)| p.to_owned());
            tree.entry(module.id.clone())
                .or_default()
                .entry(pkg)
                .or_default()
                .push(class.name.clone());
        }
    }

    let mut out = String::from("graph TD\n");
    if tree.is_empty() {
        out.push_str("    empty[(no classes)]\n");
        return out;
    }

    for (mod_id, packages) in &tree {
        let mod_node = escape_id(mod_id);
        let _ = writeln!(out, "    subgraph {mod_node}[\"{}\"]", short_module(mod_id));
        for (pkg, classes) in packages {
            let pkg_node = escape_id(&format!("{mod_id}::{pkg}"));
            let _ = writeln!(out, "        {pkg_node}[\"{pkg}\"]");
            for c in classes {
                let leaf = escape_id(&format!("{mod_id}::{pkg}::{c}"));
                let _ = writeln!(out, "        {leaf}[\"{c}\"]");
                let _ = writeln!(out, "        {pkg_node} --> {leaf}");
            }
        }
        out.push_str("    end\n");
        // Drilldown clicks: emit *outside* the subgraph block — Mermaid does not
        // allow `click` directives inside `subgraph` ... `end`.
        for (pkg, classes) in packages {
            let pkg_node = escape_id(&format!("{mod_id}::{pkg}"));
            let _ = writeln!(
                out,
                "    click {pkg_node} call onNodeClick(\"package\",\"{m}\",\"{p}\")",
                m = js_arg(mod_id),
                p = js_arg(pkg)
            );
            for c in classes {
                let leaf = escape_id(&format!("{mod_id}::{pkg}::{c}"));
                let fqn = if pkg == "<default>" {
                    c.clone()
                } else {
                    format!("{pkg}.{c}")
                };
                let _ = writeln!(
                    out,
                    "    click {leaf} call onNodeClick(\"class\",\"{m}\",\"{f}\")",
                    m = js_arg(mod_id),
                    f = js_arg(&fqn)
                );
            }
        }
    }
    out
}

/// Render a repository folder map as JSON.
///
/// The frontend can switch layouts (hierarchy, solar, ...), so this returns a
/// stable, read-only model rather than a concrete Mermaid diagram.
#[must_use]
pub fn render_folder_map(repo: &Repository) -> String {
    #[derive(Debug, Serialize)]
    struct FolderMap {
        root: PathBuf,
        max_depth: usize,
        truncated: bool,
        nodes: Vec<FolderNode>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct FolderNode {
        id: String,
        parent: Option<String>,
        label: String,
        path: PathBuf,
        kind: &'static str,
        depth: usize,
        weight: u32,
    }

    const MAX_DEPTH: usize = 5;
    const MAX_NODES: usize = 420;

    let root = repo.root.clone();
    let mut nodes = vec![FolderNode {
        id: ".".into(),
        parent: None,
        label: root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo")
            .to_string(),
        path: root.clone(),
        kind: "root",
        depth: 0,
        weight: 1,
    }];
    let mut truncated = false;

    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .parents(true)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .max_depth(Some(MAX_DEPTH))
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | "target" | "node_modules" | "dist" | ".svelte-kit"
            )
        })
        .build();

    for entry in walker.flatten() {
        if entry.path() == root {
            continue;
        }
        if nodes.len() >= MAX_NODES {
            truncated = true;
            break;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let depth = rel.components().count();
        let id = rel_id(rel);
        let parent = rel
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| ".".to_string(), rel_id);
        let kind = if entry.file_type().is_some_and(|t| t.is_dir()) {
            "folder"
        } else {
            "file"
        };
        nodes.push(FolderNode {
            id,
            parent: Some(parent),
            label: rel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string(),
            path: entry.path().to_path_buf(),
            kind,
            depth,
            weight: u32::from(kind == "file"),
        });
    }

    // Aggregate descendant file counts into folders. The weight drives visual
    // size in non-hierarchical layouts.
    let file_ids: Vec<String> = nodes
        .iter()
        .filter(|n| n.kind == "file")
        .map(|n| n.id.clone())
        .collect();
    let parent_by_id: BTreeMap<String, Option<String>> = nodes
        .iter()
        .map(|n| (n.id.clone(), n.parent.clone()))
        .collect();
    let mut weights: BTreeMap<String, u32> = BTreeMap::new();
    for id in file_ids {
        let mut cur = Some(id);
        while let Some(node_id) = cur {
            *weights.entry(node_id.clone()).or_default() += 1;
            cur = parent_by_id.get(&node_id).cloned().flatten();
        }
    }
    for node in &mut nodes {
        node.weight = weights.get(&node.id).copied().unwrap_or(1).max(1);
    }

    serde_json::to_string(&FolderMap {
        root,
        max_depth: MAX_DEPTH,
        truncated,
        nodes,
    })
    .unwrap_or_else(|_| "{}".to_string())
}

fn rel_id(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Render a repository-wide inheritance tree as Mermaid.
///
/// Every parsed class with at least one declared parent (`extends` or
/// `implements` / Rust trait-impl) becomes a node; an arrow goes from the
/// **parent** down to the **child** so visually higher = more abstract.
/// Classes are grouped by module subgraph; a parent type that resolves to
/// a class in the same repo becomes an internal node, otherwise a "ghost"
/// node is created in a synthetic `__external__` subgraph so external
/// supertypes (Object, Serializable, …) are still drawn but visually
/// separated.
///
/// Mermaid `flowchart TD` because vertical reads more naturally as
/// inheritance — pulling parents to the top and children to the bottom
/// matches how Java / Rust developers already mentally lay it out.
pub fn render_inheritance_tree(repo: &Repository) -> String {
    use projectmind_plugin_api::TypeRefKind;

    // Build a quick lookup of every parsed class by both its FQN and its
    // simple name, plus the module it lives in. Same-FQN match wins;
    // simple-name fallback uses the most-popular candidate when ambiguous
    // (rarely happens in well-organised repos but keeps the renderer
    // robust for samples). Resolution mirrors the GUI's crumb logic so
    // clicks land on the same class the user expects.
    let mut by_fqn: BTreeMap<String, &str> = BTreeMap::new();
    let mut by_simple: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new(); // simple → (fqn, module_id)
    for module in repo.modules.values() {
        for class in module.classes.values() {
            by_fqn.insert(class.fqn.clone(), module.id.as_str());
            by_simple
                .entry(class.name.clone())
                .or_default()
                .push((class.fqn.as_str(), module.id.as_str()));
        }
    }

    // Collect every edge first so we can decide which classes are involved
    // (only emit nodes for classes that participate, plus their resolved or
    // ghost parents). Edge tuple: (parent_id, child_fqn, kind).
    let mut edges: Vec<(String, String, TypeRefKind)> = Vec::new();
    let mut external_parents: BTreeMap<String, String> = BTreeMap::new(); // ghost-id → label
    for module in repo.modules.values() {
        for class in module.classes.values() {
            if class.super_types.is_empty() {
                continue;
            }
            for parent in &class.super_types {
                let head = parent.name.split('<').next().unwrap_or(&parent.name).trim();
                let resolved = resolve_super(head, &class.fqn, &by_fqn, &by_simple);
                let parent_id = match resolved {
                    Some(fqn) => fqn,
                    None => {
                        let ghost = format!("__ext::{head}");
                        external_parents
                            .entry(ghost.clone())
                            .or_insert_with(|| head.to_string());
                        ghost
                    }
                };
                edges.push((parent_id, class.fqn.clone(), parent.kind));
            }
        }
    }

    let mut out = String::from("flowchart TD\n");
    if edges.is_empty() {
        out.push_str("    empty[(no inheritance edges)]\n");
        return out;
    }

    // Bucket internal classes by module so we can group them in subgraphs.
    // Only include classes that actually appear in an edge (parent or
    // child) — keeps the diagram focused on connected types.
    let mut participating: BTreeMap<String, BTreeMap<String, &projectmind_plugin_api::Class>> =
        BTreeMap::new();
    let mut on_edge: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (p, c, _) in &edges {
        on_edge.insert(p.clone());
        on_edge.insert(c.clone());
    }
    for module in repo.modules.values() {
        for class in module.classes.values() {
            if !on_edge.contains(&class.fqn) {
                continue;
            }
            participating
                .entry(module.id.clone())
                .or_default()
                .insert(class.fqn.clone(), class);
        }
    }

    for (mod_id, classes) in &participating {
        let mod_node = escape_id(mod_id);
        let _ = writeln!(out, "    subgraph {mod_node}[\"{}\"]", short_module(mod_id));
        for class in classes.values() {
            let id = escape_id(&class.fqn);
            let _ = writeln!(out, "        {id}[\"{}\"]", class.name);
        }
        out.push_str("    end\n");
    }

    if !external_parents.is_empty() {
        out.push_str("    subgraph __ext__[\"external supertypes\"]\n");
        for (id, label) in &external_parents {
            let node = escape_id(id);
            let _ = writeln!(out, "        {node}([\"{label}\"])");
        }
        out.push_str("    end\n");
    }

    // Edges. extends → solid arrow; implements → dotted arrow. Mermaid
    // syntax: `A --> B` solid, `A -.-> B` dotted.
    for (parent, child, kind) in &edges {
        let p = escape_id(parent);
        let c = escape_id(child);
        let arrow = match kind {
            TypeRefKind::Extends => "-->",
            TypeRefKind::Implements => "-.->",
        };
        let _ = writeln!(out, "    {p} {arrow} {c}");
    }

    // Per-class drill-down clicks (parent ghosts are not clickable).
    for class_fqn in on_edge.iter().filter(|f| by_fqn.contains_key(*f)) {
        let id = escape_id(class_fqn);
        let mod_id = by_fqn.get(class_fqn).copied().unwrap_or("");
        let _ = writeln!(
            out,
            "    click {id} call onNodeClick(\"class\",\"{m}\",\"{f}\")",
            m = js_arg(mod_id),
            f = js_arg(class_fqn)
        );
    }

    out
}

/// Resolve a parent-type name to a class FQN, mirroring the GUI's three-tier
/// strategy: exact FQN, same-package match, then unique-by-simple-name.
/// Returns the resolved FQN as a String or None if no confident match.
fn resolve_super(
    name: &str,
    child_fqn: &str,
    by_fqn: &BTreeMap<String, &str>,
    by_simple: &BTreeMap<String, Vec<(&str, &str)>>,
) -> Option<String> {
    if by_fqn.contains_key(name) {
        return Some(name.to_string());
    }
    let simple = name.rsplit('.').next().unwrap_or(name);
    // Same-package wins over a global single match.
    if let Some(dot) = child_fqn.rfind('.') {
        let pkg = &child_fqn[..dot];
        let candidate = format!("{pkg}.{simple}");
        if by_fqn.contains_key(&candidate) {
            return Some(candidate);
        }
    }
    if let Some(matches) = by_simple.get(simple) {
        if matches.len() == 1 {
            return Some(matches[0].0.to_string());
        }
    }
    None
}

/// Escape a string for use as a Mermaid `click ... call fn("...")` argument.
/// Mermaid passes the literal text to JS, so we prevent quote/backslash injection.
fn js_arg(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn stereotype_lookup(repo: &Repository) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let priority = [
        "rest-controller",
        "controller",
        "service",
        "repository",
        "configuration",
        "component",
        "lombok",
    ];
    for module in repo.modules.values() {
        for class in module.classes.values() {
            if class.stereotypes.is_empty() {
                continue;
            }
            // pick the highest-priority stereotype the class has
            let chosen = priority
                .iter()
                .find(|p| class.stereotypes.iter().any(|s| s == *p))
                .copied()
                .unwrap_or_else(|| class.stereotypes[0].as_str());
            out.insert(class.fqn.clone(), chosen.to_string());
        }
    }
    out
}

fn escape_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn simple_name(fqn: &str) -> &str {
    fqn.rsplit_once('.').map_or(fqn, |(_, s)| s)
}

fn short_module(mod_id: &str) -> &str {
    // groupId:artifactId — show only artifactId
    mod_id.rsplit_once(':').map_or(mod_id, |(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{
        Annotation, Class, FrameworkPlugin, Module, PluginInfo, Result as PiResult,
    };

    struct DummyFw;
    impl FrameworkPlugin for DummyFw {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "dummy",
                name: "Dummy",
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
            // edge between every pair if both have stereotypes
            let mut out = Vec::new();
            let names: Vec<&str> = module
                .classes
                .values()
                .filter(|c| !c.stereotypes.is_empty())
                .map(|c| c.fqn.as_str())
                .collect();
            for w in names.windows(2) {
                out.push(Relation {
                    from: w[0].to_string(),
                    to: w[1].to_string(),
                    kind: RelationKind::Injects,
                });
            }
            out
        }
    }

    fn class(fqn: &str, stereo: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: simple_name(fqn).into(),
            stereotypes: vec![stereo.to_string()],
            annotations: vec![Annotation {
                name: "X".into(),
                fqn: None,
                raw_args: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn bean_graph_groups_by_module() {
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes.insert("a.A".into(), class("a.A", "service"));
        m1.classes.insert("a.B".into(), class("a.B", "controller"));
        repo.insert_module(m1);
        let out = render_bean_graph(&repo, &DummyFw);
        assert!(out.contains("subgraph"));
        assert!(out.contains("a_A"));
        assert!(out.contains("a_B"));
        assert!(out.contains("classDef stereo_service"));
        assert!(out.contains("class a_A stereo_service"));
    }

    #[test]
    fn empty_bean_graph_says_so() {
        let repo = Repository::default();
        let out = render_bean_graph(&repo, &DummyFw);
        assert!(out.contains("no beans detected"));
    }

    #[test]
    fn package_tree_groups_by_module_and_package() {
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes.insert("a.b.X".into(), class("a.b.X", "service"));
        repo.insert_module(m1);
        let out = render_package_tree(&repo);
        assert!(out.contains("subgraph"));
        assert!(out.contains("\"a.b\""));
        assert!(out.contains("\"X\""));
    }

    #[test]
    fn package_tree_emits_click_directives_for_drilldown() {
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes.insert("a.b.X".into(), class("a.b.X", "service"));
        repo.insert_module(m1);
        let out = render_package_tree(&repo);
        // package node click → onNodeClick("package", module, pkg)
        assert!(
            out.contains("call onNodeClick(\"package\",\"g:m1\",\"a.b\")"),
            "missing package click in:\n{out}"
        );
        // leaf class click → onNodeClick("class", module, fqn)
        assert!(
            out.contains("call onNodeClick(\"class\",\"g:m1\",\"a.b.X\")"),
            "missing class click in:\n{out}"
        );
    }

    #[test]
    fn bean_graph_emits_click_directives_for_drilldown() {
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes.insert("a.A".into(), class("a.A", "service"));
        m1.classes.insert("a.B".into(), class("a.B", "controller"));
        repo.insert_module(m1);
        let out = render_bean_graph(&repo, &DummyFw);
        assert!(
            out.contains("call onNodeClick(\"class\",\"g:m1\",\"a.A\")"),
            "missing class click in:\n{out}"
        );
    }

    fn class_with_supers(
        fqn: &str,
        supers: &[(projectmind_plugin_api::TypeRefKind, &str)],
    ) -> Class {
        Class {
            fqn: fqn.into(),
            name: simple_name(fqn).into(),
            super_types: supers
                .iter()
                .map(|(kind, name)| projectmind_plugin_api::TypeRef {
                    name: (*name).to_string(),
                    kind: *kind,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn inheritance_tree_resolves_internal_supers_and_uses_arrow_styles() {
        use projectmind_plugin_api::TypeRefKind::{Extends, Implements};
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        // a.User extends a.AbstractEntity, implements common.Marker.
        // AbstractEntity is in the same module, Marker isn't parsed → ghost.
        m1.classes.insert(
            "a.AbstractEntity".into(),
            class_with_supers("a.AbstractEntity", &[]),
        );
        m1.classes.insert(
            "a.User".into(),
            class_with_supers(
                "a.User",
                &[(Extends, "AbstractEntity"), (Implements, "Marker")],
            ),
        );
        repo.insert_module(m1);

        let out = render_inheritance_tree(&repo);
        // Internal extends: solid arrow from parent → child.
        assert!(
            out.contains("a_AbstractEntity --> a_User"),
            "expected solid extends arrow:\n{out}"
        );
        // External implements: dotted arrow from ghost → child. The ghost
        // node label must be the bare type name (no `__ext::` prefix
        // visible to the user); the id post-escape is `__ext__Marker`.
        assert!(
            out.contains("\"Marker\""),
            "expected external label:\n{out}"
        );
        assert!(
            out.contains("__ext__Marker -.-> a_User"),
            "expected dotted implements arrow from external:\n{out}"
        );
        // Module subgraph + external subgraph both present.
        assert!(
            out.contains("subgraph __ext__"),
            "external subgraph missing:\n{out}"
        );
    }

    #[test]
    fn inheritance_tree_drops_classes_with_no_super_types() {
        // A class with no parents and no children shouldn't appear in the
        // diagram — it has nothing to say. Keeps the picture focused on
        // hierarchies the user can actually navigate.
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes
            .insert("a.Loner".into(), class_with_supers("a.Loner", &[]));
        repo.insert_module(m1);
        let out = render_inheritance_tree(&repo);
        assert!(
            out.contains("no inheritance edges"),
            "expected empty marker:\n{out}"
        );
    }

    #[test]
    fn inheritance_tree_strips_generic_args_from_super_names() {
        use projectmind_plugin_api::TypeRefKind::Implements;
        let mut repo = Repository::default();
        let mut m1 = Module {
            id: "g:m1".into(),
            ..Default::default()
        };
        m1.classes.insert(
            "a.Bag".into(),
            class_with_supers("a.Bag", &[(Implements, "List<String>")]),
        );
        repo.insert_module(m1);
        let out = render_inheritance_tree(&repo);
        // Generic args stripped: ghost label is bare `List`, id is the
        // post-escape `__ext__List` (escape_id replaces `:` with `_`).
        assert!(
            out.contains("__ext__List"),
            "expected stripped generic ghost id:\n{out}"
        );
        assert!(out.contains("\"List\""), "expected bare label:\n{out}");
    }
}

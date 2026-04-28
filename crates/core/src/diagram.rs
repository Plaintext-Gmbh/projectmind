// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Mermaid diagram rendering shared between the MCP server and the Tauri shell.
//!
//! The renderer is intentionally pure: in → text → in. Visualizer plugins (frontend) can take
//! the same payload shape and render it differently.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use plaintext_ide_plugin_api::{FrameworkPlugin, Relation, RelationKind};

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
    }
    out
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
    use plaintext_ide_plugin_api::{
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
}

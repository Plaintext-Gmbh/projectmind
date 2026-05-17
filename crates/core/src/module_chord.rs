// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module dependency chord diagram.
//!
//! Renders one segment per module around the perimeter of a circle and
//! one chord (Bezier) per directed cross-module edge. Self-edges (a
//! module's classes referencing each other) are aggregated into the
//! module's own segment metadata so the diagram stays focused on
//! cross-module coupling.
//!
//! The payload is JSON: the SVG itself is rendered client-side in
//! `app/src/components/DiagramView.svelte` so hover / selection state
//! can be reactive.

use std::collections::BTreeMap;

use projectmind_plugin_api::{FrameworkPlugin, RelationKind};
use serde::Serialize;

use crate::Repository;

/// One module along the chord-diagram rim.
#[derive(Debug, Clone, Serialize)]
pub struct ChordModule {
    /// Stable id (matches `repo.modules` key).
    pub id: String,
    /// Short display label.
    pub label: String,
    /// Number of classes parsed for this module.
    pub classes: usize,
    /// Number of outbound cross-module edges.
    pub outgoing: usize,
    /// Number of inbound cross-module edges.
    pub incoming: usize,
    /// Number of edges that stay inside this module (only counted, not drawn).
    pub internal: usize,
}

/// Aggregated edge between two modules.
#[derive(Debug, Clone, Serialize)]
pub struct ChordEdge {
    /// Source module id.
    pub from: String,
    /// Target module id.
    pub to: String,
    /// Number of underlying class-to-class relations folded into this edge.
    pub count: usize,
}

/// Payload shipped to the GUI.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleChord {
    /// Repository root for display.
    pub root: String,
    /// Modules around the perimeter, sorted alphabetically for stable
    /// layout (the GUI is welcome to reorder).
    pub modules: Vec<ChordModule>,
    /// Cross-module edges (self-edges excluded).
    pub edges: Vec<ChordEdge>,
    /// Total number of class-to-class relations considered.
    pub total_relations: usize,
}

/// Build the chord payload for `repo` using `framework` for relations.
#[must_use]
pub fn build(repo: &Repository, framework: &dyn FrameworkPlugin) -> ModuleChord {
    let mut module_of_fqn: BTreeMap<String, String> = BTreeMap::new();
    for (mod_id, module) in &repo.modules {
        for class in module.classes.values() {
            module_of_fqn.insert(class.fqn.clone(), mod_id.clone());
        }
    }

    let mut edge_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut total_relations = 0usize;
    let mut internal_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (mod_id, module) in &repo.modules {
        for rel in framework.relations(module) {
            if matches!(rel.kind, RelationKind::Annotated) {
                continue;
            }
            let Some(target_mod) = module_of_fqn.get(&rel.to) else {
                continue;
            };
            total_relations += 1;
            if target_mod == mod_id {
                *internal_counts.entry(mod_id.clone()).or_default() += 1;
                continue;
            }
            *edge_counts
                .entry((mod_id.clone(), target_mod.clone()))
                .or_default() += 1;
        }
    }

    // Build a per-module rollup.
    let mut module_meta: BTreeMap<String, ChordModule> = BTreeMap::new();
    for (mod_id, module) in &repo.modules {
        module_meta.insert(
            mod_id.clone(),
            ChordModule {
                id: mod_id.clone(),
                label: short_label(mod_id),
                classes: module.classes.len(),
                outgoing: 0,
                incoming: 0,
                internal: internal_counts.remove(mod_id).unwrap_or(0),
            },
        );
    }
    for ((from, to), count) in &edge_counts {
        if let Some(m) = module_meta.get_mut(from) {
            m.outgoing += count;
        }
        if let Some(m) = module_meta.get_mut(to) {
            m.incoming += count;
        }
    }

    let modules: Vec<ChordModule> = module_meta.into_values().collect();
    // Already alphabetical by BTreeMap. Edges: sort by weight desc, then
    // alphabetical for stable rendering.
    let mut edges: Vec<ChordEdge> = edge_counts
        .into_iter()
        .map(|((from, to), count)| ChordEdge { from, to, count })
        .collect();
    edges.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });

    ModuleChord {
        root: repo.root.to_string_lossy().to_string(),
        modules,
        edges,
        total_relations,
    }
}

/// Trim Maven-style `group:artifact` ids down to the artifact part.
fn short_label(id: &str) -> String {
    id.rsplit_once(':')
        .map_or_else(|| id.to_string(), |(_, last)| last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{
        Annotation, Class, FrameworkPlugin, Module, PluginInfo, Relation, RelationKind,
    };
    use std::path::PathBuf;

    struct DummyFw {
        relations: Vec<(String, Vec<Relation>)>,
    }
    impl FrameworkPlugin for DummyFw {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "t",
                name: "t",
                version: "0",
            }
        }
        fn supported_languages(&self) -> &[&'static str] {
            &["lang-test"]
        }
        fn enrich(&self, _: &mut Module) -> projectmind_plugin_api::Result<()> {
            Ok(())
        }
        fn relations(&self, module: &Module) -> Vec<Relation> {
            self.relations
                .iter()
                .find(|(m, _)| m == &module.id)
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        }
        fn provided_diagrams(&self) -> &[&'static str] {
            &[]
        }
    }

    fn klass(fqn: &str) -> Class {
        Class {
            name: fqn.rsplit('.').next().unwrap_or(fqn).to_string(),
            fqn: fqn.to_string(),
            file: PathBuf::from(format!("{fqn}.java")),
            annotations: vec![Annotation {
                name: "Marker".into(),
                fqn: None,
                raw_args: None,
            }],
            ..Default::default()
        }
    }

    fn mk_module(id: &str, classes: Vec<Class>) -> Module {
        let mut m = Module {
            id: id.to_string(),
            ..Default::default()
        };
        for c in classes {
            m.classes.insert(c.fqn.clone(), c);
        }
        m
    }

    fn repo_with(modules: Vec<Module>) -> Repository {
        let mut r = Repository {
            root: PathBuf::from("/tmp/chord"),
            ..Default::default()
        };
        for m in modules {
            r.insert_module(m);
        }
        r
    }

    #[test]
    fn empty_repo_is_empty() {
        let chord = build(&repo_with(vec![]), &DummyFw { relations: vec![] });
        assert!(chord.modules.is_empty());
        assert!(chord.edges.is_empty());
        assert_eq!(chord.total_relations, 0);
    }

    #[test]
    fn internal_edges_count_per_module_but_arent_drawn() {
        let a = klass("g:web.A");
        let b = klass("g:web.B");
        let module = mk_module("g:web", vec![a.clone(), b.clone()]);
        let fw = DummyFw {
            relations: vec![(
                "g:web".to_string(),
                vec![Relation {
                    from: a.fqn.clone(),
                    to: b.fqn.clone(),
                    kind: RelationKind::Injects,
                }],
            )],
        };
        let chord = build(&repo_with(vec![module]), &fw);
        let web = &chord.modules[0];
        assert_eq!(web.internal, 1);
        assert_eq!(web.outgoing, 0);
        assert!(chord.edges.is_empty());
    }

    #[test]
    fn cross_module_edges_are_aggregated() {
        let ctrl = klass("g:web.Ctrl");
        let svc = klass("g:core.Svc");
        let web = mk_module("g:web", vec![ctrl.clone()]);
        let core = mk_module("g:core", vec![svc.clone()]);
        let fw = DummyFw {
            relations: vec![
                (
                    "g:web".to_string(),
                    vec![
                        Relation {
                            from: ctrl.fqn.clone(),
                            to: svc.fqn.clone(),
                            kind: RelationKind::Injects,
                        },
                        Relation {
                            from: ctrl.fqn.clone(),
                            to: svc.fqn.clone(),
                            kind: RelationKind::Calls,
                        },
                    ],
                ),
                ("g:core".to_string(), vec![]),
            ],
        };
        let chord = build(&repo_with(vec![web, core]), &fw);
        assert_eq!(chord.edges.len(), 1);
        assert_eq!(chord.edges[0].count, 2);
        assert_eq!(chord.edges[0].from, "g:web");
        assert_eq!(chord.edges[0].to, "g:core");
        // Roll-up:
        let web_mod = chord.modules.iter().find(|m| m.id == "g:web").unwrap();
        let core_mod = chord.modules.iter().find(|m| m.id == "g:core").unwrap();
        assert_eq!(web_mod.outgoing, 2);
        assert_eq!(web_mod.incoming, 0);
        assert_eq!(core_mod.incoming, 2);
        assert_eq!(core_mod.outgoing, 0);
    }

    #[test]
    fn short_label_strips_maven_group() {
        assert_eq!(short_label("com.example:web"), "web");
        assert_eq!(short_label("plain-id"), "plain-id");
    }

    #[test]
    fn annotated_relations_are_excluded() {
        let a = klass("g:web.A");
        let b = klass("g:core.B");
        let fw = DummyFw {
            relations: vec![(
                "g:web".to_string(),
                vec![Relation {
                    from: a.fqn.clone(),
                    to: b.fqn.clone(),
                    kind: RelationKind::Annotated,
                }],
            )],
        };
        let chord = build(
            &repo_with(vec![
                mk_module("g:web", vec![a]),
                mk_module("g:core", vec![b]),
            ]),
            &fw,
        );
        assert!(chord.edges.is_empty());
        assert_eq!(chord.total_relations, 0);
    }
}

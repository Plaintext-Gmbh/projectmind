// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Architecture flow diagram: horizontal layer bands for a Spring-style
//! (or any layered) codebase.
//!
//! Classifies every parsed class into one of four canonical layers:
//!
//! - `external`   — controllers, REST endpoints, web handlers, CLI entries
//! - `business`   — services, domain logic, configuration
//! - `data`       — repositories, DAOs, persistence
//! - `domain`     — entities / DTOs / value objects (the data carriers
//!   themselves, not the access layer)
//!
//! Edges between layers come from framework relations (`Injects`, `Calls`,
//! `Uses`, …). Same-layer edges are aggregated into a single self-count;
//! cross-layer edges drive the visual flow arrows in the GUI.
//!
//! Output is a JSON-serialisable [`ArchitectureFlow`]. The renderer in
//! `app/src/components/DiagramView.svelte` consumes the same payload.

use std::collections::BTreeMap;

use projectmind_plugin_api::{Class, FrameworkPlugin, RelationKind};
use serde::Serialize;

use crate::Repository;

/// Canonical architecture layers, ordered top-to-bottom in the diagram.
const LAYER_ORDER: &[(&str, &str, &str, &str)] = &[
    (
        "external",
        "External / Controller",
        "REST endpoints, web handlers, CLI entry points",
        "#79c0ff",
    ),
    (
        "business",
        "Business / Service",
        "Service classes, configuration, business logic",
        "#7ee787",
    ),
    (
        "data",
        "Data Access",
        "Repositories, DAOs, persistence adapters",
        "#d2a8ff",
    ),
    (
        "domain",
        "Domain / Entity",
        "Entities, DTOs, value objects",
        "#ffa657",
    ),
];

/// One class entry inside a layer band.
#[derive(Debug, Clone, Serialize)]
pub struct FlowClass {
    /// Fully qualified name (used as a stable id).
    pub fqn: String,
    /// Short name for the badge label.
    pub name: String,
    /// Owning module id (short form ok — the GUI shortens further).
    pub module: String,
    /// Primary stereotype, if any (`"service"`, `"rest-controller"`, ...).
    pub stereotype: Option<String>,
}

/// One layer in the architecture-flow diagram.
#[derive(Debug, Clone, Serialize)]
pub struct FlowLayer {
    /// Stable id: `external`, `business`, `data`, `domain`.
    pub id: String,
    /// Display label rendered as the band header.
    pub label: String,
    /// Short description shown below the label.
    pub description: String,
    /// Hex colour used for the band accent and class chips.
    pub color: String,
    /// Classes assigned to this layer, sorted by name for stable layout.
    pub classes: Vec<FlowClass>,
    /// Histogram of stereotype → class count for the sidebar.
    pub stereotypes: BTreeMap<String, usize>,
}

/// Aggregated edge between two layers.
#[derive(Debug, Clone, Serialize)]
pub struct FlowEdge {
    /// Layer id the edge originates from.
    pub from: String,
    /// Layer id the edge points to.
    pub to: String,
    /// Number of underlying class-to-class relations folded into this edge.
    pub count: usize,
}

/// Complete payload shipped to the GUI.
#[derive(Debug, Clone, Serialize)]
pub struct ArchitectureFlow {
    /// Repository root (absolute path, just for display).
    pub root: String,
    /// Total number of parsed classes considered.
    pub total_classes: usize,
    /// Total number of parsed modules.
    pub total_modules: usize,
    /// Number of edges that cross module boundaries.
    pub cross_module_edges: usize,
    /// Bands rendered top-to-bottom.
    pub layers: Vec<FlowLayer>,
    /// Edges between layers (excluding self-edges).
    pub edges: Vec<FlowEdge>,
}

/// Build the architecture-flow payload for `repo`, classifying with help
/// from the given `framework` plugin (its stereotype hints drive most of
/// the layer assignment).
#[must_use]
pub fn build(repo: &Repository, framework: &dyn FrameworkPlugin) -> ArchitectureFlow {
    let mut by_layer: BTreeMap<&'static str, Vec<FlowClass>> = BTreeMap::new();
    let mut stereo_per_layer: BTreeMap<&'static str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut layer_of_fqn: BTreeMap<String, &'static str> = BTreeMap::new();
    let mut module_of_fqn: BTreeMap<String, String> = BTreeMap::new();
    let mut total_classes = 0usize;

    for (mod_id, module) in &repo.modules {
        for class in module.classes.values() {
            total_classes += 1;
            let layer = classify(class);
            let primary = primary_stereotype(class);
            let entry = FlowClass {
                fqn: class.fqn.clone(),
                name: class.name.clone(),
                module: mod_id.clone(),
                stereotype: primary.clone(),
            };
            by_layer.entry(layer).or_default().push(entry);
            layer_of_fqn.insert(class.fqn.clone(), layer);
            module_of_fqn.insert(class.fqn.clone(), mod_id.clone());
            if let Some(s) = primary {
                *stereo_per_layer
                    .entry(layer)
                    .or_default()
                    .entry(s)
                    .or_default() += 1;
            }
        }
    }

    // Build layers in canonical order so the GUI doesn't have to sort.
    let mut layers = Vec::with_capacity(LAYER_ORDER.len());
    for (id, label, descr, color) in LAYER_ORDER {
        let mut classes = by_layer.remove(*id).unwrap_or_default();
        classes.sort_by(|a, b| a.name.cmp(&b.name).then(a.fqn.cmp(&b.fqn)));
        let stereotypes = stereo_per_layer.remove(*id).unwrap_or_default();
        layers.push(FlowLayer {
            id: (*id).to_string(),
            label: (*label).to_string(),
            description: (*descr).to_string(),
            color: (*color).to_string(),
            classes,
            stereotypes,
        });
    }

    // Edge aggregation: every framework relation contributes 1.
    let mut edge_counts: BTreeMap<(&'static str, &'static str), usize> = BTreeMap::new();
    let mut cross_module_edges = 0usize;
    for (mod_id, module) in &repo.modules {
        for rel in framework.relations(module) {
            // Skip relations to classes we never parsed — they exist in
            // the bean graph as orphans but make no sense in the layered
            // view.
            let Some(from_layer) = layer_of_fqn.get(&rel.from).copied() else {
                continue;
            };
            let Some(to_layer) = layer_of_fqn.get(&rel.to).copied() else {
                continue;
            };
            // We only count what the layered model actually conveys: code
            // pointing at another piece of code. `Annotated` is metadata,
            // not a runtime flow.
            if matches!(rel.kind, RelationKind::Annotated) {
                continue;
            }
            *edge_counts.entry((from_layer, to_layer)).or_default() += 1;
            if let Some(target_mod) = module_of_fqn.get(&rel.to) {
                if target_mod != mod_id {
                    cross_module_edges += 1;
                }
            }
        }
    }

    // Drop self-edges from the visible edge list (still counted in the
    // module histogram via `cross_module_edges`).
    let mut edges: Vec<FlowEdge> = edge_counts
        .into_iter()
        .filter(|((from, to), _)| from != to)
        .map(|((from, to), count)| FlowEdge {
            from: (*from).to_string(),
            to: (*to).to_string(),
            count,
        })
        .collect();
    // Stable ordering: heaviest first, then by layer id for ties.
    edges.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });

    ArchitectureFlow {
        root: repo.root.to_string_lossy().to_string(),
        total_classes,
        total_modules: repo.modules.len(),
        cross_module_edges,
        layers,
        edges,
    }
}

/// Classify a class into one of the four canonical layers.
///
/// Priority is stereotype → annotation → name/path hint → fallback to
/// `business`. The first hit wins so a `@Repository` named `OrderRepo`
/// lands on `data` even though `Order` also contains the substring
/// `order`.
fn classify(class: &Class) -> &'static str {
    let stereos: Vec<String> = class
        .stereotypes
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let stereo_match = |needles: &[&str]| -> bool {
        stereos
            .iter()
            .any(|s| needles.iter().any(|n| s.contains(n)))
    };

    if stereo_match(&["controller", "rest", "endpoint", "handler", "resource"]) {
        return "external";
    }
    if stereo_match(&["repository", "dao"]) {
        return "data";
    }
    if stereo_match(&["entity", "dto", "value-object", "value_object", "model"]) {
        return "domain";
    }
    if stereo_match(&["service", "component", "configuration"]) {
        return "business";
    }

    let annotations = class
        .annotations
        .iter()
        .map(|a| a.name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let ann_match = |needles: &[&str]| -> bool {
        annotations
            .iter()
            .any(|a| needles.iter().any(|n| a.contains(n)))
    };
    if ann_match(&["restcontroller", "controller", "requestmapping", "endpoint"]) {
        return "external";
    }
    if ann_match(&["repository"]) {
        return "data";
    }
    if ann_match(&["entity", "table", "embeddable"]) {
        return "domain";
    }
    if ann_match(&["service", "component", "configuration"]) {
        return "business";
    }

    // Name + path hint as a last resort. Lombok-only / utility classes
    // fall through here.
    let hay = format!(
        "{} {}",
        class.name.to_ascii_lowercase(),
        class.file.display().to_string().to_ascii_lowercase()
    );
    if contains_any(
        &hay,
        &[
            "controller",
            "endpoint",
            "handler",
            "router",
            "/web/",
            "/api/",
            "/rest/",
            "/http/",
            "/cli/",
        ],
    ) {
        return "external";
    }
    if contains_any(
        &hay,
        &[
            "repository",
            "repo",
            "dao",
            "/persistence/",
            "/storage/",
            "/db/",
        ],
    ) {
        return "data";
    }
    if contains_any(
        &hay,
        &[
            "entity",
            "dto",
            "model",
            "/entity/",
            "/entities/",
            "/domain/",
            "/dto/",
            "/dtos/",
            "/model/",
            "/models/",
        ],
    ) {
        return "domain";
    }
    "business"
}

fn primary_stereotype(class: &Class) -> Option<String> {
    class.stereotypes.first().cloned()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
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
                id: "test-fw",
                name: "Test",
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

    fn klass(name: &str, stereotypes: &[&str], annotations: &[&str], file: &str) -> Class {
        Class {
            name: name.to_string(),
            fqn: format!("test.{name}"),
            file: PathBuf::from(file),
            stereotypes: stereotypes.iter().map(|s| (*s).to_string()).collect(),
            annotations: annotations
                .iter()
                .map(|a| Annotation {
                    name: (*a).to_string(),
                    fqn: None,
                    raw_args: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn mk_module(id: &str, classes: Vec<Class>) -> Module {
        let mut module = Module {
            id: id.to_string(),
            ..Default::default()
        };
        for c in classes {
            module.classes.insert(c.fqn.clone(), c);
        }
        module
    }

    fn repo_with(modules: Vec<Module>) -> Repository {
        let mut repo = Repository {
            root: PathBuf::from("/tmp/test-repo"),
            ..Default::default()
        };
        for m in modules {
            repo.insert_module(m);
        }
        repo
    }

    #[test]
    fn empty_repo_renders_empty_layers() {
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![]), &fw);
        assert_eq!(flow.total_classes, 0);
        assert_eq!(flow.layers.len(), 4);
        assert!(flow.layers.iter().all(|l| l.classes.is_empty()));
        assert!(flow.edges.is_empty());
    }

    #[test]
    fn stereotype_classification_wins_over_name_hint() {
        // A class whose name says "controller" but whose stereotype says
        // "repository" must land on `data`.
        let c = klass(
            "UserController",
            &["repository"],
            &[],
            "src/UserController.java",
        );
        let module = mk_module("web", vec![c]);
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![module]), &fw);
        let data = flow.layers.iter().find(|l| l.id == "data").unwrap();
        assert_eq!(data.classes.len(), 1);
        let ext = flow.layers.iter().find(|l| l.id == "external").unwrap();
        assert!(ext.classes.is_empty());
    }

    #[test]
    fn annotation_classification_falls_back_when_no_stereotype() {
        let c = klass("OrderEntity", &[], &["Entity"], "src/OrderEntity.java");
        let module = mk_module("core", vec![c]);
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![module]), &fw);
        let domain = flow.layers.iter().find(|l| l.id == "domain").unwrap();
        assert_eq!(domain.classes.len(), 1);
    }

    #[test]
    fn name_hint_is_last_resort() {
        let c = klass("UserDao", &[], &[], "src/persistence/UserDao.java");
        let module = mk_module("core", vec![c]);
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![module]), &fw);
        let data = flow.layers.iter().find(|l| l.id == "data").unwrap();
        assert_eq!(data.classes.len(), 1);
    }

    #[test]
    fn unknown_class_falls_back_to_business() {
        let c = klass("Calculator", &[], &[], "src/Calculator.java");
        let module = mk_module("core", vec![c]);
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![module]), &fw);
        let biz = flow.layers.iter().find(|l| l.id == "business").unwrap();
        assert_eq!(biz.classes.len(), 1);
    }

    #[test]
    fn edges_aggregate_cross_layer_relations() {
        let ctrl = klass("UserController", &["rest-controller"], &[], "a.java");
        let svc = klass("UserService", &["service"], &[], "b.java");
        let repo = klass("UserRepository", &["repository"], &[], "c.java");
        let module = mk_module("core", vec![ctrl.clone(), svc.clone(), repo.clone()]);

        // Two ctrl→svc edges, one svc→repo edge.
        let relations = vec![
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
            Relation {
                from: svc.fqn.clone(),
                to: repo.fqn.clone(),
                kind: RelationKind::Injects,
            },
        ];

        let fw = DummyFw {
            relations: vec![("core".to_string(), relations)],
        };
        let flow = build(&repo_with(vec![module]), &fw);

        let ext_to_biz = flow
            .edges
            .iter()
            .find(|e| e.from == "external" && e.to == "business")
            .expect("ctrl→svc edge");
        assert_eq!(ext_to_biz.count, 2);
        let biz_to_data = flow
            .edges
            .iter()
            .find(|e| e.from == "business" && e.to == "data")
            .expect("svc→repo edge");
        assert_eq!(biz_to_data.count, 1);
        // No self-edges in the output.
        assert!(flow.edges.iter().all(|e| e.from != e.to));
    }

    #[test]
    fn cross_module_edges_are_counted() {
        let ctrl = klass("UserController", &["rest-controller"], &[], "a.java");
        let svc = klass("UserService", &["service"], &[], "b.java");
        let m_web = mk_module("web", vec![ctrl.clone()]);
        let m_core = mk_module("core", vec![svc.clone()]);

        // Relation lives in the `web` module pointing across to `core`.
        let relations = vec![Relation {
            from: ctrl.fqn.clone(),
            to: svc.fqn.clone(),
            kind: RelationKind::Injects,
        }];
        let fw = DummyFw {
            relations: vec![("web".to_string(), relations), ("core".to_string(), vec![])],
        };
        let flow = build(&repo_with(vec![m_web, m_core]), &fw);
        assert_eq!(flow.cross_module_edges, 1);
        assert_eq!(flow.total_modules, 2);
    }

    #[test]
    fn annotated_relations_are_ignored_for_edges() {
        let ctrl = klass("UserController", &["rest-controller"], &[], "a.java");
        let svc = klass("UserService", &["service"], &[], "b.java");
        let module = mk_module("core", vec![ctrl.clone(), svc.clone()]);
        let fw = DummyFw {
            relations: vec![(
                "core".to_string(),
                vec![Relation {
                    from: ctrl.fqn.clone(),
                    to: svc.fqn.clone(),
                    kind: RelationKind::Annotated,
                }],
            )],
        };
        let flow = build(&repo_with(vec![module]), &fw);
        assert!(flow.edges.is_empty());
    }

    #[test]
    fn stereotype_histogram_is_per_layer() {
        let a = klass("UserController", &["rest-controller"], &[], "a.java");
        let b = klass("OrderController", &["controller"], &[], "b.java");
        let c = klass("UserService", &["service"], &[], "c.java");
        let module = mk_module("core", vec![a, b, c]);
        let fw = DummyFw { relations: vec![] };
        let flow = build(&repo_with(vec![module]), &fw);

        let ext = flow.layers.iter().find(|l| l.id == "external").unwrap();
        assert_eq!(ext.stereotypes.get("rest-controller"), Some(&1));
        assert_eq!(ext.stereotypes.get("controller"), Some(&1));
        let biz = flow.layers.iter().find(|l| l.id == "business").unwrap();
        assert_eq!(biz.stereotypes.get("service"), Some(&1));
    }
}

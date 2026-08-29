// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Module dependency cycles — and the cheapest way out of each one.
//!
//! [`module_chord`](crate::module_chord) draws how strongly modules are
//! coupled. It does not say whether that coupling is *circular*, and a cycle
//! is the one coupling defect that cannot be lived with: modules in a cycle
//! cannot be released, tested or reasoned about independently, and every new
//! edge inside the cycle is invisible because the damage has already been done.
//!
//! A tool that only reports "you have a cycle" is a tool nobody acts on. The
//! question a reader actually has is **"what do I change?"**, so this module
//! answers three things per cycle:
//!
//! 1. **Who is in it** — the set of mutually reachable modules.
//! 2. **A concrete round trip** — the shortest module path that closes the
//!    loop, so the cycle can be seen rather than inferred from a set.
//! 3. **Which single edge breaks it, cheapest first** — every edge is tested
//!    by removing it and recomputing the component. Only edges that genuinely
//!    break the mutual reachability are listed, each with the exact class
//!    references that would have to move. That list is the work order.
//!
//! Point 3 is why this is not "report the thinnest edge and hope". A component
//! can carry several independent cycles, and then no single edge helps; the
//! report says so (`cuts` is empty) instead of suggesting a change that would
//! not fix anything.
//!
//! # Why mutual reachability instead of Tarjan
//!
//! Strongly connected components are computed by asking, for every pair, "does
//! a reach b and b reach a". That is `O(n^3)` where Tarjan is `O(n + e)`, and
//! it is the deliberate choice: `n` is the number of **modules**, which is
//! bounded by how many a human maintains — dozens, not millions. In exchange
//! the implementation is ten obvious lines instead of an iterative Tarjan with
//! an explicit stack, and the cut evaluation below needs to recompute
//! components repeatedly on subgraphs, which is trivial to express this way.
//! Should someone ever open a repository with thousands of modules, the honest
//! fix is Tarjan, not a cache.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use projectmind_plugin_api::{FrameworkPlugin, Relation, RelationKind};
use serde::Serialize;

use crate::Repository;

/// How many concrete class references are carried per edge in the payload.
///
/// The full count stays in `weight`; this only caps the examples. A cycle
/// between two fat modules can carry hundreds of references, and a report
/// nobody can read is a report nobody reads.
const MAX_REFERENCES: usize = 25;

/// One class-to-class reference behind a module edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CycleReference {
    /// FQN of the referencing class.
    pub from: String,
    /// FQN of the referenced class.
    pub to: String,
    /// Relation kind as reported by the framework plugin.
    pub kind: String,
}

/// A directed module edge that takes part in a cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CycleEdge {
    /// Source module id.
    pub from: String,
    /// Target module id.
    pub to: String,
    /// Number of class references behind this edge (not capped).
    pub weight: usize,
    /// Up to [`MAX_REFERENCES`] concrete references, for the work order.
    pub references: Vec<CycleReference>,
}

/// An edge whose removal genuinely breaks the component apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CycleCut {
    /// Source module id.
    pub from: String,
    /// Target module id.
    pub to: String,
    /// Number of class references that would have to move.
    pub weight: usize,
    /// Cycles left inside the component after this cut (0 = fully resolved).
    pub remaining_cycles: usize,
    /// The references to move, capped at [`MAX_REFERENCES`].
    pub references: Vec<CycleReference>,
}

/// One dependency cycle: a set of modules that all reach each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleCycle {
    /// The mutually reachable modules, sorted.
    pub modules: Vec<String>,
    /// Shortest round trip, e.g. `["a", "b", "c", "a"]`. Empty only for a
    /// module that depends on itself, which the edge collection excludes.
    pub shortest_cycle: Vec<String>,
    /// Every edge inside the component, heaviest first.
    pub edges: Vec<CycleEdge>,
    /// Single edges that break the component, cheapest first. Empty means no
    /// single edge suffices — the component carries independent cycles.
    pub cuts: Vec<CycleCut>,
}

/// Result of a cycle scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CycleReport {
    /// Cycles found, largest first.
    pub cycles: Vec<ModuleCycle>,
    /// Modules considered.
    pub modules_scanned: usize,
    /// Distinct directed module edges found (cycle or not).
    pub cross_edges: usize,
    /// Modules that take part in at least one cycle.
    pub modules_in_cycles: usize,
}

/// Scan `repo` for module dependency cycles, asking `framework` for relations.
///
/// Convenience wrapper mirroring [`crate::module_chord::build`]. Callers that
/// already hold a relation list (the engine aggregates over *all* registered
/// framework plugins, not just one) should prefer [`build_from_relations`] —
/// a cycle that only shows up through a second plugin is still a cycle.
#[must_use]
pub fn build(repo: &Repository, framework: &dyn FrameworkPlugin) -> CycleReport {
    let mut relations = Vec::new();
    for module in repo.modules.values() {
        relations.extend(framework.relations(module));
    }
    build_from_relations(repo, &relations)
}

/// Scan `repo` for module dependency cycles over an existing relation list.
#[must_use]
pub fn build_from_relations(repo: &Repository, relations: &[Relation]) -> CycleReport {
    let mut module_of_fqn: BTreeMap<String, String> = BTreeMap::new();
    for (mod_id, module) in &repo.modules {
        for class in module.classes.values() {
            module_of_fqn.insert(class.fqn.clone(), mod_id.clone());
        }
    }

    // Collect the module graph. `Annotated` is skipped for the same reason as
    // in module_chord: an annotation lives wherever the framework put it and
    // says nothing about who depends on whom.
    let mut weights: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut refs: BTreeMap<(String, String), Vec<CycleReference>> = BTreeMap::new();
    for rel in relations {
        if matches!(rel.kind, RelationKind::Annotated) {
            continue;
        }
        let (Some(source_mod), Some(target_mod)) =
            (module_of_fqn.get(&rel.from), module_of_fqn.get(&rel.to))
        else {
            continue;
        };
        if source_mod == target_mod {
            continue;
        }
        let key = (source_mod.clone(), target_mod.clone());
        *weights.entry(key.clone()).or_default() += 1;
        let bucket = refs.entry(key).or_default();
        if bucket.len() < MAX_REFERENCES {
            bucket.push(CycleReference {
                from: rel.from.clone(),
                to: rel.to.clone(),
                kind: format!("{:?}", rel.kind),
            });
        }
    }

    let nodes: Vec<String> = repo.modules.keys().cloned().collect();
    let edges: BTreeSet<(String, String)> = weights.keys().cloned().collect();
    let components = strongly_connected(&nodes, &edges);

    let mut cycles: Vec<ModuleCycle> = Vec::new();
    let mut in_cycles = 0usize;
    for component in components {
        if component.len() < 2 {
            continue;
        }
        in_cycles += component.len();
        let inside: BTreeSet<&String> = component.iter().collect();
        let sub_edges: BTreeSet<(String, String)> = edges
            .iter()
            .filter(|(a, b)| inside.contains(a) && inside.contains(b))
            .cloned()
            .collect();

        let mut cycle_edges: Vec<CycleEdge> = sub_edges
            .iter()
            .map(|key| CycleEdge {
                from: key.0.clone(),
                to: key.1.clone(),
                weight: weights.get(key).copied().unwrap_or(0),
                references: refs.get(key).cloned().unwrap_or_default(),
            })
            .collect();
        cycle_edges.sort_by(|a, b| b.weight.cmp(&a.weight).then(a.from.cmp(&b.from)));

        let mut cuts: Vec<CycleCut> = Vec::new();
        for key in &sub_edges {
            let mut without = sub_edges.clone();
            without.remove(key);
            let rest = strongly_connected(&component, &without);
            let remaining = rest.iter().filter(|c| c.len() >= 2).count();
            // The cut counts only if the ORIGINAL component no longer holds
            // together. A component that merely shrinks still has the cycle.
            let still_whole = rest.iter().any(|c| c.len() == component.len());
            if still_whole {
                continue;
            }
            cuts.push(CycleCut {
                from: key.0.clone(),
                to: key.1.clone(),
                weight: weights.get(key).copied().unwrap_or(0),
                remaining_cycles: remaining,
                references: refs.get(key).cloned().unwrap_or_default(),
            });
        }
        // Cheapest first, and a cut that resolves everything beats one that
        // leaves a smaller cycle behind at equal price.
        cuts.sort_by(|a, b| {
            a.weight
                .cmp(&b.weight)
                .then(a.remaining_cycles.cmp(&b.remaining_cycles))
                .then(a.from.cmp(&b.from))
        });

        cycles.push(ModuleCycle {
            shortest_cycle: shortest_round_trip(&component, &sub_edges),
            modules: component,
            edges: cycle_edges,
            cuts,
        });
    }
    cycles.sort_by(|a, b| {
        b.modules
            .len()
            .cmp(&a.modules.len())
            .then(a.modules.cmp(&b.modules))
    });

    CycleReport {
        cycles,
        modules_scanned: nodes.len(),
        cross_edges: edges.len(),
        modules_in_cycles: in_cycles,
    }
}

/// Groups `nodes` into strongly connected components under `edges`.
///
/// Single nodes come back as one-element components, so callers can tell a
/// module that is in no cycle from one that is.
fn strongly_connected(nodes: &[String], edges: &BTreeSet<(String, String)>) -> Vec<Vec<String>> {
    let reach = reachability(nodes, edges);
    let mut seen: BTreeSet<&String> = BTreeSet::new();
    let mut components: Vec<Vec<String>> = Vec::new();
    for a in nodes {
        if seen.contains(a) {
            continue;
        }
        let mut group: Vec<String> = nodes
            .iter()
            .filter(|b| {
                a == *b
                    || (reach.contains(&(a.clone(), (*b).clone()))
                        && reach.contains(&((*b).clone(), a.clone())))
            })
            .cloned()
            .collect();
        group.sort();
        for member in &group {
            seen.insert(nodes.iter().find(|n| *n == member).unwrap_or(a));
        }
        components.push(group);
    }
    components
}

/// Transitive closure of `edges` over `nodes`.
fn reachability(
    nodes: &[String],
    edges: &BTreeSet<(String, String)>,
) -> BTreeSet<(String, String)> {
    let mut reach: BTreeSet<(String, String)> = edges.clone();
    for k in nodes {
        for i in nodes {
            if !reach.contains(&(i.clone(), k.clone())) {
                continue;
            }
            for j in nodes {
                if reach.contains(&(k.clone(), j.clone())) {
                    reach.insert((i.clone(), j.clone()));
                }
            }
        }
    }
    reach
}

/// Shortest path that leaves a module and returns to it, as `[a, …, a]`.
///
/// Breadth-first from every member, keeping the shortest result. The graph is
/// a single component of a module graph, so this stays small.
fn shortest_round_trip(component: &[String], edges: &BTreeSet<(String, String)>) -> Vec<String> {
    let mut best: Option<Vec<String>> = None;
    for start in component {
        let mut queue: VecDeque<Vec<String>> = VecDeque::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        queue.push_back(vec![start.clone()]);
        while let Some(path) = queue.pop_front() {
            let tail = path.last().cloned().unwrap_or_default();
            for (from, to) in edges {
                if from != &tail {
                    continue;
                }
                if to == start {
                    let mut closed = path.clone();
                    closed.push(start.clone());
                    if best.as_ref().is_none_or(|b| closed.len() < b.len()) {
                        best = Some(closed);
                    }
                    continue;
                }
                if visited.insert(to.clone()) {
                    let mut next = path.clone();
                    next.push(to.clone());
                    queue.push_back(next);
                }
            }
        }
    }
    best.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Class, Module, PluginInfo, Relation};
    use std::path::PathBuf;

    // ── Graph-Bausteine ─────────────────────────────────────────────────

    fn kante(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    fn menge(pairs: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        pairs.iter().map(|(a, b)| kante(a, b)).collect()
    }

    fn knoten(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn acyclic_graph_has_only_single_node_components() {
        let nodes = knoten(&["a", "b", "c"]);
        let edges = menge(&[("a", "b"), ("b", "c")]);
        let comps = strongly_connected(&nodes, &edges);
        assert!(comps.iter().all(|c| c.len() == 1), "{comps:?}");
    }

    #[test]
    fn two_module_cycle_is_one_component() {
        let nodes = knoten(&["a", "b", "c"]);
        let edges = menge(&[("a", "b"), ("b", "a"), ("b", "c")]);
        let comps = strongly_connected(&nodes, &edges);
        let big: Vec<_> = comps.into_iter().filter(|c| c.len() > 1).collect();
        assert_eq!(big, vec![knoten(&["a", "b"])]);
    }

    #[test]
    fn round_trip_is_the_shortest_one() {
        // a -> b -> c -> a has length 3, a -> d -> a has length 2 and wins.
        let nodes = knoten(&["a", "b", "c", "d"]);
        let edges = menge(&[("a", "b"), ("b", "c"), ("c", "a"), ("a", "d"), ("d", "a")]);
        let trip = shortest_round_trip(&nodes, &edges);
        assert_eq!(trip.len(), 3, "expected [x, y, x], got {trip:?}");
        assert_eq!(trip.first(), trip.last());
    }

    // ── Der eigentliche Bericht ─────────────────────────────────────────

    /// The point of the whole module: a suggested cut has to actually work.
    #[test]
    fn only_edges_that_break_the_component_are_offered_as_cuts() {
        // a -> b -> c -> a, plus a redundant a -> c. Cutting a -> c alone
        // leaves the three-module ring fully intact.
        let repo = repo_with(&[
            ("a", &["a.A"][..]),
            ("b", &["b.B"][..]),
            ("c", &["c.C"][..]),
        ]);
        let plugin = DummyFw(vec![
            ("a.A", "b.B"),
            ("b.B", "c.C"),
            ("c.C", "a.A"),
            ("a.A", "c.C"),
        ]);

        let report = build(&repo, &plugin);

        assert_eq!(report.cycles.len(), 1);
        let cycle = &report.cycles[0];
        assert_eq!(cycle.modules, knoten(&["a", "b", "c"]));
        let angeboten: BTreeSet<(String, String)> =
            cycle.cuts.iter().map(|c| kante(&c.from, &c.to)).collect();
        assert!(
            !angeboten.contains(&kante("a", "c")),
            "a->c does not break the ring and must not be offered: {angeboten:?}"
        );
        assert!(angeboten.contains(&kante("b", "c")), "{angeboten:?}");
    }

    #[test]
    fn cuts_are_cheapest_first_and_carry_their_references() {
        // Two references a -> b, one reference b -> a: the single one is the
        // cheaper cut and has to come first.
        let repo = repo_with(&[("a", &["a.A1", "a.A2"][..]), ("b", &["b.B"][..])]);
        let plugin = DummyFw(vec![("a.A1", "b.B"), ("a.A2", "b.B"), ("b.B", "a.A1")]);

        let report = build(&repo, &plugin);

        let cycle = &report.cycles[0];
        assert_eq!(cycle.cuts.len(), 2, "both edges break a two-module ring");
        assert_eq!(
            (cycle.cuts[0].from.as_str(), cycle.cuts[0].to.as_str()),
            ("b", "a")
        );
        assert_eq!(cycle.cuts[0].weight, 1);
        assert_eq!(cycle.cuts[0].remaining_cycles, 0);
        assert_eq!(cycle.cuts[0].references.len(), 1);
        assert_eq!(cycle.cuts[0].references[0].to, "a.A1");
        assert_eq!(cycle.cuts[1].weight, 2);
    }

    #[test]
    fn a_clean_repository_reports_nothing() {
        let repo = repo_with(&[("a", &["a.A"][..]), ("b", &["b.B"][..])]);
        let plugin = DummyFw(vec![("a.A", "b.B")]);

        let report = build(&repo, &plugin);

        assert!(report.cycles.is_empty());
        assert_eq!(report.modules_scanned, 2);
        assert_eq!(report.cross_edges, 1);
        assert_eq!(report.modules_in_cycles, 0);
    }

    /// Two independent rings sharing no edge: no single cut resolves both, so
    /// the honest answer for each is the cut that fixes its own ring.
    #[test]
    fn independent_rings_are_reported_separately() {
        let repo = repo_with(&[
            ("a", &["a.A"][..]),
            ("b", &["b.B"][..]),
            ("c", &["c.C"][..]),
            ("d", &["d.D"][..]),
        ]);
        let plugin = DummyFw(vec![
            ("a.A", "b.B"),
            ("b.B", "a.A"),
            ("c.C", "d.D"),
            ("d.D", "c.C"),
        ]);

        let report = build(&repo, &plugin);

        assert_eq!(report.cycles.len(), 2);
        assert_eq!(report.modules_in_cycles, 4);
        for cycle in &report.cycles {
            assert_eq!(cycle.modules.len(), 2);
            assert!(cycle.cuts.iter().all(|c| c.remaining_cycles == 0));
        }
    }

    #[test]
    fn annotations_do_not_create_cycles() {
        let repo = repo_with(&[("a", &["a.A"][..]), ("b", &["b.B"][..])]);
        let plugin = DummyFwKind(vec![
            ("a.A", "b.B", RelationKind::Uses),
            ("b.B", "a.A", RelationKind::Annotated),
        ]);

        let report = build(&repo, &plugin);

        assert!(
            report.cycles.is_empty(),
            "an annotation lives where the framework put it and is no dependency"
        );
    }

    // ── Testhilfen ──────────────────────────────────────────────────────

    fn klass(fqn: &str) -> Class {
        Class {
            name: fqn.rsplit('.').next().unwrap_or(fqn).to_string(),
            fqn: fqn.to_string(),
            file: PathBuf::from(format!("{fqn}.java")),
            ..Default::default()
        }
    }

    fn repo_with(modules: &[(&str, &[&str])]) -> Repository {
        let mut repo = Repository {
            root: PathBuf::from("/tmp/cycles"),
            ..Default::default()
        };
        for (id, fqns) in modules {
            let mut m = Module {
                id: (*id).to_string(),
                name: (*id).to_string(),
                ..Default::default()
            };
            for fqn in *fqns {
                let c = klass(fqn);
                m.classes.insert(c.fqn.clone(), c);
            }
            repo.insert_module(m);
        }
        repo
    }

    /// Reports a fixed relation list, filtered to the module that owns the
    /// source class. Every relation is `Uses`.
    struct DummyFw(Vec<(&'static str, &'static str)>);

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
            self.0
                .iter()
                .filter(|(from, _)| module.classes.contains_key(*from))
                .map(|(from, to)| Relation {
                    from: (*from).to_string(),
                    to: (*to).to_string(),
                    kind: RelationKind::Uses,
                })
                .collect()
        }
    }

    /// Same, but the relation kind is part of the fixture.
    struct DummyFwKind(Vec<(&'static str, &'static str, RelationKind)>);

    impl FrameworkPlugin for DummyFwKind {
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
            self.0
                .iter()
                .filter(|(from, _, _)| module.classes.contains_key(*from))
                .map(|(from, to, kind)| Relation {
                    from: (*from).to_string(),
                    to: (*to).to_string(),
                    kind: *kind,
                })
                .collect()
        }
    }
}

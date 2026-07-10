// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! In-memory [`CodeGraphStore`] backend.
//!
//! The reference implementation of the code-graph cache: zero I/O, zero
//! setup, gone when the process exits. Selected via
//! `.projectmind/config.toml` (`[persistence.code_graph] backend = "memory"`)
//! — useful when a session wants graph queries without leaving a cache
//! file behind. The durable sibling lives in
//! [`crate::code_graph_sqlite::SqliteCodeGraphStore`]; both are exercised
//! by the same conformance suite so their observable behavior can't
//! drift apart.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use projectmind_plugin_api::storage::{CodeGraphStore, EdgeKind, GraphNode, GraphQuery, NodeId};
use projectmind_plugin_api::{Error as ApiError, Result as ApiResult};

/// Property key that ties a [`GraphNode`] to a source file. See the
/// "File attribution" section on [`CodeGraphStore`].
pub const FILE_PROPERTY: &str = "file";

/// Extract the source file a node is attributed to, if any.
pub(crate) fn node_file(node: &GraphNode) -> Option<&str> {
    node.properties
        .get(FILE_PROPERTY)
        .and_then(serde_json::Value::as_str)
}

/// Case-insensitive substring match used by the `label_contains` query
/// filter. Kept in one place so the memory and SQLite backends agree.
pub(crate) fn label_matches(label: &str, needle: &str) -> bool {
    label.to_lowercase().contains(&needle.to_lowercase())
}

/// In-memory code-graph store. Contents live exactly as long as the
/// store value itself.
#[derive(Debug, Default)]
pub struct MemoryCodeGraphStore {
    nodes: BTreeMap<NodeId, GraphNode>,
    /// `(from, to, kind)` triples. `BTreeSet` gives dedup + stable order.
    edges: BTreeSet<(NodeId, NodeId, EdgeKind)>,
    /// Monotonic id allocator; never reuses ids within a session.
    next_id: NodeId,
}

impl MemoryCodeGraphStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of edges currently stored (diagnostics / tests).
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

impl CodeGraphStore for MemoryCodeGraphStore {
    fn upsert_node(&mut self, mut node: GraphNode) -> ApiResult<NodeId> {
        if node.id == 0 {
            self.next_id += 1;
            node.id = self.next_id;
        } else {
            // Keep the allocator ahead of explicitly-placed ids so a
            // later `id == 0` insert can't collide.
            self.next_id = self.next_id.max(node.id);
        }
        let id = node.id;
        self.nodes.insert(id, node);
        Ok(id)
    }

    fn upsert_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> ApiResult<()> {
        for endpoint in [from, to] {
            if !self.nodes.contains_key(&endpoint) {
                return Err(ApiError::Plugin(format!(
                    "cannot add edge: node {endpoint} does not exist"
                )));
            }
        }
        self.edges.insert((from, to, kind));
        Ok(())
    }

    fn query(&self, q: &GraphQuery) -> ApiResult<Vec<GraphNode>> {
        // `map_or(true, …)` instead of `is_none_or` — the latter needs
        // Rust 1.82 and the workspace MSRV is 1.80.
        let limit = q.limit.map_or(usize::MAX, |l| l as usize);
        Ok(self
            .nodes
            .values()
            .filter(|n| q.kind.as_deref().map_or(true, |k| n.kind == k))
            .filter(|n| {
                q.label_contains
                    .as_deref()
                    .map_or(true, |needle| label_matches(&n.label, needle))
            })
            .take(limit)
            .cloned()
            .collect())
    }

    fn invalidate(&mut self, files: &[&Path]) -> ApiResult<()> {
        let files: BTreeSet<String> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let doomed: BTreeSet<NodeId> = self
            .nodes
            .values()
            .filter(|n| node_file(n).is_some_and(|f| files.contains(f)))
            .map(|n| n.id)
            .collect();
        self.nodes.retain(|id, _| !doomed.contains(id));
        self.edges
            .retain(|(from, to, _)| !doomed.contains(from) && !doomed.contains(to));
        Ok(())
    }
}

/// Backend-agnostic conformance suite. Both the memory and the SQLite
/// backend run every function in here against a fresh store, so a
/// behavioral drift between the two turns into a test failure instead
/// of a subtle runtime surprise.
#[cfg(test)]
pub(crate) mod conformance {
    use super::*;

    pub(crate) fn node(kind: &str, label: &str, file: Option<&str>) -> GraphNode {
        let mut properties = serde_json::Map::new();
        if let Some(f) = file {
            properties.insert(FILE_PROPERTY.into(), serde_json::Value::from(f));
        }
        GraphNode {
            id: 0,
            kind: kind.into(),
            label: label.into(),
            properties,
        }
    }

    pub(crate) fn upsert_assigns_fresh_ids(store: &mut dyn CodeGraphStore) {
        let a = store.upsert_node(node("class", "Alpha", None)).unwrap();
        let b = store.upsert_node(node("class", "Beta", None)).unwrap();
        assert_ne!(a, 0, "assigned ids start at 1");
        assert_ne!(a, b, "each insert gets its own id");
    }

    pub(crate) fn upsert_with_existing_id_updates_in_place(store: &mut dyn CodeGraphStore) {
        let id = store.upsert_node(node("class", "Alpha", None)).unwrap();
        let mut updated = node("class", "AlphaRenamed", Some("src/alpha.rs"));
        updated.id = id;
        let id2 = store.upsert_node(updated).unwrap();
        assert_eq!(id, id2, "explicit id upsert keeps the id");

        let all = store.query(&GraphQuery::default()).unwrap();
        assert_eq!(all.len(), 1, "update must not duplicate the node");
        assert_eq!(all[0].label, "AlphaRenamed");
        assert_eq!(node_file(&all[0]), Some("src/alpha.rs"));
    }

    pub(crate) fn query_filters_kind_label_and_limit(store: &mut dyn CodeGraphStore) {
        store
            .upsert_node(node("class", "UserService", None))
            .unwrap();
        store
            .upsert_node(node("class", "UserRepository", None))
            .unwrap();
        store.upsert_node(node("method", "findUser", None)).unwrap();

        let classes = store
            .query(&GraphQuery {
                kind: Some("class".into()),
                ..GraphQuery::default()
            })
            .unwrap();
        assert_eq!(classes.len(), 2);
        assert!(classes.iter().all(|n| n.kind == "class"));

        // Substring match is case-insensitive in every backend.
        let user = store
            .query(&GraphQuery {
                label_contains: Some("userser".into()),
                ..GraphQuery::default()
            })
            .unwrap();
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].label, "UserService");

        let limited = store
            .query(&GraphQuery {
                limit: Some(2),
                ..GraphQuery::default()
            })
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    pub(crate) fn edge_to_unknown_node_is_rejected(store: &mut dyn CodeGraphStore) {
        let a = store.upsert_node(node("class", "Alpha", None)).unwrap();
        assert!(
            store.upsert_edge(a, a + 999, EdgeKind::Uses).is_err(),
            "edges must reference existing nodes"
        );
        // A valid edge (including self-reference) is fine, and re-adding
        // it is an idempotent upsert.
        store.upsert_edge(a, a, EdgeKind::Uses).unwrap();
        store.upsert_edge(a, a, EdgeKind::Uses).unwrap();
    }

    pub(crate) fn invalidate_drops_nodes_and_edges_of_changed_files(
        store: &mut dyn CodeGraphStore,
    ) {
        let alpha = store
            .upsert_node(node("class", "Alpha", Some("src/alpha.rs")))
            .unwrap();
        let beta = store
            .upsert_node(node("class", "Beta", Some("src/beta.rs")))
            .unwrap();
        let pkg = store
            .upsert_node(node("package", "com.acme", None))
            .unwrap();
        store.upsert_edge(pkg, alpha, EdgeKind::Contains).unwrap();
        store.upsert_edge(pkg, beta, EdgeKind::Contains).unwrap();
        store.upsert_edge(alpha, beta, EdgeKind::Uses).unwrap();

        store.invalidate(&[Path::new("src/alpha.rs")]).unwrap();

        let survivors = store.query(&GraphQuery::default()).unwrap();
        let labels: Vec<&str> = survivors.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, vec!["Beta", "com.acme"], "only alpha.rs nodes drop");

        // Re-adding an edge from the surviving package to the surviving
        // class still works — the store is consistent after invalidation.
        store.upsert_edge(pkg, beta, EdgeKind::Contains).unwrap();
    }

    /// Run the whole suite against fresh stores produced by `make`.
    pub(crate) fn run_all(make: &mut dyn FnMut() -> Box<dyn CodeGraphStore>) {
        upsert_assigns_fresh_ids(make().as_mut());
        upsert_with_existing_id_updates_in_place(make().as_mut());
        query_filters_kind_label_and_limit(make().as_mut());
        edge_to_unknown_node_is_rejected(make().as_mut());
        invalidate_drops_nodes_and_edges_of_changed_files(make().as_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_passes_conformance_suite() {
        super::conformance::run_all(&mut || Box::new(MemoryCodeGraphStore::new()));
    }

    #[test]
    fn ids_stay_monotonic_after_explicit_id_upsert() {
        let mut store = MemoryCodeGraphStore::new();
        let mut explicit = conformance::node("class", "Pinned", None);
        explicit.id = 40;
        store.upsert_node(explicit).unwrap();
        let fresh = store
            .upsert_node(conformance::node("class", "Fresh", None))
            .unwrap();
        assert!(fresh > 40, "allocator must jump past explicitly-placed ids");
    }

    #[test]
    fn invalidate_removes_edges_touching_dropped_nodes() {
        let mut store = MemoryCodeGraphStore::new();
        let a = store
            .upsert_node(conformance::node("class", "A", Some("a.rs")))
            .unwrap();
        let b = store
            .upsert_node(conformance::node("class", "B", Some("b.rs")))
            .unwrap();
        store.upsert_edge(a, b, EdgeKind::Uses).unwrap();
        assert_eq!(store.edge_count(), 1);
        store.invalidate(&[Path::new("a.rs")]).unwrap();
        assert_eq!(store.edge_count(), 0, "dangling edges must not survive");
    }
}

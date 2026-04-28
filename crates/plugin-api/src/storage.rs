// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Persistence traits.
//!
//! Two trait surfaces let the host pick a backend (JSON, `SQLite`, `SurrealDB`, Mempalace)
//! without affecting plugin code:
//!
//! - [`AnnotationStore`] — user-set markers on file/line ranges.
//! - [`CodeGraphStore`] — cache of parsed entities and relations.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Identifier of a node in the code graph.
pub type NodeId = u64;

/// A node in the code graph (a class, method, package, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Stable id assigned by the store.
    pub id: NodeId,
    /// Logical kind, e.g. `class`, `method`, `package`.
    pub kind: String,
    /// Display label.
    pub label: String,
    /// Free-form properties.
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

/// Kind of edge between two graph nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Inheritance / implementation.
    Extends,
    /// Implements interface.
    Implements,
    /// Generic dependency.
    Uses,
    /// Spring/DI injection.
    Injects,
    /// Method call.
    Calls,
    /// Annotation use.
    Annotated,
    /// Containment (package contains class, class contains method, …).
    Contains,
}

/// A query against the code graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphQuery {
    /// Optional kind filter.
    pub kind: Option<String>,
    /// Optional label substring filter.
    pub label_contains: Option<String>,
    /// Limit on the number of results.
    pub limit: Option<u32>,
}

/// A user-set annotation on a region of source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationRecord {
    /// Stable id assigned by the store.
    pub id: u64,
    /// Repository-relative file path.
    pub file: String,
    /// Inclusive start line (1-based).
    pub line_from: u32,
    /// Inclusive end line (1-based).
    pub line_to: u32,
    /// Short label.
    pub label: String,
    /// Optional external link (Confluence, Jira, URL).
    pub link: Option<String>,
    /// Free-form metadata.
    #[serde(default)]
    pub extras: serde_json::Map<String, serde_json::Value>,
}

/// Backend for user annotations.
pub trait AnnotationStore: Send + Sync {
    /// List all annotations on a file.
    fn list(&self, file: &str) -> Result<Vec<AnnotationRecord>>;

    /// List every annotation in the repo.
    fn all(&self) -> Result<Vec<AnnotationRecord>>;

    /// Add a new annotation; returns the assigned id.
    fn add(&mut self, ann: AnnotationRecord) -> Result<u64>;

    /// Remove an annotation by id.
    fn remove(&mut self, id: u64) -> Result<()>;
}

/// Backend for the code graph cache.
pub trait CodeGraphStore: Send + Sync {
    /// Insert or update a node; returns its id.
    fn upsert_node(&mut self, node: GraphNode) -> Result<NodeId>;

    /// Insert or update an edge.
    fn upsert_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> Result<()>;

    /// Run a query.
    fn query(&self, q: &GraphQuery) -> Result<Vec<GraphNode>>;

    /// Drop everything tied to the given files (used when files change on disk).
    fn invalidate(&mut self, files: &[&Path]) -> Result<()>;
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public plugin API for `plaintext-ide`.
//!
//! Three plugin kinds are defined here:
//!
//! - [`LanguagePlugin`] — knows how to parse files of a programming language into [`Module`]s
//!   of [`Class`], [`Method`], etc.
//! - [`FrameworkPlugin`] — operates on top of one or more languages, adding semantic information
//!   (e.g. recognising Spring stereotypes, building a bean graph).
//! - [`VisualizerPlugin`] — renders a payload (a graph, a tree, a diff …) into a UI component.
//!
//! Phase 1 plugins are statically registered. The trait objects are designed so a future
//! dynamic loader (cdylib from `./plugins/`) can host the same plugins unchanged.

#![warn(missing_docs)]

pub mod entity;
pub mod plugin;
pub mod relation;
pub mod storage;

pub use entity::{Annotation, Class, ClassKind, Field, Method, Module, Visibility};
pub use plugin::{FrameworkPlugin, LanguagePlugin, PluginInfo, VisualizerPlugin};
pub use relation::{Relation, RelationKind};
pub use storage::{AnnotationStore, CodeGraphStore, EdgeKind, GraphNode, GraphQuery, NodeId};

/// Result alias used throughout the plugin API.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that plugins can return through the public API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// IO failure while reading a source file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The plugin failed to parse its input.
    #[error("parse error: {0}")]
    Parse(String),

    /// A configuration value was missing or malformed.
    #[error("config error: {0}")]
    Config(String),

    /// Catch-all for plugin-internal failures.
    #[error("plugin error: {0}")]
    Plugin(String),
}

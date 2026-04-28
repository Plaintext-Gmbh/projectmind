// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core engine for `plaintext-ide`.
//!
//! Wires together plugins, repository discovery, and the parsing pipeline.
//!
//! Phase 1 keeps the API minimal: callers create an [`Engine`] with the language and framework
//! plugins they want, then [`Engine::open_repo`] returns a parsed [`Repository`]. Future phases
//! will introduce caching ([`plaintext_ide_plugin_api::CodeGraphStore`]) and async parsing.

#![warn(missing_docs)]

pub mod diagram;
pub mod engine;
pub mod git;
pub mod maven;
pub mod repository;

pub use engine::Engine;
pub use maven::MavenModule;
pub use repository::{Repository, RepositoryError};

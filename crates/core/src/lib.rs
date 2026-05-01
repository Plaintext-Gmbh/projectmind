// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core engine for `projectmind`.
//!
//! Wires together plugins, repository discovery, and the parsing pipeline.
//!
//! Phase 1 keeps the API minimal: callers create an [`Engine`] with the language and framework
//! plugins they want, then [`Engine::open_repo`] returns a parsed [`Repository`]. Future phases
//! will introduce caching ([`projectmind_plugin_api::CodeGraphStore`]) and async parsing.

#![warn(missing_docs)]

pub mod cargo;
pub mod diagram;
pub mod engine;
pub mod file_access;
pub mod files;
pub mod git;
pub mod heartbeat;
pub mod html;
pub mod maven;
pub mod repository;
pub mod state;
pub mod walkthrough;

pub use cargo::CargoCrate;
pub use engine::Engine;
pub use maven::MavenModule;
pub use repository::{Repository, RepositoryError};

/// Process-wide mutex for tests that mutate the `PROJECTMIND_STATE`
/// env var. Several modules (heartbeat, walkthrough) share that env var
/// and would race when cargo runs their tests in parallel.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

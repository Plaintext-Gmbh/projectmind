// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rust language plugin (Tree-sitter based).
//!
//! Phase 1: extracts top-level `struct`, `enum`, `trait`, and `union` items as [`Class`]es,
//! attaches methods declared in matching `impl` blocks, lifts `#[derive(...)]` and other
//! outer attributes to [`Annotation`]s, and derives a best-effort namespace prefix from the
//! file path (`<file_stem>::<Name>`, with `mod.rs`/`lib.rs`/`main.rs` collapsing to the
//! parent directory). Free functions, macros, and module nesting are ignored — they don't
//! map cleanly onto the language-agnostic class-centric domain model and aren't needed for
//! Phase 1's architectural-overview goal.
//!
//! [`Class`]: projectmind_plugin_api::Class
//! [`Annotation`]: projectmind_plugin_api::Annotation

#![warn(missing_docs)]

mod parser;

use std::path::Path;

use projectmind_plugin_api::{LanguagePlugin, Module, PluginInfo, Result};

/// The Rust language plugin.
#[derive(Debug, Default)]
pub struct RustPlugin;

impl RustPlugin {
    /// Construct a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl LanguagePlugin for RustPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "lang-rust",
            name: "Rust (Tree-sitter)",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn file_extensions(&self) -> &[&'static str] {
        &["rs"]
    }

    fn parse_file(&self, file: &Path, source: &str, module: &mut Module) -> Result<()> {
        parser::parse(file, source, module)
    }

    fn provided_diagrams(&self) -> &[&'static str] {
        &["package-tree"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_metadata() {
        let p = RustPlugin::new();
        assert_eq!(p.info().id, "lang-rust");
        assert_eq!(p.file_extensions(), &["rs"]);
    }
}

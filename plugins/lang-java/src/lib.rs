// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Java language plugin (Tree-sitter based).
//!
//! Extracts classes, interfaces, enums and records together with their methods, fields and
//! annotations. Phase 1 keeps it pragmatic — names, line ranges, and annotation simple-names —
//! enough to drive the bean graph and the file/class browser.

#![warn(missing_docs)]

mod parser;

use std::path::Path;

use plaintext_ide_plugin_api::{LanguagePlugin, Module, PluginInfo, Result};

/// The Java language plugin.
#[derive(Debug, Default)]
pub struct JavaPlugin;

impl JavaPlugin {
    /// Construct a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl LanguagePlugin for JavaPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "lang-java",
            name: "Java (Tree-sitter)",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn file_extensions(&self) -> &[&'static str] {
        &["java"]
    }

    fn parse_file(&self, file: &Path, source: &str, module: &mut Module) -> Result<()> {
        parser::parse(file, source, module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_metadata() {
        let p = JavaPlugin::new();
        assert_eq!(p.info().id, "lang-java");
        assert_eq!(p.file_extensions(), &["java"]);
    }
}

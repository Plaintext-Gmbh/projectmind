// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Plugin traits.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::entity::Module;
use crate::relation::Relation;
use crate::Result;

/// Static metadata about a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Stable identifier (e.g. `lang-java`).
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Plugin version (semver).
    pub version: &'static str,
}

/// A plugin that knows how to parse a programming language.
pub trait LanguagePlugin: Send + Sync {
    /// Plugin metadata.
    fn info(&self) -> PluginInfo;

    /// File extensions this plugin handles (without the leading dot).
    fn file_extensions(&self) -> &[&'static str];

    /// Parse a single source file into a partial [`Module`].
    ///
    /// `module_root` is the absolute root directory of the module that contains the file. The
    /// implementation should populate the module's classes; module-level metadata (id, name) is
    /// filled in by the caller.
    fn parse_file(&self, file: &Path, source: &str, module: &mut Module) -> Result<()>;

    /// Diagram kinds this language can contribute. Languages with a hierarchical
    /// namespace (Java packages, Rust modules, Python dotted modules, …) should
    /// return `&["package-tree"]`. Default empty so loadable plugins don't have
    /// to know about specific diagram ids.
    fn provided_diagrams(&self) -> &[&'static str] {
        &[]
    }
}

/// A plugin that enriches one or more languages with framework-specific information.
pub trait FrameworkPlugin: Send + Sync {
    /// Plugin metadata.
    fn info(&self) -> PluginInfo;

    /// Languages this plugin builds on (by language plugin id).
    fn supported_languages(&self) -> &[&'static str];

    /// Add stereotypes / metadata to the module's classes.
    fn enrich(&self, module: &mut Module) -> Result<()>;

    /// Compute relations across the module's classes (e.g. bean injection edges).
    fn relations(&self, module: &Module) -> Vec<Relation>;

    /// Diagram kinds this framework contributes. `framework-spring` returns
    /// `&["bean-graph"]`; a future `framework-junit` could return
    /// `&["test-coverage"]`. Default empty.
    fn provided_diagrams(&self) -> &[&'static str] {
        &[]
    }
}

/// A plugin that renders a payload into a UI component.
///
/// Visualizer plugins live mainly in the frontend; the Rust side just declares which payloads
/// are available. The actual rendering is performed by a custom element registered with the same
/// `webcomponent_tag`.
pub trait VisualizerPlugin: Send + Sync {
    /// Plugin metadata.
    fn info(&self) -> PluginInfo;

    /// Logical kind of payload this visualizer can render
    /// (e.g. `spring/bean-graph`, `code/package-tree`).
    fn consumes(&self) -> &'static str;

    /// HTML tag name of the custom element that renders this visualization.
    fn webcomponent_tag(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyLang;
    impl LanguagePlugin for DummyLang {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "dummy",
                name: "Dummy",
                version: "0.0.1",
            }
        }
        fn file_extensions(&self) -> &[&'static str] {
            &["dummy"]
        }
        fn parse_file(&self, _file: &Path, _source: &str, _module: &mut Module) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn language_plugin_can_be_made_into_trait_object() {
        let plugin: Box<dyn LanguagePlugin> = Box::new(DummyLang);
        assert_eq!(plugin.info().id, "dummy");
        assert_eq!(plugin.file_extensions(), &["dummy"]);
    }
}

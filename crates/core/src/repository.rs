// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! In-memory representation of an opened repository.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use projectmind_plugin_api::Module;
use serde::{Deserialize, Serialize};

/// A parsed repository: the root directory plus its modules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Repository {
    /// Absolute path to the repository root.
    pub root: PathBuf,
    /// Modules keyed by their id.
    pub modules: BTreeMap<String, Module>,
}

impl Repository {
    /// Create an empty repository rooted at `root`.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            modules: BTreeMap::new(),
        }
    }

    /// Insert or replace a module.
    pub fn insert_module(&mut self, module: Module) {
        self.modules.insert(module.id.clone(), module);
    }

    /// Find a class by fully-qualified name across all modules.
    ///
    /// Returns the owning module so callers can resolve `class.file` (which is
    /// stored relative to the module root) back to an absolute path.
    #[must_use]
    pub fn find_class(&self, fqn: &str) -> Option<(&Module, &projectmind_plugin_api::Class)> {
        for module in self.modules.values() {
            if let Some(class) = module.classes.get(fqn) {
                return Some((module, class));
            }
        }
        None
    }

    /// Total number of classes across all modules.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.modules.values().map(|m| m.classes.len()).sum()
    }

    /// Resolve a relative path to an absolute path within the repository.
    #[must_use]
    pub fn absolute(&self, rel: &Path) -> PathBuf {
        if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.root.join(rel)
        }
    }
}

/// Errors when opening or scanning a repository.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RepositoryError {
    /// IO failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The provided path does not exist or is not a directory.
    #[error("invalid repository path: {0}")]
    InvalidPath(PathBuf),

    /// A plugin failed.
    #[error("plugin error: {0}")]
    Plugin(#[from] projectmind_plugin_api::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Class, ClassKind, Module};

    fn sample_module() -> Module {
        let mut m = Module {
            id: "sample".into(),
            name: "Sample".into(),
            root: PathBuf::from("/tmp/sample"),
            ..Default::default()
        };
        m.classes.insert(
            "com.example.Foo".into(),
            Class {
                fqn: "com.example.Foo".into(),
                name: "Foo".into(),
                kind: ClassKind::Class,
                ..Default::default()
            },
        );
        m
    }

    #[test]
    fn find_class_across_modules() {
        let mut repo = Repository::new(PathBuf::from("/tmp"));
        repo.insert_module(sample_module());
        let found = repo.find_class("com.example.Foo");
        assert!(found.is_some());
        assert_eq!(found.unwrap().0.id, "sample");
    }

    #[test]
    fn class_count_sums_across_modules() {
        let mut repo = Repository::new(PathBuf::from("/tmp"));
        repo.insert_module(sample_module());
        repo.insert_module(Module {
            id: "empty".into(),
            ..Default::default()
        });
        assert_eq!(repo.class_count(), 1);
    }
}

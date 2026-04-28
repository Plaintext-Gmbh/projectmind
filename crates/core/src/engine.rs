// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Engine — the runtime that holds plugins and runs the parse pipeline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use plaintext_ide_plugin_api::{FrameworkPlugin, LanguagePlugin, Module};
use tracing::{debug, info, warn};

use crate::repository::{Repository, RepositoryError};

/// The engine wires plugins to repositories.
pub struct Engine {
    languages: Vec<Box<dyn LanguagePlugin>>,
    frameworks: Vec<Box<dyn FrameworkPlugin>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field(
                "languages",
                &self
                    .languages
                    .iter()
                    .map(|p| p.info().id)
                    .collect::<Vec<_>>(),
            )
            .field(
                "frameworks",
                &self
                    .frameworks
                    .iter()
                    .map(|p| p.info().id)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// Create an empty engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            languages: Vec::new(),
            frameworks: Vec::new(),
        }
    }

    /// Register a language plugin.
    pub fn register_language(&mut self, plugin: Box<dyn LanguagePlugin>) {
        info!("registered language plugin: {}", plugin.info().id);
        self.languages.push(plugin);
    }

    /// Register a framework plugin.
    pub fn register_framework(&mut self, plugin: Box<dyn FrameworkPlugin>) {
        info!("registered framework plugin: {}", plugin.info().id);
        self.frameworks.push(plugin);
    }

    /// Get registered language plugin ids (for diagnostics).
    pub fn language_ids(&self) -> Vec<&'static str> {
        self.languages.iter().map(|p| p.info().id).collect()
    }

    /// Get registered framework plugin ids (for diagnostics).
    pub fn framework_ids(&self) -> Vec<&'static str> {
        self.frameworks.iter().map(|p| p.info().id).collect()
    }

    /// Walk a repository, parse files with registered language plugins, then enrich with
    /// framework plugins.
    ///
    /// If one or more `pom.xml` files are found, the repository is split into Maven modules.
    /// Otherwise the whole repo is treated as a single module.
    pub fn open_repo(&self, path: &Path) -> Result<Repository, RepositoryError> {
        let path = std::fs::canonicalize(path)
            .map_err(|_| RepositoryError::InvalidPath(path.to_path_buf()))?;
        if !path.is_dir() {
            return Err(RepositoryError::InvalidPath(path));
        }
        info!(?path, "opening repository");

        let mut repo = Repository::new(path.clone());

        let maven_modules = crate::maven::discover(&path);
        if maven_modules.is_empty() {
            let module = self.parse_root(&path)?;
            repo.insert_module(module);
        } else {
            info!(
                modules = maven_modules.len(),
                "Maven multi-module project detected"
            );
            for m in self.parse_maven_modules(&path, &maven_modules)? {
                repo.insert_module(m);
            }
        }

        info!(class_count = repo.class_count(), "repo parsed");
        Ok(repo)
    }

    fn parse_root(&self, root: &Path) -> Result<Module, RepositoryError> {
        let mut module = Module {
            id: derive_module_id(root),
            name: root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repo")
                .into(),
            root: root.to_path_buf(),
            classes: BTreeMap::new(),
        };

        let by_ext = self.index_languages_by_extension();
        if by_ext.is_empty() {
            warn!("no language plugins registered, skipping parse");
            return Ok(module);
        }

        let walker = WalkBuilder::new(root)
            .standard_filters(true)
            .hidden(false)
            .build();

        for entry in walker.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(plugin) = by_ext.get(ext) else {
                continue;
            };
            self.parse_file_into(path, *plugin, &mut module);
        }

        for fw in &self.frameworks {
            if let Err(err) = fw.enrich(&mut module) {
                warn!(plugin = fw.info().id, error = %err, "framework enrich failed");
            }
        }
        Ok(module)
    }

    fn parse_maven_modules(
        &self,
        repo_root: &Path,
        modules: &[crate::maven::MavenModule],
    ) -> Result<Vec<Module>, RepositoryError> {
        let by_ext = self.index_languages_by_extension();
        if by_ext.is_empty() {
            warn!("no language plugins registered, skipping parse");
            return Ok(Vec::new());
        }

        // Initialize one Module per Maven module.
        let mut out: BTreeMap<String, Module> = BTreeMap::new();
        for mvn in modules {
            out.insert(
                mvn.coordinate(),
                Module {
                    id: mvn.coordinate(),
                    name: mvn.artifact_id.clone(),
                    root: mvn.root.clone(),
                    classes: BTreeMap::new(),
                },
            );
        }

        // Walk the whole repo once, attribute each file to the deepest module that contains it.
        let walker = WalkBuilder::new(repo_root)
            .standard_filters(true)
            .hidden(false)
            .build();
        for entry in walker.filter_map(Result::ok) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(plugin) = by_ext.get(ext) else {
                continue;
            };
            let Some(mvn) = crate::maven::attribute(modules, path) else {
                continue;
            };
            if let Some(module) = out.get_mut(&mvn.coordinate()) {
                self.parse_file_into(path, *plugin, module);
            }
        }

        // Enrich each module with framework plugins.
        for module in out.values_mut() {
            for fw in &self.frameworks {
                if let Err(err) = fw.enrich(module) {
                    warn!(plugin = fw.info().id, error = %err, "framework enrich failed");
                }
            }
        }

        Ok(out.into_values().collect())
    }

    fn index_languages_by_extension(&self) -> BTreeMap<&'static str, &dyn LanguagePlugin> {
        let mut by_ext: BTreeMap<&'static str, &dyn LanguagePlugin> = BTreeMap::new();
        for p in &self.languages {
            for ext in p.file_extensions() {
                by_ext.insert(*ext, p.as_ref());
            }
        }
        by_ext
    }

    fn parse_file_into(&self, path: &Path, plugin: &dyn LanguagePlugin, module: &mut Module) {
        match std::fs::read_to_string(path) {
            Ok(source) => {
                if let Err(err) = plugin.parse_file(path, &source, module) {
                    warn!(file = %path.display(), error = %err, "parse failed");
                } else {
                    debug!(file = %path.display(), "parsed");
                }
            }
            Err(err) => warn!(file = %path.display(), error = %err, "read failed"),
        }
    }
}

fn derive_module_id(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .map_or_else(|| "module".to_string(), str::to_string)
}

/// Trait extension to resolve absolute paths back to repo-relative.
pub trait RelativizePath {
    /// Return `path` relative to `root`, or the absolute path if it doesn't share `root`.
    fn relative_to(path: &Path, root: &Path) -> PathBuf;
}

impl RelativizePath for Path {
    fn relative_to(path: &Path, root: &Path) -> PathBuf {
        path.strip_prefix(root)
            .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plaintext_ide_plugin_api::PluginInfo;

    struct DotPlugin;
    impl LanguagePlugin for DotPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "dot",
                name: "Dot",
                version: "0.0.1",
            }
        }
        fn file_extensions(&self) -> &[&'static str] {
            &["dot"]
        }
        fn parse_file(
            &self,
            _file: &Path,
            _source: &str,
            module: &mut Module,
        ) -> plaintext_ide_plugin_api::Result<()> {
            module.classes.insert(
                "dot.File".into(),
                plaintext_ide_plugin_api::Class {
                    fqn: "dot.File".into(),
                    name: "File".into(),
                    ..Default::default()
                },
            );
            Ok(())
        }
    }

    #[test]
    fn engine_walks_and_calls_plugin() {
        let dir = tempdir();
        std::fs::write(dir.path().join("a.dot"), "x").unwrap();
        let mut engine = Engine::new();
        engine.register_language(Box::new(DotPlugin));
        let repo = engine.open_repo(dir.path()).unwrap();
        assert_eq!(repo.class_count(), 1);
    }

    fn tempdir() -> TempDir {
        TempDir::new()
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("plaintext-ide-test-{}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

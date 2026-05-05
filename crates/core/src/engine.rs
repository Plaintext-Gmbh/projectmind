// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Engine — the runtime that holds plugins and runs the parse pipeline.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use projectmind_plugin_api::{FrameworkPlugin, LanguagePlugin, Module, TabContribution};
use serde::Serialize;
use tracing::{debug, info, warn};

use crate::repository::{Repository, RepositoryError};

/// Serializable view of a [`TabContribution`] for the public API.
///
/// `TabContribution` is a `Copy` static-string struct that's convenient for
/// plugins to declare; the host turns it into this owned, serializable form
/// before exposing it to the GUI.
#[derive(Debug, Clone, Serialize)]
pub struct TabDescriptor {
    /// Stable identifier (e.g. `files`, `diagrams`, `tests`).
    pub id: String,
    /// i18n key for the visible label.
    pub label_key: String,
    /// Frontend view-mode value the tab activates.
    pub view_mode: String,
}

impl From<TabContribution> for TabDescriptor {
    fn from(c: TabContribution) -> Self {
        Self {
            id: c.id.to_string(),
            label_key: c.label_key.to_string(),
            view_mode: c.view_mode.to_string(),
        }
    }
}

/// Built-in tab contributions the core ships unconditionally.
///
/// Kept here (not on a plugin) because they describe core capabilities — a
/// repo always has files, and `folder-map` is always renderable so the
/// Diagrams tab is always meaningful.
const CORE_TABS: &[TabContribution] = &[
    TabContribution {
        id: "files",
        label_key: "nav.files",
        view_mode: "classes",
    },
    TabContribution {
        id: "diagrams",
        label_key: "nav.diagrams",
        view_mode: "diagram",
    },
];

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

    /// Diagram kinds that the active plugin set can render against `repo`.
    /// "folder-map" is always present (it's a core capability); language and
    /// framework plugins contribute the rest. Languages only contribute when
    /// the repo has parsed classes — a docs-only repo with no Java/Rust code
    /// shouldn't advertise "package-tree" since it has no packages to draw.
    /// Frameworks only contribute when at least one of their supported
    /// languages produced a class (i.e. the framework has something to enrich).
    /// The result is sorted + deduplicated for stable UI ordering.
    /// Top-level UI tabs the active plugin set contributes for `repo`.
    /// Core ships `files` + `diagrams` unconditionally; languages and
    /// frameworks can append plugin-specific tabs (a future
    /// `framework-junit` "Tests" tab, for example). Duplicate ids across
    /// plugins are dropped — the first occurrence wins so core tabs always
    /// appear in the order declared in [`CORE_TABS`].
    ///
    /// `repo` is currently unused but accepted so the signature mirrors
    /// [`Engine::available_diagrams`] and so future plugins can scope tab
    /// visibility to repo content (e.g. only show "Tests" when the repo
    /// actually has tests).
    pub fn available_tabs(&self, _repo: &Repository) -> Vec<TabDescriptor> {
        let mut out: Vec<TabDescriptor> = Vec::new();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let push = |c: TabContribution,
                    out: &mut Vec<TabDescriptor>,
                    seen: &mut std::collections::BTreeSet<String>| {
            if seen.insert(c.id.to_string()) {
                out.push(c.into());
            }
        };
        for c in CORE_TABS {
            push(*c, &mut out, &mut seen);
        }
        for lang in &self.languages {
            for c in lang.provided_tabs() {
                push(*c, &mut out, &mut seen);
            }
        }
        for fw in &self.frameworks {
            for c in fw.provided_tabs() {
                push(*c, &mut out, &mut seen);
            }
        }
        out
    }

    /// Diagram kinds that the active plugin set can render against `repo`.
    /// "folder-map" is always present (it's a core capability); language and
    /// framework plugins contribute the rest. Languages only contribute when
    /// the repo has parsed classes — a docs-only repo with no Java/Rust code
    /// shouldn't advertise "package-tree" since it has no packages to draw.
    /// Frameworks only contribute when at least one of their supported
    /// languages produced a class (i.e. the framework has something to enrich).
    /// The result is sorted + deduplicated for stable UI ordering.
    pub fn available_diagrams(&self, repo: &Repository) -> Vec<String> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        out.insert("folder-map".to_string());
        // doc-graph is always meaningful when the repo has at least one
        // markdown file. Unconditional on plugins because it's purely a
        // filesystem scan, not language-specific parsing.
        if !crate::files::list_markdown_files(&repo.root).is_empty() {
            out.insert("doc-graph".to_string());
        }
        let has_classes = repo.class_count() > 0;
        if has_classes {
            // c4-container draws one Container per module, so it is
            // meaningful whenever we have at least one parsed module —
            // independently of which framework/language plugin produced it.
            if !repo.modules.is_empty() {
                out.insert("c4-container".to_string());
            }
            for lang in &self.languages {
                for d in lang.provided_diagrams() {
                    out.insert((*d).to_string());
                }
            }
            for fw in &self.frameworks {
                for d in fw.provided_diagrams() {
                    out.insert((*d).to_string());
                }
            }
        }
        out.into_iter().collect()
    }

    /// Walk a repository, parse files with registered language plugins, then enrich with
    /// framework plugins.
    ///
    /// Multi-module detection runs in priority order: Maven first (any `pom.xml`), then
    /// Cargo (any `Cargo.toml` with a `[package]` section). The two are mutually exclusive
    /// because a mixed Maven/Cargo monorepo is rare in practice and the natural attribution
    /// rule — deepest containing manifest wins — would still be unambiguous, but Phase 1
    /// stays simple. If neither layout is detected, the whole repo is parsed as one module.
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
            let cargo_crates = crate::cargo::discover(&path);
            if cargo_crates.is_empty() {
                let module = self.parse_root(&path)?;
                repo.insert_module(module);
            } else {
                info!(crates = cargo_crates.len(), "Cargo workspace detected");
                for m in self.parse_cargo_crates(&path, &cargo_crates)? {
                    repo.insert_module(m);
                }
            }
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

    /// Open a single Markdown document as a virtual repository.
    ///
    /// The repository root is the canonical file path itself. This keeps the
    /// UI context scoped to the selected document while still reusing the
    /// existing repository-shaped state and file-view plumbing.
    pub fn open_markdown_file(&self, path: &Path) -> Result<Repository, RepositoryError> {
        let path = std::fs::canonicalize(path)
            .map_err(|_| RepositoryError::InvalidPath(path.to_path_buf()))?;
        if !path.is_file() || !is_markdown_path(&path) {
            return Err(RepositoryError::InvalidMarkdownFile(path));
        }

        let module_root = path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let module = Module {
            id: "document".into(),
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("document")
                .into(),
            root: module_root,
            classes: BTreeMap::new(),
        };

        let mut repo = Repository::new(path);
        repo.insert_module(module);
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

    fn parse_cargo_crates(
        &self,
        repo_root: &Path,
        crates: &[crate::cargo::CargoCrate],
    ) -> Result<Vec<Module>, RepositoryError> {
        let by_ext = self.index_languages_by_extension();
        if by_ext.is_empty() {
            warn!("no language plugins registered, skipping parse");
            return Ok(Vec::new());
        }

        let mut out: BTreeMap<String, Module> = BTreeMap::new();
        for cr in crates {
            out.insert(
                cr.coordinate(),
                Module {
                    id: cr.coordinate(),
                    name: cr.name.clone(),
                    root: cr.root.clone(),
                    classes: BTreeMap::new(),
                },
            );
        }

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
            let Some(cr) = crate::cargo::attribute(crates, path) else {
                continue;
            };
            if let Some(module) = out.get_mut(&cr.coordinate()) {
                self.parse_file_into(path, *plugin, module);
            }
        }

        for module in out.values_mut() {
            for fw in &self.frameworks {
                if let Err(err) = fw.enrich(module) {
                    warn!(plugin = fw.info().id, error = %err, "framework enrich failed");
                }
            }
        }

        Ok(out.into_values().collect())
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

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx"))
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
    use projectmind_plugin_api::PluginInfo;

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
        ) -> projectmind_plugin_api::Result<()> {
            module.classes.insert(
                "dot.File".into(),
                projectmind_plugin_api::Class {
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

    struct TabPlugin;
    impl LanguagePlugin for TabPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: "tab-lang",
                name: "Tab",
                version: "0.0.1",
            }
        }
        fn file_extensions(&self) -> &[&'static str] {
            &[]
        }
        fn parse_file(
            &self,
            _file: &Path,
            _source: &str,
            _module: &mut Module,
        ) -> projectmind_plugin_api::Result<()> {
            Ok(())
        }
        fn provided_tabs(&self) -> &[TabContribution] {
            &[TabContribution {
                id: "tests",
                label_key: "nav.tests",
                view_mode: "tests",
            }]
        }
    }

    #[test]
    fn available_tabs_includes_core_and_plugin_contributions() {
        let dir = tempdir();
        let mut engine = Engine::new();
        engine.register_language(Box::new(TabPlugin));
        let repo = engine.open_repo(dir.path()).unwrap();
        let tabs = engine.available_tabs(&repo);
        let ids: Vec<_> = tabs.iter().map(|t| t.id.as_str()).collect();
        // Core tabs come first in declaration order, then plugin tabs.
        assert_eq!(ids, vec!["files", "diagrams", "tests"]);
    }

    #[test]
    fn opens_markdown_file_as_virtual_repo() {
        let dir = tempdir();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# Note").unwrap();

        let engine = Engine::new();
        let repo = engine.open_markdown_file(&file).unwrap();

        assert_eq!(repo.root, std::fs::canonicalize(&file).unwrap());
        assert_eq!(repo.modules.len(), 1);
        assert_eq!(repo.class_count(), 0);
        let module = repo.modules.get("document").unwrap();
        assert_eq!(module.name, "note");
        assert_eq!(module.root, std::fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn rejects_non_markdown_virtual_repo_file() {
        let dir = tempdir();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "not markdown").unwrap();

        let engine = Engine::new();
        assert!(engine.open_markdown_file(&file).is_err());
    }

    #[test]
    fn available_tabs_dedupes_by_id() {
        // A plugin re-declaring a core id (e.g. `files`) shouldn't double the
        // entry — first occurrence wins so core ordering stays stable.
        struct ShadowPlugin;
        impl LanguagePlugin for ShadowPlugin {
            fn info(&self) -> PluginInfo {
                PluginInfo {
                    id: "shadow",
                    name: "Shadow",
                    version: "0.0.1",
                }
            }
            fn file_extensions(&self) -> &[&'static str] {
                &[]
            }
            fn parse_file(
                &self,
                _f: &Path,
                _s: &str,
                _m: &mut Module,
            ) -> projectmind_plugin_api::Result<()> {
                Ok(())
            }
            fn provided_tabs(&self) -> &[TabContribution] {
                &[TabContribution {
                    id: "files",
                    label_key: "nav.files.shadowed",
                    view_mode: "classes",
                }]
            }
        }
        let dir = tempdir();
        let mut engine = Engine::new();
        engine.register_language(Box::new(ShadowPlugin));
        let repo = engine.open_repo(dir.path()).unwrap();
        let tabs = engine.available_tabs(&repo);
        let files: Vec<_> = tabs.iter().filter(|t| t.id == "files").collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].label_key, "nav.files");
    }

    fn tempdir() -> TempDir {
        TempDir::new()
    }

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            // Per-test directory: PID + a process-wide monotonic counter so
            // tests running in parallel (cargo's default) don't share a path
            // and stomp each other on Drop.
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-test-{}-{}", std::process::id(), n));
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

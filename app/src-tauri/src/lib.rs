// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tauri commands for the plaintext-ide shell.
//!
//! The app holds the same [`Engine`] as the MCP server. Commands are intentionally thin wrappers
//! around `core` operations so the same logic is exercised by both the LLM (via MCP) and the
//! human (via the UI).

#![warn(clippy::pedantic)]
#![allow(
    clippy::needless_pass_by_value,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use plaintext_ide_core::git::{self, ChangedFile};
use plaintext_ide_core::{diagram, Engine, Repository};
use plaintext_ide_framework_lombok::LombokPlugin;
use plaintext_ide_framework_spring::SpringPlugin;
use plaintext_ide_lang_java::JavaPlugin;
use serde::Serialize;
use tauri::State;

/// Application state shared across Tauri command handlers.
#[derive(Debug)]
pub struct AppState {
    pub engine: Engine,
    pub repo: RwLock<Option<Repository>>,
}

impl AppState {
    fn new() -> Self {
        let mut engine = Engine::new();
        engine.register_language(Box::new(JavaPlugin::new()));
        engine.register_framework(Box::new(SpringPlugin::new()));
        engine.register_framework(Box::new(LombokPlugin::new()));
        Self {
            engine,
            repo: RwLock::new(None),
        }
    }
}

/// Public summary of an opened repository.
#[derive(Debug, Serialize)]
pub struct RepoSummary {
    pub root: PathBuf,
    pub modules: usize,
    pub classes: usize,
    pub language_plugins: Vec<&'static str>,
    pub framework_plugins: Vec<&'static str>,
}

/// One class entry exposed to the UI.
#[derive(Debug, Serialize)]
pub struct ClassEntry {
    pub fqn: String,
    pub name: String,
    pub file: PathBuf,
    pub stereotypes: Vec<String>,
    pub kind: String,
    pub module: String,
}

/// Per-module summary for the UI.
#[derive(Debug, Serialize)]
pub struct ModuleEntry {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub classes: usize,
    pub stereotypes: std::collections::BTreeMap<String, u32>,
}

/// Detailed class data with source code.
#[derive(Debug, Serialize)]
pub struct ClassDetails {
    pub fqn: String,
    pub file: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
    pub source: String,
}

/// Tauri command: open a repository.
#[tauri::command]
fn open_repo(path: String, state: State<'_, Arc<AppState>>) -> Result<RepoSummary, String> {
    let repo = state
        .engine
        .open_repo(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    let summary = RepoSummary {
        root: repo.root.clone(),
        modules: repo.modules.len(),
        classes: repo.class_count(),
        language_plugins: state.engine.language_ids(),
        framework_plugins: state.engine.framework_ids(),
    };
    *state.repo.write() = Some(repo);
    Ok(summary)
}

#[tauri::command]
fn list_classes(
    stereotype: Option<String>,
    module: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ClassEntry>, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let mut out = Vec::new();
    for (mod_id, m) in &repo.modules {
        if let Some(target) = module.as_deref() {
            if mod_id != target {
                continue;
            }
        }
        for class in m.classes.values() {
            if let Some(s) = stereotype.as_deref() {
                if !class.stereotypes.iter().any(|x| x == s) {
                    continue;
                }
            }
            out.push(ClassEntry {
                fqn: class.fqn.clone(),
                name: class.name.clone(),
                file: class.file.clone(),
                stereotypes: class.stereotypes.clone(),
                kind: format!("{:?}", class.kind).to_lowercase(),
                module: mod_id.clone(),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
fn list_modules(state: State<'_, Arc<AppState>>) -> Result<Vec<ModuleEntry>, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let mut out = Vec::new();
    for module in repo.modules.values() {
        let mut counts = std::collections::BTreeMap::new();
        for class in module.classes.values() {
            for s in &class.stereotypes {
                *counts.entry(s.clone()).or_insert(0_u32) += 1;
            }
        }
        out.push(ModuleEntry {
            id: module.id.clone(),
            name: module.name.clone(),
            root: module.root.clone(),
            classes: module.classes.len(),
            stereotypes: counts,
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.classes));
    Ok(out)
}

#[tauri::command]
fn show_class(fqn: String, state: State<'_, Arc<AppState>>) -> Result<ClassDetails, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let (_, class) = repo
        .find_class(&fqn)
        .ok_or_else(|| format!("class not found: {fqn}"))?;
    let abs = repo.absolute(&class.file);
    let source = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
    Ok(ClassDetails {
        fqn: class.fqn.clone(),
        file: class.file.clone(),
        line_start: class.line_start,
        line_end: class.line_end,
        source,
    })
}

#[tauri::command]
fn list_changes_since(
    reference: String,
    to: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ChangedFile>, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    git::list_changes_since(&repo.root, &reference, to.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn show_diagram(kind: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let spring = SpringPlugin::new();
    match kind.as_str() {
        "bean-graph" => Ok(diagram::render_bean_graph(repo, &spring)),
        "package-tree" => Ok(diagram::render_package_tree(repo)),
        other => Err(format!("unknown diagram kind: {other}")),
    }
}

/// Tauri entrypoint.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::try_init().ok();
    let state = Arc::new(AppState::new());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            open_repo,
            list_classes,
            list_modules,
            show_class,
            list_changes_since,
            show_diagram,
        ])
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initialises_engine() {
        let s = AppState::new();
        assert_eq!(s.engine.language_ids(), vec!["lang-java"]);
        assert_eq!(
            s.engine.framework_ids(),
            vec!["framework-spring", "framework-lombok"]
        );
        assert!(s.repo.read().is_none());
    }

    #[test]
    fn diagram_renders_empty_when_no_repo() {
        let repo = Repository::default();
        let spring = SpringPlugin::new();
        let out = diagram::render_bean_graph(&repo, &spring);
        assert!(out.contains("no beans detected"));
    }
}

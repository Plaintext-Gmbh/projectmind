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
use plaintext_ide_core::files::{self, MarkdownFile};
use plaintext_ide_core::git::{self, ChangedFile};
use plaintext_ide_core::heartbeat;
use plaintext_ide_core::html::{self, HtmlFile, HtmlSnippet};
use plaintext_ide_core::state::{self, UiState, ViewIntent};
use plaintext_ide_core::walkthrough::{
    self as wt, FeedbackEvent, FeedbackKind, FeedbackLog, Walkthrough,
};
use plaintext_ide_core::{diagram, Engine, Repository};
use plaintext_ide_framework_lombok::LombokPlugin;
use plaintext_ide_framework_spring::SpringPlugin;
use plaintext_ide_lang_java::JavaPlugin;
use plaintext_ide_lang_rust::RustPlugin;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

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
        engine.register_language(Box::new(RustPlugin::new()));
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
    let root = repo.root.clone();
    *state.repo.write() = Some(repo);
    // Publish so the MCP server (and any other consumer) sees what we just opened.
    // Preserve the existing view intent when the repo path is unchanged — this
    // happens when applyState() loads the repo in response to an MCP-driven
    // intent (e.g. walkthrough_start), and we don't want this open_repo write
    // to clobber the intent that triggered it.
    let prev = state::read().ok().flatten().unwrap_or_default();
    let same_repo = prev.repo_root.as_ref() == Some(&root);
    publish_state(UiState {
        repo_root: Some(root),
        view: if same_repo {
            prev.view
        } else {
            ViewIntent::default()
        },
        ..UiState::default()
    });
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
    let (module, class) = repo
        .find_class(&fqn)
        .ok_or_else(|| format!("class not found: {fqn}"))?;
    let abs = module.root.join(&class.file);
    let source =
        std::fs::read_to_string(&abs).map_err(|e| format!("read {}: {e}", abs.display()))?;
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

/// Read an arbitrary file as UTF-8 text. Used by the file viewer for `view_file`
/// intents (markdown, plain source, etc.). Capped at 10 MB to keep the view
/// responsive — large binaries are not the target.
#[tauri::command]
fn read_file_text(path: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {path}"));
    }
    let bytes = std::fs::read(p).map_err(|e| format!("read {path}: {e}"))?;
    if bytes.len() > 10_000_000 {
        return Err(format!(
            "file too large ({} bytes; limit 10 MB)",
            bytes.len()
        ));
    }
    String::from_utf8(bytes).map_err(|e| format!("invalid UTF-8 in {path}: {e}"))
}

/// Return the unified diff between two refs (or `ref` vs working tree). Used
/// by the diff viewer.
#[tauri::command]
fn show_diff(
    reference: String,
    to: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    git::unified_diff(&repo.root, &reference, to.as_deref()).map_err(|e| e.to_string())
}

/// Initial state on app startup. The frontend calls this once to pick up
/// whatever the MCP server may have left behind from a previous session.
#[tauri::command]
fn current_state() -> Option<UiState> {
    state::read().ok().flatten()
}

/// Read the active walk-through body, or `None` when no tour is in progress.
#[tauri::command]
fn current_walkthrough() -> Option<Walkthrough> {
    wt::read_body().ok().flatten()
}

/// Read the feedback log for the active tour. Empty if none.
#[tauri::command]
fn current_walkthrough_feedback() -> FeedbackLog {
    wt::read_feedback().unwrap_or_default()
}

/// User clicks "Verstanden" — record the ack and (typically) advance the
/// pointer. Bumping the pointer is done by the frontend through
/// `set_walkthrough_step`; this command only writes the feedback event.
#[tauri::command]
fn walkthrough_ack(walkthrough_id: String, step: u32) -> Result<FeedbackLog, String> {
    let event = FeedbackEvent {
        walkthrough_id,
        step,
        kind: FeedbackKind::Understood,
        comment: None,
        ts: now_secs(),
    };
    wt::append_feedback(event).map_err(|e| e.to_string())
}

/// User clicks "Bitte genauer beschreiben" — record the request with an
/// optional free-text note. Pointer stays put.
#[tauri::command]
fn walkthrough_request_more(
    walkthrough_id: String,
    step: u32,
    comment: Option<String>,
) -> Result<FeedbackLog, String> {
    let event = FeedbackEvent {
        walkthrough_id,
        step,
        kind: FeedbackKind::MoreDetail,
        comment,
        ts: now_secs(),
    };
    wt::append_feedback(event).map_err(|e| e.to_string())
}

/// End the active tour from the GUI side. Removes body + feedback log
/// and resets the view intent so the user lands back on the empty
/// welcome screen. The LLM can detect that no tour is active anymore
/// via `current_state`.
#[tauri::command]
fn end_walkthrough() -> Result<(), String> {
    wt::clear().map_err(|e| e.to_string())?;
    let prev = state::read().ok().flatten().unwrap_or_default();
    let payload = UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::default(),
        ..UiState::default()
    };
    state::write(payload).map_err(|e| e.to_string())?;
    Ok(())
}

/// Move the active tour's pointer (manual sidebar click). Publishes a
/// new `UiState` so the LLM can observe where the user navigated to.
#[tauri::command]
fn set_walkthrough_step(id: String, step: u32) -> Result<(), String> {
    let prev = state::read().ok().flatten().unwrap_or_default();
    let payload = UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Walkthrough { id, step },
        ..UiState::default()
    };
    state::write(payload).map_err(|e| e.to_string())?;
    Ok(())
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// List every markdown file under `root` (recursive, gitignore-aware,
/// build-output dirs filtered). Used by the file viewer's project-wide
/// markdown picker.
#[tauri::command]
fn list_markdown_files(root: String) -> Result<Vec<MarkdownFile>, String> {
    let p = std::path::Path::new(&root);
    if !p.is_absolute() {
        return Err(format!("root must be absolute: {root}"));
    }
    if !p.is_dir() {
        return Err(format!("root is not a directory: {root}"));
    }
    Ok(files::list_markdown_files(p))
}

/// List every HTML/XHTML/JSP/template file under `root`. Used by the HTML
/// browser's file panel.
#[tauri::command]
fn list_html_files(root: String) -> Result<Vec<HtmlFile>, String> {
    let p = std::path::Path::new(&root);
    if !p.is_absolute() {
        return Err(format!("root must be absolute: {root}"));
    }
    if !p.is_dir() {
        return Err(format!("root is not a directory: {root}"));
    }
    Ok(html::list_html_files(p))
}

/// Scan source files under `root` for HTML snippets in string literals.
#[tauri::command]
fn find_html_snippets(root: String) -> Result<Vec<HtmlSnippet>, String> {
    let p = std::path::Path::new(&root);
    if !p.is_absolute() {
        return Err(format!("root must be absolute: {root}"));
    }
    if !p.is_dir() {
        return Err(format!("root is not a directory: {root}"));
    }
    Ok(html::find_html_snippets(p))
}

/// Best-effort publish: GUI tells the MCP/cooperating processes about its state.
fn publish_state(payload: UiState) {
    if let Err(err) = state::write(payload) {
        tracing::warn!(error = %err, "failed to publish UI state from GUI");
    }
}

/// Liveness signal for the MCP server. Tauri shell writes a heartbeat every
/// few seconds so the MCP side can decide whether it needs to (re)launch the
/// GUI when the LLM issues a `view_*` intent.
fn spawn_heartbeat() {
    use std::thread;
    use std::time::Duration;
    thread::spawn(|| loop {
        if let Err(err) = heartbeat::write() {
            tracing::debug!(error = %err, "heartbeat write failed");
        }
        thread::sleep(Duration::from_secs(2));
    });
}

/// Watch the statefile for external changes (i.e. an MCP write) and forward
/// each new state to the frontend via Tauri events. Best-effort: a failure to
/// set up the watcher is logged but does not block app startup.
fn spawn_state_watcher(handle: AppHandle) {
    use notify::{event::EventKind, Event, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;
    use std::thread;

    thread::spawn(move || {
        let path = state::statefile_path();
        let parent = match path.parent() {
            Some(p) => {
                let _ = std::fs::create_dir_all(p);
                p.to_path_buf()
            }
            None => return,
        };

        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(err) => {
                tracing::warn!(error = %err, "could not create state watcher");
                return;
            }
        };
        if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
            tracing::warn!(error = %err, "could not watch {}", parent.display());
            return;
        }

        let mut last_seq: u64 = state::read().ok().flatten().map_or(0, |s| s.seq);
        for ev in rx {
            let Ok(ev) = ev else { continue };
            if !matches!(
                ev.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any
            ) {
                continue;
            }
            if !ev.paths.iter().any(|p| p == &path) {
                continue;
            }
            let Ok(Some(new_state)) = state::read() else {
                continue;
            };
            if new_state.seq <= last_seq {
                continue;
            }
            last_seq = new_state.seq;
            if let Err(err) = handle.emit("state-changed", &new_state) {
                tracing::warn!(error = %err, "failed to emit state-changed");
            }
        }
    });
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
            show_diff,
            read_file_text,
            current_state,
            list_markdown_files,
            list_html_files,
            find_html_snippets,
            current_walkthrough,
            current_walkthrough_feedback,
            walkthrough_ack,
            walkthrough_request_more,
            set_walkthrough_step,
            end_walkthrough,
        ])
        .setup(|app| {
            spawn_state_watcher(app.handle().clone());
            spawn_heartbeat();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initialises_engine() {
        let s = AppState::new();
        assert_eq!(s.engine.language_ids(), vec!["lang-java", "lang-rust"]);
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

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tauri commands for the projectmind shell.
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
use projectmind_core::files::{self, MarkdownFile, MarkdownHit, ModuleFile};
use projectmind_core::git::{self, ChangedFile};
use projectmind_core::heartbeat;
use projectmind_core::html::{self, HtmlFile, HtmlSnippet};
use projectmind_core::state::{self, UiState, ViewIntent};
use projectmind_core::walkthrough::{
    self as wt, FeedbackEvent, FeedbackKind, FeedbackLog, Walkthrough,
};
use projectmind_core::{diagram, Engine, Repository};
use projectmind_framework_lombok::LombokPlugin;
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;
use projectmind_lang_rust::RustPlugin;
use projectmind_plugin_api::{Class, TypeRefKind, Visibility};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

/// Application state shared across Tauri command handlers.
#[derive(Debug)]
pub struct AppState {
    pub engine: Engine,
    pub repo: RwLock<Option<Repository>>,
    /// Per-repo annotation store. Lazily created when `open_repo` lands;
    /// reset to `None` when a different repo is opened. Read-mostly so
    /// list / all queries never block adds, but mutations briefly take
    /// a write lock.
    pub annotations: RwLock<Option<projectmind_core::annotations::JsonAnnotationStore>>,
    pub pending_markdown_file: RwLock<Option<PathBuf>>,
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
            annotations: RwLock::new(None),
            pending_markdown_file: RwLock::new(None),
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
    /// Number of markdown files. Used by the GUI to hide the MD tab when there's
    /// nothing to show.
    pub markdown_count: usize,
    /// Total HTML/XHTML/JSP/template files plus extracted snippets. The GUI
    /// hides the HTML tab when this is zero.
    pub html_count: usize,
    /// Diagram kinds available for the active plugin set + repo content.
    /// The GUI iterates this to render Diagram-tab buttons dynamically
    /// instead of hard-coding the bean-graph / package-tree / folder-map
    /// triple.
    pub available_diagrams: Vec<String>,
    /// Top-level UI tabs the active plugin set contributes for this repo.
    pub tabs: Vec<projectmind_core::TabDescriptor>,
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

/// One annotation as it appears in the class outline. `raw_args` is the
/// literal text inside the parentheses (`value="/users", method=GET`)
/// when the source declared call-style arguments; `null` for plain
/// marker annotations like `@Override`.
#[derive(Debug, Serialize)]
pub struct AnnotationRef {
    pub name: String,
    pub raw_args: Option<String>,
}

/// Structural outline of a class — methods, fields, annotations, no source.
/// Used by the GUI's `ClassViewer` to render a side-panel with click-to-jump
/// navigation. The same data is exposed via the `class_outline` MCP tool.
#[derive(Debug, Serialize)]
pub struct ClassOutline {
    pub fqn: String,
    pub name: String,
    pub kind: String,
    pub visibility: String,
    pub line_start: u32,
    pub line_end: u32,
    pub stereotypes: Vec<String>,
    pub annotations: Vec<AnnotationRef>,
    pub methods: Vec<MethodOutline>,
    pub fields: Vec<FieldOutline>,
    /// Declared parent types: `extends` targets first, then `implements` /
    /// trait-impl targets. Drives the inheritance crumb in the GUI header.
    pub super_types: Vec<SuperTypeOutline>,
}

#[derive(Debug, Serialize)]
pub struct SuperTypeOutline {
    pub name: String,
    /// `"extends"` or `"implements"`.
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct MethodOutline {
    pub name: String,
    pub visibility: String,
    pub is_static: bool,
    pub line_start: u32,
    pub line_end: u32,
    pub annotations: Vec<AnnotationRef>,
}

#[derive(Debug, Serialize)]
pub struct FieldOutline {
    pub name: String,
    #[serde(rename = "type")]
    pub type_text: String,
    pub visibility: String,
    pub is_static: bool,
    pub line: u32,
    pub annotations: Vec<AnnotationRef>,
}

/// Tauri command: open a repository.
#[tauri::command]
fn open_repo(path: String, state: State<'_, Arc<AppState>>) -> Result<RepoSummary, String> {
    let repo = state
        .engine
        .open_repo(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    Ok(open_repository(repo, &state, ViewIntent::default()))
}

/// Tauri command: open a Markdown document as a virtual repository.
#[tauri::command]
fn open_markdown_file(
    path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<RepoSummary, String> {
    let repo = state
        .engine
        .open_markdown_file(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    let file = repo.root.clone();
    Ok(open_repository(
        repo,
        &state,
        ViewIntent::File {
            path: file,
            anchor: None,
        },
    ))
}

fn open_repository(
    repo: Repository,
    state: &State<'_, Arc<AppState>>,
    new_view: ViewIntent,
) -> RepoSummary {
    let markdown_count = files::list_markdown_files(&repo.root).len();
    let html_count =
        html::list_html_files(&repo.root).len() + html::find_html_snippets(&repo.root).len();
    let available_diagrams = state.engine.available_diagrams(&repo);
    let tabs = state.engine.available_tabs(&repo);
    let summary = RepoSummary {
        root: repo.root.clone(),
        modules: repo.modules.len(),
        classes: repo.class_count(),
        language_plugins: state.engine.language_ids(),
        framework_plugins: state.engine.framework_ids(),
        markdown_count,
        html_count,
        available_diagrams,
        tabs,
    };
    let root = repo.root.clone();
    *state.repo.write() = Some(repo);
    // (Re-)open the per-repo annotation store. A failure to load is
    // surfaced to logs but doesn't fail the open — the GUI just won't
    // have annotation features for this repo.
    let annotation_root = if root.is_file() {
        root.parent().unwrap_or(&root)
    } else {
        &root
    };
    match projectmind_core::annotations::JsonAnnotationStore::open(annotation_root) {
        Ok(store) => {
            *state.annotations.write() = Some(store);
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to open annotations store; continuing without one");
            *state.annotations.write() = None;
        }
    }
    // Publish so the MCP server (and any other consumer) sees what we just opened.
    // Preserve the existing view intent when the repo path is unchanged — this
    // happens when applyState() loads the repo in response to an MCP-driven
    // intent (e.g. walkthrough_start), and we don't want this open_repo write
    // to clobber the intent that triggered it.
    let prev = state::read().ok().flatten().unwrap_or_default();
    let same_repo = prev.repo_root.as_ref() == Some(&root);
    publish_state(UiState {
        repo_root: Some(root),
        view: if same_repo { prev.view } else { new_view },
        ..UiState::default()
    });
    summary
}

#[tauri::command]
fn pending_markdown_file(state: State<'_, Arc<AppState>>) -> Option<PathBuf> {
    state.pending_markdown_file.write().take()
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
fn class_outline(fqn: String, state: State<'_, Arc<AppState>>) -> Result<ClassOutline, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let (_module, class) = repo
        .find_class(&fqn)
        .ok_or_else(|| format!("class not found: {fqn}"))?;
    Ok(build_class_outline(class))
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
fn file_recency(state: State<'_, Arc<AppState>>) -> Result<Vec<git::FileRecency>, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    git::file_recency(&repo.root).map_err(|e| e.to_string())
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
        "folder-map" => Ok(diagram::render_folder_map(repo)),
        "inheritance-tree" => Ok(diagram::render_inheritance_tree(repo)),
        "c4-container" => Ok(diagram::render_c4_container(repo, &spring)),
        "doc-graph" => serde_json::to_string(&projectmind_core::doc_graph::build(&repo.root))
            .map_err(|e| e.to_string()),
        other => Err(format!("unknown diagram kind: {other}")),
    }
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct AnnotationInput {
    pub file: String,
    pub line_from: u32,
    pub line_to: u32,
    pub label: String,
    #[serde(default)]
    pub link: Option<String>,
}

#[tauri::command]
fn list_annotations(
    file: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<projectmind_plugin_api::storage::AnnotationRecord>, String> {
    use projectmind_plugin_api::storage::AnnotationStore;
    let guard = state.annotations.read();
    let store = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    match file {
        Some(f) => store.list(&f).map_err(|e| e.to_string()),
        None => store.all().map_err(|e| e.to_string()),
    }
}

#[tauri::command]
fn add_annotation(
    annotation: AnnotationInput,
    state: State<'_, Arc<AppState>>,
) -> Result<u64, String> {
    use projectmind_plugin_api::storage::{AnnotationRecord, AnnotationStore};
    let mut guard = state.annotations.write();
    let store = guard
        .as_mut()
        .ok_or_else(|| "no repository open".to_string())?;
    let record = AnnotationRecord {
        id: 0,
        file: annotation.file,
        line_from: annotation.line_from,
        line_to: annotation.line_to,
        label: annotation.label,
        link: annotation.link,
        extras: serde_json::Map::default(),
    };
    store.add(record).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_annotation(id: u64, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    use projectmind_plugin_api::storage::AnnotationStore;
    let mut guard = state.annotations.write();
    let store = guard
        .as_mut()
        .ok_or_else(|| "no repository open".to_string())?;
    store.remove(id).map_err(|e| e.to_string())
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

/// Open an external URL in the user's default browser. The narration
/// in walk-through tours often contains GitHub / docs links; we route
/// those through `tauri-plugin-opener` so they don't accidentally
/// navigate the embedded webview away from the app shell.
#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("mailto:"))
    {
        return Err(format!(
            "refusing to open non-http(s)/mailto url: {trimmed}"
        ));
    }
    app.opener()
        .open_url(trimmed, None::<&str>)
        .map_err(|e| e.to_string())
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
    if !p.is_dir() && !p.is_file() {
        return Err(format!("root is not a directory or file: {root}"));
    }
    Ok(files::list_markdown_files(p))
}

/// Fuzzy-search markdown files. Returns scored hits across title, path,
/// and content snippets. With an empty query it falls through to the plain
/// listing — same as `list_markdown_files`.
#[tauri::command]
fn search_markdown(root: String, query: String, limit: usize) -> Result<Vec<MarkdownHit>, String> {
    let p = std::path::Path::new(&root);
    if !p.is_absolute() {
        return Err(format!("root must be absolute: {root}"));
    }
    if !p.is_dir() && !p.is_file() {
        return Err(format!("root is not a directory or file: {root}"));
    }
    let cap = if limit == 0 { 200 } else { limit };
    Ok(files::search_markdown(p, &query, cap))
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

/// List PDFs and images that live inside a module's root. Used by the
/// Code-tab sidebar so non-source assets sit alongside the parsed class
/// listing. Returns an empty Vec when the module has no matching files.
#[tauri::command]
fn list_module_files(
    module_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ModuleFile>, String> {
    let guard = state.repo.read();
    let repo = guard
        .as_ref()
        .ok_or_else(|| "no repository open".to_string())?;
    let module = repo
        .modules
        .get(&module_id)
        .ok_or_else(|| format!("module not found: {module_id}"))?;
    Ok(files::list_module_files(
        &module.root,
        &["pdf", "png", "jpg", "jpeg", "webp", "gif"],
    ))
}

/// Build-integrity payload returned to the frontend. Lets the user verify
/// they're running an official signed release vs. a self-compiled dev build.
///
/// Two markers drive `is_release_build`:
///   - `PROJECTMIND_RELEASE_BUILD=1` env var at compile time (set by the CI
///     release matrix only on tag-pushes from the official repo)
///   - `PROJECTMIND_GIT_COMMIT` env var at compile time (the SHA the bundle
///     was built from)
///
/// `updater_pubkey_hash` is a `SHA-256` over the bytes of the embedded ed25519
/// public key, so a fork that swapped the key out shows a different hash and
/// the UI can flag it as a non-official updater channel.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BuildIntegrity {
    /// Semantic version from Cargo.toml.
    pub version: String,
    /// True when the bundle was produced by the official tagged release
    /// pipeline (env var `PROJECTMIND_RELEASE_BUILD=1` at build time).
    pub is_release_build: bool,
    /// Git commit the bundle was built from, if the build-time env var was
    /// set. `None` for casual local builds.
    pub git_commit: Option<String>,
    /// Build timestamp (RFC3339) when the env var was provided by CI.
    pub built_at: Option<String>,
    /// `SHA-256` of the embedded updater public key. Stable hash → official
    /// key; different hash → fork or custom-keyed build.
    pub updater_pubkey_hash: String,
    /// Hex-truncated convenience copy of the same hash for compact display.
    pub updater_pubkey_short: String,
}

/// Tauri command: report the running bundle's integrity markers to the UI.
#[tauri::command]
fn get_build_integrity() -> BuildIntegrity {
    use sha2::{Digest, Sha256};

    let pubkey = include_str!("../tauri.conf.json");
    // Hash only the updater pubkey value so unrelated config edits don't
    // change the official-channel marker shown in the integrity dialog.
    let mut hasher = Sha256::new();
    hasher.update(extract_pubkey_bytes(pubkey));
    let digest = hasher.finalize();
    let updater_pubkey_hash = hex_encode(&digest);
    let updater_pubkey_short = updater_pubkey_hash.chars().take(12).collect::<String>();

    BuildIntegrity {
        version: env!("CARGO_PKG_VERSION").to_string(),
        is_release_build: matches!(option_env!("PROJECTMIND_RELEASE_BUILD"), Some("1")),
        git_commit: option_env!("PROJECTMIND_GIT_COMMIT").map(str::to_string),
        built_at: option_env!("PROJECTMIND_BUILT_AT").map(str::to_string),
        updater_pubkey_hash,
        updater_pubkey_short,
    }
}

/// Best-effort extraction of the updater pubkey field from `tauri.conf.json`'s
/// raw text, so the `SHA-256` we surface is over the *key value*, not the
/// whole config (which would change every time we touch unrelated fields). We
/// don't pull in `serde_json` here since this runs in a hot UI path; a
/// regex-free substring match is plenty for a fixed-format JSON config.
fn extract_pubkey_bytes(conf: &str) -> &[u8] {
    if let Some(start) = conf.find("\"pubkey\":") {
        let after = &conf[start + "\"pubkey\":".len()..];
        if let Some(q1) = after.find('"') {
            let inner = &after[q1 + 1..];
            if let Some(q2) = inner.find('"') {
                return &inner.as_bytes()[..q2];
            }
        }
    }
    conf.as_bytes()
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Render a [`Visibility`] enum as the lowercase string the frontend expects
/// (`"public"`, `"protected"`, `"package"`, `"private"`). Matches the rendering
/// the MCP `class_outline` tool uses, so MCP and GUI stay in sync.
fn visibility_str(v: Visibility) -> String {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::PackagePrivate => "package",
        Visibility::Private => "private",
    }
    .to_string()
}

/// Convert a parsed [`projectmind_plugin_api::Annotation`] into the leaner
/// `AnnotationRef` the GUI consumes. Drops the optional `fqn` (rarely
/// resolved by Phase 1 plugins, never used by the frontend) and keeps the
/// simple name + raw argument text.
fn annotation_ref(a: &projectmind_plugin_api::Annotation) -> AnnotationRef {
    AnnotationRef {
        name: a.name.clone(),
        raw_args: a.raw_args.clone(),
    }
}

/// Build a [`ClassOutline`] from a parsed [`Class`]. Pure data shaping — no
/// I/O, no source reading. Reused by the Tauri command and (in `browser-host`)
/// by the HTTP endpoint serving the same payload.
fn build_class_outline(class: &Class) -> ClassOutline {
    ClassOutline {
        fqn: class.fqn.clone(),
        name: class.name.clone(),
        kind: format!("{:?}", class.kind).to_lowercase(),
        visibility: visibility_str(class.visibility),
        line_start: class.line_start,
        line_end: class.line_end,
        stereotypes: class.stereotypes.clone(),
        annotations: class.annotations.iter().map(annotation_ref).collect(),
        methods: class
            .methods
            .iter()
            .map(|m| MethodOutline {
                name: m.name.clone(),
                visibility: visibility_str(m.visibility),
                is_static: m.is_static,
                line_start: m.line_start,
                line_end: m.line_end,
                annotations: m.annotations.iter().map(annotation_ref).collect(),
            })
            .collect(),
        fields: class
            .fields
            .iter()
            .map(|f| FieldOutline {
                name: f.name.clone(),
                type_text: f.type_text.clone(),
                visibility: visibility_str(f.visibility),
                is_static: f.is_static,
                line: f.line,
                annotations: f.annotations.iter().map(annotation_ref).collect(),
            })
            .collect(),
        super_types: class
            .super_types
            .iter()
            .map(|t| SuperTypeOutline {
                name: t.name.clone(),
                kind: match t.kind {
                    TypeRefKind::Extends => "extends".to_string(),
                    TypeRefKind::Implements => "implements".to_string(),
                },
            })
            .collect(),
    }
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

fn markdown_launch_arg(args: impl IntoIterator<Item = String>) -> Option<PathBuf> {
    args.into_iter()
        .map(PathBuf::from)
        .find(|p| is_markdown_path(p))
}

fn is_markdown_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx"))
}

fn queue_markdown_file(app: &AppHandle, state: &Arc<AppState>, path: PathBuf) {
    *state.pending_markdown_file.write() = Some(path.clone());
    if let Err(err) = app.emit("open-markdown-file", path) {
        tracing::debug!(error = %err, "failed to emit open-markdown-file");
    }
}

/// Tauri entrypoint.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::try_init().ok();
    let state = Arc::new(AppState::new());
    let mut builder = tauri::Builder::default();
    // Single-instance guard: when an `open -a ProjectMind.app` (or equivalent)
    // fires while we're already running, the second process forwards its
    // launch args here and exits. We focus the existing window so the user
    // sees the running instance reacting instead of a stray new window.
    // Belt-and-suspenders with the MCP heartbeat check in `launch::ensure_gui_running`.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        let single_state = Arc::clone(&state);
        builder = builder.plugin(tauri_plugin_single_instance::init(
            move |app: &AppHandle, args: Vec<String>, _cwd: String| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                if let Some(path) = markdown_launch_arg(args) {
                    queue_markdown_file(app, &single_state, path);
                }
            },
        ));
    }
    let setup_state = Arc::clone(&state);
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            open_repo,
            open_markdown_file,
            pending_markdown_file,
            list_classes,
            list_modules,
            show_class,
            class_outline,
            list_changes_since,
            file_recency,
            list_annotations,
            add_annotation,
            remove_annotation,
            show_diagram,
            show_diff,
            read_file_text,
            current_state,
            list_markdown_files,
            search_markdown,
            list_html_files,
            find_html_snippets,
            list_module_files,
            current_walkthrough,
            current_walkthrough_feedback,
            walkthrough_ack,
            walkthrough_request_more,
            set_walkthrough_step,
            end_walkthrough,
            open_external,
            get_build_integrity,
        ])
        .setup(move |app| {
            if let Some(path) = markdown_launch_arg(std::env::args()) {
                queue_markdown_file(app.handle(), &setup_state, path);
            }
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

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tool definitions and `tools/call` dispatch.

use std::path::PathBuf;

use projectmind_browser_host::{self as browser_host, BrowserHostConfig};
use projectmind_core::file_access;
use projectmind_core::files;
use projectmind_core::state::{self, UiState, ViewIntent};
use projectmind_core::walkthrough::{self as wt, Walkthrough, WalkthroughStep};
use projectmind_core::{diagram, git, html};
use projectmind_framework_spring::SpringPlugin;
use projectmind_plugin_api::FrameworkPlugin;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::handler::{with_repo, DispatchError, DispatchResult, ServerState, ToolCallParams};
use crate::launch;

/// JSON Schema for the `open_repo` tool.
fn open_repo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Absolute path to the repository root" }
        },
        "required": ["path"]
    })
}

fn no_args_schema() -> Value {
    json!({ "type": "object", "additionalProperties": false })
}

fn ref_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ref":   { "type": "string", "description": "Git ref to compare against (e.g. HEAD, HEAD~5)" },
            "to":    { "type": "string", "description": "Optional second ref; defaults to working tree" }
        },
        "required": ["ref"]
    })
}

fn show_class_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "fqn": { "type": "string", "description": "Fully-qualified class name" },
            "highlight": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "from": { "type": "integer", "minimum": 1 },
                        "to":   { "type": "integer", "minimum": 1 }
                    },
                    "required": ["from", "to"]
                }
            }
        },
        "required": ["fqn"]
    })
}

fn list_classes_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "stereotype": { "type": "string", "description": "Filter by stereotype, e.g. service" }
        }
    })
}

fn diagram_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": { "type": "string", "enum": ["bean-graph", "package-tree", "folder-map", "doc-graph"] }
        },
        "required": ["type"]
    })
}

fn find_class_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Case-insensitive substring of the simple or fully-qualified name" },
            "limit": { "type": "integer", "minimum": 1, "default": 25 }
        },
        "required": ["query"]
    })
}

fn class_outline_schema() -> Value {
    fqn_schema()
}

fn fqn_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "fqn": { "type": "string", "description": "Fully-qualified class name" }
        },
        "required": ["fqn"]
    })
}

fn walkthrough_step_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title":     { "type": "string", "description": "Short, human-readable step title (sidebar entry)." },
            "narration": { "type": "string", "description": "Markdown shown alongside the target. Optional but strongly recommended — this is what the user reads." },
            "target": {
                "type": "object",
                "description": "What to render in the main pane. `kind` selects the viewer: class | file | diff | note.",
                "properties": {
                    "kind": { "type": "string", "enum": ["class", "file", "diff", "note"] },
                    "fqn":  { "type": "string", "description": "Class FQN (kind=class)" },
                    "path": { "type": "string", "description": "Absolute file path (kind=file)" },
                    "anchor": { "type": "string", "description": "Heading slug (kind=file, markdown only)" },
                    "ref":  { "type": "string", "description": "Base git ref (kind=diff)" },
                    "to":   { "type": "string", "description": "Target git ref or omit for working tree (kind=diff)" },
                    "highlight": {
                        "type": "array",
                        "description": "Line ranges to colour (kind=class or kind=file with non-markdown extension). 1-based inclusive.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "from": { "type": "integer", "minimum": 1 },
                                "to":   { "type": "integer", "minimum": 1 }
                            },
                            "required": ["from", "to"]
                        }
                    }
                },
                "required": ["kind"]
            }
        },
        "required": ["title", "target"]
    })
}

fn walkthrough_start_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id":      { "type": "string", "description": "Optional stable handle. If omitted, derived from `title`." },
            "title":   { "type": "string", "description": "Tour title (header + sidebar caption)." },
            "summary": { "type": "string", "description": "Optional 1-paragraph intro shown above step 1." },
            "steps":   {
                "type": "array",
                "description": "Ordered tour. Must contain at least one step.",
                "items": walkthrough_step_schema()
            }
        },
        "required": ["title", "steps"]
    })
}

fn walkthrough_append_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "step": walkthrough_step_schema()
        },
        "required": ["step"]
    })
}

fn walkthrough_set_step_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "index": { "type": "integer", "minimum": 0, "description": "0-based step index. Clamped to the valid range." }
        },
        "required": ["index"]
    })
}

fn walkthrough_feedback_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "since_ts": { "type": "integer", "minimum": 0, "description": "Unix-seconds; only events with `ts > since_ts` are returned. Omit for the full log." }
        }
    })
}

fn file_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path":   { "type": "string", "description": "Absolute path to the file" },
            "anchor": { "type": "string", "description": "Optional heading slug (e.g. \"installation\" for `## Installation`) to scroll to after rendering. Markdown only." }
        },
        "required": ["path"]
    })
}

fn list_module_files_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "module": { "type": "string", "description": "Module id (as returned by module_summary)" }
        },
        "required": ["module"]
    })
}

fn open_browser_repo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Absolute path to the repository root. Defaults to the currently-open repo or current statefile repo." },
            "port": { "type": "integer", "minimum": 0, "maximum": 65535, "description": "Port to bind; 0 means choose a free port." },
            "open_browser": { "type": "boolean", "default": true, "description": "Open the default browser on this machine after starting." }
        }
    })
}

/// Tool registry — also serves as the response of `tools/list`.
#[allow(clippy::too_many_lines)]
pub(crate) fn list() -> Value {
    json!({
        "tools": [
            {
                "name": "open_repo",
                "description": "Open a repository for inspection. Subsequent tools operate on it.",
                "inputSchema": open_repo_schema()
            },
            {
                "name": "repo_info",
                "description": "Return summary information about the currently open repository.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "list_classes",
                "description": "List parsed classes (optionally filtered by stereotype).",
                "inputSchema": list_classes_schema()
            },
            {
                "name": "show_class",
                "description": "Return source code of a class with optional line-range highlights.",
                "inputSchema": show_class_schema()
            },
            {
                "name": "list_changes_since",
                "description": "List files changed since a given git ref (compared with working tree by default).",
                "inputSchema": ref_schema()
            },
            {
                "name": "show_diff",
                "description": "Return unified diff between a git ref and the working tree (or another ref).",
                "inputSchema": ref_schema()
            },
            {
                "name": "show_diagram",
                "description": "Return diagram data for the current repository (Mermaid for now).",
                "inputSchema": diagram_schema()
            },
            {
                "name": "find_class",
                "description": "Search classes by case-insensitive name substring (simple or FQN).",
                "inputSchema": find_class_schema()
            },
            {
                "name": "class_outline",
                "description": "Return the outline of a class (methods, fields, annotations) without source.",
                "inputSchema": class_outline_schema()
            },
            {
                "name": "module_summary",
                "description": "Per-module summary (classes, stereotype counts).",
                "inputSchema": no_args_schema()
            },
            {
                "name": "list_module_files",
                "description": "List PDFs and images (.pdf .png .jpg .jpeg .webp .gif) inside a module's root. Source files (.java .rs) are excluded — those are surfaced by list_classes.",
                "inputSchema": list_module_files_schema()
            },
            {
                "name": "relations",
                "description": "Return the full bean / injection graph as JSON: list of {from, to, kind, cross_module}.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "plugin_info",
                "description": "List active plugins (languages and frameworks).",
                "inputSchema": no_args_schema()
            },
            {
                "name": "list_html",
                "description": "List HTML / XHTML / JSP / template files (.html .htm .xhtml .jsp .vm .ftl) in the open repository.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "list_html_snippets",
                "description": "Scan source files (.java .kt .groovy .scala) for HTML snippets in string literals (≥2 tags).",
                "inputSchema": no_args_schema()
            },
            {
                "name": "view_class",
                "description": "Tell the GUI to switch to the classes view and open the given class. MCP-driven navigation; takes precedence over manual GUI navigation.",
                "inputSchema": fqn_schema()
            },
            {
                "name": "view_diff",
                "description": "Tell the GUI to switch to the diff view between two git refs (or `ref` vs working tree).",
                "inputSchema": ref_schema()
            },
            {
                "name": "view_file",
                "description": "Tell the GUI to open an arbitrary file. Markdown is rendered (mermaid blocks + images embedded); other extensions show as plain source.",
                "inputSchema": file_schema()
            },
            {
                "name": "view_diagram",
                "description": "Tell the GUI to switch to the diagram view (`bean-graph`, `package-tree`, `folder-map`, or `doc-graph`).",
                "inputSchema": diagram_schema()
            },
            {
                "name": "walkthrough_start",
                "description": "Start a guided tour. The GUI switches to the walk-through view, displaying step 0 with the LLM's narration and the chosen target (class / file / diff / note). Replaces any previous tour.",
                "inputSchema": walkthrough_start_schema()
            },
            {
                "name": "walkthrough_append",
                "description": "Append one step to the active tour. Does NOT move the pointer — useful while streaming a tour as it's being authored.",
                "inputSchema": walkthrough_append_schema()
            },
            {
                "name": "walkthrough_set_step",
                "description": "Move the active tour's pointer to the given 0-based index. Clamped to the valid range.",
                "inputSchema": walkthrough_set_step_schema()
            },
            {
                "name": "walkthrough_clear",
                "description": "End the active tour. Removes the body and feedback log; GUI returns to the previous view.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "walkthrough_feedback",
                "description": "Read user feedback events recorded against the active tour. Each event is one click on the GUI's Verstanden / Genauer-buttons. Useful when the LLM wants to react to the user (e.g. expand a step that was flagged with `more_detail`).",
                "inputSchema": walkthrough_feedback_schema()
            },
            {
                "name": "open_browser_repo",
                "description": "Start the LAN browser host, open a repository, and return tokenized browser URLs. This binds to 0.0.0.0 and requires the returned random token for all API calls.",
                "inputSchema": open_browser_repo_schema()
            },
            {
                "name": "browser_status",
                "description": "Return the running LAN browser host status, including tokenized URLs, or null if it has not been started.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "stop_browser",
                "description": "Forget the LAN browser host status for this MCP process. The listener exits when the process exits.",
                "inputSchema": no_args_schema()
            }
        ]
    })
}

pub(crate) async fn call(state: &Mutex<ServerState>, params: Value) -> DispatchResult {
    let parsed: ToolCallParams = serde_json::from_value(params)
        .map_err(|e| DispatchError::invalid_params(format!("invalid tool/call params: {e}")))?;

    match parsed.name.as_str() {
        "open_repo" => open_repo(state, parsed.arguments).await,
        "repo_info" => repo_info(state).await,
        "list_classes" => list_classes(state, parsed.arguments).await,
        "show_class" => show_class(state, parsed.arguments).await,
        "list_changes_since" => list_changes_since(state, parsed.arguments).await,
        "show_diff" => show_diff(state, parsed.arguments).await,
        "show_diagram" => show_diagram(state, parsed.arguments).await,
        "find_class" => find_class(state, parsed.arguments).await,
        "class_outline" => class_outline(state, parsed.arguments).await,
        "module_summary" => module_summary(state).await,
        "list_module_files" => list_module_files(state, parsed.arguments).await,
        "relations" => relations(state).await,
        "plugin_info" => plugin_info(state).await,
        "list_html" => list_html(state).await,
        "list_html_snippets" => list_html_snippets(state).await,
        "view_class" => view_class(state, parsed.arguments).await,
        "view_diff" => view_diff(parsed.arguments),
        "view_file" => view_file(parsed.arguments),
        "view_diagram" => view_diagram(parsed.arguments),
        "walkthrough_start" => walkthrough_start(parsed.arguments),
        "walkthrough_append" => walkthrough_append(parsed.arguments),
        "walkthrough_set_step" => walkthrough_set_step(parsed.arguments),
        "walkthrough_clear" => walkthrough_clear_handler(),
        "walkthrough_feedback" => walkthrough_feedback(parsed.arguments),
        "open_browser_repo" => open_browser_repo(state, parsed.arguments).await,
        "browser_status" => browser_status(),
        "stop_browser" => stop_browser(),
        other => Err(DispatchError::invalid_params(format!(
            "unknown tool: {other}"
        ))),
    }
}

#[derive(Deserialize)]
struct OpenRepoArgs {
    path: String,
}

async fn open_repo(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: OpenRepoArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("open_repo: {e}")))?;

    let mut server_state = state.lock().await;
    let repo = server_state
        .engine
        .open_repo(std::path::Path::new(&args.path))
        .map_err(|e| DispatchError::internal(format!("open_repo failed: {e}")))?;

    let root = repo.root.clone();
    let summary = json!({
        "root": repo.root,
        "modules": repo.modules.len(),
        "classes": repo.class_count(),
    });
    server_state.repo = Some(repo);

    // Tell the GUI to follow. Best-effort — if the statefile cannot be written
    // (read-only home, etc.), MCP-only usage still works.
    publish_state(UiState {
        repo_root: Some(root),
        view: ViewIntent::default(),
        ..UiState::default()
    });

    Ok(text_result(summary.to_string()))
}

async fn repo_info(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    let summary = with_repo(&state, |repo| {
        Ok(json!({
            "root": repo.root,
            "modules": repo.modules.len(),
            "classes": repo.class_count(),
        }))
    })?;
    Ok(text_result(summary.to_string()))
}

#[derive(Deserialize)]
struct ListClassesArgs {
    #[serde(default)]
    stereotype: Option<String>,
}

async fn list_classes(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: ListClassesArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("list_classes: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let mut out: Vec<Value> = Vec::new();
        for module in repo.modules.values() {
            for class in module.classes.values() {
                if let Some(stereo) = args.stereotype.as_deref() {
                    if !class.stereotypes.iter().any(|s| s == stereo) {
                        continue;
                    }
                }
                out.push(json!({
                    "fqn": class.fqn,
                    "name": class.name,
                    "file": class.file,
                    "stereotypes": class.stereotypes,
                    "kind": class.kind,
                }));
            }
        }
        Ok(text_result(
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

#[derive(Deserialize)]
struct ShowClassArgs {
    fqn: String,
    #[serde(default)]
    highlight: Vec<Highlight>,
}

#[derive(Deserialize, Clone, Copy)]
struct Highlight {
    from: u32,
    to: u32,
}

async fn show_class(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: ShowClassArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("show_class: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let (module, class) = repo.find_class(&args.fqn).ok_or_else(|| {
            DispatchError::invalid_params(format!("class not found: {}", args.fqn))
        })?;
        let abs_file = module.root.join(&class.file);
        let source = std::fs::read_to_string(&abs_file)
            .map_err(|e| DispatchError::internal(format!("read {}: {e}", abs_file.display())))?;

        let body = json!({
            "fqn": class.fqn,
            "file": class.file,
            "line_start": class.line_start,
            "line_end": class.line_end,
            "stereotypes": class.stereotypes,
            "source": source,
            "highlights": args.highlight.iter().map(|h| json!({"from": h.from, "to": h.to})).collect::<Vec<_>>()
        });
        Ok(text_result(body.to_string()))
    })
}

#[derive(Deserialize)]
struct RefArgs {
    #[serde(rename = "ref")]
    from_ref: String,
    #[serde(default)]
    to: Option<String>,
}

async fn list_changes_since(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: RefArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("list_changes_since: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let changes = git::list_changes_since(&repo.root, &args.from_ref, args.to.as_deref())
            .map_err(|e| DispatchError::internal(format!("git: {e}")))?;
        let body = serde_json::to_string_pretty(&changes).unwrap_or_else(|_| "[]".into());
        Ok(text_result(body))
    })
}

async fn show_diff(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: RefArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("show_diff: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let diff = git::unified_diff(&repo.root, &args.from_ref, args.to.as_deref())
            .map_err(|e| DispatchError::internal(format!("git: {e}")))?;
        Ok(text_result(diff))
    })
}

#[derive(Deserialize)]
struct DiagramArgs {
    #[serde(rename = "type")]
    kind: String,
}

async fn show_diagram(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: DiagramArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("show_diagram: {e}")))?;
    let state = state.lock().await;
    let spring = SpringPlugin::new();
    with_repo(&state, |repo| match args.kind.as_str() {
        "bean-graph" => Ok(text_result(diagram::render_bean_graph(repo, &spring))),
        "package-tree" => Ok(text_result(diagram::render_package_tree(repo))),
        "folder-map" => Ok(text_result(diagram::render_folder_map(repo))),
        "doc-graph" => Ok(text_result(
            serde_json::to_string(&projectmind_core::doc_graph::build(&repo.root))
                .map_err(|e| DispatchError::internal(format!("doc-graph failed: {e}")))?,
        )),
        other => Err(DispatchError::invalid_params(format!(
            "unknown diagram: {other}"
        ))),
    })
}

#[derive(Deserialize)]
struct FindClassArgs {
    query: String,
    #[serde(default = "default_find_limit")]
    limit: u32,
}

fn default_find_limit() -> u32 {
    25
}

async fn find_class(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: FindClassArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("find_class: {e}")))?;
    let needle = args.query.to_ascii_lowercase();
    let limit = args.limit as usize;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let mut out: Vec<Value> = Vec::new();
        for module in repo.modules.values() {
            for class in module.classes.values() {
                let lower_fqn = class.fqn.to_ascii_lowercase();
                let lower_name = class.name.to_ascii_lowercase();
                if lower_fqn.contains(&needle) || lower_name.contains(&needle) {
                    out.push(json!({
                        "fqn": class.fqn,
                        "name": class.name,
                        "stereotypes": class.stereotypes,
                        "file": class.file,
                        "line_start": class.line_start,
                    }));
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(text_result(
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

#[derive(Deserialize)]
struct ClassOutlineArgs {
    fqn: String,
}

async fn class_outline(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: ClassOutlineArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("class_outline: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let (_module, class) = repo.find_class(&args.fqn).ok_or_else(|| {
            DispatchError::invalid_params(format!("class not found: {}", args.fqn))
        })?;
        let body = json!({
            "fqn": class.fqn,
            "name": class.name,
            "kind": class.kind,
            "visibility": class.visibility,
            "file": class.file,
            "line_start": class.line_start,
            "line_end": class.line_end,
            "stereotypes": class.stereotypes,
            "annotations": class.annotations.iter().map(|a| json!({
                "name": a.name,
                "raw_args": a.raw_args
            })).collect::<Vec<_>>(),
            "methods": class.methods.iter().map(|m| json!({
                "name": m.name,
                "visibility": m.visibility,
                "is_static": m.is_static,
                "line_start": m.line_start,
                "line_end": m.line_end,
                "annotations": m.annotations.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "fields": class.fields.iter().map(|f| json!({
                "name": f.name,
                "type": f.type_text,
                "visibility": f.visibility,
                "is_static": f.is_static,
                "line": f.line,
                "annotations": f.annotations.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        Ok(text_result(
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        ))
    })
}

async fn module_summary(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let mut modules = Vec::new();
        for module in repo.modules.values() {
            let mut counts: std::collections::BTreeMap<String, u32> =
                std::collections::BTreeMap::default();
            for class in module.classes.values() {
                for s in &class.stereotypes {
                    *counts.entry(s.clone()).or_default() += 1;
                }
            }
            modules.push(json!({
                "id": module.id,
                "name": module.name,
                "root": module.root,
                "classes": module.classes.len(),
                "stereotypes": counts,
            }));
        }
        Ok(text_result(
            serde_json::to_string_pretty(&modules).unwrap_or_default(),
        ))
    })
}

#[derive(Deserialize)]
struct ListModuleFilesArgs {
    module: String,
}

async fn list_module_files(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: ListModuleFilesArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("list_module_files: {e}")))?;
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let module = repo.modules.get(&args.module).ok_or_else(|| {
            DispatchError::invalid_params(format!("module not found: {}", args.module))
        })?;
        let entries =
            files::list_module_files(&module.root, &["pdf", "png", "jpg", "jpeg", "webp", "gif"]);
        Ok(text_result(
            serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

async fn relations(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    let spring = SpringPlugin::new();
    with_repo(&state, |repo| {
        // Map fqn → module to detect cross-module edges.
        let mut node_module: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::default();
        for (mid, module) in &repo.modules {
            for class in module.classes.values() {
                node_module.insert(class.fqn.clone(), mid.clone());
            }
        }
        let mut edges: Vec<Value> = Vec::new();
        for module in repo.modules.values() {
            for rel in spring.relations(module) {
                let from_mod = node_module.get(&rel.from).cloned();
                let to_mod = node_module.get(&rel.to).cloned();
                let cross = match (&from_mod, &to_mod) {
                    (Some(a), Some(b)) => a != b,
                    _ => false,
                };
                edges.push(json!({
                    "from": rel.from,
                    "to": rel.to,
                    "kind": rel.kind,
                    "from_module": from_mod,
                    "to_module": to_mod,
                    "cross_module": cross,
                }));
            }
        }
        Ok(text_result(
            serde_json::to_string_pretty(&edges).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

async fn list_html(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let files = html::list_html_files(&repo.root);
        Ok(text_result(
            serde_json::to_string_pretty(&files).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

async fn list_html_snippets(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let snippets = html::find_html_snippets(&repo.root);
        Ok(text_result(
            serde_json::to_string_pretty(&snippets).unwrap_or_else(|_| "[]".into()),
        ))
    })
}

async fn plugin_info(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    let body = json!({
        "languages": state.engine.language_ids(),
        "frameworks": state.engine.framework_ids(),
    });
    Ok(text_result(body.to_string()))
}

/// Wrap a string into the MCP tool-result content array.
// ----- view_* tools: drive the GUI via the shared state file ----------------

#[derive(Deserialize)]
struct ViewClassArgs {
    fqn: String,
}

async fn view_class(server_state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: ViewClassArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("view_class: {e}")))?;
    // Best-effort: validate the class exists in the currently-open repo so we
    // don't hand the GUI a dangling FQN. If no repo is open we still publish —
    // the GUI may have a different repo loaded that does have it.
    {
        let server_state = server_state.lock().await;
        if let Some(repo) = server_state.repo.as_ref() {
            if repo.find_class(&args.fqn).is_none() {
                return Err(DispatchError::invalid_params(format!(
                    "class not found: {}",
                    args.fqn
                )));
            }
        }
    }

    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Classes {
            selected_fqn: Some(args.fqn.clone()),
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({"ok": true, "fqn": args.fqn}).to_string(),
    ))
}

fn view_diff(args: Value) -> DispatchResult {
    let args: RefArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("view_diff: {e}")))?;
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Diff {
            reference: args.from_ref.clone(),
            to: args.to.clone(),
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({"ok": true, "ref": args.from_ref, "to": args.to}).to_string(),
    ))
}

#[derive(Deserialize)]
struct ViewFileArgs {
    path: String,
    #[serde(default)]
    anchor: Option<String>,
}

fn view_file(args: Value) -> DispatchResult {
    let args: ViewFileArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("view_file: {e}")))?;
    let path = PathBuf::from(&args.path);
    if !path.is_absolute() {
        return Err(DispatchError::invalid_params(format!(
            "view_file: path must be absolute: {}",
            args.path
        )));
    }
    let prev = state::read().ok().flatten().unwrap_or_default();
    // Scope file viewing to the currently-open repo. Without an open repo we
    // refuse the call; with one, file_access canonicalises the path and
    // rejects anything that escapes the repo root.
    let repo_root = prev
        .repo_root
        .clone()
        .ok_or_else(|| DispatchError::invalid_params("view_file: no repository open"))?;
    let path = file_access::canonical_file_in_repo(&repo_root, &path)
        .map_err(|e| DispatchError::invalid_params(format!("view_file: {e}")))?;
    let anchor = args.anchor.clone();
    publish_state(UiState {
        repo_root: Some(repo_root),
        view: ViewIntent::File {
            path: path.clone(),
            anchor: anchor.clone(),
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({"ok": true, "path": path, "anchor": anchor}).to_string(),
    ))
}

#[derive(Deserialize)]
struct DiagramKindArgs {
    #[serde(rename = "type")]
    kind: String,
}

fn view_diagram(args: Value) -> DispatchResult {
    let args: DiagramKindArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("view_diagram: {e}")))?;
    if args.kind != "bean-graph"
        && args.kind != "package-tree"
        && args.kind != "folder-map"
        && args.kind != "doc-graph"
    {
        return Err(DispatchError::invalid_params(format!(
            "unknown diagram type: {}",
            args.kind
        )));
    }
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Diagram {
            diagram_kind: args.kind.clone(),
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({"ok": true, "type": args.kind}).to_string(),
    ))
}

// ----- walkthrough_* tools --------------------------------------------------

#[derive(Deserialize)]
struct WalkthroughStartArgs {
    #[serde(default)]
    id: Option<String>,
    title: String,
    #[serde(default)]
    summary: String,
    steps: Vec<WalkthroughStep>,
}

fn walkthrough_start(args: Value) -> DispatchResult {
    let args: WalkthroughStartArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("walkthrough_start: {e}")))?;
    if args.steps.is_empty() {
        return Err(DispatchError::invalid_params(
            "walkthrough_start: steps must not be empty".to_string(),
        ));
    }
    // Reset feedback log for the new tour.
    if let Err(err) = wt::clear() {
        tracing::warn!(error = %err, "walkthrough_start: failed to clear previous tour");
    }
    let id = args
        .id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| wt::slugify_id(&args.title));
    let body = Walkthrough {
        id: id.clone(),
        title: args.title,
        summary: args.summary,
        steps: args.steps,
        updated_at: 0,
    };
    let written = wt::write_body(body)
        .map_err(|e| DispatchError::internal(format!("walkthrough_start: {e}")))?;
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Walkthrough {
            id: id.clone(),
            step: 0,
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({
            "ok": true,
            "id": id,
            "step": 0,
            "total": written.steps.len(),
        })
        .to_string(),
    ))
}

#[derive(Deserialize)]
struct WalkthroughAppendArgs {
    step: WalkthroughStep,
}

fn walkthrough_append(args: Value) -> DispatchResult {
    let args: WalkthroughAppendArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("walkthrough_append: {e}")))?;
    let mut body = wt::read_body()
        .map_err(|e| DispatchError::internal(format!("walkthrough_append: read body: {e}")))?
        .ok_or_else(|| {
            DispatchError::invalid_params(
                "walkthrough_append: no active tour — call walkthrough_start first".to_string(),
            )
        })?;
    body.steps.push(args.step);
    let written = wt::write_body(body)
        .map_err(|e| DispatchError::internal(format!("walkthrough_append: write: {e}")))?;
    Ok(text_result(
        json!({
            "ok": true,
            "id": written.id,
            "total": written.steps.len(),
        })
        .to_string(),
    ))
}

#[derive(Deserialize)]
struct WalkthroughSetStepArgs {
    index: u32,
}

fn walkthrough_set_step(args: Value) -> DispatchResult {
    let args: WalkthroughSetStepArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("walkthrough_set_step: {e}")))?;
    let body = wt::read_body()
        .map_err(|e| DispatchError::internal(format!("walkthrough_set_step: {e}")))?
        .ok_or_else(|| {
            DispatchError::invalid_params("walkthrough_set_step: no active tour".to_string())
        })?;
    let last = u32::try_from(body.steps.len().saturating_sub(1)).unwrap_or(u32::MAX);
    let clamped = args.index.min(last);
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Walkthrough {
            id: body.id.clone(),
            step: clamped,
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({
            "ok": true,
            "id": body.id,
            "step": clamped,
            "total": body.steps.len(),
        })
        .to_string(),
    ))
}

fn walkthrough_clear_handler() -> DispatchResult {
    wt::clear().map_err(|e| DispatchError::internal(format!("walkthrough_clear: {e}")))?;
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::default(),
        ..UiState::default()
    });
    Ok(text_result(json!({"ok": true}).to_string()))
}

#[derive(Deserialize)]
struct WalkthroughFeedbackArgs {
    #[serde(default)]
    since_ts: Option<u64>,
}

fn walkthrough_feedback(args: Value) -> DispatchResult {
    let args: WalkthroughFeedbackArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("walkthrough_feedback: {e}")))?;
    let log = wt::read_feedback()
        .map_err(|e| DispatchError::internal(format!("walkthrough_feedback: {e}")))?;
    let since = args.since_ts.unwrap_or(0);
    let events: Vec<&_> = log.events.iter().filter(|e| e.ts > since).collect();
    let body = json!({
        "since_ts": since,
        "events": events,
    });
    Ok(text_result(body.to_string()))
}

#[derive(Deserialize)]
struct OpenBrowserRepoArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default = "default_open_browser")]
    open_browser: bool,
}

fn default_open_browser() -> bool {
    true
}

async fn open_browser_repo(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: OpenBrowserRepoArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("open_browser_repo: {e}")))?;
    let path = if let Some(path) = args.path {
        PathBuf::from(path)
    } else {
        let guard = state.lock().await;
        guard
            .repo
            .as_ref()
            .map(|repo| repo.root.clone())
            .or_else(|| state::read().ok().flatten().and_then(|s| s.repo_root))
            .ok_or_else(|| {
                DispatchError::invalid_params(
                    "open_browser_repo: no path given and no repository is open",
                )
            })?
    };
    if !path.is_absolute() {
        return Err(DispatchError::invalid_params(format!(
            "open_browser_repo: path must be absolute: {}",
            path.display()
        )));
    }

    let asset_dir = locate_web_dist().map_err(|e| {
        DispatchError::internal(format!(
            "open_browser_repo: could not locate frontend dist: {e}"
        ))
    })?;
    let status = browser_host::start(BrowserHostConfig {
        repo_root: Some(path),
        port: args.port.unwrap_or(0),
        asset_dir,
        open_browser: args.open_browser,
    })
    .map_err(|e| DispatchError::internal(format!("open_browser_repo: {e}")))?;
    Ok(text_result(
        serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".into()),
    ))
}

// Both helpers are infallible — they always return a `text_result(...)`.
// Clippy's `unnecessary_wraps` would flag the `-> DispatchResult` return type,
// but the dispatch table expects every tool fn to return `DispatchResult`,
// so suppress the lint locally rather than diverging from the call shape.
#[allow(clippy::unnecessary_wraps)]
fn browser_status() -> DispatchResult {
    Ok(text_result(
        serde_json::to_string_pretty(&browser_host::status()).unwrap_or_else(|_| "null".into()),
    ))
}

#[allow(clippy::unnecessary_wraps)]
fn stop_browser() -> DispatchResult {
    browser_host::stop();
    Ok(text_result(json!({"ok": true}).to_string()))
}

fn locate_web_dist() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PROJECTMIND_WEB_DIST") {
        let path = PathBuf::from(path);
        if path.join("index.html").is_file() {
            return Ok(path);
        }
    }
    let cwd = std::env::current_dir()?;
    let cwd_candidate = cwd.join("app/dist");
    if cwd_candidate.join("index.html").is_file() {
        return Ok(cwd_candidate);
    }
    let exe = std::env::current_exe()?;
    for ancestor in exe.ancestors() {
        let candidate = ancestor.join("app/dist");
        if candidate.join("index.html").is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("set PROJECTMIND_WEB_DIST or run from the ProjectMind repo root")
}

/// Best-effort statefile write. Failures are logged but never bubble up: the
/// MCP server stays usable when there's no GUI / no writable cache directory.
///
/// We also nudge the GUI awake here: if no fresh heartbeat is seen we try to
/// launch the Tauri shell so the user can actually see the LLM's intent.
/// Throttled inside `launch::ensure_gui_running` so a chain of `view_*` calls
/// doesn't double-spawn.
fn publish_state(state: UiState) {
    launch::ensure_gui_running();
    if let Err(err) = projectmind_core::state::write(state) {
        tracing::warn!(error = %err, "failed to publish UI state");
    }
}

fn text_result(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

#[cfg(test)]
fn escape_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_returns_at_least_open_repo() {
        let v = list();
        let names: Vec<&str> = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"open_repo"));
        assert!(names.contains(&"show_class"));
        assert!(names.contains(&"list_changes_since"));
    }

    #[test]
    fn escape_id_strips_special_chars() {
        assert_eq!(escape_id("com.example.Foo"), "com_example_Foo");
        assert_eq!(escape_id("UserService<T>"), "UserService_T_");
    }
}

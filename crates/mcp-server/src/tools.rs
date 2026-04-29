// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tool definitions and `tools/call` dispatch.

use std::path::PathBuf;

use plaintext_ide_core::state::{self, UiState, ViewIntent};
use plaintext_ide_core::{diagram, git};
use plaintext_ide_framework_spring::SpringPlugin;
use plaintext_ide_plugin_api::FrameworkPlugin;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::handler::{with_repo, DispatchError, DispatchResult, ServerState, ToolCallParams};

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
            "type": { "type": "string", "enum": ["bean-graph", "package-tree"] }
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

/// Tool registry — also serves as the response of `tools/list`.
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
                "description": "Tell the GUI to switch to the diagram view (`bean-graph` or `package-tree`).",
                "inputSchema": diagram_schema()
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
        "relations" => relations(state).await,
        "plugin_info" => plugin_info(state).await,
        "view_class" => view_class(state, parsed.arguments).await,
        "view_diff" => view_diff(parsed.arguments),
        "view_file" => view_file(parsed.arguments),
        "view_diagram" => view_diagram(parsed.arguments),
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
    let anchor = args.anchor.clone();
    publish_state(UiState {
        repo_root: prev.repo_root,
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
    if args.kind != "bean-graph" && args.kind != "package-tree" {
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

/// Best-effort statefile write. Failures are logged but never bubble up: the
/// MCP server stays usable when there's no GUI / no writable cache directory.
fn publish_state(state: UiState) {
    if let Err(err) = plaintext_ide_core::state::write(state) {
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

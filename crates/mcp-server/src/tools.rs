// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tool definitions and `tools/call` dispatch.

use plaintext_ide_core::{diagram, git};
use plaintext_ide_framework_spring::SpringPlugin;
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
    json!({
        "type": "object",
        "properties": {
            "fqn": { "type": "string", "description": "Fully-qualified class name" }
        },
        "required": ["fqn"]
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
                "name": "plugin_info",
                "description": "List active plugins (languages and frameworks).",
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
        "plugin_info" => plugin_info(state).await,
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

    let mut state = state.lock().await;
    let repo = state
        .engine
        .open_repo(std::path::Path::new(&args.path))
        .map_err(|e| DispatchError::internal(format!("open_repo failed: {e}")))?;

    let summary = json!({
        "root": repo.root,
        "modules": repo.modules.len(),
        "classes": repo.class_count(),
    });
    state.repo = Some(repo);
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
        let (_module_id, class) = repo.find_class(&args.fqn).ok_or_else(|| {
            DispatchError::invalid_params(format!("class not found: {}", args.fqn))
        })?;
        let abs_file = repo.absolute(&class.file);
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

async fn plugin_info(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    let body = json!({
        "languages": state.engine.language_ids(),
        "frameworks": state.engine.framework_ids(),
    });
    Ok(text_result(body.to_string()))
}

/// Wrap a string into the MCP tool-result content array.
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

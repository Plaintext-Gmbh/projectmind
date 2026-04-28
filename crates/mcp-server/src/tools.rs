// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tool definitions and `tools/call` dispatch.

use std::path::PathBuf;

use plaintext_ide_core::git;
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
    with_repo(&state, |repo| match args.kind.as_str() {
        "bean-graph" => Ok(text_result(render_bean_graph(repo))),
        "package-tree" => Ok(text_result(render_package_tree(repo))),
        other => Err(DispatchError::invalid_params(format!(
            "unknown diagram: {other}"
        ))),
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

/// Render the bean graph as a Mermaid `flowchart` for the active framework plugin.
fn render_bean_graph(repo: &plaintext_ide_core::Repository) -> String {
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use plaintext_ide_framework_spring::SpringPlugin;

    let spring = SpringPlugin::new();
    let mut out = String::from("flowchart LR\n");
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for module in repo.modules.values() {
        for rel in spring.relations(module) {
            nodes.insert(rel.from.clone());
            nodes.insert(rel.to.clone());
            let _ = writeln!(out, "    {}-->{}", escape_id(&rel.from), escape_id(&rel.to));
        }
    }
    for n in &nodes {
        let _ = writeln!(out, "    {}[\"{}\"]", escape_id(n), n);
    }
    if out == "flowchart LR\n" {
        out.push_str("    empty[(no beans detected)]\n");
    }
    out
}

fn render_package_tree(repo: &plaintext_ide_core::Repository) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    let mut tree: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module in repo.modules.values() {
        for class in module.classes.values() {
            let pkg = class
                .fqn
                .rsplit_once('.')
                .map_or(String::new(), |(p, _)| p.to_owned());
            tree.entry(pkg).or_default().push(class.name.clone());
        }
    }
    let mut out = String::from("graph TD\n");
    for (pkg, classes) in tree {
        let pkg_id = if pkg.is_empty() {
            "<default>".into()
        } else {
            pkg.clone()
        };
        let pkg_node = escape_id(&pkg_id);
        let _ = writeln!(out, "    {pkg_node}[\"{pkg_id}\"]");
        for c in classes {
            let leaf = escape_id(&format!("{pkg_id}::{c}"));
            let _ = writeln!(out, "    {leaf}[\"{c}\"]");
            let _ = writeln!(out, "    {pkg_node}-->{leaf}");
        }
    }
    out
}

fn escape_id(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
        } else {
            s.push('_');
        }
    }
    s
}

/// Re-exported for unused-warning suppression in tests.
#[allow(dead_code)]
pub(crate) fn unused_path_resolver(state: &ServerState, p: &str) -> PathBuf {
    crate::handler::resolve_path(state, p)
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

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tool definitions and `tools/call` dispatch.

use std::path::PathBuf;

use projectmind_browser_host::{self as browser_host, BrowserHostConfig};
use projectmind_core::artifact::{self, ArtifactError, ArtifactFormat};
use projectmind_core::coverage;
use projectmind_core::file_access;
use projectmind_core::files;
use projectmind_core::patterns::{self as core_patterns, Pattern, Scope as PatternScope};
use projectmind_core::risk::{self, Options as RiskOptions, Weights as RiskWeights};
use projectmind_core::state::{self, UiState, ViewIntent};
use projectmind_core::walkthrough::{self as wt, QuizQuestion, Walkthrough, WalkthroughStep};
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
            "ref":   { "type": "string", "description": "Git ref to compare from (e.g. HEAD, HEAD~5, origin/master). Also accepts the `A..B` range shorthand — in that case omit `to`." },
            "to":    { "type": "string", "description": "Optional second ref; defaults to the working tree. Leave empty when `ref` already uses `A..B` shorthand." }
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
            "type": {
                "type": "string",
                "enum": [
                    "bean-graph",
                    "package-tree",
                    "folder-map",
                    "inheritance-tree",
                    "doc-graph",
                    "c4-container",
                    "architecture-layers",
                    "architecture-flow",
                    "module-chord",
                    "activity-heatmap",
                    "language-stats"
                ]
            }
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
                "description": "What to render in the main pane. `kind` selects the viewer: class | file | diff | artifact | risk | pattern | atlas | note. The risk/pattern/atlas kinds (Cockpit 2.4) pull live signals from the Risk Atlas + Pattern Lens.",
                "properties": {
                    "kind": { "type": "string", "enum": ["class", "file", "diff", "artifact", "risk", "pattern", "atlas", "note"] },
                    "fqn":  { "type": "string", "description": "Class FQN (kind=class or kind=risk)" },
                    "path": { "type": "string", "description": "Absolute file path (kind=file)" },
                    "anchor": { "type": "string", "description": "Heading slug (kind=file, markdown only)" },
                    "id":   { "type": "string", "description": "Artifact id from present_artifact (kind=artifact)" },
                    "ref":  { "type": "string", "description": "Base git ref (kind=diff)" },
                    "to":   { "type": "string", "description": "Target git ref or omit for working tree (kind=diff)" },
                    "show": {
                        "type": "array",
                        "description": "Risk signals to surface in the header bar (kind=risk). Empty = every signal with data.",
                        "items": { "type": "string", "enum": ["churn", "cx", "cov", "fan_in", "fan_out"] }
                    },
                    "pattern": { "type": "string", "enum": ["repository", "layered", "di_only", "tx_on_service", "no_static_state"], "description": "Architecture pattern to evaluate (kind=pattern). PascalCase labels accepted too." },
                    "scope":   { "type": "string", "description": "Pattern scope (kind=pattern). `module:<id>` narrows to one module; `all` or omitted checks the whole repo." },
                    "view":    { "type": "string", "enum": ["violations"], "description": "What the pattern step renders (kind=pattern). Only `violations` is defined today." },
                    "module":  { "type": "string", "description": "Module id filter for the atlas treemap (kind=atlas). Omit for the whole repo." },
                    "highlight_fqns": {
                        "type": "array",
                        "description": "Class FQNs to ring as named hotspots on the atlas treemap (kind=atlas).",
                        "items": { "type": "string" }
                    },
                    "focus": {
                        "description": "Spotlight. kind=diff → object {file,hunk,line}: the GUI scrolls + pulses the matching hunk (tours without it render exactly like before). kind=risk → string: a member (method/field) name to scroll to inside the class.",
                        "properties": {
                            "file": { "type": "string", "description": "Repo-relative path or basename — substring match on the `+++ b/<path>` header." },
                            "hunk": { "type": "integer", "minimum": 0, "description": "0-based hunk index inside `file` (or the whole diff when `file` is omitted)." },
                            "line": { "type": "integer", "minimum": 1, "description": "1-based line number in the new file." }
                        }
                    },
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
            },
            "quiz": {
                "type": "array",
                "description": "Optional end-of-tour learning quiz. The GUI shows a quiz card after the user acks the last step. Omit for tours that don't need recall.",
                "items": quiz_question_schema()
            }
        },
        "required": ["title", "steps"]
    })
}

fn quiz_question_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "prompt":  { "type": "string", "description": "The question text." },
            "choices": {
                "type": "array",
                "description": "Possible answers in render order. 2-5 reads cleanly; tours with more get harder to scan.",
                "items": { "type": "string" },
                "minItems": 2
            },
            "answer":      { "type": "integer", "minimum": 0, "description": "0-based index into `choices` of the correct answer." },
            "step_refs":   {
                "type": "array",
                "description": "Optional 0-based step indices that explain this question. Wrong answers can offer to replay them.",
                "items": { "type": "integer", "minimum": 0 }
            },
            "explanation": { "type": "string", "description": "Optional one-line explanation shown after the user answers. Plain text — not markdown." }
        },
        "required": ["prompt", "choices", "answer"]
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

fn risk_atlas_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "module":      { "type": "string", "description": "Optional module id filter (as returned by module_summary)." },
            "top":         { "type": "integer", "minimum": 1, "default": 20, "description": "Maximum number of classes to return, sorted highest-risk first." },
            "window_days": { "type": "integer", "minimum": 1, "default": 90, "description": "Churn lookback window in days. Commits older than this are ignored." },
            "weights": {
                "type": "object",
                "description": "Optional override of the score weights. Missing fields keep the default.",
                "properties": {
                    "churn": { "type": "number" },
                    "cx":    { "type": "number" },
                    "cov":   { "type": "number" },
                    "deps":  { "type": "number" }
                }
            }
        }
    })
}

fn pattern_check_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "enum": ["repository", "layered", "di_only", "tx_on_service", "no_static_state"],
                "description": "Which architectural pattern to evaluate. Accepts the PascalCase labels too (Repository | Layered | DI | Transactional | NoStaticState)."
            },
            "module": { "type": "string", "description": "Optional module id filter. Omit to check across the whole repo (equivalent to scope `all`)." }
        },
        "required": ["pattern"]
    })
}

fn walkthrough_query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {
                "type": "string",
                "description": "Natural-language question about the code, e.g. \"how does login work\". Semantically matched against curated tour steps."
            },
            "prefer_tours": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional tour ids to bias the match toward (a small nudge, not an override)."
            },
            "top_k": {
                "type": "integer",
                "minimum": 1,
                "default": 8,
                "description": "Maximum number of steps to return for the best-matching tour, in tour order."
            }
        },
        "required": ["question"]
    })
}

fn open_browser_repo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Absolute path to the repository root. Defaults to the currently-open repo or current statefile repo." },
            "port": { "type": "integer", "minimum": 0, "maximum": 65535, "description": "Port to bind; 0 means choose a free port." },
            "open_browser": { "type": "boolean", "default": true, "description": "Open the default browser on THIS machine after starting. Set false when the user only wants the link surfaced in chat (e.g. to copy onto another device)." },
            "lan": { "type": "boolean", "default": false, "description": "Set true to bind on 0.0.0.0 so the URL contains a LAN IP and is reachable from other devices on the same WLAN (iPad / phone / another laptop). Default false binds on 127.0.0.1 — loopback only, useless off-machine. The bearer token in the URL fragment still gates every request." }
        }
    })
}

fn present_artifact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title":   { "type": "string", "description": "Human-readable title (viewer header + list caption)." },
            "format":  { "type": "string", "enum": ["html", "markdown"], "description": "How to render `content`. `html` is sandboxed in a locked-down iframe; `markdown` renders like a .md file (mermaid + images)." },
            "content": { "type": "string", "description": "The document body. HTML source or Markdown. Max ~2 MB." },
            "id":      { "type": "string", "description": "Optional stable handle. If omitted, derived from `title`. Re-using an id replaces that artifact in place — use it to iterate / stream." }
        },
        "required": ["title", "format", "content"]
    })
}

fn list_artifacts_schema() -> Value {
    no_args_schema()
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
                "name": "file_recency",
                "description": "Per-file recency index for the current repo: every path's most-recent commit (sha, summary, age in seconds). Sorted newest-first. Capped at the 5,000 most-recent files / 10,000 commits walked. Use to drive heatmaps, author overlays, and other change-map visualisations.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "list_refs",
                "description": "List local branches and tags from the open repository. Each entry has name, kind (branch|tag) and target_sha (7-char). Branches first (master/main floated to the top), then tags sorted descending.",
                "inputSchema": no_args_schema()
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
                "description": "List PDFs and images (.pdf .png .jpg .jpeg .webp .gif .svg .bmp .ico) inside a module's root. Source files (.java .rs) are excluded — those are surfaced by list_classes.",
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
                "description": "Open a class in every ProjectMind viewer that is currently running (Desktop GUI and/or browser webapp from `open_browser_repo`). Use after the user says `show me class X` / `open class X`. Pushes UI state via the shared statefile — no per-viewer routing exists. Auto-launches the Desktop GUI if no viewer is up; takes precedence over manual GUI navigation.",
                "inputSchema": fqn_schema()
            },
            {
                "name": "view_diff",
                "description": "Open the diff view between two git refs (or `ref` vs working tree) in every running ProjectMind viewer. Mirrors to Desktop GUI and/or browser webapp simultaneously — there is no per-viewer routing. Auto-launches the Desktop GUI if nothing is open.",
                "inputSchema": ref_schema()
            },
            {
                "name": "view_file",
                "description": "Open an arbitrary file in every running ProjectMind viewer (Desktop GUI and/or browser webapp). Markdown is rendered (mermaid blocks + images embedded); other extensions show as plain source. Use after `show me file X` / `open README` etc. Mirrors to all open viewers — there is no per-viewer routing. Auto-launches the Desktop GUI if nothing is open.",
                "inputSchema": file_schema()
            },
            {
                "name": "view_diagram",
                "description": "Open a diagram (`bean-graph`, `package-tree`, `folder-map`, …) in every running ProjectMind viewer. Mirrors to Desktop GUI and/or browser webapp simultaneously. Auto-launches the Desktop GUI if nothing is open.",
                "inputSchema": diagram_schema()
            },
            {
                "name": "present_artifact",
                "description": "Render an AI-generated HTML or Markdown artifact live in every open ProjectMind viewer (Desktop GUI and/or browser webapp). Unlike view_file the content is NOT read from the repo — you pass it inline, so use this to show generated dashboards, notes, tables or diagrams without writing them to the user's disk. HTML is rendered inside a sandboxed, CSP-locked iframe (no script execution, no network); Markdown renders like a .md file (mermaid + images). Pass a stable `id` and call again with the same id to replace the body (iterate / stream). Auto-launches the Desktop GUI if nothing is open. Artifacts persist across viewer restarts and are cleared when a different repo is opened.",
                "inputSchema": present_artifact_schema()
            },
            {
                "name": "list_artifacts",
                "description": "List the artifacts pushed via present_artifact: id, title, format, size, created/updated timestamps. Bodies are not included.",
                "inputSchema": list_artifacts_schema()
            },
            {
                "name": "walkthrough_start",
                "description": "Start a guided tour. Pushes the tour body + step 0 to every viewer currently open (Desktop GUI and/or browser webapp from `open_browser_repo`). Use after `give me a tour` / `walk me through ...`. For `tour in the browser` / `tour me through this on my iPad`, call `open_browser_repo` first (with `lan: true` if a remote device is involved) so the user has a URL to open before the tour starts. Replaces any previous tour. Auto-launches the Desktop GUI if nothing else is open.",
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
                "name": "risk_atlas",
                "description": "Composite per-class risk scores for the open repository. Combines four signals: git churn (commits touching the file in the last `window_days` days), cyclomatic complexity (decision-point heuristic over the class's source lines), test coverage (`1 - line%` from a JaCoCo/LCOV/Cobertura report, `cov` field, null when no report), and fan-in (`fan_in`/`fan_out`, how many classes reference this one, from the relations graph). Missing coverage degrades gracefully — its weight is rebalanced. Returns the top-N classes sorted by score descending with raw signals and a `why` hint (e.g. `hot+uncovered+central`). Use to find the hotspots before reading any source — much cheaper than grepping the repo.",
                "inputSchema": risk_atlas_schema()
            },
            {
                "name": "pattern_check",
                "description": "Evaluate an architectural pattern (drift detection) against the open repository. Five Spring-flavoured detectors: `repository` (only @Repository classes may touch EntityManager / JdbcTemplate directly — a @Service that reaches for persistence primitives is drift), `layered` (web/controller classes must not reference repositories or entities directly — v1 flags intra-module only), `di_only` (no manual `new XxxService()` inside a @Component — inject beans instead), `tx_on_service` (@Transactional belongs on @Service, not @Controller), and `no_static_state` (Spring components must not own mutable static fields). Honours `.projectmind/patterns.toml` (layer order, package matchers, disabled detectors). Returns per-module compliance counts (`holds`), a list of `violations` with file:line, message, severity and a per-violation `confidence`, plus a detector-level `confidence`. Violations below 0.6 confidence are noise and hidden from the heatmap.",
                "inputSchema": pattern_check_schema()
            },
            {
                "name": "walkthrough_query",
                "description": "Semantic lookup over curated walk-through tours — the PREFERRED way to answer \"how does X work / where is X\" before grepping. An architect builds tours once; this matches your question against every tour step (title + narration + target) and returns the best tour's steps with `title`, `fqn`/`file`/`lines` and `narration`, plus a `confidence` (0..1). A ten-step tour of ten lines each answers a question in ~500 lines vs 5000-20000 lines of reading source, so try this FIRST. When `fallback` is `\"grep\"` (no tour indexed, weak match < 0.45, or the semantic index is unavailable because the server was built without the `embed` feature) there is no good tour answer — fall back to the normal search/read tools. Optional `prefer_tours` biases toward named tour ids; `top_k` caps returned steps.",
                "inputSchema": walkthrough_query_schema()
            },
            {
                "name": "open_browser_repo",
                "description": "Start the in-process browser host that serves the ProjectMind webapp at a tokenized URL, then surface that URL to the user verbatim — they will open it themselves; you cannot. Use after `open in browser` / `im Browser zeigen` / `show me on my iPad / phone / laptop`. Pass `lan: true` whenever the user mentions a remote device (iPad, phone, second machine on the same WLAN) — otherwise the URL is `http://127.0.0.1:...` and unreachable from anything but this machine. The bearer token in the URL fragment gates every API call regardless of bind address. Idempotent: calling again with a different `path` reopens the host on the existing port; call `browser_status` first to avoid restarting the host. Once the user has opened the URL, every subsequent `view_*` / `walkthrough_*` push will mirror to that browser tab in addition to the Desktop GUI.",
                "inputSchema": open_browser_repo_schema()
            },
            {
                "name": "browser_status",
                "description": "Return the running browser host's bind address, tokenized URLs and open repo, or null if no host is running. Side-effect-free — call this before `open_browser_repo` to re-surface the existing URL/token instead of restarting the host (or to show the user the link again).",
                "inputSchema": no_args_schema()
            },
            {
                "name": "stop_browser",
                "description": "Forget the cached browser host status for this MCP process so the next `open_browser_repo` call starts fresh. The actual TCP listener exits when the MCP process exits — this does not kill it mid-session.",
                "inputSchema": no_args_schema()
            },
            {
                "name": "start_gui",
                "description": "Bring up the ProjectMind Desktop window (Tauri shell) if not already running. Most `view_*` / `walkthrough_*` tools auto-launch the GUI on demand, so call this explicitly only when the user says `open the desktop app` / `starte die Desktop-App`, or when you want the window up before the first push of a tour. Returns whether it was already running. On macOS uses `open -a ProjectMind`; on Linux execs the binary. Honours $PROJECTMIND_APP for an override. Note: this is the local desktop counterpart to `open_browser_repo` — use `open_browser_repo` instead when the user says `in browser` / `auf dem iPad`.",
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
        "file_recency" => file_recency(state).await,
        "list_refs" => list_refs(state).await,
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
        "present_artifact" => present_artifact(parsed.arguments),
        "list_artifacts" => list_artifacts(),
        "walkthrough_start" => walkthrough_start(parsed.arguments),
        "walkthrough_append" => walkthrough_append(parsed.arguments),
        "walkthrough_set_step" => walkthrough_set_step(parsed.arguments),
        "walkthrough_clear" => walkthrough_clear_handler(),
        "walkthrough_feedback" => walkthrough_feedback(parsed.arguments),
        "walkthrough_query" => walkthrough_query(state, parsed.arguments).await,
        "risk_atlas" => risk_atlas(state, parsed.arguments).await,
        "pattern_check" => pattern_check(state, parsed.arguments).await,
        "open_browser_repo" => open_browser_repo(state, parsed.arguments).await,
        "browser_status" => browser_status(),
        "stop_browser" => stop_browser(),
        "start_gui" => start_gui_handler(),
        other => Err(DispatchError::invalid_params(format!(
            "unknown tool: {other}"
        ))),
    }
}

fn start_gui_handler() -> DispatchResult {
    match launch::start_gui_explicit() {
        Ok(launch::StartGuiOutcome::AlreadyRunning) => Ok(text_result(
            "ProjectMind GUI is already running (heartbeat fresh).",
        )),
        Ok(launch::StartGuiOutcome::Launched { path }) => Ok(text_result(format!(
            "Launched ProjectMind GUI from {path}. Window may take a couple of seconds to appear."
        ))),
        Err(err) => Err(DispatchError::internal(format!(
            "could not launch ProjectMind GUI: {err}"
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

    // Opening a *different* repo drops any AI-generated artifacts from the
    // previous one; re-opening the same repo keeps them. Must run before the
    // state write below, which overwrites the previous repo root.
    if let Err(err) = artifact::clear_on_repo_change(&root) {
        tracing::warn!(error = %err, "open_repo: failed to clear artifacts on repo change");
    }

    // Build the semantic tour index (Cockpit 2.5, #161) for
    // `walkthrough_query`. Best-effort and no-op without the `embed`
    // feature; a failure (no model, unwritable cache) must never block
    // opening the repo — the query path rebuilds lazily anyway.
    if let Err(err) = projectmind_core::tour_index::build_index_on_open(&root) {
        tracing::warn!(error = %err, "open_repo: failed to build tour index");
    }

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

async fn file_recency(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let recency = git::file_recency(&repo.root)
            .map_err(|e| DispatchError::internal(format!("git: {e}")))?;
        let body = serde_json::to_string_pretty(&recency).unwrap_or_else(|_| "[]".into());
        Ok(text_result(body))
    })
}

async fn list_refs(state: &Mutex<ServerState>) -> DispatchResult {
    let state = state.lock().await;
    with_repo(&state, |repo| {
        let refs =
            git::list_refs(&repo.root).map_err(|e| DispatchError::internal(format!("git: {e}")))?;
        let body = serde_json::to_string_pretty(&refs).unwrap_or_else(|_| "[]".into());
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
        "inheritance-tree" => Ok(text_result(diagram::render_inheritance_tree(repo))),
        "c4-container" => Ok(text_result(diagram::render_c4_container(repo, &spring))),
        "architecture-layers" => Ok(text_result(diagram::render_architecture_layers_drawio(
            repo,
        ))),
        "doc-graph" => Ok(text_result(
            serde_json::to_string(&projectmind_core::doc_graph::build(&repo.root))
                .map_err(|e| DispatchError::internal(format!("doc-graph failed: {e}")))?,
        )),
        "language-stats" => Ok(text_result(
            serde_json::to_string(&projectmind_core::language_stats::build(&repo.root))
                .map_err(|e| DispatchError::internal(format!("language-stats failed: {e}")))?,
        )),
        "architecture-flow" => Ok(text_result(
            serde_json::to_string(&projectmind_core::architecture_flow::build(repo, &spring))
                .map_err(|e| DispatchError::internal(format!("architecture-flow failed: {e}")))?,
        )),
        "module-chord" => Ok(text_result(
            serde_json::to_string(&projectmind_core::module_chord::build(repo, &spring))
                .map_err(|e| DispatchError::internal(format!("module-chord failed: {e}")))?,
        )),
        "activity-heatmap" => Ok(text_result(
            serde_json::to_string(&projectmind_core::activity_heatmap::build(&repo.root))
                .map_err(|e| DispatchError::internal(format!("activity-heatmap failed: {e}")))?,
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
                "annotations": m.annotations.iter().map(|a| json!({
                    "name": a.name,
                    "raw_args": a.raw_args,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "fields": class.fields.iter().map(|f| json!({
                "name": f.name,
                "type": f.type_text,
                "visibility": f.visibility,
                "is_static": f.is_static,
                "line": f.line,
                "annotations": f.annotations.iter().map(|a| json!({
                    "name": a.name,
                    "raw_args": a.raw_args,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "super_types": class.super_types.iter().map(|t| json!({
                "name": t.name,
                "kind": t.kind,
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
        let entries = files::list_module_files(
            &module.root,
            &[
                "pdf", "png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "ico",
            ],
        );
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
        && args.kind != "inheritance-tree"
        && args.kind != "doc-graph"
        && args.kind != "c4-container"
        && args.kind != "architecture-layers"
        && args.kind != "architecture-flow"
        && args.kind != "module-chord"
        && args.kind != "activity-heatmap"
        && args.kind != "language-stats"
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

// ----- artifact tools -------------------------------------------------------

#[derive(Deserialize)]
struct PresentArtifactArgs {
    title: String,
    format: String,
    content: String,
    #[serde(default)]
    id: Option<String>,
}

fn present_artifact(args: Value) -> DispatchResult {
    let args: PresentArtifactArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("present_artifact: {e}")))?;
    let format = ArtifactFormat::parse(&args.format).ok_or_else(|| {
        DispatchError::invalid_params(format!(
            "present_artifact: unknown format `{}` (expected html | markdown)",
            args.format
        ))
    })?;
    let stored = artifact::store(args.id.as_deref(), &args.title, format, &args.content).map_err(
        |e| match e {
            ArtifactError::TooLarge { .. } => DispatchError::invalid_params(format!(
                "present_artifact: {e} — trim the artifact or split it"
            )),
            other => DispatchError::internal(format!("present_artifact: {other}")),
        },
    )?;
    let prev = state::read().ok().flatten().unwrap_or_default();
    publish_state(UiState {
        repo_root: prev.repo_root,
        view: ViewIntent::Artifact {
            id: stored.id.clone(),
        },
        ..UiState::default()
    });
    Ok(text_result(
        json!({
            "ok": true,
            "id": stored.id,
            "format": stored.format.as_str(),
            "size": stored.size,
        })
        .to_string(),
    ))
}

fn list_artifacts() -> DispatchResult {
    let metas =
        artifact::list().map_err(|e| DispatchError::internal(format!("list_artifacts: {e}")))?;
    Ok(text_result(
        serde_json::to_string_pretty(&metas).unwrap_or_else(|_| "[]".into()),
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
    #[serde(default)]
    quiz: Vec<QuizQuestion>,
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
        schema_version: wt::CURRENT_SCHEMA_VERSION,
        id: id.clone(),
        title: args.title,
        summary: args.summary,
        steps: args.steps,
        quiz: args.quiz,
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
    #[serde(default)]
    lan: bool,
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
        lan: args.lan,
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

#[derive(Deserialize, Default)]
struct RiskAtlasArgs {
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    top: Option<u32>,
    #[serde(default)]
    window_days: Option<u32>,
    #[serde(default)]
    weights: Option<RiskWeightArgs>,
}

#[derive(Deserialize, Default)]
struct RiskWeightArgs {
    #[serde(default)]
    churn: Option<f64>,
    #[serde(default)]
    cx: Option<f64>,
    #[serde(default)]
    cov: Option<f64>,
    #[serde(default)]
    deps: Option<f64>,
}

#[derive(Deserialize)]
struct WalkthroughQueryArgs {
    question: String,
    #[serde(default)]
    prefer_tours: Vec<String>,
    #[serde(default)]
    top_k: Option<u32>,
}

/// Semantic tour lookup (Cockpit 2.5, #161). Ranks the question against
/// the indexed tour steps and returns the best tour's steps. Without the
/// `embed` feature (or when the model/network is unavailable) the core
/// answers `fallback: "grep"` — this handler never errors on "no answer".
///
/// A `class`-target step carries only its `fqn` in the index; here, with
/// the open repository in hand, we enrich each such step with its
/// `file` + `lines` so the caller gets a jump target without a second
/// round-trip. Enrichment is best-effort: an unresolved fqn just keeps
/// whatever the index had.
async fn walkthrough_query(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: WalkthroughQueryArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("walkthrough_query: {e}")))?;
    let top_k = args.top_k.map_or(8, |v| v as usize).max(1);

    let state = state.lock().await;
    with_repo(&state, |repo| {
        let mut result = projectmind_core::tour_index::query_repo(
            &repo.root,
            &args.question,
            &args.prefer_tours,
            top_k,
        )
        .map_err(|e| DispatchError::internal(format!("walkthrough_query: {e}")))?;

        // Enrich class-target steps (fqn but no file/lines) from the repo.
        for step in &mut result.steps {
            if step.file.is_some() {
                continue;
            }
            if let Some(fqn) = &step.fqn {
                if let Some((module, class)) = repo.find_class(fqn) {
                    step.file = Some(module.root.join(&class.file).to_string_lossy().into_owned());
                    if step.lines.is_none() {
                        step.lines = Some([class.line_start, class.line_end]);
                    }
                }
            }
        }

        let body = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into());
        Ok(text_result(body))
    })
}

async fn risk_atlas(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: RiskAtlasArgs = if args.is_null() {
        RiskAtlasArgs::default()
    } else {
        serde_json::from_value(args)
            .map_err(|e| DispatchError::invalid_params(format!("risk_atlas: {e}")))?
    };

    let state = state.lock().await;
    let engine = &state.engine;
    with_repo(&state, |repo| {
        let mut weights = RiskWeights::default();
        if let Some(w) = args.weights {
            if let Some(v) = w.churn {
                weights.churn = v;
            }
            if let Some(v) = w.cx {
                weights.cx = v;
            }
            if let Some(v) = w.cov {
                weights.cov = v;
            }
            if let Some(v) = w.deps {
                weights.deps = v;
            }
        }
        let opts = RiskOptions {
            module: args.module,
            top: args.top.map_or(20, |v| v as usize).max(1),
            window_days: args.window_days.unwrap_or(risk::DEFAULT_CHURN_WINDOW_DAYS),
            weights,
        };
        // Fan-in/out reads the already-parsed relations graph; coverage is a
        // one-shot report scan. Both degrade to empty/None without crashing.
        let relations = engine.relations(repo);
        let coverage = coverage::load(&repo.root);
        let scores = risk::compute(repo, &relations, coverage.as_ref(), &opts)
            .map_err(|e| DispatchError::internal(format!("risk_atlas: {e}")))?;
        let cov_meta = coverage.as_ref().map_or(Value::Null, |c| {
            json!({
                "format": c.format.as_str(),
                "path": c.path,
                "age_secs": c.age_secs(),
                "stale": c.is_stale(),
            })
        });
        let body = serde_json::to_string_pretty(&json!({
            "window_days": opts.window_days,
            "weights": {
                "churn": opts.weights.churn,
                "cx": opts.weights.cx,
                "cov": opts.weights.cov,
                "deps": opts.weights.deps,
            },
            "coverage": cov_meta,
            "scores": scores,
        }))
        .unwrap_or_else(|_| "{}".into());
        Ok(text_result(body))
    })
}

#[derive(Deserialize)]
struct PatternCheckArgs {
    pattern: String,
    #[serde(default)]
    module: Option<String>,
}

async fn pattern_check(state: &Mutex<ServerState>, args: Value) -> DispatchResult {
    let args: PatternCheckArgs = serde_json::from_value(args)
        .map_err(|e| DispatchError::invalid_params(format!("pattern_check: {e}")))?;
    let pattern = Pattern::parse(&args.pattern).ok_or_else(|| {
        DispatchError::invalid_params(format!(
            "pattern_check: unknown pattern `{}` (expected repository | layered | di_only | tx_on_service | no_static_state)",
            args.pattern
        ))
    })?;
    let scope = PatternScope {
        module: args.module,
    };
    let state = state.lock().await;
    with_repo(&state, |repo| {
        // Per-repo config toggles detectors / layer rules; missing file = defaults.
        let config = core_patterns::PatternConfig::load(&repo.root);
        let result = core_patterns::check_with_config(repo, pattern, &scope, &config);
        let body = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into());
        Ok(text_result(body))
    })
}

/// Web frontend embedded into the MCP binary at compile time.
///
/// The `app/dist` directory must exist at build time — the workspace CI
/// always runs `pnpm build` before `cargo build` because the Tauri crate's
/// `tauri::generate_context!()` macro requires it as well, so this is a
/// shared invariant rather than an extra build step. Embedding solves the
/// "Linux .deb / macOS .app installs `projectmind-mcp` to a system path
/// where no sibling `app/dist` exists, so `open_browser_repo` could never
/// find the assets" problem without duplicating the assets onto disk: one
/// binary, one source-of-truth, extracted lazily on first use.
static EMBEDDED_WEB_DIST: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../app/dist");

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
    extract_embedded_web_dist()
}

/// Extract the embedded `app/dist` payload to a versioned cache directory
/// so subsequent calls (and other concurrent MCP processes on the same
/// machine) reuse the same files. The version segment in the path means a
/// `projectmind-mcp` upgrade automatically lands in a fresh directory and
/// the old one can be garbage-collected by the user without breaking the
/// running process.
fn extract_embedded_web_dist() -> anyhow::Result<PathBuf> {
    let cache_root = dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("no cache dir available for extracting web assets"))?
        .join("projectmind")
        .join(format!("web-dist-{}", env!("CARGO_PKG_VERSION")));
    if cache_root.join("index.html").is_file() {
        return Ok(cache_root);
    }
    std::fs::create_dir_all(&cache_root)?;
    EMBEDDED_WEB_DIST
        .extract(&cache_root)
        .map_err(|e| anyhow::anyhow!("failed to extract embedded web assets: {e}"))?;
    if !cache_root.join("index.html").is_file() {
        anyhow::bail!(
            "embedded web assets extracted but index.html is missing — \
             this is a packaging bug, please file an issue"
        );
    }
    Ok(cache_root)
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
        assert!(names.contains(&"risk_atlas"));
        assert!(names.contains(&"pattern_check"));
        assert!(names.contains(&"present_artifact"));
        assert!(names.contains(&"list_artifacts"));
    }

    #[test]
    fn present_artifact_schema_enumerates_formats() {
        let v = list();
        let tools = v["tools"].as_array().unwrap();
        let pa = tools
            .iter()
            .find(|t| t["name"] == "present_artifact")
            .expect("present_artifact tool registered");
        let schema = &pa["inputSchema"];
        for k in ["title", "format", "content", "id"] {
            assert!(
                schema["properties"].get(k).is_some(),
                "missing property {k}"
            );
        }
        let formats: Vec<&str> = schema["properties"]["format"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(formats.contains(&"html"));
        assert!(formats.contains(&"markdown"));
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(required.contains(&"title"));
        assert!(required.contains(&"format"));
        assert!(required.contains(&"content"));
    }

    #[test]
    fn walkthrough_step_schema_allows_artifact_target() {
        let v = list();
        let tools = v["tools"].as_array().unwrap();
        let start = tools
            .iter()
            .find(|t| t["name"] == "walkthrough_start")
            .expect("walkthrough_start tool registered");
        let kinds: Vec<&str> = start["inputSchema"]["properties"]["steps"]["items"]["properties"]
            ["target"]["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"artifact"), "got kinds: {kinds:?}");
    }

    #[test]
    fn walkthrough_step_schema_advertises_cockpit_2_4_kinds() {
        // Cockpit 2.4 (#160): risk / pattern / atlas must be offered to the LLM
        // so it can author them via walkthrough_start / _append.
        let v = list();
        let tools = v["tools"].as_array().unwrap();
        let start = tools
            .iter()
            .find(|t| t["name"] == "walkthrough_start")
            .expect("walkthrough_start tool registered");
        let target = &start["inputSchema"]["properties"]["steps"]["items"]["properties"]["target"];
        let kinds: Vec<&str> = target["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for k in ["risk", "pattern", "atlas"] {
            assert!(kinds.contains(&k), "kind `{k}` missing from {kinds:?}");
        }
        // The atlas hotspot list and the risk `show` list must be documented.
        let props = &target["properties"];
        assert!(props.get("highlight_fqns").is_some());
        assert!(props.get("show").is_some());
        assert!(props.get("pattern").is_some());
    }

    #[test]
    fn risk_atlas_schema_documents_weights() {
        let v = list();
        let tools = v["tools"].as_array().unwrap();
        let atlas = tools
            .iter()
            .find(|t| t["name"] == "risk_atlas")
            .expect("risk_atlas tool registered");
        let schema = &atlas["inputSchema"];
        assert_eq!(schema["type"], "object");
        let props = &schema["properties"];
        for k in ["module", "top", "window_days", "weights"] {
            assert!(props.get(k).is_some(), "missing property {k}");
        }
        let wprops = &props["weights"]["properties"];
        for k in ["churn", "cx", "cov", "deps"] {
            assert!(wprops.get(k).is_some(), "missing weight {k}");
        }
    }

    #[test]
    fn pattern_check_schema_enumerates_v1_patterns() {
        let v = list();
        let tools = v["tools"].as_array().unwrap();
        let pc = tools
            .iter()
            .find(|t| t["name"] == "pattern_check")
            .expect("pattern_check tool registered");
        let enum_values: Vec<&str> = pc["inputSchema"]["properties"]["pattern"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(enum_values.contains(&"repository"));
        assert!(enum_values.contains(&"layered"));
        assert!(enum_values.contains(&"di_only"));
        assert!(enum_values.contains(&"tx_on_service"));
        assert!(enum_values.contains(&"no_static_state"));
    }

    #[test]
    fn escape_id_strips_special_chars() {
        assert_eq!(escape_id("com.example.Foo"), "com_example_Foo");
        assert_eq!(escape_id("UserService<T>"), "UserService_T_");
    }

    #[test]
    fn embedded_web_dist_contains_index_html() {
        // Pins the contract that `app/dist/index.html` is built and embedded
        // into the binary. Without this the Linux .deb / macOS .app bundles
        // ship an MCP server that cannot serve the browser webapp at all.
        assert!(
            EMBEDDED_WEB_DIST.get_file("index.html").is_some(),
            "app/dist/index.html must exist at build time and be embedded"
        );
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Method dispatch and shared server state.

use projectmind_core::{Engine, Repository};
use projectmind_framework_lombok::LombokPlugin;
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;
use projectmind_lang_rust::RustPlugin;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::tools;
use crate::{JsonRpcErrorCode, PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION};

/// Server state held across requests.
pub(crate) struct ServerState {
    pub engine: Engine,
    pub repo: Option<Repository>,
}

impl ServerState {
    pub(crate) fn new() -> Self {
        let mut engine = Engine::new();
        engine.register_language(Box::new(JavaPlugin::new()));
        engine.register_language(Box::new(RustPlugin::new()));
        engine.register_framework(Box::new(SpringPlugin::new()));
        engine.register_framework(Box::new(LombokPlugin::new()));
        Self { engine, repo: None }
    }
}

/// Errors returned from method handlers.
pub(crate) struct DispatchError {
    code: JsonRpcErrorCode,
    message: String,
}

impl DispatchError {
    pub(crate) fn code(&self) -> JsonRpcErrorCode {
        self.code
    }
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
    pub(crate) fn invalid_params<S: Into<String>>(msg: S) -> Self {
        Self {
            code: JsonRpcErrorCode::InvalidParams,
            message: msg.into(),
        }
    }
    pub(crate) fn internal<S: Into<String>>(msg: S) -> Self {
        Self {
            code: JsonRpcErrorCode::InternalError,
            message: msg.into(),
        }
    }
    pub(crate) fn method_not_found(method: &str) -> Self {
        Self {
            code: JsonRpcErrorCode::MethodNotFound,
            message: format!("method not found: {method}"),
        }
    }
}

pub(crate) type DispatchResult = Result<Value, DispatchError>;

pub(crate) async fn dispatch(
    state: &Mutex<ServerState>,
    method: &str,
    params: Value,
) -> DispatchResult {
    match method {
        "initialize" => Ok(initialize_response()),
        "ping" => Ok(json!({})),
        "notifications/initialized" | "notifications/cancelled" | "notifications/exit" => {
            Ok(Value::Null)
        }
        "tools/list" => Ok(tools::list()),
        "tools/call" => tools::call(state, params).await,
        other => Err(DispatchError::method_not_found(other)),
    }
}

/// Server-level guidance surfaced via the MCP `instructions` field on
/// `initialize`. Compatible MCP clients (Claude Code, Codex, …) load this
/// once into the model context, so it is the right place for cross-tool
/// routing rules that would otherwise have to be duplicated into every tool
/// description. Keep it short and concrete — verbose guidance pushes useful
/// tokens out of the prompt.
const SERVER_INSTRUCTIONS: &str = r"ProjectMind ships two viewers that share state through a filesystem statefile:

1. Desktop GUI (Tauri shell). Auto-launches on the first `view_*` / `walkthrough_*` call. Use `start_gui` to bring the window up explicitly.
2. Browser webapp. Started on demand via `open_browser_repo`, accessed at the tokenized URL the tool returns (`http://<host>:<port>/#token=<token>`). Polls the statefile every couple of seconds.

`view_*` and `walkthrough_*` push state. EVERY viewer that is currently open mirrors the change — there is no per-viewer routing. ``Send something only to the browser'' means: make sure the user has only the browser open. Same the other way round.

User intent → tool routing:

- ``open in browser'' / ``im Browser zeigen'' / ``open it on my iPad / phone / laptop'': call `open_browser_repo`. Pass `lan: true` whenever the user mentions another device (iPad, phone, second laptop, anyone else on the WLAN) — otherwise the URL is loopback-only and useless off-machine. Surface the returned URL (including `#token=...`) verbatim to the user; you cannot open it for them.
- ``start a tour'' / ``walk me through ...'' (no qualifier): call `walkthrough_start`. The Desktop GUI auto-launches.
- ``start a tour in the browser'' / ``tour me through this on my iPad'': make sure `open_browser_repo` has been called (use `browser_status` to check first), surface the URL, then `walkthrough_start`.
- ``show me file X'' / ``open class Y'': use `view_file` / `view_class`. Mirrors to whatever viewer(s) are open.
- ``show me X locally'' / ``in the desktop app'': there is no separate desktop-only push. If the browser viewer is also open it will mirror. If the user really wants the browser quiet, tell them to close that tab — do not silently fail.

`browser_status` is a free, side-effect-less status check. Always call it before a second `open_browser_repo` to re-surface the existing URL/token instead of restarting the host.";

fn initialize_response() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION,
        },
        "instructions": SERVER_INSTRUCTIONS,
    })
}

/// Helper used by tools to require an opened repository.
pub(crate) fn with_repo<F, T>(state: &ServerState, f: F) -> Result<T, DispatchError>
where
    F: FnOnce(&Repository) -> Result<T, DispatchError>,
{
    let repo = state.repo.as_ref().ok_or_else(|| {
        DispatchError::invalid_params("no repository open — call open_repo first")
    })?;
    f(repo)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

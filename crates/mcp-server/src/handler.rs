// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Method dispatch and shared server state.

use plaintext_ide_core::{Engine, Repository};
use plaintext_ide_framework_lombok::LombokPlugin;
use plaintext_ide_framework_spring::SpringPlugin;
use plaintext_ide_lang_java::JavaPlugin;
use plaintext_ide_lang_rust::RustPlugin;
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

fn initialize_response() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION,
        }
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

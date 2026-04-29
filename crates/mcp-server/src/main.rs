// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `projectmind-mcp` — MCP server binary.
//!
//! Speaks newline-delimited JSON-RPC 2.0 on stdio, exposing the projectmind tools.
//! Logs go to stderr only (stdout is the protocol channel).

#![warn(clippy::pedantic)]

mod handler;
mod launch;
mod tools;

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::handler::ServerState;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = env!("CARGO_PKG_NAME");
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = SERVER_VERSION, "projectmind MCP server starting");

    let state = Mutex::new(ServerState::new());

    // Stdin reader on a blocking thread; stdout writer is also synchronous (single-threaded).
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                error!(?err, "stdin read error");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Ok(req) => process(&state, req).await,
            Err(err) => Some(JsonRpcResponse::error_with_id(
                Value::Null,
                JsonRpcErrorCode::ParseError,
                &format!("parse error: {err}"),
            )),
        };
        if let Some(resp) = response {
            let bytes = serde_json::to_vec(&resp).context("serialise response")?;
            stdout.write_all(&bytes)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    info!("projectmind MCP server exiting");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("PROJECTMIND_LOG")
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("default filter parses");
    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_target(false)
        .with_env_filter(filter)
        .try_init();
}

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum JsonRpcErrorCode {
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
}

impl JsonRpcResponse {
    pub(crate) fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub(crate) fn error_with_id(id: Value, code: JsonRpcErrorCode, message: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: code as i32,
                message: message.to_string(),
                data: None,
            }),
        }
    }
}

async fn process(state: &Mutex<ServerState>, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
    if req.jsonrpc != "2.0" {
        return req.id.map(|id| {
            JsonRpcResponse::error_with_id(
                id,
                JsonRpcErrorCode::InvalidRequest,
                "expected jsonrpc=2.0",
            )
        });
    }
    // Notifications (no id) get no response.
    let id = req.id.clone();
    let result = handler::dispatch(state, &req.method, req.params).await;
    match (id, result) {
        (None, _) => None,
        (Some(id), Ok(value)) => Some(JsonRpcResponse::success(id, value)),
        (Some(id), Err(err)) => Some(JsonRpcResponse::error_with_id(
            id,
            err.code(),
            err.message(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "ping");
        assert_eq!(req.id, Some(serde_json::json!(1)));
    }

    #[test]
    fn serialises_success() {
        let resp = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains(r#""jsonrpc":"2.0""#));
        assert!(s.contains(r#""result":{"ok":true}"#));
    }
}

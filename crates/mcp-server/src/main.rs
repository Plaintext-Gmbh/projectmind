// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `projectmind-mcp` — MCP server binary.
//!
//! Speaks newline-delimited JSON-RPC 2.0 on stdio, exposing the projectmind tools.
//! Logs go to stderr only (stdout is the protocol channel).

#![warn(clippy::pedantic)]

mod briefing;
mod handler;
mod launch;
mod record;
mod tools;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::handler::ServerState;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = env!("CARGO_PKG_NAME");
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `projectmind-mcp` CLI.
///
/// With no subcommand the binary runs the stdio MCP server (its original,
/// default behaviour — MCP clients spawn it with no arguments). Subcommands
/// add out-of-band utilities like `record` (Cockpit 2.6, #162).
#[derive(Debug, Parser)]
#[command(
    name = "projectmind",
    version,
    about = "ProjectMind MCP server + tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Export a walk-through tour to a self-contained file (PDF by default).
    ///
    /// `.pdf` renders each step as a structured page (title, `file:line`,
    /// code snippet, narration, risk / pattern annotations) with no `FFmpeg`
    /// dependency. `.mp4` needs the `record-mp4` cargo feature.
    Record {
        /// Tour id to export. Pass `active` (or `-`) to record whatever tour
        /// is currently live.
        tour_id: String,
        /// Output file. The extension picks the format: `.pdf` (default
        /// deliverable) or `.mp4` (needs the `record-mp4` feature).
        #[arg(short, long, default_value = "tour.pdf")]
        output: PathBuf,
        /// Repository root for source / risk / pattern resolution. Falls back
        /// to the repo recorded in the statefile when omitted.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Embed narration as an audio track (MP4 only; ignored for PDF).
        #[arg(long)]
        narrate: bool,
    },

    /// Print a morning briefing of what got worse since the last session.
    ///
    /// The CLI face of the `architect_briefing` MCP tool (Cockpit 2.7, #163):
    /// opens the repo (which appends a fresh health snapshot to
    /// `.projectmind/state/sessions.jsonl`), diffs it against the baseline
    /// chosen by `--since`, and prints new hotspots, pattern drift, and the
    /// risk delta as plain text (default), Markdown, or JSON. Built for cron
    /// jobs and Slack bots.
    Briefing {
        /// Repository root. Falls back to the repo recorded in the statefile.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Baseline to diff against: `last_session` (default), `1d` / `7d`,
        /// an ISO-8601 timestamp, or bare Unix seconds.
        #[arg(long, default_value = "last_session")]
        since: String,
        /// Output format: `text` (default), `markdown`, or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Don't append a fresh snapshot before diffing — a read-only peek at
        /// the history as it already stands.
        #[arg(long)]
        no_record: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Record {
            tour_id,
            output,
            repo,
            narrate,
        }) => {
            init_tracing();
            let args = record::RecordArgs {
                tour_id,
                output,
                repo,
                narrate,
            };
            let message = record::run(&args)?;
            println!("{message}");
            Ok(())
        }
        Some(Command::Briefing {
            repo,
            since,
            format,
            no_record,
        }) => {
            init_tracing();
            let format = briefing::Format::parse(&format).with_context(|| {
                format!("unknown --format `{format}` (expected text | markdown | json)")
            })?;
            let args = briefing::BriefingArgs {
                repo,
                since,
                format,
                no_record,
            };
            let output = briefing::run(&args)?;
            print!("{output}");
            Ok(())
        }
        None => run_server(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run_server() -> Result<()> {
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

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Best-effort GUI launcher.
//!
//! When an LLM calls a `view_*` tool the user almost always wants to *see*
//! the result. If the Tauri shell isn't running we try to start it. Failure
//! is silent-ish (a `tracing::warn!`) — the MCP tool itself still succeeds
//! and the statefile is still written, so when the user does eventually open
//! the GUI it picks up where the LLM left off.
//!
//! Resolution order for the executable:
//! 1. `$PROJECTMIND_APP` — explicit user override (path or app bundle).
//! 2. Platform defaults (see [`platform_candidates`]).
//!
//! A small in-process throttle prevents a series of `view_*` calls from
//! spawning the app over and over while it's still cold-starting.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use projectmind_core::heartbeat;

/// Maximum age of a heartbeat for the GUI to be considered alive. The shell
/// writes every ~2s, so 5s is comfortable headroom.
const HEARTBEAT_FRESH: Duration = Duration::from_secs(5);

/// Don't try to spawn the GUI more often than this — cold start can take a
/// few seconds, during which the heartbeat is still missing.
const RELAUNCH_COOLDOWN: Duration = Duration::from_secs(15);

static LAST_LAUNCH: Mutex<Option<Instant>> = Mutex::new(None);

/// Ensure the GUI shell is running, launching it if not. Idempotent and
/// throttled — safe to call before every `view_*` intent.
pub(crate) fn ensure_gui_running() {
    if heartbeat::is_alive(HEARTBEAT_FRESH) {
        return;
    }

    {
        let mut last = LAST_LAUNCH.lock().expect("LAST_LAUNCH poisoned");
        if let Some(prev) = *last {
            if prev.elapsed() < RELAUNCH_COOLDOWN {
                return;
            }
        }
        *last = Some(Instant::now());
    }

    if let Err(err) = launch() {
        tracing::warn!(error = %err, "failed to auto-launch GUI");
    } else {
        tracing::info!("auto-launched projectmind GUI");
    }
}

#[derive(Debug, thiserror::Error)]
enum LaunchError {
    #[error("no projectmind GUI binary found (set $PROJECTMIND_APP)")]
    NotFound,
    #[error("spawn failed for {path}: {source}")]
    Spawn {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

fn launch() -> Result<(), LaunchError> {
    let candidate = resolve_app().ok_or(LaunchError::NotFound)?;
    spawn_detached(&candidate).map_err(|e| LaunchError::Spawn {
        path: candidate.display().to_string(),
        source: e,
    })
}

fn resolve_app() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PROJECTMIND_APP") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    platform_candidates().into_iter().find(|cand| cand.exists())
}

#[cfg(target_os = "macos")]
fn platform_candidates() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/Applications/projectmind.app"),
        PathBuf::from("/Applications/ProjectMind.app"),
    ];
    if let Some(home) = dirs::home_dir() {
        v.push(home.join("Applications/projectmind.app"));
    }
    v
}

#[cfg(target_os = "linux")]
fn platform_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".local/bin/projectmind"));
        v.push(home.join(".local/bin/projectmind-app"));
    }
    v.push(PathBuf::from("/usr/local/bin/projectmind"));
    v.push(PathBuf::from("/usr/bin/projectmind"));
    v.push(PathBuf::from("/opt/projectmind/projectmind"));
    v
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn spawn_detached(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    // `open -a` handles .app bundles and detaches naturally.
    Command::new("open")
        .arg("-a")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn spawn_detached(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    // Direct exec; the resulting child is reparented to init when this
    // process exits, so Drop on the Child handle is fine.
    Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn spawn_detached(_path: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "auto-launch not implemented for this platform",
    ))
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Liveness signal between the Tauri shell and the MCP server.
//!
//! The shell writes `{ pid, ts }` to a small JSON file every few seconds.
//! The MCP server reads that file before publishing a `view_*` intent —
//! if the heartbeat is missing or stale, it tries to launch the shell so
//! the user actually *sees* what the LLM wanted to show.
//!
//! The file lives next to the statefile (see [`crate::state`]). Both files
//! together form the cross-process contract; nothing else is shared.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// One liveness sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    /// Process id of the writing shell.
    pub pid: u32,
    /// Unix timestamp (seconds) when the sample was written.
    pub ts: u64,
}

/// Path of the heartbeat file. Always next to the statefile so the same
/// `$PLAINTEXT_IDE_STATE` override applies to both.
#[must_use]
pub fn heartbeat_path() -> PathBuf {
    let state = crate::state::statefile_path();
    let parent = state
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf);
    parent.join("ui-heartbeat.json")
}

/// Write a fresh heartbeat with the current process's PID. Best-effort —
/// IO errors bubble up so the caller can log them, but a missing parent
/// directory is created on the fly.
pub fn write() -> std::io::Result<()> {
    let path = heartbeat_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let payload = Heartbeat {
        pid: std::process::id(),
        ts: now_secs(),
    };
    let json = serde_json::to_string(&payload)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Read the most recent heartbeat. Returns `None` if the file is missing
/// or unreadable; never errors — heartbeat presence is advisory only.
#[must_use]
pub fn read() -> Option<Heartbeat> {
    let path = heartbeat_path();
    let txt = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&txt).ok()
}

/// `true` if a heartbeat exists and is younger than `stale_after`.
#[must_use]
pub fn is_alive(stale_after: Duration) -> bool {
    let Some(hb) = read() else {
        return false;
    };
    let now = now_secs();
    now.saturating_sub(hb.ts) < stale_after.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;

    fn override_state_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("plaintext-ide-hb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("current.json");
        std::env::set_var("PLAINTEXT_IDE_STATE", &p);
        p
    }

    #[test]
    fn write_then_read_round_trips() {
        let _g = test_lock();
        let _state = override_state_path("rt");
        write().unwrap();
        let hb = read().expect("heartbeat present");
        assert_eq!(hb.pid, std::process::id());
        assert!(is_alive(Duration::from_secs(60)));
    }

    #[test]
    fn missing_file_is_not_alive() {
        let _g = test_lock();
        let _state = override_state_path("missing");
        let _ = std::fs::remove_file(heartbeat_path());
        assert!(read().is_none());
        assert!(!is_alive(Duration::from_secs(60)));
    }
}

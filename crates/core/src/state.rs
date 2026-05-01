// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared UI state between the MCP server and the Tauri shell.
//!
//! Both processes read and write the same JSON file. The MCP server writes
//! after every state-changing tool (`open_repo`, `view_class`, `view_file`,
//! `view_diff`); the Tauri shell watches the file and follows. The shell may
//! also write — when the user manually picks a repo — so the MCP server can
//! see what's currently open.
//!
//! The file is intentionally tiny and append-only-ish: the schema is one
//! struct with a tagged-union [`ViewIntent`] inside. A monotonically
//! increasing `seq` lets watchers ignore duplicate writes.
//!
//! Path: `$PROJECTMIND_STATE` if set, else
//! `$XDG_CACHE_HOME/projectmind/current.json` on Linux,
//! `~/Library/Caches/projectmind/current.json` on macOS.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The full UI state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UiState {
    /// Schema version. Bump if the layout changes incompatibly.
    pub version: u32,

    /// Absolute root of the currently-loaded repository, if any.
    pub repo_root: Option<PathBuf>,

    /// What the GUI should be showing.
    #[serde(default)]
    pub view: ViewIntent,

    /// Monotonic counter incremented on every write so watchers can detect
    /// updates even if the file mtime resolution is too coarse to disambiguate.
    pub seq: u64,
}

/// What the GUI should be displaying.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ViewIntent {
    /// The classes browser. Optionally with a class selected.
    Classes {
        /// FQN of the class to highlight + open in the source viewer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_fqn: Option<String>,
    },
    /// The diagram browser. `bean-graph` or `package-tree`.
    Diagram {
        /// Which diagram to show.
        #[serde(default = "default_diagram_kind")]
        diagram_kind: String,
    },
    /// A unified diff between two git refs.
    Diff {
        /// Base ref (e.g. `HEAD~5`, branch name).
        reference: String,
        /// Optional target ref. `None` means working tree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    /// An arbitrary file (markdown rendered with embedded mermaid + images;
    /// other extensions show as plain source).
    File {
        /// Absolute path to the file.
        path: PathBuf,
        /// Optional heading anchor (slug) to scroll to after rendering.
        /// Markdown only; ignored for plain files.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    /// A guided tour authored by an LLM. The body lives in
    /// [`crate::walkthrough::body_path`]; the GUI is told *which* step
    /// is current via this intent.
    Walkthrough {
        /// Tour handle. Should match `Walkthrough::id` in the body file.
        id: String,
        /// 0-based step pointer. Bumped by `walkthrough_set_step` and
        /// by user clicks on the step sidebar.
        step: u32,
    },
}

fn default_diagram_kind() -> String {
    "bean-graph".to_string()
}

impl Default for ViewIntent {
    fn default() -> Self {
        Self::Classes { selected_fqn: None }
    }
}

/// Where the statefile lives.
#[must_use]
pub fn statefile_path() -> PathBuf {
    if let Some(p) = std::env::var_os("PROJECTMIND_STATE") {
        return PathBuf::from(p);
    }
    let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    cache.join("projectmind").join("current.json")
}

/// Read the current state, returning `None` if the file does not exist yet.
pub fn read() -> std::io::Result<Option<UiState>> {
    read_at(&statefile_path())
}

/// Like [`read`], but for a custom path (useful in tests).
pub fn read_at(path: &Path) -> std::io::Result<Option<UiState>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write `state` atomically (write to a temp file, then rename). The watcher
/// on the other side never observes a half-written JSON document.
///
/// `seq` is bumped automatically by an internal counter — callers do not need
/// to set it themselves; the value passed in is overwritten.
pub fn write(mut state: UiState) -> std::io::Result<UiState> {
    state.version = SCHEMA_VERSION;
    state.seq = next_seq();
    write_at(&statefile_path(), &state)?;
    Ok(state)
}

/// Like [`write`] but to a custom path. Does *not* bump `seq` — callers
/// that go through this path own their own ordering.
pub fn write_at(path: &Path, state: &UiState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

const SCHEMA_VERSION: u32 = 1;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn next_seq() -> u64 {
    // Initialise from the current statefile on first use so a freshly-spawned
    // process does not undo a recent write by a sibling. After that, increment.
    let mut current = SEQ.load(Ordering::Relaxed);
    if current == 0 {
        if let Ok(Some(prev)) = read() {
            current = prev.seq;
            // Fine if this CAS fails (another thread won the race) — they'll
            // have set a non-zero value.
            let _ = SEQ.compare_exchange(0, current, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
    let previous = SEQ
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |seq| {
            Some(seq.checked_add(1).unwrap_or(1))
        })
        .unwrap_or_else(|seq| seq);
    previous.checked_add(1).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_state(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "projectmind-state-{}-{}-{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("current.json")
    }

    #[test]
    fn round_trip_classes_intent() {
        let path = tmp_state("classes");
        let s = UiState {
            version: SCHEMA_VERSION,
            repo_root: Some(PathBuf::from("/x")),
            view: ViewIntent::Classes {
                selected_fqn: Some("a.b.C".into()),
            },
            seq: 7,
        };
        write_at(&path, &s).unwrap();
        let read = read_at(&path).unwrap().unwrap();
        match read.view {
            ViewIntent::Classes { selected_fqn } => {
                assert_eq!(selected_fqn.as_deref(), Some("a.b.C"));
            }
            other => panic!("wrong intent: {other:?}"),
        }
        assert_eq!(read.repo_root, Some(PathBuf::from("/x")));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn round_trip_file_intent() {
        let path = tmp_state("file");
        let s = UiState {
            version: SCHEMA_VERSION,
            repo_root: None,
            view: ViewIntent::File {
                path: PathBuf::from("/repo/README.md"),
                anchor: None,
            },
            seq: 0,
        };
        write_at(&path, &s).unwrap();
        let read = read_at(&path).unwrap().unwrap();
        assert!(matches!(read.view, ViewIntent::File { .. }));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn read_returns_none_on_missing_file() {
        let path = tmp_state("missing").parent().unwrap().join("nope.json");
        assert!(read_at(&path).unwrap().is_none());
    }

    #[test]
    fn write_is_atomic_via_tmpfile() {
        // The temp file should not be left behind after a successful write.
        let path = tmp_state("atomic");
        let s = UiState::default();
        write_at(&path, &s).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn next_seq_wraps_instead_of_panicking_at_u64_max() {
        SEQ.store(u64::MAX, Ordering::Relaxed);
        assert_eq!(next_seq(), 1);
        SEQ.store(0, Ordering::Relaxed);
    }

    #[test]
    fn statefile_path_honours_env_override() {
        std::env::set_var("PROJECTMIND_STATE", "/tmp/projectmind-explicit.json");
        assert_eq!(
            statefile_path(),
            PathBuf::from("/tmp/projectmind-explicit.json")
        );
        std::env::remove_var("PROJECTMIND_STATE");
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AI-generated artifact storage.
//!
//! An *artifact* is a self-contained HTML or Markdown document authored by
//! an LLM and pushed straight into the viewer via the MCP tool
//! `present_artifact`. Unlike `view_file`, artifacts are not read from the
//! open repository — they exist only in ProjectMind's own cache, so the LLM
//! can render generated dashboards, notes, or diagrams without writing them
//! to the user's disk first.
//!
//! Storage mirrors the walk-through body/pointer split
//! ([`crate::walkthrough`]): the (potentially large, up to 2 MB) body lives
//! in its own file next to the statefile, and only a lightweight pointer
//! ([`crate::state::ViewIntent::Artifact`]) rides in the high-traffic
//! `current.json`. Each artifact is one JSON file under an `artifacts/`
//! directory so a re-`present_artifact` with the same id replaces exactly
//! one file (atomic `.tmp` + rename), and `list_artifacts` can enumerate
//! metadata without loading every body twice.
//!
//! ```text
//! $cache/projectmind/
//!   current.json            # existing — UiState; pointer is in here
//!   walkthrough.json        # existing — tour body
//!   artifacts/
//!     <id>.json             # ← one artifact body each (this module)
//! ```

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Hard cap on an artifact's content size in bytes (~2 MB). Larger payloads
/// are rejected so a runaway generation can't fill the cache or choke the
/// viewers, which fetch the whole body over HTTP / IPC.
pub const MAX_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;

/// Render mode of an artifact. Decides which viewer path renders the body:
/// `Html` goes through a sandboxed iframe, `Markdown` through the markdown
/// pipeline (mermaid + images) shared with the file viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactFormat {
    /// Raw HTML, always rendered inside a sandboxed, CSP-locked iframe.
    Html,
    /// Markdown, rendered like a `.md` file in the viewer.
    Markdown,
}

impl ArtifactFormat {
    /// Parse the tool-facing string (`"html"` / `"markdown"`).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "html" => Some(Self::Html),
            "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    /// The tool-facing string form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "markdown",
        }
    }
}

/// A full artifact body — the thing a viewer fetches and renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Stable, filesystem-safe handle. Derived from the title when the caller
    /// doesn't pass one; re-using it replaces the artifact in place.
    pub id: String,
    /// Human-readable title (viewer header + list caption).
    pub title: String,
    /// Render mode.
    pub format: ArtifactFormat,
    /// The document body (HTML source or markdown).
    pub content: String,
    /// Byte length of `content`. Persisted so `list_artifacts` reports size
    /// without re-measuring, and mirrors the `size` field on other listings.
    pub size: u64,
    /// Unix-seconds timestamp when the id was first created. Preserved across
    /// replacements so the viewer list keeps a stable creation order.
    pub created_at: u64,
    /// Unix-seconds timestamp of the most recent write.
    pub updated_at: u64,
}

/// Lightweight metadata for `list_artifacts` — everything but the body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    /// Stable handle.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Render mode.
    pub format: ArtifactFormat,
    /// Content byte length.
    pub size: u64,
    /// Creation timestamp (Unix-seconds).
    pub created_at: u64,
    /// Last-write timestamp (Unix-seconds).
    pub updated_at: u64,
}

impl From<&Artifact> for ArtifactMeta {
    fn from(a: &Artifact) -> Self {
        Self {
            id: a.id.clone(),
            title: a.title.clone(),
            format: a.format,
            size: a.size,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// Errors from [`store`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArtifactError {
    /// The content exceeded [`MAX_ARTIFACT_BYTES`].
    #[error("artifact content too large ({actual} bytes; limit {limit} bytes)")]
    TooLarge {
        /// Actual content size in bytes.
        actual: usize,
        /// Configured limit in bytes.
        limit: usize,
    },
    /// An IO failure while reading or writing the artifact file.
    #[error("artifact io: {0}")]
    Io(#[from] std::io::Error),
}

// ----- Paths ---------------------------------------------------------------

/// The `artifacts/` directory, always a sibling of the statefile.
#[must_use]
pub fn artifacts_dir() -> PathBuf {
    let state = crate::state::statefile_path();
    let parent = state
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf);
    parent.join("artifacts")
}

/// On-disk path of a single artifact body.
#[must_use]
pub fn artifact_path(id: &str) -> PathBuf {
    artifacts_dir().join(format!("{id}.json"))
}

// ----- IO ------------------------------------------------------------------

/// Read one artifact by id, or `None` if it doesn't exist.
pub fn read(id: &str) -> std::io::Result<Option<Artifact>> {
    let path = artifact_path(id);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// List metadata for every stored artifact, oldest-created first (ties broken
/// by id). Returns an empty list when the directory doesn't exist yet.
pub fn list() -> std::io::Result<Vec<ArtifactMeta>> {
    let dir = artifacts_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Tolerate a stray / half-written file: skip rather than fail the
        // whole listing.
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(a) = serde_json::from_str::<Artifact>(&s) {
                out.push(ArtifactMeta::from(&a));
            }
        }
    }
    out.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(out)
}

/// Create or replace an artifact. When `id` is `None` (or blank) it is
/// derived from `title`. Re-using an existing id overwrites the body while
/// preserving its original `created_at`. Rejects bodies over
/// [`MAX_ARTIFACT_BYTES`].
pub fn store(
    id: Option<&str>,
    title: &str,
    format: ArtifactFormat,
    content: &str,
) -> Result<Artifact, ArtifactError> {
    let size = content.len();
    if size > MAX_ARTIFACT_BYTES {
        return Err(ArtifactError::TooLarge {
            actual: size,
            limit: MAX_ARTIFACT_BYTES,
        });
    }
    let id = id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| slugify(title), slugify);
    // Preserve the original creation time on replacement so the list order is
    // stable across the LLM's iterative re-renders.
    let created_at = match read(&id) {
        Ok(Some(existing)) => existing.created_at,
        _ => now_secs(),
    };
    let artifact = Artifact {
        id: id.clone(),
        title: title.to_string(),
        format,
        content: content.to_string(),
        size: size as u64,
        created_at,
        updated_at: now_secs(),
    };
    write_atomic(&artifact_path(&id), &artifact)?;
    Ok(artifact)
}

/// Remove every stored artifact. No-op when the directory is already absent.
pub fn clear_all() -> std::io::Result<()> {
    match std::fs::remove_dir_all(artifacts_dir()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Clear all artifacts when `new_root` differs from the repo currently
/// recorded in the statefile. Every `open_repo` path calls this so switching
/// repositories starts with a clean artifact slate, while re-opening the same
/// repo (or issuing view intents within it) leaves generated artifacts alone.
///
/// Reads the *previous* statefile, so callers must invoke it before writing
/// the new repo root into the state.
pub fn clear_on_repo_change(new_root: &Path) -> std::io::Result<()> {
    let same = crate::state::read()
        .ok()
        .flatten()
        .and_then(|s| s.repo_root)
        .is_some_and(|prev| prev == new_root);
    if same {
        Ok(())
    } else {
        clear_all()
    }
}

fn write_atomic(path: &Path, value: &Artifact) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- ID generation -------------------------------------------------------

/// Make a stable, filesystem-safe slug from arbitrary text. Unlike
/// [`crate::walkthrough::slugify_id`] this is deterministic (no timestamp
/// suffix) so the same title always maps to the same artifact id, which is
/// what makes a repeat `present_artifact` replace rather than duplicate.
/// Falls back to `"artifact"` when the input has no usable characters.
#[must_use]
pub fn slugify(input: &str) -> String {
    let slug = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "artifact".to_string()
    } else {
        slug[..slug.len().min(80)].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;

    fn override_state(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("projectmind-art-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("current.json");
        std::env::set_var("PROJECTMIND_STATE", &p);
        p
    }

    #[test]
    fn store_and_read_round_trip() {
        let _g = test_lock();
        let _ = override_state("round");
        let _ = clear_all();
        let a = store(None, "My Report", ArtifactFormat::Html, "<h1>hi</h1>").unwrap();
        assert_eq!(a.id, "my-report");
        assert_eq!(a.format, ArtifactFormat::Html);
        assert!(a.created_at > 0);
        assert_eq!(a.size, "<h1>hi</h1>".len() as u64);

        let read = read("my-report").unwrap().expect("artifact present");
        assert_eq!(read.content, "<h1>hi</h1>");
        assert_eq!(read.title, "My Report");
    }

    #[test]
    fn store_replaces_and_preserves_created_at() {
        let _g = test_lock();
        let _ = override_state("replace");
        let _ = clear_all();
        let first = store(Some("iter"), "First", ArtifactFormat::Markdown, "# one").unwrap();
        // Force a later timestamp for the second write.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = store(Some("iter"), "Second", ArtifactFormat::Markdown, "# two").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.created_at, first.created_at, "created_at preserved");
        assert!(second.updated_at >= first.updated_at);
        // Only one file on disk — replacement, not append.
        let all = list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "Second");
    }

    #[test]
    fn store_rejects_oversized_content() {
        let _g = test_lock();
        let _ = override_state("toobig");
        let _ = clear_all();
        let big = "a".repeat(MAX_ARTIFACT_BYTES + 1);
        let err = store(None, "Big", ArtifactFormat::Markdown, &big).unwrap_err();
        assert!(matches!(err, ArtifactError::TooLarge { .. }));
        // Nothing persisted.
        assert!(list().unwrap().is_empty());
    }

    #[test]
    fn store_accepts_content_at_the_limit() {
        let _g = test_lock();
        let _ = override_state("atlimit");
        let _ = clear_all();
        let at = "a".repeat(MAX_ARTIFACT_BYTES);
        let a = store(Some("edge"), "Edge", ArtifactFormat::Markdown, &at).unwrap();
        assert_eq!(a.size, MAX_ARTIFACT_BYTES as u64);
    }

    #[test]
    fn list_is_sorted_and_metadata_only() {
        let _g = test_lock();
        let _ = override_state("list");
        let _ = clear_all();
        store(Some("a"), "Alpha", ArtifactFormat::Html, "<p>a</p>").unwrap();
        store(Some("b"), "Beta", ArtifactFormat::Markdown, "b").unwrap();
        let all = list().unwrap();
        assert_eq!(all.len(), 2);
        // Same-second creation ties break by id, so `a` precedes `b`.
        assert_eq!(all[0].id, "a");
        assert_eq!(all[1].id, "b");
        assert_eq!(all[1].format, ArtifactFormat::Markdown);
    }

    #[test]
    fn clear_all_removes_everything() {
        let _g = test_lock();
        let _ = override_state("clear");
        let _ = clear_all();
        store(Some("x"), "X", ArtifactFormat::Html, "<p>x</p>").unwrap();
        assert_eq!(list().unwrap().len(), 1);
        clear_all().unwrap();
        assert!(list().unwrap().is_empty());
        assert!(read("x").unwrap().is_none());
    }

    #[test]
    fn clear_on_repo_change_keeps_same_repo() {
        let _g = test_lock();
        let _ = override_state("samerepo");
        let _ = clear_all();
        // Record a repo in the statefile.
        crate::state::write(crate::state::UiState {
            repo_root: Some(PathBuf::from("/repo/a")),
            ..crate::state::UiState::default()
        })
        .unwrap();
        store(Some("keep"), "Keep", ArtifactFormat::Html, "<p>k</p>").unwrap();
        // Re-opening the SAME repo must not clear.
        clear_on_repo_change(Path::new("/repo/a")).unwrap();
        assert_eq!(list().unwrap().len(), 1, "same repo keeps artifacts");
        // A DIFFERENT repo clears.
        clear_on_repo_change(Path::new("/repo/b")).unwrap();
        assert!(
            list().unwrap().is_empty(),
            "different repo clears artifacts"
        );
    }

    #[test]
    fn slugify_is_stable_and_sanitises() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("a/b/../c"), "a-b-c");
        assert_eq!(slugify("Hello, World!"), slugify("Hello, World!"));
        assert_eq!(slugify("!!! ???"), "artifact");
    }

    #[test]
    fn format_parse_round_trip() {
        assert_eq!(ArtifactFormat::parse("html"), Some(ArtifactFormat::Html));
        assert_eq!(
            ArtifactFormat::parse("markdown"),
            Some(ArtifactFormat::Markdown)
        );
        assert_eq!(ArtifactFormat::parse("pdf"), None);
        assert_eq!(ArtifactFormat::Html.as_str(), "html");
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSON-backed annotation persistence.
//!
//! Per Issue #59 the **default** backend for user annotations is a single
//! JSON file at `.projectmind/annotations.json` inside the repo root —
//! human-readable, diffable, easy to commit when a team agrees to share
//! markers. SQLite / SurrealDB backends can land later behind the same
//! [`AnnotationStore`] trait without touching consumers.
//!
//! The file shape is intentionally tight:
//!
//! ```jsonc
//! {
//!   "version": 1,
//!   "next_id": 5,
//!   "records": [
//!     {
//!       "id": 1,
//!       "file": "src/foo.rs",
//!       "line_from": 10,
//!       "line_to": 15,
//!       "label": "CONF-1234",
//!       "link": "https://confluence.example.com/CONF-1234",
//!       "extras": {}
//!     }
//!   ]
//! }
//! ```
//!
//! Writes are atomic (write to `.tmp`, then rename) so a crash in the
//! middle of a save can't corrupt the file. The whole document is
//! re-serialised on every mutation — fine while the volume cap from
//! the design doc holds (a few thousand entries per repo at most).
//!
//! Before every successful overwrite the previous file is rotated to
//! `annotations.json.bak`. If `annotations.json` is later found missing
//! or unparseable, [`JsonAnnotationStore::open`] silently recovers from
//! the `.bak` so a torn write or accidental edit can't lose user data.

use std::path::{Path, PathBuf};

use projectmind_plugin_api::storage::{AnnotationRecord, AnnotationStore};
use projectmind_plugin_api::Result as ApiResult;
use serde::{Deserialize, Serialize};

const FILE_VERSION: u32 = 1;
const STORE_DIR: &str = ".projectmind";
const STORE_FILE: &str = "annotations.json";
const BACKUP_SUFFIX: &str = "bak";

/// Disk shape of `.projectmind/annotations.json`. Unknown fields are kept
/// (`#[serde(default)]` on optional ones) so a newer client can land
/// extras without older builds clobbering them on the next save.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AnnotationsFile {
    /// Schema version. Bumped when the on-disk shape breaks compat.
    #[serde(default = "default_version")]
    version: u32,
    /// Monotonically increasing id allocator. We never reuse ids, even
    /// after removal — keeping references in commit messages / external
    /// systems stable across the lifetime of the file.
    #[serde(default)]
    next_id: u64,
    /// Records in insertion order. Removal preserves order of survivors.
    #[serde(default)]
    records: Vec<AnnotationRecord>,
}

fn default_version() -> u32 {
    FILE_VERSION
}

/// JSON-file annotation store. Loads once on `open`, mutates an in-memory
/// copy, and re-saves the whole document on every mutation.
#[derive(Debug)]
pub struct JsonAnnotationStore {
    path: PathBuf,
    inner: AnnotationsFile,
}

impl JsonAnnotationStore {
    /// Path the store will read from / write to for `repo_root`.
    /// Exposed so callers (or tests) can predict the location without
    /// duplicating the constants.
    #[must_use]
    pub fn store_path(repo_root: &Path) -> PathBuf {
        repo_root.join(STORE_DIR).join(STORE_FILE)
    }

    /// Path of the rotating backup written on every successful save.
    fn backup_path(&self) -> PathBuf {
        self.path.with_extension(format!("json.{BACKUP_SUFFIX}"))
    }

    /// Open (or create) the store for `repo_root`. A missing file is
    /// equivalent to an empty store — we don't materialise it until the
    /// first write, so opening a read-only repo doesn't litter it with
    /// an empty `.projectmind/` directory.
    ///
    /// If the main file is missing or unparseable, the rotating
    /// `.bak` from the previous successful save is tried as a fallback
    /// (annotations are user-created data; losing them silently to a
    /// torn write would be a data-loss bug). If both are missing we
    /// start empty; if both exist but the main one is corrupt the
    /// recovered records are reused, but a fresh save is deferred until
    /// the next mutation so a read-only browse stays read-only.
    pub fn open(repo_root: &Path) -> std::io::Result<Self> {
        let path = Self::store_path(repo_root);
        let backup = path.with_extension(format!("json.{BACKUP_SUFFIX}"));
        let inner = match read_annotations(&path) {
            Ok(Some(file)) => file,
            Ok(None) => match read_annotations(&backup)? {
                Some(file) => file,
                None => empty_file(),
            },
            Err(_) => match read_annotations(&backup)? {
                Some(file) => file,
                None => {
                    // Main file is corrupt and there's no usable backup —
                    // surface the original error rather than silently
                    // wiping the user's data.
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "annotations.json is malformed and no usable .bak exists",
                    ));
                }
            },
        };
        Ok(Self { path, inner })
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.inner)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        // Rotate the previous good copy to `.bak` before the rename so a
        // crash between the rotate and the rename still leaves the user
        // with their last save in `.bak`. `rename` overwrites the target
        // on the platforms we support, so we don't have to clear it.
        let backup = self.backup_path();
        if self.path.exists() {
            std::fs::rename(&self.path, &backup)?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn empty_file() -> AnnotationsFile {
    AnnotationsFile {
        version: FILE_VERSION,
        next_id: 0,
        records: Vec::new(),
    }
}

/// Read and parse an annotations file. Returns `Ok(None)` only for a
/// missing file; a present-but-malformed file is returned as `Err` so
/// the caller can decide whether to fall back to `.bak`.
fn read_annotations(path: &Path) -> std::io::Result<Option<AnnotationsFile>> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let parsed: AnnotationsFile = serde_json::from_str(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl AnnotationStore for JsonAnnotationStore {
    fn list(&self, file: &str) -> ApiResult<Vec<AnnotationRecord>> {
        Ok(self
            .inner
            .records
            .iter()
            .filter(|r| r.file == file)
            .cloned()
            .collect())
    }

    fn all(&self) -> ApiResult<Vec<AnnotationRecord>> {
        Ok(self.inner.records.clone())
    }

    fn add(&mut self, mut ann: AnnotationRecord) -> ApiResult<u64> {
        // Always allocate fresh — caller-supplied ids are ignored. Avoids
        // a class of bugs where a stale UI hands back the id of a record
        // that's been removed in the meantime.
        self.inner.next_id += 1;
        let id = self.inner.next_id;
        ann.id = id;
        self.inner.records.push(ann);
        self.save()
            .map_err(|e| projectmind_plugin_api::Error::Plugin(e.to_string()))?;
        Ok(id)
    }

    fn remove(&mut self, id: u64) -> ApiResult<()> {
        let before = self.inner.records.len();
        self.inner.records.retain(|r| r.id != id);
        if self.inner.records.len() == before {
            // Idempotent: removing a non-existent id is silently OK.
            // Keeps the API forgiving when the GUI sends a stale delete
            // for a record that was already removed by another session.
            return Ok(());
        }
        self.save()
            .map_err(|e| projectmind_plugin_api::Error::Plugin(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-ann-test-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn record(file: &str, line_from: u32, line_to: u32, label: &str) -> AnnotationRecord {
        AnnotationRecord {
            id: 0, // will be overwritten by `add`
            file: file.to_string(),
            line_from,
            line_to,
            label: label.to_string(),
            link: None,
            extras: serde_json::Map::default(),
        }
    }

    #[test]
    fn missing_file_yields_empty_store() {
        let tmp = TempDir::new();
        let store = JsonAnnotationStore::open(tmp.path()).unwrap();
        assert!(store.all().unwrap().is_empty());
        // We don't create the file just by opening — a read-only
        // browse must not litter the repo.
        assert!(!JsonAnnotationStore::store_path(tmp.path()).exists());
    }

    #[test]
    fn add_assigns_monotonic_ids_and_persists() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let id1 = store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        let id2 = store.add(record("b.rs", 5, 6, "beta")).unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        // Reopen — survives across instances.
        drop(store);
        let store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].label, "alpha");
        assert_eq!(all[1].label, "beta");
    }

    #[test]
    fn list_filters_by_file() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        store.add(record("a.rs", 4, 4, "alpha-2")).unwrap();
        store.add(record("b.rs", 1, 1, "beta")).unwrap();
        let only_a = store.list("a.rs").unwrap();
        assert_eq!(only_a.len(), 2);
        assert!(only_a.iter().all(|r| r.file == "a.rs"));
    }

    #[test]
    fn remove_drops_the_record_and_keeps_others() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let id1 = store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        let id2 = store.add(record("a.rs", 4, 4, "beta")).unwrap();
        store.remove(id1).unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id2);
    }

    #[test]
    fn remove_unknown_id_is_a_noop() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        // Empty store — removal of any id should succeed silently.
        store.remove(999).unwrap();
        assert!(store.all().unwrap().is_empty());
    }

    #[test]
    fn ids_never_recycle_after_removal() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let id1 = store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        store.remove(id1).unwrap();
        let id2 = store.add(record("a.rs", 1, 1, "alpha-again")).unwrap();
        assert_eq!(
            id2,
            id1 + 1,
            "ids must monotonically grow even across deletions"
        );
    }

    #[test]
    fn write_is_atomic_via_tmpfile() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        let path = JsonAnnotationStore::store_path(tmp.path());
        let tmp_path = path.with_extension("json.tmp");
        // After a successful write, the tmp file must be renamed away.
        assert!(path.exists(), "real file should exist");
        assert!(!tmp_path.exists(), "tmp file should be cleaned up");
    }

    #[test]
    fn malformed_file_without_bak_is_rejected_loudly() {
        let tmp = TempDir::new();
        let path = JsonAnnotationStore::store_path(tmp.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not valid json").unwrap();
        let err = JsonAnnotationStore::open(tmp.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_rotates_previous_main_into_bak() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let path = JsonAnnotationStore::store_path(tmp.path());
        let bak = path.with_extension("json.bak");

        // First save: nothing to rotate yet.
        store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        assert!(path.exists(), "main file written");
        assert!(!bak.exists(), "no previous file → no .bak yet");

        // Second save: previous main rotates into .bak.
        store.add(record("a.rs", 2, 2, "beta")).unwrap();
        assert!(path.exists());
        assert!(bak.exists(), ".bak should hold the previous good copy");
        let prior: AnnotationsFile =
            serde_json::from_str(&std::fs::read_to_string(&bak).unwrap()).unwrap();
        assert_eq!(prior.records.len(), 1, ".bak holds the pre-save state");
        assert_eq!(prior.records[0].label, "alpha");
    }

    #[test]
    fn corrupt_main_recovers_from_bak() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        store.add(record("a.rs", 2, 2, "beta")).unwrap();
        drop(store);

        // Simulate a torn write: the main file is garbage, but .bak still
        // holds the last successful save (i.e. the state before "beta").
        let path = JsonAnnotationStore::store_path(tmp.path());
        std::fs::write(&path, "garbage").unwrap();

        let store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1, "recovered the .bak content");
        assert_eq!(all[0].label, "alpha");
    }

    #[test]
    fn missing_main_with_bak_uses_bak() {
        let tmp = TempDir::new();
        let mut store = JsonAnnotationStore::open(tmp.path()).unwrap();
        store.add(record("a.rs", 1, 1, "alpha")).unwrap();
        store.add(record("a.rs", 2, 2, "beta")).unwrap();
        drop(store);

        // Simulate the user (or some unrelated tool) deleting the main
        // file while the .bak survives. Re-opening must recover, not
        // silently start empty.
        let path = JsonAnnotationStore::store_path(tmp.path());
        std::fs::remove_file(&path).unwrap();

        let store = JsonAnnotationStore::open(tmp.path()).unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].label, "alpha");
    }
}

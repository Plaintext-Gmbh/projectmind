// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! SQLite-backed [`CodeGraphStore`] (Issue #114).
//!
//! Durable code-graph cache at `~/.cache/projectmind/<repo-hash>.db`
//! (see `docs/persistence.md`). The cache is **disposable by design** —
//! it is rebuildable from sources, so `rm -rf ~/.cache/projectmind/` is
//! a supported recovery action and no backup/restore path exists.
//!
//! # Durability settings
//!
//! The connection runs in WAL mode with `synchronous = NORMAL`:
//!
//! - **WAL** gives atomic commits and lets a future reader (GUI) look
//!   at the cache while an indexer writes.
//! - **`synchronous = NORMAL`** syncs the WAL on checkpoint, not on
//!   every commit. A power loss can drop the last few commits but can
//!   never corrupt the database — the right trade-off for a cache
//!   whose content can always be re-parsed from the repo. User data
//!   (annotations) lives elsewhere and keeps full durability.
//!
//! # Schema versioning
//!
//! `PRAGMA user_version` tracks the schema version. On open, the store
//! applies every migration from the on-disk version up to
//! [`SCHEMA_VERSION`], each inside one transaction (DDL + version bump
//! commit atomically). A database from a **newer** client is rejected
//! loudly instead of guessing — deleting the cache file is always safe.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use projectmind_plugin_api::storage::{CodeGraphStore, EdgeKind, GraphNode, GraphQuery, NodeId};
use projectmind_plugin_api::{Error as ApiError, Result as ApiResult};
use rusqlite::{params, Connection};

use crate::code_graph::node_file;

/// Current schema version, written to `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

/// Migration scripts, index = starting version. `MIGRATIONS[v]` brings
/// a database from version `v` to `v + 1`. Append-only: released
/// migrations are never edited, new ones are pushed at the end and
/// [`SCHEMA_VERSION`] is bumped alongside.
const MIGRATIONS: &[&str] = &[
    // v0 → v1: initial schema.
    "CREATE TABLE nodes (
        id         INTEGER PRIMARY KEY,
        kind       TEXT NOT NULL,
        label      TEXT NOT NULL,
        properties TEXT NOT NULL DEFAULT '{}'
    );
    CREATE INDEX nodes_kind_idx ON nodes(kind);

    CREATE TABLE edges (
        from_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        to_id   INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        kind    TEXT NOT NULL,
        PRIMARY KEY (from_id, to_id, kind)
    ) WITHOUT ROWID;
    CREATE INDEX edges_to_idx ON edges(to_id);

    CREATE TABLE node_files (
        node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
        file    TEXT NOT NULL,
        PRIMARY KEY (node_id, file)
    ) WITHOUT ROWID;
    CREATE INDEX node_files_file_idx ON node_files(file);",
];

/// SQLite-backed code-graph store. One store = one connection = one
/// database file (or an in-memory database for tests).
///
/// The connection sits behind a `Mutex` because [`CodeGraphStore`]
/// requires `Sync` while `rusqlite::Connection` is only `Send` (its
/// statement cache is a `RefCell`). Contention is a non-issue: hosts
/// hold the whole store behind their own lock anyway.
pub struct SqliteCodeGraphStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SqliteCodeGraphStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteCodeGraphStore")
            .field("path", &self.conn().path())
            .finish()
    }
}

impl SqliteCodeGraphStore {
    /// Open (or create) the cache database at `path`, creating parent
    /// directories as needed, and migrate it to [`SCHEMA_VERSION`].
    pub fn open(path: &Path) -> ApiResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(db_err)?;
        Self::from_connection(conn)
    }

    /// Open a private in-memory database (tests, throwaway sessions).
    pub fn open_in_memory() -> ApiResult<Self> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> ApiResult<Self> {
        // WAL + NORMAL: see module docs. `journal_mode` returns the mode
        // actually in effect — in-memory databases stay on "memory",
        // which is fine (same atomicity guarantees, no file).
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db_err)?;
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")
            .map_err(db_err)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Lock the connection. Poisoning is ignored deliberately: SQLite
    /// transactions keep the database itself consistent even if a
    /// panic unwound mid-operation, and the cache is disposable.
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Default cache-file location for `repo_root`:
    /// `~/.cache/projectmind/<name>-<hash>.db` (per `docs/persistence.md`).
    ///
    /// The file name combines the repo directory name (human-friendly
    /// when listing the cache dir) with a stable 64-bit FNV-1a hash of
    /// the full path (distinguishes two checkouts with the same name).
    #[must_use]
    pub fn default_cache_path(repo_root: &Path) -> PathBuf {
        let name = repo_root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo");
        let hash = fnv1a64(repo_root.to_string_lossy().as_bytes());
        let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        cache
            .join("projectmind")
            .join(format!("{name}-{hash:016x}.db"))
    }

    /// Schema version of the open database (diagnostics / tests).
    pub fn schema_version(&self) -> ApiResult<i64> {
        user_version(&self.conn()).map_err(db_err)
    }

    /// Number of edges currently stored (diagnostics / tests).
    pub fn edge_count(&self) -> ApiResult<u64> {
        let count: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
            .map_err(db_err)?;
        Ok(count as u64)
    }
}

/// Map a rusqlite error into the plugin-api error space.
fn db_err(e: rusqlite::Error) -> ApiError {
    ApiError::Plugin(format!("sqlite code-graph store: {e}"))
}

/// Convert a [`NodeId`] into a SQLite integer. Ids handed out by this
/// store come from SQLite rowids, so they always fit; a value beyond
/// `i64::MAX` can only reach us from a foreign source and is rejected
/// instead of wrapped.
fn id_param(id: NodeId) -> ApiResult<i64> {
    i64::try_from(id)
        .map_err(|_| ApiError::Plugin(format!("node id {id} exceeds the sqlite id range")))
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
}

fn migrate(conn: &Connection) -> ApiResult<()> {
    let mut version = user_version(conn).map_err(db_err)?;
    if version > SCHEMA_VERSION {
        return Err(ApiError::Plugin(format!(
            "code-graph cache has schema version {version}, this build supports at most \
             {SCHEMA_VERSION} — written by a newer ProjectMind? Deleting the cache file \
             is safe; it will be rebuilt."
        )));
    }
    while version < SCHEMA_VERSION {
        let step = MIGRATIONS
            .get(usize::try_from(version).unwrap_or(usize::MAX))
            .ok_or_else(|| {
                ApiError::Plugin(format!(
                    "no migration registered for schema version {version}"
                ))
            })?;
        let next = version + 1;
        // DDL and version bump commit atomically: a crash mid-migration
        // rolls back and the next open retries from the same version.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;\n{step}\nPRAGMA user_version = {next};\nCOMMIT;"
        ))
        .map_err(db_err)?;
        version = next;
    }
    Ok(())
}

impl CodeGraphStore for SqliteCodeGraphStore {
    fn upsert_node(&mut self, node: GraphNode) -> ApiResult<NodeId> {
        let properties = serde_json::to_string(&node.properties)
            .map_err(|e| ApiError::Plugin(format!("serializing node properties: {e}")))?;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        let id = if node.id == 0 {
            tx.execute(
                "INSERT INTO nodes (kind, label, properties) VALUES (?1, ?2, ?3)",
                params![node.kind, node.label, properties],
            )
            .map_err(db_err)?;
            tx.last_insert_rowid() as NodeId
        } else {
            tx.execute(
                "INSERT INTO nodes (id, kind, label, properties) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                     kind = excluded.kind,
                     label = excluded.label,
                     properties = excluded.properties",
                params![id_param(node.id)?, node.kind, node.label, properties],
            )
            .map_err(db_err)?;
            node.id
        };
        // Refresh the file-attribution index (used by `invalidate`).
        tx.execute("DELETE FROM node_files WHERE node_id = ?1", [id_param(id)?])
            .map_err(db_err)?;
        if let Some(file) = node_file(&node) {
            tx.execute(
                "INSERT INTO node_files (node_id, file) VALUES (?1, ?2)",
                params![id_param(id)?, file],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(id)
    }

    fn upsert_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> ApiResult<()> {
        // `foreign_keys = ON` turns an edge to a missing node into a
        // constraint violation — same contract as the memory backend.
        self.conn()
            .execute(
                "INSERT OR IGNORE INTO edges (from_id, to_id, kind) VALUES (?1, ?2, ?3)",
                params![id_param(from)?, id_param(to)?, kind.as_str()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    fn query(&self, q: &GraphQuery) -> ApiResult<Vec<GraphNode>> {
        let mut sql = String::from("SELECT id, kind, label, properties FROM nodes WHERE 1 = 1");
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(kind) = &q.kind {
            sql.push_str(" AND kind = ?");
            args.push(Box::new(kind.clone()));
        }
        if let Some(needle) = &q.label_contains {
            // `instr(lower(), lower())` instead of LIKE: no wildcard
            // escaping worries, and it matches the memory backend's
            // case-insensitive contains semantics.
            sql.push_str(" AND instr(lower(label), lower(?)) > 0");
            args.push(Box::new(needle.clone()));
        }
        sql.push_str(" ORDER BY id");
        if let Some(limit) = q.limit {
            sql.push_str(" LIMIT ?");
            args.push(Box::new(i64::from(limit)));
        }

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), row_to_node)
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    fn invalidate(&mut self, files: &[&Path]) -> ApiResult<()> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(db_err)?;
        {
            // ON DELETE CASCADE clears matching `edges` and `node_files`
            // rows along with each node.
            let mut stmt = tx
                .prepare(
                    "DELETE FROM nodes WHERE id IN
                         (SELECT node_id FROM node_files WHERE file = ?1)",
                )
                .map_err(db_err)?;
            for file in files {
                stmt.execute([file.to_string_lossy()]).map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }
}

fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphNode> {
    let id: i64 = row.get(0)?;
    let properties: String = row.get(3)?;
    let properties = serde_json::from_str(&properties).unwrap_or_default();
    Ok(GraphNode {
        id: id as NodeId,
        kind: row.get(1)?,
        label: row.get(2)?,
        properties,
    })
}

/// Stable 64-bit FNV-1a. Implemented inline (8 lines) instead of pulling
/// a hashing dependency; `DefaultHasher` is explicitly *not* guaranteed
/// stable across Rust releases, and the cache path must not move on a
/// toolchain bump.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_graph::conformance;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-cg-test-{}-{}", std::process::id(), n));
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

    #[test]
    fn sqlite_store_passes_conformance_suite_in_memory() {
        conformance::run_all(&mut || Box::new(SqliteCodeGraphStore::open_in_memory().unwrap()));
    }

    #[test]
    fn sqlite_store_passes_conformance_suite_on_disk() {
        let tmp = TempDir::new();
        let mut n = 0;
        conformance::run_all(&mut || {
            n += 1;
            Box::new(SqliteCodeGraphStore::open(&tmp.path().join(format!("{n}.db"))).unwrap())
        });
    }

    #[test]
    fn graph_survives_reopen_across_sessions() {
        let tmp = TempDir::new();
        let db = tmp.path().join("cache.db");

        let (alpha, beta);
        {
            let mut store = SqliteCodeGraphStore::open(&db).unwrap();
            alpha = store
                .upsert_node(conformance::node("class", "Alpha", Some("src/alpha.rs")))
                .unwrap();
            beta = store
                .upsert_node(conformance::node("class", "Beta", Some("src/beta.rs")))
                .unwrap();
            store.upsert_edge(alpha, beta, EdgeKind::Uses).unwrap();
        } // drop = session end

        let mut store = SqliteCodeGraphStore::open(&db).unwrap();
        let all = store.query(&GraphQuery::default()).unwrap();
        assert_eq!(all.len(), 2, "nodes survive a reopen");
        assert_eq!(all[0].id, alpha);
        assert_eq!(all[0].label, "Alpha");
        assert_eq!(store.edge_count().unwrap(), 1, "edges survive a reopen");

        // Invalidation still works against reloaded data.
        store.invalidate(&[Path::new("src/alpha.rs")]).unwrap();
        let survivors = store.query(&GraphQuery::default()).unwrap();
        assert_eq!(survivors.len(), 1);
        assert_eq!(survivors[0].label, "Beta");
        assert_eq!(store.edge_count().unwrap(), 0);
    }

    #[test]
    fn fresh_db_is_migrated_to_current_version() {
        let store = SqliteCodeGraphStore::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopen_does_not_rerun_migrations() {
        let tmp = TempDir::new();
        let db = tmp.path().join("cache.db");
        drop(SqliteCodeGraphStore::open(&db).unwrap());
        // Second open must be a no-op migration-wise (re-running v1 DDL
        // would fail on the already-existing tables).
        let store = SqliteCodeGraphStore::open(&db).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn db_from_a_newer_client_is_rejected_loudly() {
        let tmp = TempDir::new();
        let db = tmp.path().join("cache.db");
        drop(SqliteCodeGraphStore::open(&db).unwrap());
        // Simulate a cache written by a future build.
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        drop(conn);

        let err = SqliteCodeGraphStore::open(&db).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("schema version 99"), "got: {msg}");
        assert!(
            msg.contains("Deleting the cache file is safe"),
            "recovery hint expected, got: {msg}"
        );
    }

    #[test]
    fn wal_mode_is_active_on_file_databases() {
        let tmp = TempDir::new();
        let store = SqliteCodeGraphStore::open(&tmp.path().join("cache.db")).unwrap();
        let mode: String = store
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn default_cache_path_is_stable_and_distinguishes_repos() {
        let a1 = SqliteCodeGraphStore::default_cache_path(Path::new("/work/alpha"));
        let a2 = SqliteCodeGraphStore::default_cache_path(Path::new("/work/alpha"));
        let b = SqliteCodeGraphStore::default_cache_path(Path::new("/other/alpha"));
        assert_eq!(a1, a2, "same repo → same cache file");
        assert_ne!(a1, b, "same dir name, different path → different file");
        assert!(a1.to_string_lossy().contains("projectmind"));
        assert!(a1
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("alpha-"));
    }

    #[test]
    fn properties_roundtrip_as_json() {
        let mut store = SqliteCodeGraphStore::open_in_memory().unwrap();
        let mut node = conformance::node("class", "Alpha", Some("src/alpha.rs"));
        node.properties
            .insert("stereotype".into(), serde_json::Value::from("service"));
        let id = store.upsert_node(node).unwrap();

        let got = &store.query(&GraphQuery::default()).unwrap()[0];
        assert_eq!(got.id, id);
        assert_eq!(
            got.properties.get("stereotype"),
            Some(&serde_json::Value::from("service"))
        );
        assert_eq!(
            got.properties.get("file"),
            Some(&serde_json::Value::from("src/alpha.rs"))
        );
    }
}

# Persistence

> **Status:** decisions ratified in [#59](https://github.com/Plaintext-Gmbh/projectmind/issues/59). This page records the *what* and *why* so future contributors don't have to dig through the issue history.

ProjectMind persists three classes of data, each with very different value:

| Class | Backend | Location | Lifetime | User data? |
|---|---|---|---|---|
| **Annotations** | JSON file | `<repo>/.projectmind/annotations.json` | per repo, kept across sessions | **yes** — never reset without a clear user action |
| **Code-graph cache** | (none today) future: SQLite | `~/.cache/projectmind/<repo-hash>.db` | rebuildable from sources | no — disposable |
| **Session state** | JSON file | platform cache dir, see `crates/core/src/state.rs` | per user | low value — ok to reset |

The trait surface for the first two lives in `crates/plugin-api/src/storage.rs` (`AnnotationStore`, `CodeGraphStore`). New backends (SurrealDB, Mempalace, …) plug in behind these traits without touching consumers.

## Annotations — the only user data

Annotations are user-created markers on file/line ranges, optionally with a label and external link. They are the *only* persistence class we treat as valuable; everything else is metadata for navigation.

Storage rules:

- **Format:** JSON, human-readable, diffable, commit-friendly so a team can share markers when they agree to.
- **Atomic writes:** the whole document is re-serialised on every mutation, written to `annotations.json.tmp`, then renamed onto the live file. A crash mid-write can never produce a half-written file.
- **`.bak` rotation:** before each rename, the previous good copy is rotated to `annotations.json.bak`. On open, if the main file is missing or unparseable, the `.bak` is used to recover. Both gone → start empty; main corrupt and no `.bak` → loud error, never silent data loss.
- **Stable ids:** the `next_id` allocator is monotonic. Removed ids are never recycled, so external references (commit messages, Confluence pages, …) stay meaningful for the lifetime of the file.
- **Forward compatibility:** unknown fields are kept on the round-trip; a newer client can land extras without older builds clobbering them.

Implementation: [`projectmind_core::annotations::JsonAnnotationStore`](../crates/core/src/annotations.rs).

Explicit export/import is **not** offered yet — the file *is* the export. Once annotations grow beyond simple markers (e.g. richer link types, attachments), revisit.

## Code-graph cache — disposable

Today the engine parses the repo on every `open_repo`. That's fine for the current footprint (a few thousand classes per repo, sub-second walks) and means there is no cache to invalidate, no migration to ship, no recovery to write.

When the parse cost grows (large monorepos, async indexing), the SQLite-backed `CodeGraphStore` lands at `~/.cache/projectmind/<repo-hash>.db`. By definition the file is rebuildable — no backup/restore UI is ever needed, and `rm -rf ~/.cache/projectmind/` is a supported recovery action.

## Session state — low value

The session state file (current repo, view intent, monotonic seq) is shared between the MCP server and the Tauri shell. Resetting it loses convenience, never user data. See [`crates/core/src/state.rs`](../crates/core/src/state.rs) for the schema and the platform-specific path resolution.

## Reset policy

| Action | Effect |
|---|---|
| Delete `~/.cache/projectmind/` | session state and (future) code-graph cache regenerate. Safe. |
| Delete `<repo>/.projectmind/` | annotations are lost. **Treat as user-destructive.** Recovery: restore from VCS or backup. |
| Edit `annotations.json` by hand | supported. Atomic write next save → previous edit rotates into `.bak`. |

## What's *not* in this doc

- SurrealDB / Mempalace integrations — when concrete need surfaces, follow-up issues will ratify config schema and wiring.
- `.projectmind/config.toml` for backend selection — only useful once there is more than one backend implementation. Tracked separately.

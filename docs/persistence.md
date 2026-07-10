# Persistence

> **Status:** decisions ratified in [#59](https://github.com/Plaintext-Gmbh/projectmind/issues/59). This page records the *what* and *why* so future contributors don't have to dig through the issue history.

ProjectMind persists three classes of data, each with very different value:

| Class | Backend | Location | Lifetime | User data? |
|---|---|---|---|---|
| **Annotations** | JSON file | `<repo>/.projectmind/annotations.json` | per repo, kept across sessions | **yes** — never reset without a clear user action |
| **Code-graph cache** | off by default; opt-in SQLite or in-memory via config | `~/.cache/projectmind/<name>-<hash>.db` (path override possible) | rebuildable from sources | no — disposable |
| **Session state** | JSON file | platform cache dir, see `crates/core/src/state.rs` | per user | low value — ok to reset |

The trait surface for the first two lives in `crates/plugin-api/src/storage.rs` (`AnnotationStore`, `CodeGraphStore`). New backends (SurrealDB, Mempalace, …) plug in behind these traits without touching consumers. Which backend a repo uses is a config decision (see [Backend selection](#backend-selection--projectmindconfigtoml)), resolved by `projectmind_core::persistence::resolve_stores` when a repo is opened — both the Tauri shell and the browser host construct their stores through that resolver, never through concrete types.

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

The engine still parses the repo on every `open_repo` — that's fine for the current footprint (a few thousand classes per repo, sub-second walks) and stays the zero-config default. Since [#114](https://github.com/Plaintext-Gmbh/projectmind/issues/114) two `CodeGraphStore` backends exist for repos that opt in:

- **`sqlite`** ([`projectmind_core::code_graph_sqlite::SqliteCodeGraphStore`](../crates/core/src/code_graph_sqlite.rs)): a durable cache at `~/.cache/projectmind/<name>-<hash>.db` (or a `path` override from the config). WAL journal + `synchronous = NORMAL` — atomic commits, but the last few commits may be lost on power failure, which is the right trade-off for a cache that can always be re-parsed from sources. Schema version rides in `PRAGMA user_version`; migrations apply on open, each atomically (DDL + version bump in one transaction). A database written by a *newer* client is rejected loudly instead of guessed at.
- **`memory`** ([`projectmind_core::code_graph::MemoryCodeGraphStore`](../crates/core/src/code_graph.rs)): same trait, no I/O, gone at process exit. Doubles as the reference implementation — both backends run the same conformance test suite so their behavior can't drift.

Nodes are tied to source files through the `"file"` property (repo-relative path); `invalidate(files)` drops every node and edge that came from a changed file.

By definition the cache file is rebuildable — no backup/restore UI is ever needed, and `rm -rf ~/.cache/projectmind/` is a supported recovery action.

Note: nothing *feeds* the cache yet. The engine grows cache-aware parsing (write-on-parse, warm-start reads, async indexing) in a follow-up; #114/#115/#116 deliver the storage, the config surface, and the wiring so that step is purely an engine change.

## Backend selection — `.projectmind/config.toml`

A repo can pin its persistence backends in `<repo>/.projectmind/config.toml` ([#115](https://github.com/Plaintext-Gmbh/projectmind/issues/115)):

```toml
[persistence.annotations]
backend = "json"       # default — the only annotation backend today

[persistence.code_graph]
backend = "sqlite"     # "none" (default) | "memory" | "sqlite"
path = "cache/graph.db"  # optional, sqlite only; relative = repo-relative
```

Discovery order: `<repo>/.projectmind/config.toml`, then `$XDG_CONFIG_HOME/projectmind/defaults.toml` (machine-wide default for repos without their own file), then built-in defaults. Files are not merged — the first one found wins whole.

Error policy ([#116](https://github.com/Plaintext-Gmbh/projectmind/issues/116)):

| Situation | Behavior |
|---|---|
| No config file | Built-in defaults — JSON annotations, no code-graph cache. Zero-config behavior is unchanged. |
| Malformed TOML | Loud warning in the log, then defaults. A typo never bricks opening a repo. |
| Unknown keys | Warned and ignored (a newer client may have written them). |
| Unknown backend name | Actionable error naming the offender and the supported values — an explicit ask we can't honor is not silently ignored. |
| Cache file can't be created | Warning, repo opens without a cache — the cache is disposable, never load-bearing. |

Implementation: [`projectmind_core::persistence`](../crates/core/src/persistence.rs).

## Session state — low value

The session state file (current repo, view intent, monotonic seq) is shared between the MCP server and the Tauri shell. Resetting it loses convenience, never user data. See [`crates/core/src/state.rs`](../crates/core/src/state.rs) for the schema and the platform-specific path resolution.

## Reset policy

| Action | Effect |
|---|---|
| Delete `~/.cache/projectmind/` | session state and code-graph cache regenerate. Safe. |
| Delete `<repo>/.projectmind/` | annotations (and the repo's `config.toml`) are lost. **Treat as user-destructive.** Recovery: restore from VCS or backup. |
| Edit `annotations.json` by hand | supported. Atomic write next save → previous edit rotates into `.bak`. |
| Edit `config.toml` by hand | supported — it's the intended interface. Re-read on the next repo open. |

## What's *not* in this doc

- SurrealDB / Mempalace integrations — when concrete need surfaces, follow-up issues will ratify additional `backend` values and wiring; the config schema above is where they slot in.

# Persistence Design

> **Status:** 2026-04-28 — design proposal. To be revisited once the MVP is in use.

## What Needs Persisting?

Three classes of data with different access patterns and lifetimes:

| Data | Volume | Access | Lifetime |
|---|---|---|---|
| **Annotations** — user-set markers on file/line ranges, with optional label and external link (Confluence, Jira, etc.) | Small (< 10 K entries per repo) | Read-heavy, occasional write | Per repo, kept across sessions |
| **Code graph cache** — parsed entities (classes, methods) and their relations (calls, inheritance, beans, …) | Medium-to-large (10 K–1 M nodes for a 400-module monorepo) | Read-heavy, rebuilt on file change | Cache; can always be regenerated |
| **User session state** — last opened repo, panel layout, filters, recent selections | Small | Read at startup, write on change | Per user |

Different storage choices may suit each class. The configuration model below lets the user pick a backend per data class.

## Backend Options

### A. Mempalace (user's existing tool)

`plaintext-mempalace` already provides a knowledge graph with `kg_add`, `kg_query`, `kg_invalidate`, plus the room/wing/drawer model.

- **Pro:** zero new infrastructure if the user is already running it; annotations could live alongside other personal knowledge; rich KG queries; cross-project memory.
- **Con:** ties `projectmind` to a Mempalace instance; not ideal for sharing a repo with colleagues without one; the code graph cache might bloat the KG.
- **Best for:** annotations + selected high-level relations the user wants to remember long-term.

### B. SurrealDB (embedded)

[SurrealDB](https://surrealdb.com) is a Rust-native multi-model database that runs as an embedded engine, in-browser via WASM, or as a server cluster. Supports document, graph, and KV models with a single query language.

- **Pro:** Rust-native, single binary, no external dependency; fits Tauri workflow; can scale up to a server later; modern, well-funded, active.
- **Con:** larger binary footprint than a pure KV store; query language is its own dialect.
- **Best for:** code graph cache + cross-cutting queries ("which beans use this DTO?").

### C. CozoDB

Datalog-based embedded graph DB, Rust-native.

- **Pro:** elegant query model, predictable performance.
- **Con:** development pace has slowed in 2025–2026; smaller community; learning curve for Datalog.
- **Best for:** users who already love Datalog; otherwise SurrealDB is the safer choice.

### D. Grafeo

Newer embeddable graph DB in Rust (2026).

- **Pro:** native Rust, designed for embedding.
- **Con:** very new — production readiness unclear at the time of writing.
- **Best for:** experimentation, not Phase 1.

### E. SQLite + recursive CTEs / DuckPGQ

SQLite for the cache, with property-graph queries via [DuckPGQ](https://duckdb.org/) (DuckDB's graph extension) when needed.

- **Pro:** ubiquitous, rock-solid, tiny footprint; SQLite can model the cache easily; DuckPGQ adds property-graph queries when needed.
- **Con:** two engines if we use both; SQLite alone needs hand-written graph traversals.
- **Best for:** annotations (a single `annotations` table) and a "lite" variant for the graph cache.

### F. JSON / TOML files

Plain files in `.projectmind/` per repo.

- **Pro:** zero dependencies, human-readable, diffable in git.
- **Con:** doesn't scale beyond a few thousand annotations; no transactional guarantees.
- **Best for:** zero-config default for small repos; portability.

### Discarded

- **KuzuDB** — was a strong candidate, but the GitHub repository was archived in October 2025 and the maintainers stopped active support. Not safe for new projects.
- **Neo4j embedded** — Java VM dependency; too heavy for a Tauri app.

## Recommendation

A two-axis configuration:

```
Annotations:    [mempalace | surrealdb | sqlite | json] (default: json)
Code-graph:     [surrealdb | sqlite | mempalace]        (default: sqlite)
Session-state:  [json]                                   (default: json, no choice)
```

### Default for the MVP

- **Annotations → JSON** in `.projectmind/annotations.json` per repo. Zero-config, human-readable, diffable.
- **Code-graph cache → SQLite** in `~/.cache/projectmind/<repo-hash>.db`. Fast, no server, can be rebuilt freely.
- **Session state → JSON** in `~/.config/projectmind/session.json`.

Mempalace and SurrealDB integrations land later behind the same trait, swappable via config.

## Configuration Model

Per-repo config: `.projectmind/config.toml` (committed if the team agrees on a backend):

```toml
[storage.annotations]
backend = "json"              # one of: json | sqlite | mempalace | surrealdb

[storage.code_graph]
backend = "sqlite"            # one of: sqlite | surrealdb | mempalace
location = "cache"            # cache (XDG cache dir) | repo (.projectmind/) | custom

[storage.code_graph.surrealdb]
# only used if backend = "surrealdb"
mode = "embedded"             # embedded | remote
path  = ".projectmind/graph.db"
# url = "ws://localhost:8000"
# ns  = "plaintext"
# db  = "ide"

[storage.annotations.mempalace]
# only used if backend = "mempalace"
endpoint = "http://localhost:8081"
wing     = "code"
room     = "annotations"
```

Global user config: `~/.config/projectmind/config.toml` overrides defaults but is overridden by per-repo config.

## Plugin API for Storage

```rust
trait AnnotationStore: Send + Sync {
    fn list(&self, file: &str) -> Result<Vec<Annotation>>;
    fn add(&mut self, ann: Annotation) -> Result<AnnotationId>;
    fn remove(&mut self, id: AnnotationId) -> Result<()>;
}

trait CodeGraphStore: Send + Sync {
    fn upsert_node(&mut self, node: GraphNode) -> Result<NodeId>;
    fn upsert_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) -> Result<()>;
    fn query(&self, q: &GraphQuery) -> Result<Vec<GraphNode>>;
    fn invalidate(&mut self, files: &[Path]) -> Result<()>;
}
```

Each backend implements these traits. The `core` crate selects the implementation at startup based on config.

## Sources

- [SurrealDB — Embedded Rust](https://surrealdb.com/docs/surrealdb/embedding/rust)
- [KuzuDB archival news (Oct 2025)](https://biggo.com/news/202510130126_KuzuDB-embedded-graph-database-archived)
- [New Rust Databases 2026](https://libs.tech/rust/databases)
- [Cozo on Lobsters](https://lobste.rs/s/gcepzn/cozo_new_graph_db_with_datalog_embedded)
- [Grafeo on dev.to](https://dev.to/alanwest/grafeo-an-embeddable-graph-database-in-rust-that-actually-makes-sense-1nik)

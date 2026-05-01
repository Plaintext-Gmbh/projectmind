# Architecture Review: ProjectMind

Date: 2026-05-02  
Branch: `feature/architecture-review`

## Executive Summary

ProjectMind already has a sound high-level split:

- `crates/plugin-api` defines the public extension surface.
- `crates/core` owns repository discovery, parsing, diagrams, git helpers, files, HTML scanning, state and walkthrough primitives.
- `crates/mcp-server` exposes the product to MCP clients over JSON-RPC.
- `app/src-tauri` exposes the same product to the Svelte desktop UI.
- `plugins/*` contain statically linked language/framework implementations.
- `app/src` is a focused Svelte UI layer.

The current architecture is workable for the Phase 1 MVP, but it is starting to show pressure at the application boundary. MCP and Tauri both know too much about plugin wiring, DTO shaping, command semantics and some security decisions. If Phase 2 adds dynamic plugins, annotation round-trips and more UI/MCP workflows without a shared application layer, the project will likely accumulate duplicated behavior and inconsistent contracts.

## Current Shape

```text
Svelte UI
  -> Tauri commands in app/src-tauri
      -> projectmind-core
          -> plugin-api traits
          -> statically linked plugins

MCP client
  -> projectmind-mcp JSON-RPC tools
      -> projectmind-core
          -> plugin-api traits
          -> statically linked plugins

MCP <-> GUI coordination
  -> cache/state JSON file
  -> walkthrough body/feedback files
  -> heartbeat file
```

This is a reasonable MVP architecture because the expensive logic is in Rust and the UI remains comparatively thin. The concern is that the two product entrypoints are not actually thin in the same way: they independently assemble plugin sets and independently translate core state into user-facing responses.

## Strengths

1. Clear workspace-level ownership.
   The crates and plugins are easy to discover from `Cargo.toml`, and the repository layout matches the README architecture table.

2. The plugin API is small and understandable.
   `LanguagePlugin`, `FrameworkPlugin` and `VisualizerPlugin` are cohesive traits. The static metadata type also gives a clean base for diagnostics and future loading.

3. The core engine is intentionally read-only.
   Repository loading uses `canonicalize`, ignore-aware walking and in-memory `Repository` output. That matches the product promise.

4. The MCP/GUI sync mechanism is pragmatic.
   A small JSON state file plus sequence number is much simpler than prematurely introducing a local daemon or database.

5. Tests exist at multiple useful layers.
   There are parser tests, core tests and MCP protocol tests. That is a good foundation for refactoring without losing behavior.

## Findings And Risks

### 1. Plugin registration is duplicated across product entrypoints

Evidence:

- `crates/mcp-server/src/handler.rs` registers Java, Rust, Spring and Lombok in `ServerState::new`.
- `app/src-tauri/src/lib.rs` repeats the same registration in `AppState::new`.

Risk:

Adding or reordering a plugin requires edits in multiple crates. This becomes fragile when dynamic loading arrives because MCP and GUI may end up with different plugin sets.

Recommendation:

Move Phase 1 plugin assembly behind one shared constructor, for example:

```rust
// crates/core/src/default_engine.rs or a new app/service crate
pub fn default_engine() -> Engine {
    let mut engine = Engine::new();
    engine.register_language(Box::new(JavaPlugin::new()));
    engine.register_language(Box::new(RustPlugin::new()));
    engine.register_framework(Box::new(SpringPlugin::new()));
    engine.register_framework(Box::new(LombokPlugin::new()));
    engine
}
```

Because `core` currently should not depend on `plugins/*`, the cleaner variant is a new crate such as `crates/runtime` or `crates/app-core` that depends on `core` and the bundled plugins. MCP and Tauri would depend on that crate.

Priority: High.

### 2. MCP and Tauri duplicate application use-cases and DTO shaping

Evidence:

- `list_classes`, `show_class`, `module_summary`, `show_diagram`, `list_changes_since` and `show_diff` exist independently in MCP tools and Tauri commands.
- MCP responses are mostly `serde_json::Value` converted to text, while Tauri responses are typed structs.

Risk:

The same product operation can diverge across clients. For example, the UI `ClassEntry` includes `module`; MCP `list_classes` currently omits it. The UI supports module filtering; MCP only supports stereotype filtering. These differences may be deliberate, but there is no shared boundary that makes them explicit.

Recommendation:

Introduce an application service layer with typed request/response models:

```text
crates/app-core
  - ProjectMindService
  - OpenRepoRequest / RepoSummary
  - ClassQuery / ClassEntry / ClassDetails
  - ModuleSummary
  - DiagramKind / DiagramOutput
  - DiffRequest
  - FileReadPolicy
```

Then:

- Tauri commands call service methods and return typed DTOs.
- MCP tools call the same service methods and only adapt output into MCP `content`.
- Shared DTOs get serialization tests.

Priority: High.

### 3. `Engine::open_repo` combines discovery, walking, parsing and enrichment

Evidence:

- `Engine::open_repo` chooses Maven vs Cargo vs root mode.
- `parse_root`, `parse_cargo_crates` and `parse_maven_modules` each build language indexes, walk files, attribute modules, parse files and enrich modules.

Risk:

The engine is still understandable, but it has three parallel parsing paths with repeated mechanics. Future layouts such as Gradle, npm workspaces, mixed repositories or user-configured roots will push more branching into one type.

Recommendation:

Split repository opening into a small pipeline:

```text
RepoDiscovery
  -> Vec<DiscoveredModule>
SourceIndex
  -> Vec<SourceFile { path, module_id, language_id }>
ParserPipeline
  -> Repository
FrameworkPipeline
  -> Repository
```

Keep `Engine` as the orchestrator, but move layout-specific discovery and source indexing into focused components. This also makes it easier to test "mixed repo" behavior without invoking all parsers.

Priority: Medium-high.

### 4. Module detection is exclusive, but the roadmap points toward polyglot repos

Evidence:

The engine currently gives Maven priority over Cargo and treats the two as mutually exclusive. The comment says mixed Maven/Cargo monorepos are rare and Phase 1 stays simple.

Risk:

ProjectMind's target users are likely to point it at real monorepos. A Java backend plus Rust tooling, JS frontend or generated code is not unusual. Exclusive discovery means some files are invisible to the architecture model once a higher-priority layout is detected.

Recommendation:

Move toward "deepest containing manifest wins" across all known module detectors:

- Run all module detectors.
- Normalize discovered modules into a shared `DiscoveredModule`.
- Attribute files to the deepest module whose root contains the file.
- Fall back to a synthetic root module only for unattributed files.

Priority: Medium.

### 5. The file-read boundary is too broad for a read-only architecture browser

Evidence:

`read_file_text` accepts any absolute path and reads up to 10 MB. The MCP `view_file` command also accepts any absolute path and publishes it to the GUI state.

Risk:

The product is read-only, but not necessarily repo-scoped. A compromised or confused MCP client can ask the GUI to display arbitrary UTF-8 files outside the opened repository. Even if this is local-only, it weakens the stated security posture.

Recommendation:

Centralize file access in a repo-scoped service:

- Resolve and canonicalize paths before reading.
- Require `path.starts_with(repo.root)` unless an explicit future capability allows external docs.
- Return a typed `AccessDenied` error.
- Apply the same rule for Tauri `read_file_text`, MCP `view_file`, markdown browsing and HTML browsing.

Priority: High.

### 6. Shared state file is pragmatic but needs ownership semantics before multi-client use

Evidence:

The shared state file stores `repo_root`, `view` and `seq`. Both MCP and GUI write to it. The `seq` value is process-local with initialization from the existing file.

Risk:

Two clients can overwrite each other's intent, and there is no writer identity, timestamp or compare-and-swap style conflict check. This is acceptable for a single MCP client plus one GUI, but weak for multiple agents, multiple windows or long-running guided tours.

Recommendation:

Extend the schema while it is still version 1-compatible or before version 2:

- `writer`: `mcp`, `gui`, or a process/session id.
- `updated_at_ms`.
- `repo_epoch` or `session_id` so stale view intents cannot apply to a newly opened repository.
- Optional `intent_id` for walkthrough/view commands.

Priority: Medium.

### 7. Framework relations are computed ad hoc instead of owned by the parsed repository

Evidence:

Spring relations are recomputed by constructing a `SpringPlugin` in diagram, MCP and Tauri paths. The `Repository` itself stores modules/classes but not the graph edges produced by framework plugins.

Risk:

As more frameworks arrive, relation computation may scatter across diagrams, tools and UI commands. Cross-module graph behavior will also be harder to keep consistent.

Recommendation:

Promote relations to a first-class output of parsing/enrichment:

```rust
pub struct Repository {
    pub root: PathBuf,
    pub modules: BTreeMap<String, Module>,
    pub relations: Vec<Relation>,
}
```

Framework plugins can still compute module-local relations, but the engine should aggregate them once after enrichment. Diagrams and MCP tools should read the same graph.

Priority: Medium.

### 8. Documentation has drifted from the current implementation

Evidence:

`docs/plan/03-architecture.md` still describes `app/frontend`, Cytoscape/Monaco, a smaller plugin list and an initial MCP tool set. README is much closer to reality.

Risk:

Architecture docs will mislead contributors during Phase 2, especially around plugin loading and UI responsibilities.

Recommendation:

Keep `docs/plan/*` as historical design notes, but add a current architecture document:

```text
docs/architecture/current.md
docs/architecture/runtime-boundaries.md
docs/architecture/plugin-loading.md
```

The current document should explicitly mark planned vs implemented architecture.

Priority: Medium.

## Proposed Target Architecture

```text
app/src
  Svelte UI only
  calls typed Tauri commands

app/src-tauri
  thin command adapter
  owns desktop-only concerns: dialogs, events, watcher, heartbeat
  calls app-core service

crates/mcp-server
  thin MCP adapter
  owns JSON-RPC and MCP content wrapping
  calls app-core service

crates/app-core or crates/runtime
  default engine/plugin assembly
  typed use-cases
  DTOs shared by MCP and Tauri
  repo-scoped file access policy
  view intent validation

crates/core
  repository model
  discovery/indexing/parsing/enrichment pipeline
  git/files/html/state/walkthrough primitives if kept as infrastructure

crates/plugin-api
  stable extension contracts

plugins/*
  plugin implementations only
```

This keeps `core` free of bundled plugin dependencies while removing duplication from the product entrypoints.

## Suggested Refactoring Sequence

1. Add `crates/app-core` with no behavior changes.
   Move shared DTOs and `default_engine()` there. Update MCP and Tauri to use it.

2. Move duplicated read/query operations into `ProjectMindService`.
   Start with `repo_info`, `list_classes`, `show_class`, `module_summary`, `show_diagram`, `show_diff`.

3. Add repo-scoped file access.
   Replace direct `std::fs::read_to_string` / `std::fs::read` calls in adapters with service calls.

4. Aggregate relations into `Repository`.
   Keep existing diagram output stable, but change its data source to `repo.relations`.

5. Split discovery/indexing from `Engine::open_repo`.
   Introduce `DiscoveredModule` and a source attribution pass.

6. Update architecture docs.
   Mark the old plan as historical and make the implemented runtime boundary explicit.

## Branch Scope Recommendation

For the current branch, keep this as a review/documentation branch. The next implementation branch should be narrow:

```text
feature/app-core-service
```

First milestone:

- create `crates/app-core`
- move default plugin registration there
- move shared class/module DTOs there
- update MCP and Tauri without changing behavior
- add tests that MCP and Tauri-visible summaries are derived from the same service DTOs

That reduces risk before deeper pipeline changes.

# Architecture

> **Status:** 2026-04-28 — initial architecture design. Iterates with the MVP.

## Workspace Layout

```
projectmind/
├── Cargo.toml                # workspace root
├── rust-toolchain.toml       # pinned stable
├── rustfmt.toml              # formatting rules
├── clippy.toml               # lint thresholds
├── crates/
│   ├── plugin-api/           # public traits + types (no impl)
│   ├── core/                 # repo loading, file walking, plugin registry
│   └── mcp-server/           # MCP server binary (stdio JSON-RPC)
├── plugins/
│   ├── lang-java/            # Tree-sitter Java + entity extraction
│   └── framework-spring/     # Spring annotation detection + bean graph
├── app/                      # Tauri shell (later)
│   ├── src-tauri/
│   └── frontend/
├── docs/
│   └── plan/
└── .github/
    └── workflows/
```

## Module Responsibilities

### `plugin-api`

Pure trait + type crate. **No implementations**, **no heavy dependencies**. Stable surface for plugin authors.

- `LanguagePlugin` — file-extension to AST to entities
- `FrameworkPlugin` — recognise patterns on top of language entities
- `VisualizerPlugin` — render an input shape into a UI component (web component)
- `AnnotationStore`, `CodeGraphStore` — persistence (see `02-persistence.md`)
- Domain types: `Class`, `Method`, `Field`, `Annotation`, `Module`, `RelationKind`, `EntityId`

### `core`

The runtime that wires everything together.

- **Plugin registry** — holds plugin trait objects (statically registered for now; dynamically loaded from `./plugins/` later)
- **Repo loader** — discovers modules, file roots
- **File walker** — `ignore`-respecting walk
- **Pipeline** — `walk → parse (lang plugin) → enrich (framework plugin) → store`
- **Diff service** — uses `git2` to get changes vs a ref
- **Config** — reads `~/.config/projectmind/config.toml` and `<repo>/.projectmind/config.toml`

### `mcp-server`

Binary that:

- Listens on stdio (JSON-RPC)
- Implements MCP `tools/list` and `tools/call`
- Wraps `core` operations into MCP tools
- Used by Claude Code via `.mcp.json`

### `plugins/lang-java`

- Wraps `tree-sitter-java`
- Extracts: class, interface, enum, record; method, field; annotation values; imports; Javadoc
- Outputs `core::Module` populated with `Class`/`Method`

### `plugins/framework-spring`

- Recognises Spring stereotypes (`@Service`, `@Controller`, `@RestController`, `@Component`, `@Configuration`, `@Bean`)
- Recognises injection (`@Autowired`, constructor injection, `@Value`)
- Builds a bean graph (nodes: beans; edges: dependencies)
- Adds metadata to classes (`stereotype = service`, etc.)

### `app/` (Tauri)

- Hosts the same `core` engine plus a `tauri-bridge` that exposes Tauri commands
- Frontend = Svelte + Vite + TypeScript
- Embeds Mermaid, Cytoscape, Monaco
- The Tauri build also runs the MCP server in-process when launched standalone, so the user sees the same data they get via Claude Code

## Plugin API Sketch (Rust)

```rust
// crates/plugin-api/src/lib.rs

pub trait LanguagePlugin: Send + Sync {
    fn id(&self) -> &'static str;            // e.g. "java"
    fn name(&self) -> &'static str;           // human-readable
    fn file_extensions(&self) -> &[&'static str];
    fn parse_module(&self, root: &Path) -> Result<Module>;
}

pub trait FrameworkPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn supported_languages(&self) -> &[&'static str];
    fn enrich(&self, module: &mut Module) -> Result<()>;
    fn relations(&self, module: &Module) -> Vec<Relation>;
}

pub trait VisualizerPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn consumes(&self) -> &'static str;        // e.g. "spring/bean-graph"
    fn webcomponent_tag(&self) -> &'static str;
    fn render_payload(&self, input: &Value) -> Result<Value>;
}
```

For Phase 1, all plugins are **statically registered** in `core::registry` via a build-time list of crate references. Phase 2 adds dynamic `cdylib` loading from `./plugins/` next to the binary.

## MCP Tools (initial set)

| Tool | Input | Output | Notes |
|---|---|---|---|
| `open_repo` | `{ "path": "/abs/path" }` | `{ "repo_id": "...", "modules": [...] }` | Opens a repo; subsequent calls are scoped to it |
| `list_files` | `{ "filter"?: "glob" }` | `{ "files": [...] }` | Files known to active language plugins |
| `list_classes` | `{ "module"?: "..." }` | `{ "classes": [{ "fqn", "file", "stereotype", … }] }` | Stereotype = `service`, `controller`, etc. (when Spring plugin enabled) |
| `show_class` | `{ "fqn": "...", "highlight"?: [{ "from": 42, "to": 58 }] }` | `{ "file", "source", "highlights" }` | For LLM to "show this class" with optional highlight |
| `list_changes_since` | `{ "ref": "HEAD~1" }` | `{ "files": [{ "path", "status" }] }` | Files changed since a git ref |
| `show_diff` | `{ "from": "HEAD~1", "to"?: "HEAD" }` | `{ "diff": [...] }` | Unified diff per file |
| `show_diagram` | `{ "type": "bean-graph" \| "package-tree" \| "pom-deps", "scope"?: "..." }` | `{ "format": "mermaid" \| "cytoscape", "data": {...} }` | Polymorphic — visualizer plugins can register new diagram types |
| `get_user_selection` | `{}` | `{ "file", "from", "to", "text" }` or empty | Last code region selected in the UI |
| `annotate` | `{ "file", "from", "to", "label", "link"? }` | `{ "id" }` | Sets a marker the UI surfaces above the code |
| `list_changes_summary` | `{ "ref": "HEAD~5" }` | `{ "by_module": [...], "stats": {...} }` | High-level summary for "show me the change" |

The MCP server is the bus between the LLM and the IDE: every operation in the UI is, ultimately, one of these tools (so anything the user does is reproducible by the LLM, and vice versa).

## Build Targets

- **Linux x86_64** (Ubuntu 22.04+) — primary user target
- **macOS arm64** — primary dev target
- **Linux arm64** — secondary
- **Windows** — out of scope for Phase 1

## Build Commands (Phase 1)

```bash
# Build everything
cargo build --release --workspace

# Run the MCP server (used by Claude Code)
./target/release/projectmind-mcp

# Run tests
cargo test --workspace

# Lint
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Tauri app (later)
cd app && pnpm install && pnpm tauri dev
```

## Logging & Observability

- `tracing` + `tracing-subscriber` everywhere
- MCP server logs to stderr (stdout is the JSON-RPC channel)
- Default level: `info`; `RUST_LOG` overrides

## Security Posture

- **Read-only by default.** No write operations on user code.
- **Filesystem scope** — plugins are passed paths inside the opened repo only.
- **No network calls** by Phase 1 plugins. (Confluence/Jira plugins in Phase 2 are opt-in and scoped to their declared endpoints.)
- **Plugin sandboxing** — Phase 1 trusts in-tree plugins. Phase 3 with third-party plugins must reconsider (WASM sandbox, capability tokens).

## Open Items

See [`01-brainstorming-vision.md` §10](01-brainstorming-vision.md) for the running list of open questions.

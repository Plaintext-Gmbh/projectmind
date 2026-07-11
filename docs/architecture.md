# Architecture

> **Status:** living reference — kept in sync with the codebase. For roadmap and open design questions, see [GitHub Issues](https://github.com/Plaintext-Gmbh/projectmind/issues) and the [Vision & Roadmap discussion](https://github.com/Plaintext-Gmbh/projectmind/discussions/58).

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
│   ├── mcp-server/           # MCP server binary (stdio JSON-RPC)
│   └── browser-host/         # in-process HTTP host serving the webapp to any browser
├── plugins/
│   ├── lang-java/            # Tree-sitter Java + entity extraction
│   ├── lang-rust/            # Tree-sitter Rust + entity extraction
│   ├── framework-spring/     # Spring annotation detection + bean graph
│   └── framework-lombok/     # Lombok annotation recognition
├── app/                      # Tauri shell + Svelte frontend
│   ├── src-tauri/
│   └── src/
├── docs/
│   ├── sketches/             # design sketches (status headers mark what shipped)
│   └── reviews/              # historical architecture reviews
└── .github/
    └── workflows/
```

## Module Responsibilities

### `plugin-api`

Pure trait + type crate. **No implementations**, **no heavy dependencies**. Stable surface for plugin authors.

- `LanguagePlugin` — file-extension to AST to entities
- `FrameworkPlugin` — recognise patterns on top of language entities
- `VisualizerPlugin` — render an input shape into a UI component (web component)
- `AnnotationStore`, `CodeGraphStore` — persistence (see [`persistence.md`](persistence.md))
- Domain types: `Class`, `Method`, `Field`, `Annotation`, `Module`, `RelationKind`, `EntityId`

### `core`

The runtime that wires everything together.

- **Plugin registry** — holds plugin trait objects (statically registered for now; dynamically loaded from `./plugins/` later)
- **Repo loader** — discovers modules, file roots
- **File walker** — `ignore`-respecting walk
- **Pipeline** — `walk → parse (lang plugin) → enrich (framework plugin) → store`
- **Diff service** — uses `git2` to get changes vs a ref
- **Config** — reads `<repo>/.projectmind/config.toml`, with `$XDG_CONFIG_HOME/projectmind/defaults.toml` as machine-wide fallback

### `mcp-server`

Binary that:

- Listens on stdio (JSON-RPC)
- Implements MCP `tools/list` and `tools/call`
- Wraps `core` operations into MCP tools
- Used by Claude Code via `.mcp.json`

### `browser-host`

- In-process HTTP host that serves the same Svelte webapp to any browser — started via the `open_browser_repo` MCP tool
- Tokenized URL (bearer token in the fragment) gates every API call; loopback by default, `lan: true` binds on `0.0.0.0` for other devices on the WLAN
- Mirrors the same statefile-driven view intents as the desktop GUI

### `plugins/lang-java`

- Wraps `tree-sitter-java`
- Extracts: class, interface, enum, record; method, field; annotation values; imports; Javadoc
- Outputs `core::Module` populated with `Class`/`Method`

### `plugins/lang-rust`

- Wraps `tree-sitter-rust`
- Extracts top-level `struct` / `enum` / `trait` / `union` items as classes, attaches methods from matching `impl` blocks, lifts `#[derive(...)]` and other outer attributes to annotations

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

## MCP Tools

40 tools as of v0.11 + unreleased — the full name/parameter reference lives in the
[README tool table](../README.md); this section only maps them onto the
architecture. Grouped by concern:

- **Repo & structure** — `open_repo`, `repo_info`, `module_summary`,
  `list_module_files`, `list_classes`, `find_class`, `class_outline`,
  `show_class`, `relations`, `plugin_info`
- **Git-derived signals** — `list_changes_since`, `list_refs`, `show_diff`,
  `file_recency`, `commit_activity`
- **Diagrams** — `show_diagram` (returns Mermaid/JSON payloads to the client),
  `view_diagram` (pushes a diagram into the open viewers; live/3D kinds such
  as `bean-graph-live`, `timeline-river`, `code-city` render from their own
  endpoints; the code city's time-lapse player (V5) replays the same
  `commit_activity` + cumulative `list_changes_since` data frontend-only —
  buildings first added within the activity window grow in step by step,
  everything older stands as the base city)
- **Viewer pushes (statefile intents)** — `view_class`, `view_file`,
  `view_diff`, `start_gui`
- **Browser host** — `open_browser_repo`, `browser_status`, `stop_browser`
- **Walkthroughs & cockpit** — `walkthrough_start`, `walkthrough_append`,
  `walkthrough_set_step`, `walkthrough_clear`, `walkthrough_feedback`,
  `walkthrough_query` (semantic tour search), `tour_scaffold` (auto-narrated
  tour skeleton), `self_demo` (one-click self-demo: materialise a tour and open
  it in present + autoplay via the `tour_suggest::self_demo` core path — same
  code the `▶ Demo` button, the Tauri command and the browser host take),
  `risk_atlas`, `pattern_check`, `architect_briefing`
- **Docs & artifacts** — `list_html`, `list_html_snippets`, `docs_for_class`
  (ranked in-repo Markdown mentions of a class — the code↔doc bridge),
  `present_artifact`, `list_artifacts`

The MCP server is the bus between the LLM and the IDE: every operation in the UI is, ultimately, one of these tools (so anything the user does is reproducible by the LLM, and vice versa).

### Artifact storage & rendering

`present_artifact` follows the walk-through body/pointer split: the (up to ~2 MB)
body is persisted to `artifacts/<id>.json` next to the statefile (atomic `.tmp` +
rename), while only a lightweight `{"kind":"artifact","id":…}` pointer rides in the
high-traffic `current.json`. Bodies survive viewer restarts and are cleared when a
**different** repo is opened (`walkthrough_clear` leaves them untouched). Both
viewers fetch the body over their normal channels — the Tauri `current_artifact`
command and the browser host's `GET /api/current_artifact?id=…`. HTML artifacts are
**only** rendered inside an `<iframe sandbox="">` whose injected CSP is
`default-src 'none'; img-src data:; style-src 'unsafe-inline' data:; …; form-action
'none'; base-uri 'none'` (shared `app/src/lib/htmlSandbox.ts`, reused by the HTML
browser) — AI-authored `<script>` never executes and never reaches the app DOM.
Markdown artifacts render through the same `marked` + mermaid pipeline as `.md`
files. A step target `{"kind":"artifact","id":…}` lets walk-throughs embed artifacts
as tour steps.

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

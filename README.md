<p align="center">
  <img src="docs/assets/logo.png" alt="ProjectMind" width="96" height="96"/>
</p>

<h1 align="center">ProjectMind</h1>
<p align="center"><strong>Your project, explained by AI.</strong></p>
<p align="center"><sub>by Plaintext · MPL-2.0</sub></p>

ProjectMind uses AI-ready project maps to explain software architecture,
classes, modules and relationships in a way humans and coding agents can
navigate. It's a lightweight, **read-only** architecture browser for source
code that pairs bidirectionally with LLM-driven coding agents (Claude Code,
etc.) via the **Model Context Protocol (MCP)**.

> **Status:** Early MVP — the **MCP server** and a basic **Tauri UI** both work. Java + Rust language plugins, Spring + Lombok framework recognisers, Mermaid bean graph and package tree, plus an HTML browser that renders `.html` / `.xhtml` / `.jsp` files and embedded HTML snippets in a sandboxed iframe.

## Why

Modern AI-assisted development with CLI agents is great — until you want to *see* what just changed, *visualise* how the architecture is evolving, or *drill into* the structure without firing up a heavy IDE.

`projectmind` aims to be the missing piece:

- **Standalone** desktop app (Mac & Linux); not a VS Code extension.
- **Read-only** — no editing, no builds. Just an "architecture lens".
- **MCP-bidirectional** — your LLM can say *"show class X with lines 42-58 highlighted"* and the viewer renders it. You can mark code regions and the selection flows back into the conversation.
- **Plugin-based** — languages (Java, Kotlin, TypeScript, …), frameworks (Spring, Lombok, JSF, …) and visualisations (bean graph, package tree, C4, …) are all plugins.

## GUI tabs

The Tauri shell has four tabs (each disabled until a repository is open):

- **Code** — module sidebar, class list, source viewer with stereotype filters, package drilldown.
- **Diagrams** — Mermaid bean graph or package tree; click a node to drill in.
- **MD** — every Markdown file in the repo, grouped by top-level directory, with rendered preview, mermaid blocks, and embedded images.
- **HTML** — every `.html` / `.xhtml` / `.htm` / `.jsp` / `.vm` / `.ftl` file plus HTML snippets extracted from `.java` / `.kt` / `.groovy` / `.scala` string literals (Java text blocks supported). Toggle Rendered ↔ Source; Rendered uses a strict sandbox iframe (no JS, no network) so untrusted repo content stays inert.

## What works today

The Phase 1 MVP ships a **Rust MCP server** (`projectmind-mcp`) that Claude Code can connect to. It implements:

| Tool | What it does |
|---|---|
| `open_repo` | Open a repository. Detects Maven multi-module layouts (any `pom.xml`) and Cargo workspaces (any `Cargo.toml` with a `[package]`); falls back to a single module otherwise. |
| `repo_info` | Summary (modules, classes) of the active repo. |
| `module_summary` | Per-module class count and stereotype histogram. |
| `list_classes` | List parsed classes (filter by stereotype). |
| `find_class` | Case-insensitive substring search by simple or fully-qualified name. |
| `class_outline` | Methods, fields, annotations and visibility of a class — without source. |
| `show_class` | Source of a class with optional line-range highlights. |
| `list_changes_since` | Files changed since a given git ref. |
| `show_diff` | Unified diff between two refs (or ref vs working tree). |
| `show_diagram` | Mermaid bean graph (subgraphs per Maven module, colour-coded by stereotype) or package tree. |
| `list_html` | List HTML / XHTML / JSP / Velocity / FreeMarker template files in the open repository. |
| `list_html_snippets` | Scan source files (`.java`, `.kt`, `.groovy`, `.scala`, incl. Java text blocks) for HTML snippets in string literals — filtered to ≥2 tags so XML namespace declarations and short error strings drop out. |
| `plugin_info` | List active language and framework plugins. |

Active language plugins in Phase 1:

- **Java** — Tree-sitter parser. Classes, interfaces, enums, records; methods, fields, annotations, visibility.
- **Rust** — Tree-sitter parser. Structs, enums, traits, unions; `impl` blocks attach methods and lift `impl Trait for T` as annotations on `T`. Module namespace is derived from the nearest `[package].name`.

Active framework recognisers in Phase 1:

- **Spring** — `@Service`, `@RestController`, `@Controller`, `@Component`, `@Repository`, `@Configuration`. Constructor and field injection (`@Autowired`, `@Inject`, `@Resource`) become Mermaid edges.
- **Lombok** — `@Data`, `@Value`, `@Builder`, `@SuperBuilder`, `@*ArgsConstructor`, `@ToString`, `@EqualsAndHashCode`, `@Slf4j`/`@Log*`, `@Getter`, `@Setter`, `@With`, … attached as a `lombok` stereotype with the detected annotations in `class.extras`.

Smoke-tested against real codebases: a 21-module Spring Boot Maven monorepo parses to **~500 classes** with stereotype histograms and a Mermaid bean graph grouped by module; an 8-crate Cargo workspace (this repo) parses to ~60 classes with per-crate modules.

## Build the MCP server (Ubuntu / Debian)

This is the path you want for **using `projectmind` from Claude Code on Ubuntu** — no GUI required.

The repo ships an installer for the impatient:

```bash
git clone git@github.com:Plaintext-Gmbh/projectmind.git
cd projectmind
./scripts/install-ubuntu.sh             # MCP server only
# or:
./scripts/install-ubuntu.sh --with-app  # also build the Tauri shell
```

The script installs build prerequisites, the Rust toolchain (via rustup, if missing), then builds and prints a ready-made `.mcp.json` snippet. If you'd rather install manually:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev cmake git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

git clone git@github.com:Plaintext-Gmbh/projectmind.git
cd projectmind
cargo build --release --bin projectmind-mcp

# Result: target/release/projectmind-mcp
```

## Build the MCP server (macOS)

```bash
# Prerequisites — Homebrew + Rust
brew install rustup-init || true
rustup-init -y
source "$HOME/.cargo/env"

git clone git@github.com:Plaintext-Gmbh/projectmind.git
cd projectmind
cargo build --release --bin projectmind-mcp
```

## Build the Tauri shell (optional, GUI)

The Tauri app is the read-only graphical browser. It is the same engine, just with a UI on top.

### Ubuntu / Debian

```bash
# Tauri prerequisites
sudo apt install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev patchelf

# Node toolchain (for the frontend)
curl -fsSL https://get.pnpm.io/install.sh | sh -

# Build
cd app
pnpm install
pnpm tauri build
```

### macOS

```bash
# Node toolchain
brew install pnpm

cd app
pnpm install
pnpm tauri build
```

Run the app in dev mode (live-reload):

```bash
cd app && pnpm tauri dev
```

## Use with Claude Code

Add to your project's `.mcp.json` (or your global Claude Code config):

```json
{
  "mcpServers": {
    "projectmind": {
      "type": "stdio",
      "command": "/absolute/path/to/projectmind/target/release/projectmind-mcp",
      "env": {
        "PROJECTMIND_LOG": "info"
      }
    }
  }
}
```

Restart Claude Code. From a session, you can then ask things like:

- *"Open the repo at `/home/me/codeplain/plaintext-app` and tell me how many services and controllers there are per module."*
- *"Find any class containing `Auszahl` and outline the most relevant one."*
- *"Show me the `UserService` class — highlight lines 80-95."*
- *"Which files changed since HEAD~5? Group them by module."*
- *"Render the bean graph as a Mermaid diagram."*

The agent will pick the right tool calls from the list above.

### Pre-built binary

When a `v*.*.*` tag is pushed, GitHub Actions publishes a release with `projectmind-mcp` binaries for **Linux x86_64**, **macOS arm64** and **macOS x86_64** (each as a `.tar.gz` plus a `.sha256`). Until the first tag is cut, build from source as shown above.

## Tests / development

The same commands CI runs are wrapped in `scripts/ci.sh`:

```bash
./scripts/ci.sh check    # cargo fmt --check + cargo clippy
./scripts/ci.sh test     # cargo test --workspace --all-targets + --doc
./scripts/ci.sh all      # check + test
```

If you'd rather invoke cargo directly:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```

CI runs on **Ubuntu 22.04** and **macOS 14** for every push and pull request, plus a Linux release-build smoke test.

## Architecture

A Cargo workspace with seven crates plus a Svelte frontend:

| Crate | Purpose |
|---|---|
| `crates/plugin-api` | Public traits and types (no implementations) |
| `crates/core` | Repo loader, file walker, plugin pipeline, Maven + Cargo discovery, git helpers |
| `crates/mcp-server` | The `projectmind-mcp` binary (JSON-RPC over stdio) |
| `plugins/lang-java` | Java parser via Tree-sitter |
| `plugins/lang-rust` | Rust parser via Tree-sitter |
| `plugins/framework-spring` | Spring stereotypes + bean graph |
| `plugins/framework-lombok` | Lombok annotation recogniser |
| `app/src-tauri` | Tauri shell (Rust backend exposing Tauri commands) |
| `app/src/` | Svelte + TypeScript frontend with Mermaid integration |

Phase 1 plugins are **statically registered**. Phase 2 will add dynamic loading from a `./plugins/` directory next to the binary, so third-party plugins can drop in `.so` / `.dylib` files.

See [`docs/SYNC.md`](docs/SYNC.md) for how the MCP server and the Tauri GUI stay in sync (statefile + view intents).

See [`docs/plan/`](docs/plan/) for the full design notes:

- [01-brainstorming-vision.md](docs/plan/01-brainstorming-vision.md) — vision, requirements, user stories
- [02-persistence.md](docs/plan/02-persistence.md) — annotation + graph storage backends (Mempalace, SurrealDB, SQLite, JSON)
- [03-architecture.md](docs/plan/03-architecture.md) — workspace, plugin API, MCP tool schemas
- [04-visualizations.md](docs/plan/04-visualizations.md) — visualisation catalogue and "wow factor" sketches

## Roadmap

- **Phase 1 (in progress):** MCP server with Java + Spring + Lombok plugins, package tree, bean graph, pom dependency graph, diff view, Markdown viewer, core MCP tools.
- **Phase 2:** Tauri shell with a graphical browser, annotation round-trip, draw.io embed, Confluence MCP bridge.
- **Phase 3:** Plugin marketplace, more languages, JSF / PrimeFaces preview, C4 diagram generator.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The project is in early design — discussions, ideas, and visualisation sketches are very welcome.

## License

[MPL 2.0](LICENSE) — Mozilla Public License Version 2.0.

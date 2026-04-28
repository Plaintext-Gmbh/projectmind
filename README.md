# plaintext-ide

A lightweight, **read-only** architecture browser for source code, designed to work bidirectionally with LLM-driven coding agents (Claude Code, etc.) via the **Model Context Protocol (MCP)**.

> **Status:** Early MVP — the **MCP server works**. The Tauri UI shell is the next milestone.

## Why

Modern AI-assisted development with CLI agents is great — until you want to *see* what just changed, *visualise* how the architecture is evolving, or *drill into* the structure without firing up a heavy IDE.

`plaintext-ide` aims to be the missing piece:

- **Standalone** desktop app (Mac & Linux); not a VS Code extension.
- **Read-only** — no editing, no builds. Just an "architecture lens".
- **MCP-bidirectional** — your LLM can say *"show class X with lines 42-58 highlighted"* and the viewer renders it. You can mark code regions and the selection flows back into the conversation.
- **Plugin-based** — languages (Java, Kotlin, TypeScript, …), frameworks (Spring, Lombok, JSF, …) and visualisations (bean graph, package tree, C4, …) are all plugins.

## What works today

The Phase 1 MVP ships a **Rust MCP server** (`plaintext-ide-mcp`) that Claude Code can connect to. It implements:

| Tool | What it does |
|---|---|
| `open_repo` | Open a repository. Detects Maven multi-module layout via `pom.xml`; otherwise treats the whole tree as one module. |
| `repo_info` | Summary (modules, classes) of the active repo. |
| `module_summary` | Per-module class count and stereotype histogram. |
| `list_classes` | List parsed classes (filter by stereotype). |
| `find_class` | Case-insensitive substring search by simple or fully-qualified name. |
| `class_outline` | Methods, fields, annotations and visibility of a class — without source. |
| `show_class` | Source of a class with optional line-range highlights. |
| `list_changes_since` | Files changed since a given git ref. |
| `show_diff` | Unified diff between two refs (or ref vs working tree). |
| `show_diagram` | Mermaid bean graph (subgraphs per Maven module, colour-coded by stereotype) or package tree. |
| `plugin_info` | List active language and framework plugins. |

Active framework recognisers in Phase 1:

- **Spring** — `@Service`, `@RestController`, `@Controller`, `@Component`, `@Repository`, `@Configuration`. Constructor and field injection (`@Autowired`, `@Inject`, `@Resource`) become Mermaid edges.
- **Lombok** — `@Data`, `@Value`, `@Builder`, `@SuperBuilder`, `@*ArgsConstructor`, `@ToString`, `@EqualsAndHashCode`, `@Slf4j`/`@Log*`, `@Getter`, `@Setter`, `@With`, … attached as a `lombok` stereotype with the detected annotations in `class.extras`.

Smoke-tested against a real Spring Boot multi-module repo (`plaintext-app`): **426 classes parsed across 21 Maven modules**, with per-module stereotype histograms and a 200-line Mermaid bean graph that groups beans by module and colours them by stereotype.

## Build the MCP server (Ubuntu / Debian)

This is the path you want for **using `plaintext-ide` from Claude Code on Ubuntu** — no GUI required.

The repo ships an installer for the impatient:

```bash
git clone git@github.com:daniel-marthaler/plaintext-ide.git
cd plaintext-ide
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

git clone git@github.com:daniel-marthaler/plaintext-ide.git
cd plaintext-ide
cargo build --release --bin plaintext-ide-mcp

# Result: target/release/plaintext-ide-mcp
```

## Build the MCP server (macOS)

```bash
# Prerequisites — Homebrew + Rust
brew install rustup-init || true
rustup-init -y
source "$HOME/.cargo/env"

git clone git@github.com:daniel-marthaler/plaintext-ide.git
cd plaintext-ide
cargo build --release --bin plaintext-ide-mcp
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
    "plaintext-ide": {
      "type": "stdio",
      "command": "/absolute/path/to/plaintext-ide/target/release/plaintext-ide-mcp",
      "env": {
        "PLAINTEXT_IDE_LOG": "info"
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

When a `v*.*.*` tag is pushed, GitHub Actions publishes a release with `plaintext-ide-mcp` binaries for **Linux x86_64**, **macOS arm64** and **macOS x86_64** (each as a `.tar.gz` plus a `.sha256`). Until the first tag is cut, build from source as shown above.

## Tests / development

```bash
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs on **Ubuntu 22.04** and **macOS 14** for every push and pull request.

## Architecture

A Cargo workspace with six crates plus a Svelte frontend:

| Crate | Purpose |
|---|---|
| `crates/plugin-api` | Public traits and types (no implementations) |
| `crates/core` | Repo loader, file walker, plugin pipeline, git helpers |
| `crates/mcp-server` | The `plaintext-ide-mcp` binary (JSON-RPC over stdio) |
| `plugins/lang-java` | Java parser via Tree-sitter |
| `plugins/framework-spring` | Spring stereotypes + bean graph |
| `app/src-tauri` | Tauri shell (Rust backend exposing Tauri commands) |
| `app/src/` | Svelte + TypeScript frontend with Mermaid integration |

Phase 1 plugins are **statically registered**. Phase 2 will add dynamic loading from a `./plugins/` directory next to the binary, so third-party plugins can drop in `.so` / `.dylib` files.

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

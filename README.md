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
| `open_repo` | Open a repository, parse all Java sources, run framework plugins |
| `repo_info` | Summary (modules, classes) of the active repo |
| `list_classes` | List parsed classes (optionally filter by stereotype) |
| `show_class` | Source of a class with optional line-range highlights |
| `list_changes_since` | Files changed since a given git ref |
| `show_diff` | Unified diff between two refs (or ref vs working tree) |
| `show_diagram` | Mermaid diagram of the bean graph or package tree |
| `plugin_info` | List active language and framework plugins |

Smoke-tested against a real Spring Boot repo (`plaintext-app`): **417 classes parsed, 30 services detected, full bean-injection graph**.

## Build (Ubuntu / Debian)

```bash
# Prerequisites — install once
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev cmake git curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Build
git clone git@github.com:daniel-marthaler/plaintext-ide.git
cd plaintext-ide
cargo build --release --bin plaintext-ide-mcp

# Result: target/release/plaintext-ide-mcp
```

## Build (macOS)

```bash
# Prerequisites — Homebrew + Rust
brew install rustup-init || true
rustup-init -y
source "$HOME/.cargo/env"

git clone git@github.com:daniel-marthaler/plaintext-ide.git
cd plaintext-ide
cargo build --release --bin plaintext-ide-mcp
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

- *"Open the repo at `/home/me/codeplain/plaintext-app` and tell me how many services and controllers there are."*
- *"Show me the `UserService` class — highlight lines 80-95."*
- *"Which files changed since HEAD~5? Group them by module."*
- *"Render the bean graph as a Mermaid diagram."*

The agent will pick the right tool calls from the list above.

## Tests / development

```bash
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs on **Ubuntu 22.04** and **macOS 14** for every push and pull request.

## Architecture

A Cargo workspace with five crates:

| Crate | Purpose |
|---|---|
| `crates/plugin-api` | Public traits and types (no implementations) |
| `crates/core` | Repo loader, file walker, plugin pipeline, git helpers |
| `crates/mcp-server` | The `plaintext-ide-mcp` binary (JSON-RPC over stdio) |
| `plugins/lang-java` | Java parser via Tree-sitter |
| `plugins/framework-spring` | Spring stereotypes + bean graph |

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

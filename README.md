# plaintext-ide

A lightweight, **read-only** architecture browser for source code, designed to work bidirectionally with LLM-driven coding agents (Claude Code, etc.) via the **Model Context Protocol (MCP)**.

> **Status:** Early planning / brainstorming. No code yet — see [`docs/plan/`](docs/plan/) for the design notes.

## Why

Modern AI-assisted development with CLI agents (e.g. Claude Code) is great — until you want to *see* what just changed in your codebase, *visualise* how the architecture is evolving, or *drill into* the structure without firing up a heavy IDE.

`plaintext-ide` aims to be the missing piece:

- **Standalone** desktop app (Mac & Linux), not a VS Code extension.
- **Read-only** — no editing, no builds. Just an "architecture lens".
- **MCP-bidirectional** — your LLM can say *"show class X with lines 42-58 highlighted"* and the viewer renders it. You can mark code regions and the selection flows back into the conversation.
- **Plugin-based** — languages (Java, Kotlin, TypeScript, …), frameworks (Spring, Lombok, JSF, …) and visualisations (bean graph, package tree, C4, …) are all plugins.

## Planned Features

- Multi-level drill-down: repo → module → package → class → method
- Diff highlighting (what changed since commit X)
- Markdown viewer with Mermaid + draw.io
- Bean graphs, package trees, Maven dependency graphs
- Code annotations that sync back to the LLM via MCP
- Confluence / Jira link markers above code blocks
- *(later)* JSF / PrimeFaces XHTML preview without starting the app

## Architecture (planned)

- **Shell:** [Tauri 2](https://v2.tauri.app/) — Rust core + Web frontend, native on Mac & Linux
- **Parsing:** [Tree-sitter](https://tree-sitter.github.io/) for languages, [JavaParser](https://javaparser.org/) for Java-specific depth
- **Frontend:** Svelte / React + Mermaid.js + Cytoscape.js
- **MCP server:** companion process exposing tools like `show_class`, `show_diff`, `get_user_selection`
- **Plugins:** in-repo workspace crates, dynamically loaded from `./plugins/` at runtime

See [docs/plan/01-brainstorming-vision.md](docs/plan/01-brainstorming-vision.md) for the full vision document.

## Roadmap

- **Phase 1 (MVP):** Tauri shell, Java + Spring + Lombok plugins, package tree, bean graph, pom dependency graph, diff view, Markdown viewer, core MCP tools.
- **Phase 2:** Annotation round-trip, draw.io embed, Confluence MCP bridge.
- **Phase 3:** Plugin marketplace, more languages, JSF / PrimeFaces preview, C4 generator.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The project is in early design — discussions and ideas are very welcome.

## License

[MPL 2.0](LICENSE) — Mozilla Public License Version 2.0.

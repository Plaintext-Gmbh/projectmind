# Brainstorming: projectmind Vision

> **Status:** 2026-04-28 — initial brainstorming. Some design decisions are still open.

## 1. Vision

A **lightweight, standalone architecture browser** (no editor, no builder) that works **bidirectionally with Claude Code (CLI)** via MCP:

- The LLM can say *"show class X / diff since commit Y / bean graph"* and the viewer renders it.
- The user can **annotate / mark** code regions in the viewer; the selection flows back to the LLM for further work.
- Goal: replace IntelliJ for the cases where you just want to "look into the code" without firing up a heavy IDE.

## 2. Core Requirements

| # | Requirement | Priority |
|---|---|---|
| R1 | Standalone app (Mac + Linux), **not** VS Code-based | Must |
| R2 | Read-only — no editor, no builds | Must |
| R3 | MCP server as the backbone — bidirectional with Claude Code | Must |
| R4 | Plugin architecture for languages, frameworks, and visualisations | Must |
| R5 | Multi-level drill-down: repo → module → package → class → method | Must |
| R6 | Diff highlighting (what changed since X) | Must |
| R7 | Markdown reader with Mermaid + draw.io | Should |
| R8 | Code → Confluence doc jumps (annotations above code regions) | Should |
| R9 | JSF / PrimeFaces mockup (XHTML preview without app start) | Could |
| R10 | Polished and easy to use, fast startup | Must |

## 3. User Stories (Brainstorm)

- **US-1:** From the CLI I say "show me the change" → the IDE comes to the foreground with a diff of recently changed files, grouped by module.
- **US-2:** I drill down from module → package → class, see annotations (`@Service`, `@Controller`) highlighted in colour.
- **US-3:** I select three methods + one interface, press "send to Claude" — the selection is available in the CLI as context for the next prompt.
- **US-4:** I open a `README.md` with Mermaid diagrams — they render, and references to classes are clickable.
- **US-5:** Above a class I see a marker "Requirement: CONF-1234" — clicking opens the Confluence page (or shows it inline).
- **US-6 (later):** I open a `.xhtml` and see a static PrimeFaces mockup (no EL resolution).
- **US-7 (LLM-driven):** Claude responds *"I refactored X"* and shows the before/after diff in the viewer plus a Mermaid sequence diagram of the new call chain.

## 4. Research: Existing Tools

### Closest matches

- **code-review-graph** — Tree-sitter-based codebase indexed as a graph in local SQLite, exposed via **MCP** to AI tools. Java support. **80 % of Phase 1** as a backend component. Limitations: no GUI, not plugin-based for frameworks. → **Candidate as inspiration or embedded component.**
- **claude-context** (Zilliztech) — semantic-search MCP for Claude Code, indexes codebases. Complementary (search) rather than visualisation.
- **AppMap** — IntelliJ plugin, runtime-based mapping. Different angle (executes code instead of static parsing) but interesting for Phase 2/3.

### Related building blocks

- **Sourcetrail** — established for legacy code, but inactive. Architecture as inspiration.
- **CodeCharta** — interactive code maps, open source.
- **CodeSee** — commercial, cloud-based. Not a fit (SaaS, no LLM channel).
- **Sourcegraph** — heavy, oriented at search.
- **CodeSparks** — Java IDE plugin framework (JetBrains-only).
- **PlantUML** — DSL → diagrams. Usable as a library.
- **Sidex** — VS Code clone built on Tauri instead of Electron, 96 % smaller. **Proof that Tauri scales for this kind of app.**

### Key insight

Existing tools fall into two camps: (a) IDE plugins (locked to IntelliJ / VS Code) and (b) cloud / SaaS tools. **Standalone + MCP-bidirectional + plugin-based for frameworks** — this combination does not exist yet.

## 5. Tech Stack

### Shell

- **Tauri 2** (Rust core + Web frontend)
  - Small, fast, native on Mac + Linux
  - Proven at scale (Sidex)
  - Plugin system on the Rust side (dynamic libraries / WASM)

### Code parsing

- **Tree-sitter** as the universal parser — grammars available for Java, Kotlin, TypeScript, Python, Rust, etc.
- **JavaParser** as an alternative for deeper Java semantics (symbol resolution, annotations)

### Frontend

- **Svelte** or **React** + Vite
- **Monaco editor** (read-only mode) or **Shiki** for syntax highlighting
- **Mermaid.js** for diagrams
- **Cytoscape.js** or **D3** for graph visualisations
- **draw.io** as embedded iframe
- **Markdown:** `marked` or `unified` / `remark` with plugins

### MCP server

- Companion process (stdio or HTTP)
- Implementation in Rust (same stack as Tauri core) — or Node, if faster to bootstrap

## 6. Plugin Architecture

### 6.1 Distribution (decided)

Plugins live as **workspace members in the same repository** (Cargo workspace). They are built as separate dynamic libraries (`cdylib`) and loaded at runtime from a `./plugins/` directory next to the application binary.

This gives:

- A clean, enforced plugin API from day one
- A single repo / single PR for cross-plugin changes during early development
- An easy upgrade path to third-party plugins later (drop a `.so` / `.dylib` into `plugins/`)

### 6.2 Plugin types

Three plugin kinds:

#### Language plugin

```
LanguagePlugin {
  name: "java"
  file_extensions: [".java"]
  parser: TreeSitter("java")
  extract_entities(ast) -> [Class, Interface, Method, Field, Annotation]
  extract_imports(ast) -> [Import]
}
```

#### Framework plugin

```
FrameworkPlugin {
  name: "spring"
  depends_on_languages: ["java", "kotlin"]
  detect_annotations: ["@Service", "@Controller", "@Component", "@Autowired", ...]
  extract_relations(entities) -> BeanGraph
  augment_class_card(class) -> { badges, color, group }
}
```

#### Visualizer plugin

```
VisualizerPlugin {
  name: "spring-bean-graph"
  consumes: "spring/bean-graph"
  render(input) -> WebComponent (custom element)
}
```

### 6.3 Concrete plugins for Phase 1

- `lang-java` — Tree-sitter Java + annotations + class hierarchies
- `framework-spring` — bean graph, `@Configuration`, `@ComponentScan` awareness
- `framework-lombok` — `@Data` / `@Builder` etc. — show virtual methods
- `viz-mermaid` — generic Mermaid renderer
- `viz-bean-graph` — interactive bean graph (Cytoscape)
- `viz-package-tree` — hierarchical package model
- `viz-pom-deps` — Maven dependency graph from `pom.xml`

### 6.4 Later plugins

- `lang-kotlin`, `lang-typescript`, `lang-python`
- `framework-jsf` (PrimeFaces XHTML preview)
- `framework-quarkus`, `framework-micronaut`
- `integration-confluence`, `integration-jira`
- `viz-c4` — C4 model generator

## 7. MCP Tools (initial set)

| Tool | Description |
|---|---|
| `open_repo(path)` | Open a repository in the viewer |
| `show_class(fqn, highlight_lines?)` | Show a class with optional line highlight |
| `show_diff(from_ref, to_ref?)` | Diff view between two refs |
| `show_diagram(type, scope)` | Bean graph / package tree / sequence diagram, scoped |
| `show_markdown(path)` | Render a Markdown file |
| `get_user_selection()` | Return the user's most recent code selection |
| `annotate(file, lines, label, link?)` | Set a marker (e.g. Confluence link) |
| `list_changes_since(ref)` | List files changed since a ref |

## 8. Phases

### Phase 1 — MVP (Java + Spring + Lombok)
- Tauri shell (Mac + Linux), open repo
- Plugin system with `lang-java`, `framework-spring`, `framework-lombok`
- Visualizers: package tree, bean graph, pom deps, diff view, Markdown reader (Mermaid)
- MCP server with core tools: `show_class`, `show_diff`, `show_diagram`, `get_user_selection`, `list_changes_since`
- Drill-down: module → package → class → method

### Phase 2 — Annotations & Confluence
- Annotation round-trip (user → MCP → LLM)
- draw.io embed
- Confluence MCP bridge (separate MCP server, Atlassian API)
- Code markers with doc links

### Phase 3 — Plugin marketplace & JSF
- Plugin discovery / install from the UI
- Additional language plugins (Kotlin, TypeScript)
- JSF / PrimeFaces XHTML preview (static, no EL)
- C4 diagram generator

## 9. Resolved Decisions

| Question | Decision |
|---|---|
| Open source? | **Yes** — MPL 2.0 (consistent with the rest of the `plaintext-*` family). |
| Plugin distribution | In-repo Cargo workspace; built as `cdylib`; loaded at runtime from `./plugins/`. |

## 10. Open Questions

1. **Persistence:** where are annotations + markers stored? `.projectmind/` folder per repo, or central per user?
2. **MCP transport:** stdio only (local Claude Code) or also HTTP / SSE for remote use?
3. **Per-repo configuration:** `projectmind.toml` / `.yml` with plugin activation + conventions?
4. **Caching:** cache parsed ASTs (SQLite, like code-review-graph)?
5. **Self-update mechanism** for the app itself?
6. **Framework auto-detection** (read `pom.xml` → detect Spring) or manual activation?
7. **Plugin sandboxing** — do we need to constrain what plugins can read / do, or trust them fully (since they ship with the binary in Phase 1)?
8. **UI framework** — Svelte vs React. Decision criteria: bundle size, team familiarity, ecosystem (Mermaid, Cytoscape integrations).

## 11. Sources / References

- code-review-graph: <https://medium.com/@velvrix/how-i-set-up-code-review-graph-on-my-spring-boot-project-with-cursor-why-it-changed-how-i-review-ee799c55d77b>
- claude-context (zilliztech): <https://github.com/zilliztech/claude-context>
- Tauri v2 architecture: <https://v2.tauri.app/concept/architecture/>
- Sidex (Tauri VS Code clone): <https://github.com/Sidenai/sidex>
- AppMap IntelliJ plugin: <https://dev.to/appmap/visualize-the-architecture-of-your-java-app-in-intellij-idea-in-2-minutes-2dp7>
- 15 Best Code Visualization Tools 2026 (CTO Club): <https://thectoclub.com/tools/best-code-visualization-tools/>
- code-visualization GitHub topic: <https://github.com/topics/code-visualization>
- IntelliJ Spring diagrams: <https://www.jetbrains.com/help/idea/spring-diagrams.html>
- Awesome Tauri: <https://github.com/tauri-apps/awesome-tauri>
- Building MCP with LLMs: <https://modelcontextprotocol.io/tutorials/building-mcp-with-llms>

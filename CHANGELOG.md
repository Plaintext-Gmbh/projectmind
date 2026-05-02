# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Annotation tooltips show raw arguments** — hovering a method or field row in the class outline (or a cell in the annotated gutter) now shows each annotation with its full call-arg text: `@RequestMapping(value="/users", method=GET)` instead of just `@RequestMapping`. Backed by a wider `AnnotationRef` shape on the `class_outline` response (Tauri / HTTP / MCP all switched from `string[]` to `[{name, raw_args}]`). Chips on the row stay compact (`@Name+N`); the long form lives in the title-attribute tooltip, one annotation per line.
- **Recency heatmap on the folder map** — toolbar gains a colour-by toggle (`S` structure / `R` recency) when the folder-map diagram is active. In recency mode each leaf is tinted by `log10(secs_ago)`: brand-new edits glow hot orange, week-old files stay yellow, year-plus stale code recedes into cool grey-blue. Folders inherit the most-recent timestamp from their subtree. Recency data is fetched on demand and cached per repo. First frontend consumer of the `file_recency` endpoint from #63.
- **`file_recency` data plumbing** — new `git::file_recency(repo_root)` walks commits from HEAD backwards (capped at 10,000 commits / 5,000 distinct files) and returns one entry per touched path with the most recent commit's `sha`, `summary`, and `secs_ago`. Exposed as a new `file_recency` MCP tool, a Tauri command, and a `/api/file_recency` HTTP endpoint, plus a typed `fileRecency()` wrapper in `app/src/lib/api.ts`. Foundation for the four change-map visualisations captured in #63 (recency heatmap, author overlay, diff overlay on bean graph, timeline river).
- **Inheritance crumb above the class header** — a small breadcrumb shows declared parent types (`extends` first, then `implements` / Rust trait-impls). Click any resolved parent to jump to it. Resolution prefers same-package matches over a single global match, and degrades gracefully (unresolved names render as muted text). Backed by a new `Class.super_types: Vec<TypeRef>` on the plugin-API entity, populated by the Java parser (`superclass` / `super_interfaces` / `extends_interfaces` AST nodes) and by the Rust parser's existing `impl Trait for X` handling. The MCP `class_outline` tool, the Tauri command, and the HTTP endpoint all expose `super_types` in their JSON. Third annotated-source step from #64.
- **Method outline pane in the class viewer** — when a class is selected, a collapsible right-hand panel lists its methods and fields with visibility glyphs (`+`/`#`/`-`/`~`), the first annotation, and the source line. Click jumps the source pane to the matching line and flashes it briefly. New `class_outline` Tauri command + `/api/class_outline` HTTP endpoint reuse the same data shape as the existing MCP `class_outline` tool, so what the user sees and what the LLM sees are exactly aligned. Open/closed state persists in `localStorage`. First step toward the "annotated source" / code-level maps captured in #64.
- **Annotated gutter in the class viewer** — a fixed-width column between line numbers and source code surfaces a visibility glyph + the first annotation chip on every method / field declaration line, plus a stereotype chip on the class header. Hover any cell for the full annotation list. Reuses the outline data already loaded — no extra fetches. Toggle (`◧`) in the class header is independent from the outline-panel toggle, both states persist in `localStorage`. Second annotated-source step from #64.

### Fixed

- `maven::tests` flake under parallel execution — the per-test temp directory used a nanosecond-precision suffix that occasionally collided when cargo's parallel test runner landed two calls in the same nanosecond. Switched to a process-wide atomic counter (same pattern the engine tests already use).

## [0.3.1] — 2026-05-02

### Added

- **Tab registry as a plugin contract** — `TabContribution` + `provided_tabs()` on `LanguagePlugin` / `FrameworkPlugin`; `Engine::available_tabs(repo)` aggregates core tabs (`files`, `diagrams`) with plugin contributions; `App.svelte` renders nav buttons dynamically from `RepoSummary.tabs`. Future plugins (e.g. a `framework-junit` "Tests" tab) can drop in a top-level entry without touching frontend code — the prerequisite for Phase 2's dynamic plugin loading.

### Changed

- **Roadmap and backlog moved from `docs/plan/` and `TODO.md` to GitHub Issues / Discussions.** `docs/plan/03-architecture.md` is now `docs/architecture.md` (living reference). Vision, persistence, visualisation catalogue, and walkthrough follow-ups are tracked as Issues; vision discussion lives on GitHub Discussions. README and CONTRIBUTING updated to point at the new locations.

### Fixed

- Release workflow now installs concrete Rust targets for the macOS
  universal desktop build while keeping Tauri's
  `universal-apple-darwin` build target.
- Linux desktop archives package `.deb`, `.rpm`, and `.AppImage`
  outputs reliably.
- Windows desktop builds reference the checked-in `.ico` icon.
- MCP release artifacts no longer include unsupported macOS x86_64
  builds.

## [0.3.0] — 2026-05-02

The first release that bundles the Phase-1 UI work that had been sitting
on an unmerged local branch — folder maps, drag-and-drop, walkthrough
sync, lazy-load perf — plus the Phase-2 draw.io embed and the multi-
language i18n that went in via Codex.

### Added

- **LAN browser host** (`crates/browser-host/`) — optional
  `open_browser_repo` / `browser_status` / `stop_browser` MCP tools spin
  up a token-protected HTTP server that serves the SPA from any LAN-
  reachable client. Self-contained crate; not wired into the desktop
  shell.
- **Folder-map diagram** — third diagram kind alongside bean-graph and
  package-tree, with hierarchy / solar / top-down layouts.
- **PDF + image files in the module sidebar** — module sidebar +
  class-list aggregate non-source files; click jumps straight to a
  PDF viewer with shift+wheel zoom + pan, or an image viewer.
- **Walkthrough sync** — bidirectional MCP↔GUI sync of walkthrough
  cursor, per-view zoom, and code-link interception inside the tour.
- **Plugin-contributed diagram registry** — the Diagrams tab now lists
  whatever the active framework / language plugins contributed.
- **Drag-and-drop** — drop any file in the Tauri shell to open its
  parent directory as a repo, with the file already selected.
- **Files tab** absorbs the Markdown + HTML browsers — no more separate
  "Classes" wording when a repo has no parsed classes.
- **draw.io viewer** (`app/src/components/DrawIoView.svelte`) — `.drawio`
  files open in an embedded `diagrams.net` iframe via the viewer's
  `proto=json` channel. Lazy-loaded next to the PDF / image / Markdown
  viewers; reuses `createShiftWheelZoom` for zoom parity.
- **5-language i18n** — DE / EN / FR / IT / ES via Codex's
  `app/src/lib/i18n.ts`, with a header-level language toggle.
- **Tauri desktop bundles in the release** — Linux x86_64, macOS
  universal, and Windows x86_64 `.dmg / .deb / .AppImage / .msi`
  alongside the MCP server tarballs.
- **Cross-platform install scripts** — `scripts/install.sh` (POSIX) and
  `scripts/install.ps1` (Windows) pick the right pre-built bundle.

### Changed

- **Markdown index** gets index zoom, TOC arrow-navigation, macOS
  shift-wheel axis-swap fix.
- **Shift+wheel zoom** consolidated into one helper
  (`app/src/lib/shiftWheelZoom.ts`) and applied to every viewer
  (FileView / ClassViewer / DiffView / HtmlIndex / MarkdownIndex /
  WalkthroughView / DrawIoView).
- **Mermaid + marked** lazy-loaded as their own chunks via Vite
  `manualChunks` — the welcome screen + Files tab no longer pay the
  Mermaid tax.

### Fixed

- **PDF view** shift+wheel zoom now actually fires (an invisible
  wheel-catcher overlay sits above the rendered PDF so wheel events
  don't reach the canvas first).
- **Tour** code-links no longer leave the tour pane; PDF pan inside
  the tour scroller respects the parent zoom.
- **Module-files store** gets re-fetched when the modules list
  populates after `open_repo`, and aggregates across every module
  when the filter is null.
- **`view_file`** scoped to the currently-open repo
  (`file_access::canonical_file_in_repo`) — matches the security
  fix from #22 even after the cherry-picked code reverted it.

### Roadmap

- Phase 2 progress: draw.io embed shipped; annotation round-trip,
  Confluence MCP bridge, and dynamic plugin loading still open.

## [0.2.0] — 2026-05-01

First release after the rebrand from `plaintext-ide` to **ProjectMind**
and the public-repo switch. The headline is the green CI / release
pipeline; the next minor release will pick up the UI feature work that
was sitting in unpublished branches.

### Added

- **Auto-Release workflow** (`.github/workflows/auto-release.yml`):
  manual `workflow_dispatch` entry-point that bumps the version (minor
  by default, `major` opt-in), opens a `release/vX.Y.Z` PR, waits for
  CI green, squash-merges, tags the merge commit, and dispatches
  `release.yml` so the binaries get built + published in one shot.
- **CodeQL workflow** for Rust + JavaScript/TypeScript + GitHub Actions
  with a weekly re-scan, paths-ignore for `app/dist/**` and `target/**`.
- `SECURITY.md` policy with `info@plaintext.ch` as the contact and a
  pointer to GitHub's private vulnerability reporting.

### Changed

- Repository is now **public** (was a private MPL-2.0 repo).
- `master` is **branch-protected**: PR-only, required `Rust ubuntu-22.04`
  status check, linear history, no force-pushes, no deletions.
- All GitHub Actions references in workflows are pinned to **full-length
  commit SHAs** with a trailing `# vN` comment so Dependabot keeps them
  current.
- Default `GITHUB_TOKEN` permissions on `ci.yml` / `release.yml` /
  `auto-release.yml` are scoped to `contents: read`; jobs that need
  more elevate explicitly (the release-publish job to `contents: write`,
  the auto-release job to `contents: write` + `pull-requests: write`).
- Tauri app source files (`app/src-tauri/Cargo.toml`,
  `app/src/components/ClassViewer.svelte`, etc.) and Cargo workspace
  manifests bumped to **0.2.0**.
- README rewritten around the **MCP** angle — the server speaks MCP,
  so any frontier-LLM client (Claude Code, ChatGPT, Gemini CLI,
  Cursor, Continue, custom agents) can drive it. Earlier wording made
  it sound Claude-Code-specific.
- README "Status" line acknowledges the shipped Phase 1 MVP scope
  (Tauri shell, Markdown + HTML browsers, walkthrough mode,
  bidirectional MCP sync, folder-map diagram).
- `docs/SYNC.md` examples anonymised — replaced personal paths
  (`/Users/mad/codeplain/plaintext-app`) and class names with generic
  `/path/to/repo` / `com.example.UserService` placeholders.

### Fixed

- **CI**: build the SPA before clippy/test so
  `tauri::generate_context!()` finds `app/dist`. The proc macro had
  been panicking on every PR push since the project was renamed.
- **CI**: `mktemp -t projectmind-smoke` template now ends in `.XXXXXX`
  so it works on the Linux release-smoke job (BSD `mktemp` was lenient,
  GNU `mktemp` rejected the un-suffixed template).
- **ClassViewer**: shift+wheel actually scales the source code now —
  `.source` switched from `font-size: 12.5px` to `0.78em` so the
  `.root` em-scaling propagates.
- **Maven POM reader**: ported off `BytesText::unescape()` (removed in
  quick-xml 0.39) to `decode()` + `quick_xml::escape::unescape()`.
- **Tree-sitter parsers**: ported to the `LANGUAGE: LanguageFn` API
  (tree-sitter-rust 0.24 / tree-sitter-java 0.23). The workspace
  `tree-sitter` dep moved 0.22 → 0.26 to pick up `Into<Language>` for
  `LanguageFn`.

### Security

- Enabled **Dependabot security updates**, **secret scanning** with
  push protection, and **CodeQL** code scanning.
- npm `uuid` pinned to ≥ 14 via `pnpm.overrides` to satisfy
  GHSA-w5hq-g745-h8pq (the vulnerable v3/v5/v6 algorithms in mermaid's
  transient `uuid@11`).
- npm `vite` 5 → 8 + `esbuild` security patches via the multi-package
  Dependabot security update.
- `glib` (transient via wry) and `rand` (build-time only via
  `phf_macros` → `tauri-utils`) advisories dismissed as
  `tolerable_risk` after analysis; the `cargo update` since then has
  also patched `rand` to its fixed range.

### Removed

- Stale `app/package-lock.json`. The project uses pnpm; the npm
  lockfile was a residue from the rebrand and was triggering a
  duplicate `uuid` Dependabot alert against the npm manifest.

### Cargo dependency bumps

| Crate              | From    | To       |
| ------------------ | ------- | -------- |
| `notify`           | 6.1.1   | 8.2.0    |
| `thiserror`        | 1.0.69  | 2.0.18   |
| `quick-xml`        | 0.36.2  | 0.39.2   |
| `tree-sitter`      | 0.22.6  | 0.26.8   |
| `tree-sitter-java` | 0.21.0  | 0.23.5   |
| `tree-sitter-rust` | 0.21.2  | 0.24.2   |
| `git2`             | 0.19.0  | 0.20.4   |
| `dirs`             | 5.0.1   | 6.0.0    |
| `tauri`            | 2.10.3  | 2.11.0   |

### npm dependency bumps

| Package                       | From    | To      |
| ----------------------------- | ------- | ------- |
| `vite`                        | 5.4.21  | 8.0.10  |
| `@sveltejs/vite-plugin-svelte`| 5.x     | 7.0.0   |
| `esbuild`                     | (sec.)  | latest  |

## [0.1.0] — 2026-04-29

> **Note:** v0.1.0 was published under the pre-rebrand
> `plaintext-ide-mcp-*` asset name and only included a single macOS arm64
> tarball. It is superseded by 0.2.0; the install script and Auto-Release
> workflow target the 0.2+ asset naming.

### Added

- Initial repository scaffolding
- Vision and brainstorming document under `docs/plan/`
- MPL 2.0 license, Code of Conduct, contribution guidelines
- Issue and pull request templates
- Dependabot configuration

[Unreleased]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Plaintext-Gmbh/projectmind/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Plaintext-Gmbh/projectmind/releases/tag/v0.1.0

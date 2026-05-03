# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`start_gui` MCP tool** — explicit "bring the desktop window up" intent for LLMs. The `view_*` tools already auto-launch the GUI on demand (lazy, throttled); `start_gui` exposes the same launcher as a first-class call so an agent can ensure the window is up before a sequence of intents (e.g. before kicking off a walkthrough). Returns whether the GUI was already running or was just launched (with the resolved binary path). Honours `$PROJECTMIND_APP` for an override; on macOS uses `open -a`, on Linux execs the binary directly. Bypasses the `view_*` cooldown since the call is user-initiated.
- **Sidebar-toggle button in the header** — single button left of "Open repo" that hides both the modules and files columns at once for an unobstructed viewer. The two existing per-column rails remain for fine-grained control. `aria-pressed` reflects the collapsed state; visibility persists via the existing `moduleSidebarVisible` / `classSidebarVisible` stores.
- **Interactive LLM-CLI registration in `scripts/install.sh`** — after dropping `projectmind-mcp` on disk the bash installer now detects `claude` and `codex` on `PATH` and offers (one `[Y/n]` prompt per CLI) to wire up the MCP server via the official subcommand (`claude mcp add` / `codex mcp add`). Other MCP-capable CLIs (`gemini`, `cursor`, `windsurf`, `cline`, `opencode`, `aider`, `continue`) are detected and the manual binary path is surfaced. `PM_REGISTER=yes` auto-registers every detected CLI without prompting; `PM_REGISTER=no` skips the prompt and just prints manual hints. The prompt reads from `/dev/tty` so it survives `curl … | sh`.

### Changed

- **Header navigation arrows moved next to the new sidebar toggle.** The `‹ ›` history buttons used to live inside `.brand` on the far left of the toolbar; they now sit immediately left of the sidebar-toggle in the action group, which keeps every toolbar control the user actually clicks (back / forward / collapse / open repo / lang / theme / ?) in one cluster on the right. The brand block on the left is now logo + title + repo crumb only, so it has more room to render long repo paths before clipping.

### Fixed

- **Viewer pane went blank when the files sidebar was collapsed.** Hitting either the new header sidebar-toggle or the pre-existing per-column `‹` collapse button replaced the entire `<aside class="sidebar"> + resizer + <main class="viewer">` block with just a `pane-rail`, so the actively-shown class / file / PDF / image content disappeared too. Restructured the conditional so the inner `{#if classSidebarVisible}` swaps **only** sidebar+resizer for the rail; the viewer renders unconditionally. Affects every viewMode that uses the layout grid (`classes` / `pdf` / `image` / `file`); `md` and `html` are unaffected since they already render through `files-fullspan`.
- **Welcome screen rendered raw i18n keys** — `welcome.title`, `welcome.tagline`, `welcome.openButton`, `welcome.empty`, `welcome.hint.{browser,tauri}` and the browser-mode token panel keys (`browserMode.banner`, `browserMode.tokenLabel`, `browserMode.tokenSubmit`) were referenced from `App.svelte` but had no entry in any locale file, so the i18n fallback dumped the key string verbatim. Added across `en` / `de` / `fr` / `it` / `es`. Same patch fills several other long-standing gaps surfaced while auditing the welcome view (`diagram.hint`, `drop.overlay`, `files.aria.list`, `files.filter.{all,md.title,html.title}`, `files.package.{label,clear}`, `files.placeholder`, `status.followingMcp{,Title}`, `status.walkthroughTitle`, `layout.both.{show,hide}`).

## [0.3.2] — 2026-05-03

### Added

- **MCP server bundled with the desktop app** — every Tauri release bundle (`.dmg` / `.deb` / `.AppImage` / `.msi`) now ships `projectmind-mcp` next to the GUI binary, so installing or upgrading the desktop app keeps the MCP server in lockstep automatically — no more `cargo install` round-trip, no GUI/MCP version skew. Wired through Tauri 2's `bundle.externalBin`, so the binary is code-signed alongside the app on macOS and lands in the same install dir on every platform; for the macOS universal target we lipo the aarch64 + x86_64 slices into one fat binary because Tauri 2 doesn't auto-combine externalBin entries the way it does the main bundle. After installing, point the LLM client at the in-app path: `/Applications/ProjectMind.app/Contents/MacOS/projectmind-mcp` on macOS, `/usr/bin/projectmind-mcp` on Linux `.deb`, `C:\Program Files\ProjectMind\projectmind-mcp.exe` on Windows. README updated with per-OS guidance and a `claude mcp add` one-liner. (#101)
- **Doc-graph diagram** — new diagram kind alongside bean-graph / package-tree / folder-map / inheritance-tree. Renders the repo's markdown documents as a clickable graph with three layouts (Network / Radial / Orphans), surfaces orphan + dangling-link counts in a side panel, and routes node-clicks to open the source markdown. Only offered when the repo contains at least one markdown file. Backend lives in `crates/core/src/doc_graph.rs` and is exposed via the Tauri command, browser-host route, and MCP tool. (#97)
- **Repo-switcher dropdown in the header** — the repo pill is now a clickable button that opens a menu of every other recent repo. Picking an entry calls `openRepo` and resets filters via the existing welcome-screen path; failed loads drop the entry from the recents list so deleted/moved repos stop showing up. An "Open repo…" action stays at the bottom for the directory picker. (#98)
- **Collapsible modules + files sidebars** — both left columns can now be hidden so the viewer expands. Each panel grows a small `‹` collapse button; when hidden, a 28px rail with `›` takes the column's place to restore. Visibility persists in `localStorage` (`projectmind.layout.{modulesVisible,filesVisible}`). The grid template adapts to either or both sidebars being collapsed; the markdown/HTML "files-fullspan" view shifts left when modules are hidden. (#99)
- **Module filter on the Markdown + HTML indexes** — when a module is selected in the modules sidebar, the markdown and HTML indexes narrow to files (and snippets) whose absolute path lives under that module's root. An active filter renders as a small chip next to the title; clicking it clears the filter. Counts in the title bar and tab badges follow the visible list. (#100)

- **JSON-backed annotation store** — first concrete persistence backend. `crates/core/src/annotations.rs` implements `AnnotationStore` against `.projectmind/annotations.json` inside the repo root: human-readable, diffable, atomic writes (`.tmp` then rename), monotonically growing ids that never recycle. Wired through Tauri commands (`list_annotations` / `add_annotation` / `remove_annotation`) and matching HTTP routes on `browser-host` (`GET /api/list_annotations` plus `POST /api/add_annotation` / `POST /api/remove_annotation`). TypeScript wrappers (`listAnnotations`, `addAnnotation`, `removeAnnotation`) live in `app/src/lib/api.ts`. Eight unit tests cover the round-trip (empty repo / multi-record persistence / file filter / removal / id-monotonicity / atomicity / malformed-input rejection). No UI yet — annotation rendering and creation flows land in follow-up PRs. First step toward closing #59.
- **Walkthrough start surfaces in the background** — when an LLM kicks off a tour via `walkthrough_start` and the user is on a different tab in the app or has the window minimised behind something else, two new signals fire: a dedicated MCP toast (`mcp.toast.walkthrough`) and a `▶ ` prefix on `document.title` so the tab strip / dock badge reflects the change. Title is restored on focus / visibility change. Closes one of the open follow-ups in #67.
- **Walkthrough narration: `[step:N]` anchor sugar** — LLM-authored tours can now write `[step:3]` or `[step:5|the bean graph stop]` inside markdown narration to link to other steps. The shortcut is rewritten to a `pm:step:<N-1>` link before rendering and the existing pm-link click handler routes it through the same `goTo` path manual clicks use. New `pm:step:N` URI scheme is documented alongside `pm:class:`, `pm:file:`, and `pm:diff:`. Shortcut logic lives in `app/src/lib/walkthroughText.ts` with 8 vitest cases. Closes one of the open follow-ups in #67.
- **Inheritance tree diagram** — new diagram kind alongside bean-graph / package-tree / folder-map. Renders every parsed class with declared parents (`extends` / `implements` / Rust trait-impls) as a Mermaid `flowchart TD`: solid arrows for `extends`, dotted arrows for `implements`. Internal classes are grouped per module subgraph; external supertypes (`Object`, `Serializable`, …) land in a synthetic `external supertypes` subgraph so they're visible without polluting module groups. Click any class to drill into its source. Generic argument lists are stripped for readability (`List<String>` → `List`). Same resolution heuristic as the inheritance crumb in #85 (FQN match → same-package match → unique simple-name match). Both `lang-java` and `lang-rust` advertise the new kind via `provided_diagrams()`. Closes #64 concept 4.
- **Author overlay on the folder map** — third colour-by mode (`A`) next to structure (`S`) and recency (`R`). Each leaf is tinted by the author of its most-recent commit; the same author identity always maps to the same hue (djb2 hash of email-or-name → HSL). Folders inherit the author of their most-recent descendant so both git modes stay consistent. The `FileRecency` payload now carries `author_name` + `author_email` and the toggle reuses the cache populated by recency mode — toggling between R and A re-renders without re-fetching.
- **Annotation tooltips show raw arguments** — hovering a method or field row in the class outline (or a cell in the annotated gutter) now shows each annotation with its full call-arg text: `@RequestMapping(value="/users", method=GET)` instead of just `@RequestMapping`. Backed by a wider `AnnotationRef` shape on the `class_outline` response (Tauri / HTTP / MCP all switched from `string[]` to `[{name, raw_args}]`). Chips on the row stay compact (`@Name+N`); the long form lives in the title-attribute tooltip, one annotation per line.
- **Recency heatmap on the folder map** — toolbar gains a colour-by toggle (`S` structure / `R` recency) when the folder-map diagram is active. In recency mode each leaf is tinted by `log10(secs_ago)`: brand-new edits glow hot orange, week-old files stay yellow, year-plus stale code recedes into cool grey-blue. Folders inherit the most-recent timestamp from their subtree. Recency data is fetched on demand and cached per repo. First frontend consumer of the `file_recency` endpoint from #63.
- **`file_recency` data plumbing** — new `git::file_recency(repo_root)` walks commits from HEAD backwards (capped at 10,000 commits / 5,000 distinct files) and returns one entry per touched path with the most recent commit's `sha`, `summary`, and `secs_ago`. Exposed as a new `file_recency` MCP tool, a Tauri command, and a `/api/file_recency` HTTP endpoint, plus a typed `fileRecency()` wrapper in `app/src/lib/api.ts`. Foundation for the four change-map visualisations captured in #63 (recency heatmap, author overlay, diff overlay on bean graph, timeline river).
- **Inheritance crumb above the class header** — a small breadcrumb shows declared parent types (`extends` first, then `implements` / Rust trait-impls). Click any resolved parent to jump to it. Resolution prefers same-package matches over a single global match, and degrades gracefully (unresolved names render as muted text). Backed by a new `Class.super_types: Vec<TypeRef>` on the plugin-API entity, populated by the Java parser (`superclass` / `super_interfaces` / `extends_interfaces` AST nodes) and by the Rust parser's existing `impl Trait for X` handling. The MCP `class_outline` tool, the Tauri command, and the HTTP endpoint all expose `super_types` in their JSON. Third annotated-source step from #64.
- **Method outline pane in the class viewer** — when a class is selected, a collapsible right-hand panel lists its methods and fields with visibility glyphs (`+`/`#`/`-`/`~`), the first annotation, and the source line. Click jumps the source pane to the matching line and flashes it briefly. New `class_outline` Tauri command + `/api/class_outline` HTTP endpoint reuse the same data shape as the existing MCP `class_outline` tool, so what the user sees and what the LLM sees are exactly aligned. Open/closed state persists in `localStorage`. First step toward the "annotated source" / code-level maps captured in #64.
- **Annotated gutter in the class viewer** — a fixed-width column between line numbers and source code surfaces a visibility glyph + the first annotation chip on every method / field declaration line, plus a stereotype chip on the class header. Hover any cell for the full annotation list. Reuses the outline data already loaded — no extra fetches. Toggle (`◧`) in the class header is independent from the outline-panel toggle, both states persist in `localStorage`. Second annotated-source step from #64.

### Changed

- **`scripts/ci.sh` learns `stage-mcp-sidecar [<target>]`** for staging the MCP binary into `app/src-tauri/binaries/<triple>{ext}` ahead of `tauri build`. The existing `app-build` step calls it automatically, so the release CI workflow picks up the new sidecar without any workflow edit. `cmd_check` and `cmd_test` ensure an empty placeholder exists at the host-triple path so tauri-build's externalBin validation doesn't fail clippy/test runs that don't need the real binary.

### Fixed

- **Header nav buttons could disappear on narrow windows** — the `.brand` block on the left had no `min-width: 0`, so a long repo path refused to shrink and pushed the right-side `<nav>` (Files / Diagrams / Open repo / theme) off-screen. Brand block now `flex: 1 1 auto; min-width: 0; overflow: hidden` so it clips its inner crumb instead, and `nav` gets `flex-shrink: 0; flex-wrap: wrap` so it stays visible (and wraps onto a second row at extreme widths) instead of vanishing.
- **Missing `status.*` and `nav.*` i18n keys surfaced as raw key strings** in the header status badge ("status.repoCount") and a few tooltips. Added: `status.repoCount`, `status.files.{one,other}`, `status.modules.{one,other}`, `status.noRepo`, `status.loading`, `nav.followingMcp.title`, `nav.token`, `nav.langToggle`, `nav.themeToggle.{toLight,toDark}` in EN + DE. Other locales fall through to EN via the existing dictionary fallback.
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

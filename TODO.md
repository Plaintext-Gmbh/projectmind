# TODO

Backlog of small-to-medium improvements after the HTML-tab milestone. Sorted
roughly by implementation cost, with rough effort tags so we can pick things
off as time allows.

## Quick wins (≤ 1 hour each)

### ~~Hide nav buttons that have no content~~ ✅ done
Tabs hide based on `RepoSummary.{classes, markdown_count, html_count}`. The
Code tab renames to **Files** when the repo has zero parsed classes. Diagrams
still uses the legacy bean-graph/package-tree pair — per-plugin diagram
contributions land in the Larger features section below.

### ~~Shift + wheel zoom in detail views~~ ✅ done
Wired into `FileView`, `ClassViewer`, and `HtmlIndex` (source pane + rendered
iframe via CSS `zoom`). Persisted per-view in `localStorage`.

### ~~Diff-mode readability + shift-wheel zoom in tour~~ ✅ done
Theme-aware add/del colours (the original `#b8eaa6` was unreadable on the
light theme), default font size up from 0.86em to 1em, and the existing
shift-wheel zoom on `DiffView` now works inside the walk-through view too.

### ~~PDF pan + shift-wheel zoom~~ ✅ done
A persistent overlay on top of the `<embed>` plugin catches wheel and
pointer events so a zoomed PDF can be panned by drag, scrolled by wheel,
or moved by arrow / Page-Up / Page-Down keys. Native PDF re-renders at the
scaled `zoom:` size so it stays crisp.

### ~~Tour link handling~~ ✅ done
Narration `<a>` clicks are intercepted: `https?://` and `mailto:` open
through `tauri-plugin-opener`, `pm:class:FQN` / `pm:file:/abs/path` /
`pm:diff:refA..refB` schemes navigate inside the app, anchor links fall
through, anything else is refused so the shell never accidentally
navigates away from the tour.

### ~~Classes → Files wording~~ ✅ done
Top-tab is permanently labelled "Files", the placeholder reads "Select a
file", repo-status counts files instead of classes, the file-list aria
label is "Files". Backend types still say `classes` (programming-language
term); only the user-facing surface changed.

## Small features (1–3 hours)

### ~~Resizable sidebar panes~~ ✅ done
Both Code and HTML layouts have drag handles (Pointer Events, persisted in
`localStorage`, double-click resets to default). Reusable Svelte action
lives in `app/src/lib/resizable.ts`.

### ~~Indexed / fuzzy markdown search~~ ✅ done
Backend `search_markdown(root, query, limit)` powered by `nucleo-matcher`,
scoring against title, path, and the first ~4 KB of content. The frontend
debounces the query (80 ms) and shows a flat scored list with match-kind
badges + content snippets when the hit comes from the body. Empty query
falls through to the grouped browse view.

Same treatment is a natural fit later for **HTML snippets** and the **class
list** in the Code tab — the Rust function is generic enough to crib.

### ~~MD + HTML as chips inside Files~~ ✅ done
The two top-level MD and HTML tabs are gone — instead, two chips inside
the Files filter row toggle the Markdown / HTML browsers in-place. The
Files tab now hosts every per-repo content surface (parsed classes, plain
files, MD index + search, HTML index + view modes + snippets).

## Medium features (half a day each)

### ~~Internationalisation (DE + EN)~~ ✅ done
A ~50-line hand-rolled translator (`app/src/lib/i18n.ts`) reads
`app/src/i18n/{en,de}.json` shards through a Svelte derived store. Default
follows `navigator.language`, override persists in `localStorage`. Header
has a small DE/EN toggle next to the theme switcher. Translated surfaces
are listed in the i18n commit.

Plug-in story (still open): a third-party plugin should be able to ship
its own `i18n/<lang>.json` shard that gets merged at load time. Land that
once dynamic plugin loading exists; for now, core ships the two languages.

### ~~Startup performance~~ ✅ done
Initial bundle dropped from 815 KB → 106 KB (gzip 36 KB) by lazy-importing
DiagramView, FileView, MarkdownIndex, HtmlIndex, WalkthroughView through a
cache-aware helper, and splitting mermaid (~640 KB) and marked (~40 KB)
into manualChunks. The welcome screen and Files tab now load without
paying the diagram / markdown rendering tax.

## Larger features (1+ day)

### Per-plugin tab + diagram contributions
**Effort:** L

The current tabs (Code / Diagrams / MD / HTML) and diagrams (Bean graph /
Package tree) are hard-coded. Move the registry to plugins so the visible
set adapts to what's actually in the repo.

**Diagram registry: ✅ done.** `LanguagePlugin` and `FrameworkPlugin`
now have an optional `provided_diagrams()` hook (default empty).
`lang-java` + `lang-rust` contribute `package-tree`, `framework-spring`
contributes `bean-graph`. `Engine::available_diagrams(repo)` aggregates
across plugins (folder-map is unconditional core), and `RepoSummary`
ships the resulting list to the UI which renders Diagram-tab buttons
dynamically.

**Tab registry: still open.** The `Code` / `Diagrams` tab pair is still
hard-coded in `App.svelte`. Plumbing should mirror the diagram path:
`plugin-api` gets a `Contributions` trait with `tabs()`; plugins return
small descriptors (id, label key, view-mode value). Tabs that produce
classes should fold into the existing Files tab; standalone tabs (e.g. a
future `framework-junit` "Tests" tab) appear as their own entries. Land
this before Phase 2's dynamic plugin loading so adding a `.dylib` doesn't
require touching frontend code.

## Distribution (recently shipped)

### ~~Cross-platform local + GitHub release pipeline~~ ✅ done
`./build dist` produces a distributable bundle for the host platform (Mac
universal app + dmg, Linux .deb/.AppImage, Windows .msi/.exe) without
GitHub Actions. The release workflow has separate `mcp` and `app` jobs
that build for macOS / Linux / Windows. `scripts/install.{sh,ps1}` are
one-shot installers that fetch the right asset from GitHub Releases and
drop binaries in standard locations; the README has a `curl … | sh` and
PowerShell `iwr | iex` quickstart.

## Notes

- Items in **Quick wins** are good first issues / weekend afternoons.
- The **per-plugin tab contributions** rework should land before Phase 2's
  dynamic plugin loading — once plugins can drop in as `.dylib`/`.so`,
  having a single registry surface to extend will save a lot of churn.

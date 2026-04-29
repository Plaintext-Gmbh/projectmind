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

## Medium features (half a day each)

### Internationalisation (DE + EN)
**Effort:** M-L

Make every user-facing string switchable. Plug-in friendly so future
language packs can be added without code changes.

- Pick a tiny i18n lib for Svelte (e.g.
  [`svelte-i18n`](https://github.com/kaisermann/svelte-i18n)) or roll a
  100-line store-based translator (we have very few strings).
- Translation files live in `app/src/i18n/{en,de}.json` — plain key → string.
- Default language follows `navigator.language`; user override stored in
  `localStorage` (`projectmind.lang`).
- Add a small language switcher to the header next to the theme toggle
  (`☀ / ☾` already there → add `EN / DE`).
- Translate: nav labels (Code / Diagrams / MD / HTML / Open repo /
  Walk-through), the welcome screen, error toasts, viewer placeholders,
  the HTML/MD search placeholders.
- Plug-in story: a third-party plugin should be able to ship its own
  `i18n/<lang>.json` shard that gets merged at load time. Phase 2 work
  (after dynamic plugin loading lands) — for now, just have core ship the
  two languages.

## Larger features (1+ day)

### Per-plugin tab + diagram contributions
**Effort:** L

The current tabs (Code / Diagrams / MD / HTML) and diagrams (Bean graph /
Package tree) are hard-coded. Move the registry to plugins so the visible
set adapts to what's actually in the repo.

- Frameworks contribute **diagram providers**: `framework-spring` provides
  `bean-graph`; a future `framework-junit` could provide `test-coverage`,
  etc. The Diagrams tab lists only the providers whose
  `applies_to(repo)` returns true.
- Languages contribute **package-tree** when their model has a hierarchical
  namespace (Java `package`, Rust `mod`, Python dotted modules). When no
  active language plugin claims a tree, hide the package-tree tab.
- The `Code` tab itself is a contribution from any plugin that produces
  classes. With zero such plugins active (a docs-only repo), it falls back
  to a plain file/folder browser titled **Files**.
- Plumbing: `plugin-api` gets a `Contributions` trait with optional
  `tabs() / diagrams()` methods. `Engine::open_repo` aggregates the active
  contributions and returns them in `RepoSummary` so the frontend renders
  exactly what's available.

## Notes

- Items in **Quick wins** are good first issues / weekend afternoons.
- The **Per-plugin contributions** rework should land before Phase 2's
  dynamic plugin loading — once plugins can drop in as `.dylib`/`.so`,
  having a single registry surface to extend will save a lot of churn.

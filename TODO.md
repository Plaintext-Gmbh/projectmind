# TODO

Backlog of small-to-medium improvements after the HTML-tab milestone. Sorted
roughly by implementation cost, with rough effort tags so we can pick things
off as time allows.

## Quick wins (≤ 1 hour each)

### Hide nav buttons that have no content
**Effort:** S

The nav buttons should disappear (not just be `disabled`) when the open
repository has nothing to show under them:

- **MD** → hide if `list_markdown_files(root)` is empty.
- **HTML** → hide if `list_html_files(root)` and `find_html_snippets(root)` are both empty.
- **Code** → keep visible always (it's the main view), but rename to **Files** if
  the repo has zero parsed classes (e.g. an "office" repo of pure folders +
  documents). Trigger: `repo.classes === 0`.
- **Diagrams** → hide if there are no diagrams to render — i.e. when no
  diagram-producing plugin is active for the open repo.

Implementation sketch: extend `RepoSummary` with `markdown_count`,
`html_count`, `available_diagrams: string[]`, populated by `open_repo`. The
frontend checks those fields when deciding whether to render each tab.

### Shift + wheel zoom in detail views
**Effort:** S

`FileView.svelte` already has a `zoom` factor with `+`/`−` buttons. Add a
`wheel` listener that, when `shiftKey` is true, multiplies/divides by
`ZOOM_STEP`. Same treatment for `ClassViewer.svelte` (currently no zoom)
and the `HtmlIndex` source/rendered panes.

Persist per-component in `localStorage` (`plaintext-ide.zoom.<view>`) so it
survives reopens.

## Small features (1–3 hours)

### Resizable sidebar panes
**Effort:** M

Today the Code tab uses a fixed `grid-template-columns: 220px 360px 1fr`
in `App.svelte`. Make the two sidebar widths user-adjustable via a vertical
drag handle between them and between the second sidebar and the viewer.

- Persist widths in `localStorage` (`plaintext-ide.layout.code.{module,class}`).
- Apply the same pattern to the HTML tab (currently `360px 1fr`).
- Avoid pulling in a third-party split-pane lib; a 30-line custom handle
  with `pointermove` + CSS variable update is plenty.

### Indexed / fuzzy markdown search
**Effort:** M

The MD-tab search currently filters by exact substring on title and path.
Switch to a fuzzy matcher so typos and partial words still hit:

- Use [`nucleo-matcher`](https://docs.rs/nucleo-matcher) (the matcher behind
  Helix) on the Rust side. Build the index once when `list_markdown_files`
  runs and cache it keyed by `repo.root`.
- Optional: index a snippet of the *content* (first 4 KB) so the user can
  search by phrases that appear inside the document, not just titles.
- Frontend keeps the same UI; the Tauri command is upgraded to take a
  `query` param and return scored hits.

Same treatment is a natural fit later for **HTML snippets** and the **class
list** in the Code tab.

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
  `localStorage` (`plaintext-ide.lang`).
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

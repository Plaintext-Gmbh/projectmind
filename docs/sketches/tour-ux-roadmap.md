# Tour UX — sequencing roadmap for #124–#127

> Four new tour-UX issues opened together: end-of-tour quiz (#124), before/after
> architecture snapshots (#125), animated diff focus rail (#126), change compass
> overlay (#127). Each issue carries a UX sketch, so this doc skips re-describing the
> feature shapes and focuses on the *order* they should ship in, the shared
> infrastructure they want, and which one is the small first step that unblocks the
> rest.

## TL;DR

Land in this order, smallest-cheapest first:

1. **#127 change compass overlay** — pure UI on existing `list_changes_since` data; new
   `<TourCompass>` Svelte component slotted into `WalkthroughView`'s step header.
2. **#126 animated diff focus rail** — refactor `DiffView` to expose a hunk index, then
   add a side-rail driven by the new index plus optional tour-step focus metadata.
3. **#125 before/after architecture snapshots** — needs the Cytoscape migration the
   #62 evaluation defers; gated on that work landing first.
4. **#124 end-of-tour quiz** — new MCP schema, new persistence shape (per-tour scoring),
   biggest UI surface; ship after the other three so the tour-step renderer is stable.

## Shared infrastructure these features want

All four reach into one of two seams that don't exist yet:

### 1. Per-step **change context** (file → status, diff line, hunk)

Today a tour step's `target` is one of `class | file | diff | note`. There's no "this
step is about hunk N of file F in diff D-vs-HEAD" handle. #126 needs it explicitly,
#127 wants it to show the breadcrumb, #125 wants it for the changed/unchanged badge.

**Proposed shape** (additive, optional, no breaking change to existing payloads):

```rust
// crates/core/src/walkthrough.rs — extend WalkthroughTarget::Diff and ::File
// without breaking older tours.
pub struct DiffFocus {
    /// Optional repo-relative file path the step is *about* inside the diff.
    pub file: Option<String>,
    /// Optional hunk index (0-based) inside that file.
    pub hunk: Option<u32>,
    /// Optional 1-based line number that wins over hunk when both are set.
    pub line: Option<u32>,
}
```

Wire-format: a single `focus` object on the diff/file target. Renderers ignore it when
absent. Falls out naturally from #126's index.

### 2. **Hunk-aware diff parser** (currently line-by-line classification only)

`DiffView.svelte` parses unified-diff text into 6 line kinds (`meta`, `header`, `add`,
`del`, `context`, `hunk`) and renders them flat. To pulse / scroll to a specific hunk
the parser has to also produce a structured tree:

```ts
interface DiffFile {
  oldPath: string;
  newPath: string;
  hunks: DiffHunk[];
}

interface DiffHunk {
  oldStart: number; oldLen: number;
  newStart: number; newLen: number;
  header: string;        // e.g. "@@ -10,4 +10,5 @@"
  startLine: number;     // index into the flat lines[] array (for scrollIntoView)
  lines: number;         // count of belonging lines
}
```

The flat-line render stays the way it is today (it's right for visual comparison);
the hunk index is a parallel structure for navigation. `WalkthroughView` consumes the
index to drive the rail; `DiffView` exposes it via `bind:` or an `on:parsed` event.

This refactor is **the natural first step** before any of the four issues lands a real
feature, because three of them (#125, #126, #127) want hunk-level addressability and
none have it today. **One small PR for the parser refactor, no UX change** — then the
four feature PRs each pull from it.

## Issue-by-issue notes

### #127 change compass — ship first

Why first: pure UI on existing data. `list_changes_since` already runs in the
walkthrough viewer's repo, file-status info is one fetch, the breadcrumb pulls from
the same module sidebar metadata `<App>` already holds.

Smallest first iteration:

- New `<TourCompass>` component, ~80 LOC, takes the active step + the most recently
  cached `ChangedFile[]` and renders three pills (module breadcrumb, changed/unchanged
  badge, file-progress dots) above the existing step header.
- Tour-step targets without git data / without a diff ref skip the compass entirely
  (no error UI — just don't render).
- Hover expansion + click-to-open are step 2.

### #126 animated diff focus rail — ship after the parser refactor

Sequence: parser refactor PR → rail PR.

- Rail PR adds `<DiffRail>` next to `<DiffView>`'s pre, one button per hunk.
- Tour step's optional `focus` metadata (see "shared infrastructure" above) drives an
  initial scroll-into-view + a brief CSS pulse on the targeted hunk.
- Click on rail entry = jump to hunk + announce. Same pulse.

Punt: animation library. CSS-only `@keyframes` covers the pulse cheaply.

### #125 before/after snapshots — gated on Cytoscape migration

The issue body itself flags this: *"Cytoscape renderer is a better fit than Mermaid
for animated graph classes"*. The #62 evaluation deferred Code-City for the same
reason (700 KB bundle hit) — the Cytoscape migration is its own decision and should
not be carried in on the back of a tour-UX feature.

Recommend: open a separate issue **"Migrate bean-graph from Mermaid to Cytoscape"**,
land that, then circle back here. In the meantime we can ship a *non-animated* before/
after fallback that just renders two static snapshots side-by-side, but the animated
version (the actual ask) waits.

### #124 quiz — ship last

Why last: largest surface. Needs a quiz-payload MCP schema, scoring storage decisions
(do we persist scores? if so, where? `.projectmind/quiz-results.json`?), question-type
plumbing (multiple-choice, file-pick, true/false), result-summary UI with "replay weak
steps" wiring back into `setWalkthroughStep`. Roughly the size of the original
walkthrough feature itself.

The biggest open design question is whether the quiz is **authored by the LLM that
ran the tour** or **generated on-demand at "Generate quiz" click**. The issue lists
both as options; before any code lands we should pick one — generated-on-demand is
the cheaper start because it doesn't widen the tour payload.

## Recommendation

1. **Spike PR**: refactor `DiffView`'s parser to expose the hunk index. No UX change.
2. **#127 PR**: `<TourCompass>` overlay. Lands the tour-step header redesign that #126
   and #124 will both want.
3. **#126 PR**: `<DiffRail>` + tour-step `focus` metadata. Reuses the index from step
   1.
4. **Cytoscape migration issue + PR**: separate work, owned by whoever picks it up.
5. **#125 PR**: animated before/after snapshots, after step 4.
6. **#124 PR**: quiz, last.

Steps 1–3 are mechanical and unblock most of the visible value. Steps 4–6 are bigger
and benefit from the tour-step renderer + hunk index settling first.

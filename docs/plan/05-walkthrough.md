# Walk-through — guided tours through code & docs

> Status: **proposal**, not yet implemented. Drafted 2026-04-29.

## Why

Today the LLM can pop a single file/class/diff into the GUI via `view_*`.
That's enough for *"show me X"* but useless for *"explain this PR"* or
*"introduce me to module Y"* — both of which want a **sequence** of stops
with narration: open file A line 12, then file B line 40, then this
markdown ADR, then this diff. Multi-step navigation is the missing
abstraction; everything else (rendering files, slugs, anchors) we already
have.

The deliverable is a **walk-through view**: an LLM-authored, human-paced
tour. Steps stay editable from the LLM side while the user is reading,
so it's just as useful for live demos as for canned PR write-ups.

## Shape of one tour

```jsonc
{
  "id": "wt-2026-04-29-md-viewer",     // stable handle, slug-friendly
  "title": "Markdown viewer rollout",  // shown in header + sidebar title
  "summary": "What changed and why",   // optional 1-paragraph intro
  "current": 0,                        // 0-based pointer
  "steps": [
    {
      "title": "ViewIntent::File now carries an anchor",
      "narration": "Optional `anchor` lets us scroll to a heading.\n\n…markdown…",
      "target": {
        "kind": "class",               // → ClassViewer
        "fqn": "plaintext_ide_core::state::ViewIntent",
        "highlight": [{ "from": 60, "to": 78 }]
      }
    },
    {
      "title": "Picker UI",
      "narration": "Header dropdown lists every .md in the repo.",
      "target": {
        "kind": "file",                // → FileView
        "path": "/abs/path/FileView.svelte",
        "anchor": "markdown-picker"    // optional
      }
    },
    {
      "title": "Architecture note",
      "narration": "",                 // optional
      "target": { "kind": "markdown", "path": "/abs/path/docs/SYNC.md" }
    },
    {
      "title": "Full diff",
      "narration": "Everything together.",
      "target": { "kind": "diff", "ref": "v1.0.0", "to": null }
    },
    {
      "title": "Just text",
      "narration": "Sometimes the LLM has nothing to point at.",
      "target": { "kind": "note" }    // narration is the whole step
    }
  ]
}
```

Step `kind`s map 1:1 to viewers we already have, plus a `note` kind
that's narration-only. New kinds (e.g. `html`, `image`) can land later
without breaking older tours — unknown kinds render as a placeholder
with the narration shown.

## Storage

A walk-through is bigger than `current.json` should hold; we keep it
side-by-side instead:

```
$XDG_CACHE_HOME/plaintext-ide/
  current.json           # existing UiState
  ui-heartbeat.json      # existing
  walkthrough.json       # ← new, see schema above
```

`UiState::view` gains one variant:

```rust
ViewIntent::Walkthrough { id: String, step: u32 }
```

The pointer is in the statefile (cheap to write), the body is in
`walkthrough.json` (written once, mutated by `walkthrough_*` tools).
That keeps the existing watcher → emit loop unchanged: every step
change is a `seq` bump on `current.json`, the GUI re-reads the body
when `id` changes.

## MCP tools (additive, no breakage)

| Tool | Body | Notes |
|---|---|---|
| `walkthrough_start` | `{ title, summary?, steps[] }` | Resets `walkthrough.json`, jumps to step 0, switches the GUI to the walk-through view. Returns the assigned `id`. |
| `walkthrough_append` | `{ step }` | Appends to `steps[]`. Useful when the LLM streams a tour as it's writing it. |
| `walkthrough_set_step` | `{ index }` | Move the pointer; clamped to `[0, len-1]`. Bumps `seq`. |
| `walkthrough_clear` | `{}` | Removes the body, returns the GUI to the previous view. |

These do *not* replace `view_*` — they layer on top. A walk-through
step's `target` is just one of the existing intents, and we render it
through the same components.

## GUI layout

New view-mode `'walkthrough'`. Switch happens automatically when an
`id` first appears (same pattern as `view_file`).

```
┌──────────────────────────────────────────────────────────────┐
│ header (existing nav, "Walk-through" tab while active)       │
├────────────────┬────────────────────────────────────────────┤
│ STEPS          │  ┌────────────────────────────────────┐    │
│ ▸ 1. Anchor    │  │   target render                    │    │
│   2. Picker    │  │   (ClassViewer / FileView /        │    │
│   3. ADR       │  │    DiffView / MarkdownView / note) │    │
│   4. Diff      │  └────────────────────────────────────┘    │
│   5. Note      │  ┌────────────────────────────────────┐    │
│                │  │   narration (markdown)             │    │
│ Prev | Next    │  └────────────────────────────────────┘    │
└────────────────┴────────────────────────────────────────────┘
```

- Left sidebar: ordered step list, click jumps, current is highlighted.
- Main pane: split horizontally. Top renders the target, bottom the
  narration. Ratio is dragged-resizable (default 65/35); collapse
  bottom when narration is empty.
- Footer: Prev / Next, plus "Step n/N" indicator. ←/→ keyboard.

When the user clicks a step manually we *do not* clear `followingMcp`
— the walk-through is co-authored. We do bump our own `seq` so the
LLM's MCP `walkthrough_set_step` calls and our manual clicks merge
naturally; the LLM can read `current_state` to see where the user is.

## Edge cases worth thinking about

- **Stale targets** — a step references a file that no longer exists,
  or a class FQN that vanished after a rename. Render a placeholder
  (just the narration + a "missing target" hint) instead of erroring.
- **Dangling tours** — `walkthrough.json` left from a previous session
  when the GUI starts cold. Same policy as today's `current.json`: if
  the tour's `repo_root` doesn't exist, ignore it silently.
- **Concurrent edits** — LLM appends step 6 while the user is reading
  step 3. The pointer must not jump. `append` never moves `current`;
  only `set_step` and `start` do.
- **Size cap** — narration is markdown and could get unbounded. Cap
  per-step narration at e.g. 64 KB and the whole tour at e.g. 2 MB,
  reject larger payloads at the MCP boundary.

## Out of scope (for v1)

- Branching tours / "choose your own adventure".
- Per-user state (where the user paused). Tours are ephemeral.
- Animations / scroll-tied narration. Static target + narration is
  enough for the "explain this PR" use case.
- Authoring UI for humans — humans will write JSON or use the LLM.

## Implementation order

1. **Core**: `ViewIntent::Walkthrough` variant + `walkthrough.rs`
   module (read/write `walkthrough.json`, atomic write like statefile).
2. **MCP**: `walkthrough_*` tools, schema & dispatch.
3. **Tauri shell**: `walkthrough.json` watcher (analogous to the
   statefile watcher), Tauri commands `current_walkthrough()` and
   `walkthrough_set_step(index)` (so manual clicks bump `seq`).
4. **Frontend**: `WalkthroughView.svelte` with the split layout, a
   dispatcher that renders `target` through the existing viewers.
5. **Docs**: update `README.md` with one example (`walkthrough_start`
   call → screenshot of the resulting tour).

Each step is independently shippable. (1) + (2) alone gives an MCP-
only walk-through (you can poke the body with curl-like JSON-RPC even
without GUI work); (3) + (4) wire up the GUI.

## Open questions

- Should `narration` support the same anchor sugar as `view_file`
  (i.e. fenced links to `[step:3]`)? Probably yes, but defer to v1.1
  once we know what the LLM actually writes.
- Do we want a "tour over" terminal step that's just a celebratory
  card? Cosmetic; skip for now.
- How do we surface that a tour exists when the user wasn't watching
  the GUI? A toast on `walkthrough_start`, maybe; revisit after we
  build the heartbeat-based auto-launch.

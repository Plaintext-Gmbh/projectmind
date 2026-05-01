# MCP ↔ GUI sync

How the `projectmind-mcp` server (driven by the LLM) and the Tauri GUI
(driven by the user) stay in sync.

## Goal

> The user is in the GUI, an LLM drives MCP. When the LLM says "open repo X /
> show me class Y / render this README / show your changes", the GUI follows
> live. **MCP has precedence**: anything the LLM does wins over the human's
> recent navigation, until the human takes a new action.

## Mechanism — a single shared state file

Both processes read and write one JSON file:

| Source                       | Path                                            |
| ---------------------------- | ----------------------------------------------- |
| Linux / others (`XDG_CACHE_HOME`) | `~/.cache/projectmind/current.json`       |
| macOS                        | `~/Library/Caches/projectmind/current.json`   |
| Override                     | `$PROJECTMIND_STATE`                          |

Schema (`crates/core/src/state.rs`):

```jsonc
{
  "version": 1,
  "repo_root": "/path/to/repo",
  "view": {
    "kind": "classes",                  // or "diagram" | "diff" | "file"
    "selected_fqn": "com.example.UserService"
  },
  "seq": 42                              // monotonic; bumped on every write
}
```

Writes are **atomic** (write to `current.json.tmp`, then rename) so a watcher
never reads a half-written document.

```
   ┌─ MCP server ────────────┐    ┌─ Tauri GUI ─────────────────┐
   │ open_repo / view_class  │    │ notify watcher              │
   │ view_file / view_diff   ├───▶│ → emit("state-changed")     │
   │ view_diagram            │    │ → frontend applies intent   │
   │ → state::write()        │    │                             │
   │                         │    │ user picks repo manually    │
   │                         │◀───│ → state::write()            │
   └─────────────────────────┘    └─────────────────────────────┘
                       ~/.cache/projectmind/current.json
```

## Why a file (and not a socket)

| Why                                              |
| ------------------------------------------------ |
| MCP must work without the GUI running.           |
| No port discovery, no firewall popups on macOS.  |
| Atomic write + sequence number = trivial robustness. |
| Either side can be restarted independently.      |

The cost is one extra parse: when MCP opens a repo, both processes parse it
(MCP for its `Engine`, GUI for its own). For Phase 1 this is acceptable.

## Lifecycle

* **Statefile missing** → both sides treat it as "no state". GUI shows the
  welcome screen; MCP works fine.
* **GUI starts** → reads `current_state` once, applies the intent, then
  subscribes to `state-changed` events.
* **MCP writes** → file system event → GUI reads, drops the event if
  `seq <= last_seq` (idempotent), otherwise dispatches.
* **GUI writes** → after `open_repo` (manual repo picker). The MCP server's
  next `view_*` call sees the updated `repo_root`.
* **Both write the same `seq`** → impossible by construction: `seq` is bumped
  by an atomic counter that is initialised from the on-disk value.
* **GUI is closed** → MCP keeps writing. Next time GUI opens it picks up the
  latest state on startup.

## View intents

A `ViewIntent` is a tagged union. Each variant is one of the navigation
states the GUI can be in. Adding a new view = adding one variant + one
case in the frontend dispatcher.

| Variant                              | What the GUI shows                  |
| ------------------------------------ | ----------------------------------- |
| `classes { selected_fqn? }`          | Classes browser, optional selection |
| `diagram { diagram_kind }`           | Bean graph or package tree          |
| `diff { reference, to? }`            | Unified diff (line-coloured)        |
| `file { path }`                      | Markdown render or plain source     |

## MCP tools that drive the GUI

| Tool             | Effect                                                                |
| ---------------- | --------------------------------------------------------------------- |
| `open_repo`      | Loads the repo *and* publishes `repo_root` so the GUI follows.        |
| `view_class`     | `view = classes(selected_fqn)`. Validates the FQN against the loaded repo. |
| `view_diagram`   | `view = diagram(bean-graph | package-tree)`.                          |
| `view_diff`      | `view = diff(ref, to?)`. The GUI renders via `show_diff`.             |
| `view_file`      | `view = file(absolute path)`. Markdown is rendered, anything else as plain text. |

Existing read-only tools (`list_classes`, `class_outline`, `show_class`,
`show_diff`, `show_diagram`, `relations`, …) do **not** publish state — they
just return data. This keeps "read" and "navigate" cleanly separated.

## Markdown rendering

`view_file` on a `.md` / `.markdown` / `.mdx` file goes through the
`FileView` component:

1. Tauri command `read_file_text(path)` returns the raw text (capped at 10 MB).
2. [`marked`](https://marked.js.org/) parses GFM markdown to HTML.
3. The component walks the rendered DOM:
   * `<img src="…">` with relative paths is rewritten to
     `convertFileSrc(absolute)` so the Tauri asset protocol (`assetProtocol`
     in `tauri.conf.json`) serves the local file.
   * `<pre><code class="language-mermaid">…</code></pre>` blocks are replaced
     with the rendered SVG via the same `mermaid` instance the diagram view
     uses.
4. Non-markdown extensions render the file content inside a monospace `<pre>`.

## Diff rendering

`view_diff` triggers `DiffView`, which calls the Tauri `show_diff` command
(`crates/core/src/git.rs::unified_diff`) and colours each line by its prefix
(`@@`, `+`, `-`, file headers). No external diff library — the format is
already a parseable line stream.

## Precedence rules

* **MCP > GUI**. Every `state-changed` event the GUI receives is applied,
  even if it overrides the user's most recent click. The header shows a
  "following MCP" badge while this is the case.
* The user reasserts control by clicking any tab in the header — that clears
  the badge but does **not** stop future MCP events from winning. The next
  `view_*` puts the GUI back into "following" mode.

## Failure modes (and what they look like)

| Failure                                  | Behaviour                                                          |
| ---------------------------------------- | ------------------------------------------------------------------ |
| Cache directory not writable             | MCP logs a warning, continues serving read tools normally.         |
| Statefile contains invalid JSON          | Read returns a hard error; GUI logs and ignores until next write.  |
| Watcher fails to start (e.g. bad path)   | GUI logs a warning and falls back to the initial `current_state`. |
| `view_class` for an unknown FQN          | MCP returns `invalid_params`; GUI is not touched.                  |
| `view_file` for a non-absolute path      | MCP returns `invalid_params`.                                      |
| Repo reload race (two repos in flight)   | The later `seq` wins. Filters/selection are reset on every load.   |

## What's intentionally *not* here

* **No two-way data binding** — the GUI's diagram zoom / scroll is local
  state. Only navigation crosses the bridge.
* **No live-collaboration** — the statefile is per-user, single-machine. A
  multi-host design would need a daemon (Phase 2 territory).
* **No diff between two intents** — every write is a *replace*. The GUI does
  not try to compute a delta.

## Quick demo flow

With Claude Code wired to `projectmind-mcp`:

```
You: Open ~/projects/example-app, then show me UserService.
LLM: → open_repo  → view_class("com.example.UserService")
GUI: jumps to example-app, switches to classes view, opens UserService.

You: Show me the README.
LLM: → view_file("/…/example-app/README.md")
GUI: switches to file view, renders markdown with embedded mermaid.

You: What did I change since main?
LLM: → view_diff("main")
GUI: switches to diff view, colours add/del lines.
```

## File-by-file map

| File                                              | Role                                            |
| ------------------------------------------------- | ----------------------------------------------- |
| `crates/core/src/state.rs`                        | Schema, atomic read/write, sequence numbers.    |
| `crates/mcp-server/src/tools.rs`                  | `open_repo` + `view_*` tools call `state::write`. |
| `app/src-tauri/src/lib.rs`                        | `notify` watcher → `emit("state-changed")`; commands `current_state`, `read_file_text`, `show_diff`. |
| `app/src/lib/store.ts`                            | `viewMode`, `fileViewPath`, `diffViewRef`, `followingMcp`. |
| `app/src/App.svelte`                              | Listener + intent dispatcher, "following MCP" badge. |
| `app/src/components/FileView.svelte`              | Markdown / plain file renderer.                 |
| `app/src/components/DiffView.svelte`              | Unified diff renderer.                          |
| `app/src-tauri/tauri.conf.json`                   | `assetProtocol` enabled for embedded images.    |

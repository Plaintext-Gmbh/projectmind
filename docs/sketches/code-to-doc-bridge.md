# Code ↔ doc bridge — phasing & sketch

> Scope of [#65](https://github.com/Plaintext-Gmbh/projectmind/issues/65). The doc-graph
> half (Cytoscape candidate) is shipped — Markdown links between repo docs render today
> in the Diagrams tab as `doc-graph` (`network` / `radial` / `orphans` layouts), backed
> by `crates/core/src/doc_graph.rs`. This sketch covers the still-open half: a side-bar
> in the code view that surfaces "documents that talk about this class".

## Status of #65 candidates

| Concept | State |
|---|---|
| Doc graph (Markdown ↔ Markdown) | ✅ shipped — `doc-graph` diagram kind, MCP `show_diagram` route, `<DocGraph>` Svelte renderer |
| Code ↔ doc bridge — in-repo Markdown only (Phase A, this sketch) | proposed |
| Code ↔ doc bridge — Confluence / Jira (Phase B) | blocked on Confluence MCP bridge (Phase 2) |

The original issue conflated the Confluence variant with the in-repo variant. They have
very different costs: the in-repo variant reuses the existing Markdown index plus the
class symbol table the language plugins already produce; the Confluence variant needs an
authenticated MCP server, OAuth flow, page caching, attachment handling. **Phase A
unblocks the user-facing UI surface; Phase B fills it from a richer source later.**

## Phase A — in-repo code-to-doc index

### What the user sees

A `<CodeDocLinks>` side-panel in the class viewer, hidden by default until the open
class has at least one mention. When present, it lists Markdown documents that mention
the class, with the section heading nearest the mention as the link label:

```
┌─ class UserController ─────────┐  ┌─ Mentioned in ─────────────┐
│  package controller            │  │  docs/architecture.md      │
│  @RestController               │  │     § Request flow         │
│  public class UserController { │  │  docs/onboarding.md        │
│      …                         │  │     § Auth boundary        │
│  }                             │  │  README.md                 │
│                                │  │     § What works today     │
└────────────────────────────────┘  └────────────────────────────┘
```

Click → opens the Markdown tab at the linked anchor. Single-source-of-truth: no
duplicated rendering, no "rich preview", just navigation. The MD tab already does the
hard work.

### Backend

A new MCP tool, `code_doc_mentions`, exposing one function:

```rust
pub fn mentions_for_class(
    repo: &Repository,
    docs: &DocGraph,
    fqn: &str,
) -> Vec<DocMention>;

pub struct DocMention {
    pub doc_id: String,        // doc-graph node id (repo-relative path)
    pub doc_title: String,     // for the side-bar header
    pub anchor: Option<String>,// nearest preceding heading slug
    pub line: u32,             // 1-based, for callers that want to deep-link
    pub kind: MentionKind,     // Fqn | SimpleName | Stereotype
}

pub enum MentionKind { Fqn, SimpleName, Stereotype }
```

The match strategy is intentionally cheap and explicit:

1. **Exact FQN** (`com.example.UserController`) — strong signal, never a false positive
   on a real codebase.
2. **Simple name in code-fence or backticks** (`` `UserController` ``) — matches what
   developers actually write. Skip plain-text occurrences to avoid false positives on
   common nouns.
3. **Stereotype tag** (`@RestController`, `@Service`) — matches discussions of the
   stereotype rather than a specific class; keep this off by default and let the user
   toggle it on.

The implementation lives in `crates/core/src/code_doc_mentions.rs` and is essentially a
folded version of `doc_graph.rs`'s scan loop:

- Walk Markdown files (reuse `files::list_markdown_files`).
- For each file, build a one-pass index: heading slug → line range, fenced-code spans,
  backtick spans.
- For each FQN in the open `Repository`, scan once and emit a `DocMention` per hit
  classified by which span kind it sits in.

Cost: O(N_docs · L_doc + N_classes · M) where `M` is the per-file match overhead. On a
repo with ~200 markdown files and ~3 000 classes that's still a sub-second one-shot
build. Cache invalidation is trivial: rebuild on `open_repo` and on filesystem
notifications inside `<repo>/**/*.md`. Same cadence as the doc-graph today.

### Frontend wiring

A new method `mentionsForClass(fqn)` in `app/src/lib/api.ts`. The class viewer (`<ClassViewer>`)
calls it on `selectedClass` change, throttled. The result feeds a tiny Svelte component
that re-uses the Markdown tab's anchor-link routing.

### Why not just regex over file contents?

Two reasons.

- **Anchor support.** The user wants the link to drop them at the nearest heading.
  Building anchors lazily from raw regex matches works but quickly becomes a pile of
  whitespace heuristics; consuming the existing `markdown_links` style scanner gives us
  the slugs Mermaid / GFM already standardised on.
- **Code-fence detection.** Plain regex would over-match — every README that talks about
  *"the User class"* would tag every `User` class in the repo. Even the simple
  fence/backtick filter cuts the false-positive rate to roughly zero on the repos we've
  inspected.

## Phase B — Confluence / Jira (deferred)

The Phase B variant needs a Confluence MCP server we don't yet ship, plus:

- OAuth flow (delegated to the MCP server itself — `projectmind` should never see
  Confluence credentials).
- Per-class index of "pages that mention this class". Confluence search gets us 80% of
  the way there but is rate-limited and lossy; an opt-in nightly crawl into a local
  cache is the realistic shape.
- A "section preview" in the side-bar (Confluence pages don't navigate as cleanly as
  local Markdown). This is the one place the code-to-doc bridge actually wants
  rich-content rendering.

That's a substantial chunk of work, and most of the user value is captured by Phase A
because internal Markdown is where ~90% of architecture docs live for the repos
`projectmind` was designed for. **Land Phase A, ship the side-panel surface, then layer
Confluence / Jira sources behind the same UI when the bridge MCP exists.**

## Recommendation

1. Land **Phase A** as one PR per layer (backend tool, frontend panel) — this gives
   the issue a concrete completion path that doesn't depend on Phase 2 work.
2. Keep #65 open until Phase A ships; add a comment when the Confluence bridge work
   starts so the side-panel can grow a "source: confluence" facet without churning the
   Phase A code.
3. Do **not** implement Phase B until the Confluence MCP server lands and at least one
   real repo wants it.

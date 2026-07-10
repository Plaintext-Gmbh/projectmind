# Walkthrough Query — semantic tour lookup

> **Status:** shipped in Cockpit Phase 2.5 ([#161](https://github.com/Plaintext-Gmbh/projectmind/issues/161)). Part of the Architect's Cockpit vision — makes curated walk-throughs the AI's *preferred* code-navigation interface.

An architect builds a walk-through **once** ("auth flow", "request lifecycle", "data-model evolution"). Every future AI session then asks a question and gets the relevant tour steps back instead of grepping the repo. A curated tour is a **reduced model** of the codebase:

- 10-50 lines per step × ~10 steps = ~500 lines to "explain X"
- versus reading 8-20 files = 5000-20000 lines

That is a **10-20× token saving** on the most common navigation task.

## The MCP tool

```jsonc
walkthrough_query({
  question: string,          // "how does login work"
  prefer_tours?: string[],   // optional: bias toward these tour ids
  top_k?: number             // optional: max steps to return (default 8)
}) →
{
  tour_id?: string,          // the winning tour
  steps: [{ title, fqn?, file?, lines?, narration, score }],
  confidence: 0.0..1.0,      // max similarity found
  fallback?: "grep"          // present when there is no good tour answer
}
```

**Try this first**, before `list_classes` / `find_class` / grep. When `fallback` is `"grep"` there is no good tour answer — fall back to the normal search/read tools.

`fallback: "grep"` is returned when any of these hold:

- no tour is indexed for the repo,
- the best match scores below the **0.45** similarity threshold, or
- the MCP server was built **without the `embed` feature** (see below), so there is no semantic index at all.

The tool is wired identically across all three surfaces — MCP (`walkthrough_query`), the Tauri command, and the browser-host route `GET /api/walkthrough_query` — so the same answer is available to the LLM and to the webapp.

## How it works

1. **Indexing (at `open_repo`).** For every tour step, the searchable text is `title + narration + target fqn`. Those strings are embedded into fixed-width vectors and persisted to `.projectmind/cache/tours.idx` (a versioned, `dim`-stamped JSON file). A format-version or embedding-dimension mismatch triggers a transparent rebuild — the cache is disposable, like the code-graph cache.
2. **Querying.** The question is embedded, cosine-ranked against every step vector, grouped by tour, and the best tour's top-`k` steps are returned in tour order, each with its similarity `score`. `class`-target steps are enriched with their `file` + `lines` from the open repo so you get a jump target without a second round-trip.
3. **Fallback.** Below the 0.45 threshold the answer is `fallback: "grep"`.

Implementation: [`projectmind_core::tour_index`](../crates/core/src/tour_index.rs) (the pure ranking core, written against an `Embedder` trait) and [`projectmind_core::tour_embed`](../crates/core/src/tour_embed.rs) (embedder selection).

### The `Embedder` seam

The whole ranking core is written against a single trait and is **pure** — no model, no I/O, no global state:

```rust
pub trait Embedder {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn dim(&self) -> usize;
}
```

That makes the ranking deterministically unit-testable with an injected fake embedder (bag-of-words), so the **default test suite never needs a model or the network**. The real embedder lives behind the `embed` cargo feature.

### The `embed` feature

The local embedding model (`fastembed`, `all-MiniLM-L6-v2`, 384-dim, ~30MB) is compiled in **only** with the `embed` feature:

```bash
cargo build -p projectmind-mcp --features projectmind-core/embed
```

- **Without `embed`** (the default): the MCP server binary stays slim and `walkthrough_query` always returns `fallback: "grep"`. Nothing crashes; the semantic index is simply absent.
- **With `embed`**: the model is fetched once (via `hf-hub`, `rustls` TLS) and cached; the ONNX runtime binary is downloaded at build time by `ort`. If the model can't be loaded (offline, no cache) the query path degrades to `fallback: "grep"` rather than panicking.

Because `ort` downloads a runtime binary at build time, the `embed` feature is verified in CI, not in every offline dev sandbox.

## Authoring tours that answer well

Semantic search matches on `title + narration + target fqn`. To make a tour a good answer target:

- **Write the narration in the words a future asker would use.** "The user submits credentials and the login controller validates the password and issues a session token" matches "how does login work" far better than "entry point #1". Describe the *concept*, not just the mechanics.
- **Front-load domain vocabulary in the step title.** Titles are short and weigh heavily. Prefer "Password verification & session issuance" over "Step 2".
- **Point steps at concrete code.** `class` and `risk` steps carry a `fqn` (added to the searchable text and resolved to `file`/`lines` in the answer). Narration-only `note` steps still match on their text but return no jump target — use them for framing, not as the primary answer to a "where is X" question.
- **One idea per step.** A step that tries to cover three subsystems dilutes its own vector; split it. Ten focused steps rank better than three sprawling ones.
- **Name tours by the question they answer.** "Authentication flow", "Request lifecycle", "How invoices are generated" — a good tour id/title is itself a strong match signal and a natural `prefer_tours` value.
- **Avoid pure boilerplate narration.** "This class does stuff" adds noise, not signal. If a step has no title, no narration and no fqn it is skipped at index time entirely.

## Cache & reset

| Action | Effect |
|---|---|
| Delete `<repo>/.projectmind/cache/tours.idx` | rebuilt on the next `open_repo` / query. Safe — disposable. |
| Swap the embedding model (`dim` changes) | the old index is treated as stale and rebuilt automatically. |
| Author / edit a tour | the index rebuilds lazily on the next query (or eagerly on the next `open_repo`). |

The index lives under `.projectmind/cache/` — the same repo-local cache area described in [persistence.md](persistence.md). It carries no user data, so deleting it is always safe.

# Timeline river — sketch (#63)

> Concept: a horizontal time axis where each module is a band; commits land
> as drops in their band so the user can see *"the auth module went quiet six
> months ago"* in one glance.
>
> Concept owner: ProjectMind core, on top of the existing `git2::Repository`
> wrapper. Question this answers: *"how has activity moved through the
> codebase over time?"*
> Candidate library: pure SVG. No new bundle weight.

## TL;DR

Timeline river is the last open concept under [#63](https://github.com/Plaintext-Gmbh/projectmind/issues/63)
("Change maps"). The four shipped concepts (file-recency endpoint, recency
heatmap, author overlay, diff overlay) all answered "what does *now* look
like?". Timeline river answers the orthogonal question — *"how did we get
here?"* — and reuses the same `file_recency` walk plus the parsed
`Repository::modules` to bucket commits by `(module, week)`.

This sketch parks the design with enough detail that the implementation can
proceed in two PRs:

1. **Backend bucket endpoint** (S, ~half a day). New
   `git::commit_buckets(repo_root, opts)` returning week-bucketed commit
   counts per repo-relative path. Tests cover the bucket math, the
   newest-first cap, and root-commit handling.
2. **Frontend `commit-river` diagram kind** (M, ~1-2 days). New `<TimelineRiver>`
   Svelte component, registered as a diagram kind in `engine.available_diagrams`
   and routed through `show_diagram` like `folder-map` (JSON payload, not
   Mermaid).

## What the user sees

```
  ┌─ Timeline river — last 12 weeks ────────────────────────────────────────┐
  │  module           12w ago     8w ago     4w ago     this week           │
  │  ─────            │           │          │          │                   │
  │  api              ●●●  ●  ●  ●●●  ●●●  ●●●●●  ●●●●●●●●●     ← peak     │
  │  core             ●●  ●●●  ●●●  ●●  ●  ●  ●  ●●●  ●●  ●                 │
  │  framework-spring ●  •  •  ●  ●  ●  ●  ●  ●  ●●  ●●  ●●                 │
  │  lang-java        ●●●  ●●●  ●●  ●●  ●  ●  ●  ●  ●  ●  ●●                │
  │  plugins/lang-rust ●●  ●  ●  ●●  ●●  ●●●●  ●●●  ●●  ●                   │
  │  app                                            ●●  ●●●●  ●●●●          │
  │                                                                          │
  │  Hover  → commit messages for that bucket                                │
  │  Click  → drill into the diff range that that bucket covers              │
  └──────────────────────────────────────────────────────────────────────────┘
```

- **One band per module**, ordered by total commit volume so the busy modules
  rise to the top.
- **Dot size** maps to commits in that bucket (clamped: 1 commit = 4 px,
  10+ commits = 14 px), so the eye reads peaks without a legend.
- **Sparkline-style alpha**: buckets older than the active range fade to
  ~30% so the recent-history hotspots dominate without trimming history.
- **Time axis** is logarithmic-ish: weekly buckets for the last 12 weeks,
  monthly buckets beyond that, yearly summaries past two years. Defaults to
  "last year" range; the toolbar offers `1m / 3m / 1y / all`.

## Data shape

The frontend gets a single JSON payload (no Mermaid string), shaped after
the existing `folder-map` payload:

```json
{
  "buckets": ["2025-50", "2025-51", "2025-52", "2026-01", "..."],
  "modules": [
    {
      "id": "g:api",
      "label": "api",
      "counts": [4, 7, 12, 18, 9, 3, 1, 0, 2, 5, 11, 22],
      "examples": [
        { "bucket": "2026-01", "sha": "a1b2c3d", "summary": "feat(api): batch endpoint" }
      ]
    }
  ],
  "first_bucket": 1734393600,
  "last_bucket": 1736899200,
  "max_count": 22,
  "truncated": false
}
```

`examples` carries one or two representative commits per bucket so hover
tooltips can show *what* happened, not just *how many*. We cap that list
at three per `(module, bucket)` pair and at 200 entries total — enough to
populate the visible buckets without bloating the payload.

## Backend: `commit_buckets`

Lives in `crates/core/src/git.rs` next to `file_recency`. Same revwalk
machinery, different aggregation:

```rust
pub struct CommitBucketsOpts {
    pub bucket_secs: i64,        // 7*86_400 = weekly default
    pub since_secs: Option<i64>, // cap walk at this lower bound; None = full history
    pub max_commits: usize,      // safety cap, default 5_000
}

pub struct PathBucket {
    pub path: PathBuf,            // repo-relative
    pub bucket_start_secs: i64,
    pub commit_count: u32,
    pub examples: Vec<BucketExample>,
}

pub fn commit_buckets(repo_root: &Path, opts: &CommitBucketsOpts)
    -> Result<Vec<PathBucket>, GitError>;
```

Module aggregation lives in the diagram renderer (next section), not in
`git`, so the backend stays generic — a future "team activity heatmap"
could reuse the same per-path output.

The walk is the same as `file_recency` (revwalk from HEAD, paths from
`diff_tree_to_tree`), but instead of "first-write-wins per path" we
increment a `BTreeMap<(PathBuf, bucket_start), CommitCount>` for every
touched path. Cost is O(commits × paths-per-commit) — well-bounded by
the existing `RECENCY_MAX_COMMITS` style cap.

## Module aggregation

`crates/core/src/diagram.rs` gets `render_commit_river(repo, framework)`
that:

1. Calls `git::commit_buckets(&repo.root, default_opts())`.
2. Maps each path → owning module via the `path.starts_with(module.root)`
   check that the C4 container view (#62) already uses.
3. Folds `PathBucket` rows into per-module `(bucket → count)` rows.
4. Serialises to the JSON shape above.

The frontend then renders without needing to know about `Module`s — same
contract as `folder-map`.

## Frontend rendering

`<DiagramView>` already has a JSON-payload branch (`folder-map`,
`doc-graph`). Add a new branch for `commit-river`:

- Pure SVG `<g>` per module, x-position = bucket index → CSS pixels,
  y-position = module band index → CSS pixels.
- Dots are `<circle>` with `r = clamp(count, 4, 14)`.
- Hover shows the `examples[*]` for that bucket as a tooltip; click opens
  the diff between `bucket_start` and `bucket_start + bucket_secs` via
  the existing `pm:diff` viewer.
- Reuse the pan/zoom behaviour the folder map already has (drag horizontal
  axis, wheel zoom on time scale).

Estimated frontend LOC: ~250 (rendering + tooltip + click handling +
1-2 vitest specs for the time-bucket math).

## Why this and not Code-City / dependency-wheel first

Code-City and the dependency wheel both visualise the *current* shape of
the codebase. Timeline river is the only concept under #63 that surfaces
*temporal* information the user can't already get from `git log` without
spending hours.

The recency heatmap (already shipped) gives the same data at a single
point in time. Timeline river gives the trend — "auth has been quiet for
six months" is a different signal than "auth was last touched six months
ago" because the second statement could be true on a still-active module
that just had a six-month lull.

## Trade-offs

- ✅ Reuses the proven `git2::Repository` revwalk pattern from `file_recency`.
- ✅ One JSON payload, no Mermaid runtime — pan/zoom is cheap.
- ✅ Slot fits the existing diagram-kind enum + sidebar (next to `folder-map`,
  `doc-graph`).
- ⚠️  Bucket math has edge cases (DST, leap weeks, repos that span timezones).
   Recommend ISO 8601 week numbers (`%G-%V`) so the display is timezone-
   independent; spec that explicitly when the implementation lands.
- ⚠️  10k-commit repos generate big payloads. Cap at 5 000 commits in the
   default opts; users who need more can pass `since_secs` for a narrower
   window.
- ❌ Single-module repos (e.g. ProjectMind itself before the recent split)
   degenerate to a single-band view — useful but not the "river" the name
   promises. The toolbar should auto-select the per-author colouring in
   that case so the band still has internal structure.

## Out of scope for this sketch

- Per-author tinting of dots (already covered by the author overlay layer
  — combining the two would be a follow-up issue once both ship).
- Animated playback ("press play and watch the architecture morph") —
  that lives under #66 ("Diff cinematics"), not here.
- Cross-repo timelines.

## Recommendation

Promote to a real implementation issue, split into the two PRs from the
TL;DR. The first PR (`commit_buckets`) is independently useful even
without a frontend — the MCP `show_diagram(commit-river)` can return the
JSON payload directly and a Claude-Code session can already reason about
"when did the team last touch this module?" without a UI rendering it.

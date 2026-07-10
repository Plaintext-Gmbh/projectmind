# Auto-narrated Tour — `tour_scaffold`

> **Status:** shipped in Viz V4.4 ([#201](https://github.com/Plaintext-Gmbh/projectmind/issues/201)). Part of the "living, wow" visualization track ([#66](https://github.com/Plaintext-Gmbh/projectmind/issues/66)).

## Why a *scaffold*, not a tour generator

ProjectMind is the MCP **server**; the LLM is the **client**. There is no
LLM-call path inside the repo and V4.4 deliberately does **not** add one — that
would be an architecture break. Instead, `tour_scaffold` returns the *machine
skeleton* of a "welcome to this repo" walk-through:

- a **ranking** of the modules worth touring, each with the raw numbers and a
  set of human-readable **facts** bullets, and
- ready-made **suggested steps** whose `target`s already point at the right
  class / atlas view.

**You (the client) write the narration** in your own words — one idea per step,
in the asker's language, per the guidance in
[`walkthrough-query.md`](./walkthrough-query.md) — and then call the existing
`walkthrough_start`. The whole Cockpit stack (GUI, browser viewer, presenter,
TTS, PDF export) renders the result with **no frontend change**.

## The MCP tool

```jsonc
tour_scaffold({
  top?: number,          // how many modules to rank (default 5, min 1)
  persona?: "new-dev" | "architect",   // framing only; ranking is identical
  materialize?: boolean  // default false — see "Materialize mode" below
}) →
{
  repo: { title, modules_total, classes_total },
  ranking: [{
    module, score,               // score is the blended 0..1 rank
    classes, fan_in, fan_out,    // module-level coupling + size
    commits_90d,                 // activity signal
    top_class: { fqn, risk_score, fan_in, why } | null,
    facts: [string, …]           // narrate these verbatim or paraphrase
  }, …],
  suggested_steps: [
    { title: "Overview",            target: { kind: "note" },  facts: […] },
    { title: "<module>",            target: { kind: "class", fqn },  facts: […] },
    …,
    { title: "Where change happens", target: { kind: "atlas", highlight: [fqn…] }, facts: […] }
  ]
}
```

## The client flow

1. `open_repo({ path })`.
2. `tour_scaffold({ top: 5 })`.
3. For each `suggested_steps[i]`, write a short `narration` from its `facts`
   (one idea per step, the asker's words).
4. `walkthrough_start({ title, summary, steps })` with your narrated steps,
   translating each scaffold `target` into the matching `WalkthroughTarget`
   (`note` → `Note`, `class` → `Class { fqn }`, `atlas` → `Atlas { highlight_fqns }`).

You are free to drop, reorder, merge or add steps — the scaffold is a starting
point, not a contract.

## The ranking

Each module gets a blended score from three signals, **min-max normalised
across the repo's modules** so the numbers are comparable:

| signal    | source                                              | default weight |
|-----------|-----------------------------------------------------|:--------------:|
| coupling  | cross-module edges `incoming + outgoing` ([`module_chord`](../crates/core/src/module_chord.rs)) | **0.4** |
| size      | class count of the module                           | **0.3** |
| activity  | commits in the trailing 90-day window ([`git::commit_activity`](../crates/core/src/git.rs)) | **0.3** |

`score = 0.4·norm(coupling) + 0.3·norm(classes) + 0.3·norm(commits_90d)`

Modules are sorted by score descending (ties broken on module id for a stable
order) and truncated to `top`. Each module's `top_class` is its
**highest-`fan_in`** class from the risk atlas ([`risk::compute`](../crates/core/src/risk.rs))
— the one the most other classes depend on; ties break on risk score, then fqn.

The ranking is a **pure, deterministic** function of the repo (see
[`tour_suggest.rs`](../crates/core/src/tour_suggest.rs)) — the same repo always
yields the same order. `persona` only changes the Overview framing sentence.

An empty or non-git repo degrades gracefully: an empty `ranking` and a lone
Overview + closing step, never an error.

## Materialize mode (offline / TTS, no client LLM)

Set `materialize: true` and the server builds the walkthrough itself, filling
each step's narration from a **template** (the `facts` bullets joined as a
markdown list), persists it exactly the way `walkthrough_start` does, points the
viewers at step 0, and returns the generated `walkthrough_id`:

```jsonc
tour_scaffold({ top: 5, materialize: true }) →
{
  materialized: true,
  walkthrough_id: "tour-projectmind-1720…",
  total: 7,
  scaffold: { … }               // the same scaffold, for reference
}
```

Use this for a `projectmind` CLI demo or an offline TTS playback where no
client LLM is available to write prose. The materialized steps are ordinary
walk-through steps — refine any of them afterwards with `walkthrough_append` /
`walkthrough_set_step`, or clear with `walkthrough_clear`.

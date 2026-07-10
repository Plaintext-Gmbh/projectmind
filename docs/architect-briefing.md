# Architect Briefing — daily delta / morning summary

> **Status:** shipped in Cockpit Phase 2.7 ([#163](https://github.com/Plaintext-Gmbh/projectmind/issues/163)). The final phase of the Architect's Cockpit epic ([#164](https://github.com/Plaintext-Gmbh/projectmind/issues/164)) — all seven phases (2.1–2.7) are now implemented.

The Risk Atlas answers "where is the risk **right now**?". The briefing answers the proactive question: **"what got worse since I last looked?"** Run it first thing in a session (or from a cron job) to decide what to worry about before reading a single line of source.

## The MCP tool

```jsonc
architect_briefing({
  since?: "last_session" | "1d" | "7d" | "<iso8601>" | "<unix-seconds>"
}) →
{
  new_hotspots: [{ fqn, score_now, score_then, delta, reason }],
  pattern_drift: [{ pattern, count_then, count_now, delta }],
  risk_delta: { up: [{ fqn, delta }], down: [{ fqn, delta }] },
  session_window: { from: <unix>, to: <unix> },
  coverage_comparable: boolean,   // true only when tests ran in BOTH sessions
  note?: string                   // set (with no window) when there is no baseline
}
```

The tool returns both the JSON payload above and a ready-to-paste **Markdown** summary (`markdown` field) for chat embedding.

`since` picks the baseline to diff the *current* session against:

- `last_session` (default) — the most recent prior `open_repo`.
- `1d` / `7d` — the most recent session at or before *now − N days*.
- an ISO-8601 timestamp (`2026-07-01T00:00:00Z`) or bare Unix seconds — the most recent session at or before that instant.

When the log is younger than the requested window, the briefing falls back to the **oldest** recorded session so a young log still gives *some* baseline rather than an empty answer. With only one session on record (or none), the briefing is empty and carries a `note` explaining why.

## The session log

Every `open_repo` — in all three hosts (MCP server, Tauri desktop shell, browser host) — appends one line to:

```
<repo>/.projectmind/state/sessions.jsonl
```

Each line is a compact health snapshot:

| field | meaning |
|---|---|
| `ts` | Unix seconds when the session was recorded |
| `atlas_hash` | hash over `(fqn, rounded-score)` of the top classes — a cheap "did anything scored change?" check |
| `top_classes` | the top-50 scored classes: `{ fqn, file, score, why }` |
| `pattern_violations` | visible violation count per detector label (`Layered`, `Repository`, …) |
| `tests_ran` | whether a coverage report was present (gates coverage-driven deltas) |

Writing the log is **best-effort and never blocks opening a repo** — a read-only home or a non-git repo degrades to a `warn!` and the open proceeds. The file is rewritten atomically (temp file + rename) on each append, and **auto-trimmed to the last 90 days** in the same pass, so it can never grow unbounded. It is machine-local runtime state and is git-ignored (`.projectmind/state/`).

## How the delta is computed

The current session is the newest line in the log (the `open_repo` that just ran wrote it). The baseline is chosen by `since`. Then:

- **new hotspots** — a class that is now in the high-risk band (score ≥ 60) *and* is either brand new to the top list or jumped ≥ 8 points from a lower baseline. Its `reason` is the current risk `why` hint (`hot+complex`, …) or `new class in the hot band`.
- **pattern drift** — every detector whose visible-violation count grew, sorted by the size of the increase. A *drop* in violations is not drift (it's an improvement) and is omitted.
- **risk delta** — every class whose composite score moved by ≥ 3 points, split into `up` (got worse) and `down` (got better), each sorted by magnitude.

**Class identity is stable via FQN**, with the repo-relative file path as the fallback key when a class has no FQN (rare — synthetic/anonymous types). This is what lets a class be matched across two sessions even as the score list reshuffles.

**Coverage caveat.** The composite score folds in test coverage, which only exists after a test run. A coverage-driven risk move is therefore only *comparable* when tests ran in **both** the baseline and the current session — `coverage_comparable` reports whether that held, and the Markdown/text output notes when it didn't. Risk moves are still shown regardless (churn + complexity + fan-in are always present); the flag just qualifies how much of the delta could be coverage.

Because the risk score is **z-normalised per repo**, one class getting much worse can nudge others slightly *down* in relative terms even if their raw signals didn't change — the delta reflects relative risk, which is the point.

## The CLI

```
projectmind briefing [--repo <path>] [--since <spec>] [--format text|markdown|json] [--no-record]
```

The CLI face of the tool, for cron jobs and Slack bots. It opens the repo — which appends a fresh snapshot, exactly like the MCP `open_repo` — then prints the briefing:

- `--repo` — the repository root (falls back to the repo recorded in the statefile).
- `--since` — the baseline (`last_session` default, `1d` / `7d`, ISO-8601, Unix seconds).
- `--format` — `text` (default, terminal/cron friendly), `markdown` (the chat embed), or `json` (the raw payload).
- `--no-record` — diff the history as it already stands without appending a snapshot (a read-only peek).

Running `briefing` twice in a row therefore compares the two runs, matching the interactive tool's semantics. The `projectmind-mcp` binary with **no** subcommand still runs the stdio MCP server unchanged.

Example cron use:

```sh
# 08:00 daily: post the overnight delta to Slack
projectmind briefing --repo /srv/repos/monolith --since 1d --format markdown | slack-post '#arch'
```

## Authoring for a useful briefing

- The signal is only as good as the history: open the repo (or run `briefing`) regularly so there are sessions to diff.
- A coverage report present at open time makes the `cov` signal — and thus coverage-driven deltas — meaningful. Run tests before the open you want to compare against.
- The 90-day retention means the `7d` / `30d` windows keep working for months without any maintenance; older lines are dropped automatically.

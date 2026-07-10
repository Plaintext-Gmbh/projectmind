// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Session log + morning briefing (Cockpit 2.7, issue #163).
//!
//! Every `open_repo` appends one line to `.projectmind/state/sessions.jsonl`
//! capturing a lightweight snapshot of the repo's health at that moment:
//!
//! - a timestamp,
//! - a hash of the top scored classes (the *atlas snapshot hash*),
//! - the top-50 [`RiskScore`]s reduced to `{fqn, file, score}`,
//! - a pattern-violation count per detector.
//!
//! [`architect_briefing`] then diffs the *current* session against the most
//! recent prior one (or one N days back) to answer "what got worse since I
//! last looked" — new hotspots, pattern drift, and the risk delta up/down.
//!
//! # Design notes
//!
//! - **Best-effort, never fatal.** Writing the session log must never block
//!   opening a repo. Every entry point swallows I/O errors into a `warn!`.
//! - **Auto-trim to 90 days.** [`append`] rewrites the file dropping entries
//!   older than [`RETENTION_DAYS`] so the log can't grow unbounded.
//! - **Stable class identity via FQN.** A class is matched across sessions by
//!   its fully-qualified name; the repo-relative file path is the fallback key
//!   when an FQN is absent (rare — only synthetic/anonymous types).
//! - **Coverage delta is only meaningful when tests ran in both sessions**,
//!   which we record via `tests_ran` on each snapshot and gate on.
//! - **Known limitation:** the delta iterates the *current* session's top-50,
//!   so a class that improved so much it dropped out of the top-50 is not
//!   reported in `risk_delta.down`. That is by design — a class no longer in
//!   the top band is no longer a worry — but it means the down-list can
//!   undercount improvements on a churny list. The new-hotspot / up path is
//!   the priority (this tool is about what got *worse*).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use projectmind_plugin_api::Relation;

use crate::patterns::{self, PatternConfig, Scope};
use crate::persistence::CONFIG_DIR;
use crate::repository::Repository;
use crate::risk::{self, Options as RiskOptions, RiskScore};
use crate::{coverage, patterns::Pattern};

/// How long session-log entries are kept before [`append`] trims them.
pub const RETENTION_DAYS: u64 = 90;

/// Seconds in one day.
const DAY_SECS: u64 = 86_400;
/// Seconds in one day, signed — for the civil-date arithmetic.
const DAY_SECS_I64: i64 = 86_400;

/// A class's identity + score inside a session snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredClass {
    /// Fully-qualified name. Stable identity key across sessions.
    pub fqn: String,
    /// Repo-relative source path. Fallback identity when `fqn` is empty.
    pub file: PathBuf,
    /// Composite risk score in 0..=100 at snapshot time.
    pub score: f64,
    /// Short "why" hint copied from the [`RiskScore`] (e.g. `hot+complex`).
    #[serde(default)]
    pub why: String,
}

/// One `sessions.jsonl` line: a snapshot of repo health at an `open_repo`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Schema version so the reader can skip incompatible lines.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Unix seconds when the session was recorded.
    pub ts: u64,
    /// Hash over `(fqn, rounded score)` of the top classes. A cheap way to
    /// tell "nothing scored changed" without diffing the whole list.
    pub atlas_hash: u64,
    /// Top-N scored classes (default 50), highest risk first.
    pub top_classes: Vec<ScoredClass>,
    /// Visible pattern-violation count per detector label (`Layered`, …).
    pub pattern_violations: HashMap<String, u32>,
    /// Whether a coverage report was present when this snapshot was taken.
    /// Gate coverage-based deltas on both sessions having it.
    #[serde(default)]
    pub tests_ran: bool,
}

fn default_schema_version() -> u32 {
    1
}

/// Current session-record schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// How many top classes a snapshot records.
pub const TOP_CLASSES: usize = 50;

impl SessionRecord {
    /// Build a record from freshly-computed risk scores + pattern counts.
    ///
    /// `scores` should already be sorted highest-first (as [`crate::risk::compute`]
    /// returns them); only the first [`TOP_CLASSES`] are kept.
    #[must_use]
    pub fn from_signals(
        scores: &[RiskScore],
        pattern_violations: HashMap<String, u32>,
        tests_ran: bool,
    ) -> Self {
        let top_classes: Vec<ScoredClass> = scores
            .iter()
            .take(TOP_CLASSES)
            .map(|s| ScoredClass {
                fqn: s.fqn.clone(),
                file: s.file.clone(),
                score: s.score,
                why: s.why.clone(),
            })
            .collect();
        let atlas_hash = atlas_hash(&top_classes);
        Self {
            schema_version: SCHEMA_VERSION,
            ts: now_secs(),
            atlas_hash,
            top_classes,
            pattern_violations,
            tests_ran,
        }
    }
}

/// Build a [`SessionRecord`] snapshot from a parsed repository.
///
/// Computes the top-[`TOP_CLASSES`] risk scores (churn, complexity, coverage,
/// fan-in) and the visible pattern-violation count per detector, exactly as the
/// `risk_atlas` and `pattern_check` tools do, so the log mirrors what an
/// architect would see live. `relations` is the already-parsed framework
/// relations graph (pass `engine.relations(repo)`) so this stays a flat pass.
#[must_use]
pub fn snapshot(repo: &Repository, relations: &[Relation]) -> SessionRecord {
    let opts = RiskOptions {
        top: TOP_CLASSES,
        ..RiskOptions::default()
    };
    let cov = coverage::load(&repo.root);
    // `risk::compute` only fails when git churn can't be walked; degrade to an
    // empty score list so a non-git repo still logs pattern counts.
    let scores = risk::compute(repo, relations, cov.as_ref(), &opts).unwrap_or_default();

    let config = PatternConfig::load(&repo.root);
    let mut pattern_violations = HashMap::new();
    for result in patterns::check_all(repo, &Scope::default(), &config) {
        pattern_violations.insert(
            result.pattern.label().to_string(),
            u32::try_from(result.visible_violations().len()).unwrap_or(u32::MAX),
        );
    }
    // Detectors switched off in config still deserve a zero so drift math has a
    // baseline of every known detector.
    for pattern in Pattern::ALL {
        pattern_violations
            .entry(pattern.label().to_string())
            .or_insert(0);
    }

    SessionRecord::from_signals(&scores, pattern_violations, cov.is_some())
}

/// Compute a snapshot and append it to the session log. Best-effort: returns
/// the recorded snapshot on success. Errors bubble up so callers on the
/// `open_repo` path can downgrade them to a `warn!` and never block the open.
pub fn snapshot_and_log(
    repo: &Repository,
    relations: &[Relation],
) -> std::io::Result<SessionRecord> {
    let record = snapshot(repo, relations);
    append(&repo.root, &record)?;
    Ok(record)
}

/// Hash the `(fqn, rounded-score)` pairs of a class list into one `u64`.
///
/// Rounding to one decimal keeps the hash stable against float jitter while
/// still flipping when a score moves meaningfully.
#[must_use]
pub fn atlas_hash(classes: &[ScoredClass]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for c in classes {
        c.fqn.hash(&mut hasher);
        // Round to 0.1 and hash the integer form so the hash is
        // deterministic and free of float NaN pitfalls.
        let scaled = (c.score * 10.0).round() as i64;
        scaled.hash(&mut hasher);
    }
    hasher.finish()
}

/// Path to the append-only session log for `repo_root`.
#[must_use]
pub fn log_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join(CONFIG_DIR)
        .join("state")
        .join("sessions.jsonl")
}

/// Append `record` to the session log at `repo_root`, trimming entries older
/// than [`RETENTION_DAYS`].
///
/// The whole file is rewritten atomically (temp file + rename) so a reader
/// never observes a half-written line and the trim is applied in the same
/// pass. Errors bubble up; callers on the `open_repo` path downgrade them to
/// a `warn!` so a read-only home never blocks opening a repo.
pub fn append(repo_root: &Path, record: &SessionRecord) -> std::io::Result<()> {
    let path = log_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut records = read_all(&path)?;
    records.push(record.clone());
    trim_old(&mut records, record.ts);

    let mut body = String::new();
    for r in &records {
        let line = serde_json::to_string(r)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        body.push_str(&line);
        body.push('\n');
    }
    // Per-process temp name so two concurrent openers don't clobber each
    // other's temp file mid-write (the rename stays atomic; at worst one
    // writer's snapshot is lost — acceptable for a best-effort log — but the
    // file is never left torn or picking up a sibling's partial write).
    let tmp = path.with_extension(format!("jsonl.tmp.{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Drop records older than [`RETENTION_DAYS`] relative to `now_ts`.
fn trim_old(records: &mut Vec<SessionRecord>, now_ts: u64) {
    let cutoff = now_ts.saturating_sub(RETENTION_DAYS * DAY_SECS);
    records.retain(|r| r.ts >= cutoff);
}

/// Read every parseable line of the session log. Missing file → empty vec.
/// Unparseable lines are skipped (forward-compat with a newer writer) rather
/// than failing the whole read.
pub fn read_all(path: &Path) -> std::io::Result<Vec<SessionRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        // A path component being a file (ENOTDIR) — e.g. a virtual repo rooted
        // at a markdown file — behaves like "no log yet".
        Err(_) if !path.exists() => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<SessionRecord>(trimmed) {
            out.push(rec);
        }
    }
    Ok(out)
}

/// Load the session history for `repo_root`, oldest first.
pub fn load_history(repo_root: &Path) -> std::io::Result<Vec<SessionRecord>> {
    read_all(&log_path(repo_root))
}

// ---- briefing (delta) -----------------------------------------------------

/// How far back the briefing reaches for its baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Since {
    /// The most recent session strictly before the current one.
    LastSession,
    /// The most recent session at or before `now - N days`.
    Days(u64),
    /// The most recent session at or before this absolute Unix-seconds
    /// timestamp.
    At(u64),
}

impl Since {
    /// Parse the `since` argument accepted by the MCP tool / CLI.
    ///
    /// Accepts `last_session` (default), `<N>d` (e.g. `1d`, `7d`), or an
    /// ISO-8601 / RFC-3339 timestamp (`2026-07-01T00:00:00Z`). A bare integer
    /// is treated as Unix seconds. Unrecognised input → `None`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("last_session") || s.eq_ignore_ascii_case("last")
        {
            return Some(Self::LastSession);
        }
        // `<N>d` day window.
        if let Some(num) = s.strip_suffix('d').or_else(|| s.strip_suffix('D')) {
            if let Ok(days) = num.trim().parse::<u64>() {
                return Some(Self::Days(days));
            }
        }
        // Bare integer → Unix seconds.
        if let Ok(secs) = s.parse::<u64>() {
            return Some(Self::At(secs));
        }
        // ISO-8601 / RFC-3339.
        parse_iso8601(s).map(Self::At)
    }
}

/// A class whose risk crossed into the "worry" band since the baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewHotspot {
    /// Fully-qualified name.
    pub fqn: String,
    /// Score now.
    pub score_now: f64,
    /// Score at the baseline session (`0.0` if the class is brand new there).
    pub score_then: f64,
    /// `score_now - score_then`.
    pub delta: f64,
    /// Human-readable reason (the current "why" hint, or `new class`).
    pub reason: String,
}

/// A pattern violation that appeared since the baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternDrift {
    /// Detector label (`Layered`, `Repository`, …).
    pub pattern: String,
    /// Violation count now.
    pub count_now: u32,
    /// Violation count at the baseline.
    pub count_then: u32,
    /// `count_now - count_then` (always > 0 in the drift list).
    pub delta: i64,
}

/// One class's risk movement (used in both `up` and `down`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskMove {
    /// Fully-qualified name.
    pub fqn: String,
    /// `score_now - score_then` (positive in `up`, negative in `down`).
    pub delta: f64,
}

/// Risk movements split into classes that got worse and better.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RiskDelta {
    /// Classes whose score rose, largest jump first.
    pub up: Vec<RiskMove>,
    /// Classes whose score fell, largest drop first.
    pub down: Vec<RiskMove>,
}

/// The window the briefing covered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionWindow {
    /// Baseline session timestamp (Unix seconds).
    pub from: u64,
    /// Current session timestamp (Unix seconds).
    pub to: u64,
}

/// The full briefing payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Briefing {
    /// Classes that newly crossed into the hotspot band.
    pub new_hotspots: Vec<NewHotspot>,
    /// Patterns that drifted (more violations than before).
    pub pattern_drift: Vec<PatternDrift>,
    /// Risk movements up/down.
    pub risk_delta: RiskDelta,
    /// The session window, or `None` when there was nothing to compare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_window: Option<SessionWindow>,
    /// True when both sessions ran tests, so coverage-driven risk moves are
    /// comparable. Surfaced so the reader can qualify the numbers.
    #[serde(default)]
    pub coverage_comparable: bool,
    /// Non-fatal note when the briefing is empty and why (no history, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Briefing {
    /// True when there is nothing to report.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_hotspots.is_empty()
            && self.pattern_drift.is_empty()
            && self.risk_delta.up.is_empty()
            && self.risk_delta.down.is_empty()
    }
}

/// A risk jump at or above this delta promotes a class to `new_hotspots`
/// (when it was previously below the hot band or absent).
pub const HOTSPOT_DELTA: f64 = 8.0;

/// Score at or above which a class counts as "in the hot band" now.
pub const HOTSPOT_FLOOR: f64 = 60.0;

/// Only surface risk moves whose magnitude is at least this, to keep the
/// briefing signal-dense.
pub const RISK_MOVE_FLOOR: f64 = 3.0;

/// Compute the briefing from a full session `history` (oldest → newest) using
/// the last entry as *current* and `since` to pick the baseline.
///
/// Returns an empty briefing with a `note` when there is no current session or
/// no baseline to compare against — an empty history yields an empty briefing.
#[must_use]
pub fn briefing(history: &[SessionRecord], since: Since) -> Briefing {
    let Some((current, prior)) = history.split_last() else {
        return Briefing {
            note: Some("no sessions recorded yet — open the repo to start the log".into()),
            ..Briefing::default()
        };
    };
    let Some(baseline) = pick_baseline(current, prior, since) else {
        return Briefing {
            session_window: None,
            note: Some(match since {
                Since::LastSession => {
                    "only one session on record — nothing earlier to compare against".into()
                }
                Since::Days(n) => format!("no session on record from {n}d ago or earlier"),
                Since::At(_) => "no session on record at or before the requested time".into(),
            }),
            ..Briefing::default()
        };
    };
    compute_delta(baseline, current)
}

/// Pick the baseline record out of the `prior` slice per `since`. `prior` is
/// everything before `current`, oldest first.
fn pick_baseline<'a>(
    current: &SessionRecord,
    prior: &'a [SessionRecord],
    since: Since,
) -> Option<&'a SessionRecord> {
    match since {
        Since::LastSession => prior.last(),
        Since::Days(n) => {
            // Saturating throughout so an absurd `--since 99999999999d` clamps
            // to epoch instead of overflowing (debug panic / release wrap).
            let cutoff = current.ts.saturating_sub(n.saturating_mul(DAY_SECS));
            // Most recent session at or before the cutoff; if none is that
            // old, fall back to the oldest we have so a young log still gives
            // *some* baseline rather than an empty briefing.
            prior
                .iter()
                .rev()
                .find(|r| r.ts <= cutoff)
                .or_else(|| prior.first())
        }
        Since::At(ts) => prior
            .iter()
            .rev()
            .find(|r| r.ts <= ts)
            .or_else(|| prior.first()),
    }
}

/// Diff a `baseline` snapshot against `current`.
fn compute_delta(baseline: &SessionRecord, current: &SessionRecord) -> Briefing {
    let then: HashMap<&str, &ScoredClass> = baseline
        .top_classes
        .iter()
        .map(|c| (identity(c), c))
        .collect();

    let mut new_hotspots = Vec::new();
    let mut up = Vec::new();
    let mut down = Vec::new();

    for c in &current.top_classes {
        let id = identity(c);
        let score_then = then.get(id).map_or(0.0, |b| b.score);
        let delta = c.score - score_then;

        // New-hotspot rule: now in the hot band AND either brand new or a
        // meaningful jump from a lower baseline.
        let was_absent = !then.contains_key(id);
        let crossed_up = c.score >= HOTSPOT_FLOOR
            && (was_absent || (score_then < HOTSPOT_FLOOR && delta >= HOTSPOT_DELTA));
        if crossed_up {
            new_hotspots.push(NewHotspot {
                fqn: c.fqn.clone(),
                score_now: c.score,
                score_then,
                delta,
                reason: if was_absent {
                    "new class in the hot band".to_string()
                } else if c.why.is_empty() {
                    "risk rose sharply".to_string()
                } else {
                    c.why.clone()
                },
            });
        }

        if delta.abs() >= RISK_MOVE_FLOOR {
            let mv = RiskMove {
                fqn: c.fqn.clone(),
                delta: round1(delta),
            };
            if delta > 0.0 {
                up.push(mv);
            } else {
                down.push(mv);
            }
        }
    }

    // Sort worst-first / best-first by magnitude.
    new_hotspots.sort_by(|a, b| {
        b.delta
            .partial_cmp(&a.delta)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    up.sort_by(|a, b| {
        b.delta
            .partial_cmp(&a.delta)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    down.sort_by(|a, b| {
        a.delta
            .partial_cmp(&b.delta)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let pattern_drift = pattern_drift(baseline, current);

    Briefing {
        new_hotspots,
        pattern_drift,
        risk_delta: RiskDelta { up, down },
        session_window: Some(SessionWindow {
            from: baseline.ts,
            to: current.ts,
        }),
        coverage_comparable: baseline.tests_ran && current.tests_ran,
        note: None,
    }
}

/// Detectors whose visible-violation count grew between the two sessions.
fn pattern_drift(baseline: &SessionRecord, current: &SessionRecord) -> Vec<PatternDrift> {
    let mut out = Vec::new();
    for (pattern, &count_now) in &current.pattern_violations {
        let count_then = baseline
            .pattern_violations
            .get(pattern)
            .copied()
            .unwrap_or(0);
        let delta = i64::from(count_now) - i64::from(count_then);
        if delta > 0 {
            out.push(PatternDrift {
                pattern: pattern.clone(),
                count_now,
                count_then,
                delta,
            });
        }
    }
    out.sort_by(|a, b| {
        b.delta
            .cmp(&a.delta)
            .then_with(|| a.pattern.cmp(&b.pattern))
    });
    out
}

/// Stable identity of a class: FQN when present, else the file path.
fn identity(c: &ScoredClass) -> &str {
    if c.fqn.is_empty() {
        c.file.to_str().unwrap_or("")
    } else {
        &c.fqn
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Minimal ISO-8601 / RFC-3339 → Unix-seconds parser for the `since` argument.
///
/// Handles `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM:SS`, an optional fractional part,
/// and a trailing `Z` or `±HH:MM` offset. Good enough for the briefing window;
/// avoids pulling `chrono` into the core just for this. Returns `None` on any
/// shape it doesn't understand.
fn parse_iso8601(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    if bytes.get(4) != Some(&b'-') {
        return None;
    }
    let month: i64 = s.get(5..7)?.parse().ok()?;
    if bytes.get(7) != Some(&b'-') {
        return None;
    }
    let day: i64 = s.get(8..10)?.parse().ok()?;
    // Reject impossible calendar dates (e.g. 2021-02-31) rather than letting
    // days_from_civil silently roll them into the next month.
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }

    let mut hour: i64 = 0;
    let mut min: i64 = 0;
    let mut sec: i64 = 0;
    let mut offset_secs: i64 = 0;

    if bytes.len() > 10 {
        // Expect a 'T' or space separator.
        let sep = bytes[10];
        if sep != b'T' && sep != b't' && sep != b' ' {
            return None;
        }
        let rest = &s[11..];
        // Split off timezone designator.
        let (time_part, tz) = split_tz(rest);
        let mut tp = time_part.split(':');
        hour = tp.next()?.parse().ok()?;
        if let Some(m) = tp.next() {
            min = m.parse().ok()?;
        }
        if let Some(sec_str) = tp.next() {
            // Drop a fractional part.
            let sec_str = sec_str.split('.').next().unwrap_or(sec_str);
            sec = sec_str.parse().ok()?;
        }
        offset_secs = tz;
    }

    if !(0..=23).contains(&hour) || !(0..=59).contains(&min) || !(0..=60).contains(&sec) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let total = days * DAY_SECS_I64 + hour * 3600 + min * 60 + sec - offset_secs;
    u64::try_from(total).ok()
}

/// Split a `HH:MM:SS±HH:MM` / `...Z` tail into `(time, offset_secs)`.
fn split_tz(rest: &str) -> (&str, i64) {
    if let Some(stripped) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        return (stripped, 0);
    }
    // Search for a +/- after the time (skip the first char in case of a lone
    // sign — the time part itself never starts with a sign).
    for (i, ch) in rest.char_indices().skip(1) {
        if ch == '+' || ch == '-' {
            let (time, off) = rest.split_at(i);
            let sign = if ch == '+' { 1 } else { -1 };
            let off = &off[1..];
            let mut parts = off.split(':');
            // Clamp to a sane UTC-offset range; a garbage offset like `+99:99`
            // shouldn't mis-date the window by four days.
            let oh = parts
                .next()
                .and_then(|p| p.parse::<i64>().ok())
                .unwrap_or(0)
                .clamp(0, 14);
            let om = parts
                .next()
                .and_then(|p| p.parse::<i64>().ok())
                .unwrap_or(0)
                .clamp(0, 59);
            return (time, sign * (oh * 3600 + om * 60));
        }
    }
    (rest, 0)
}

/// Number of days in month `m` of year `y` (Gregorian, leap-aware).
fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since the Unix epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---- markdown formatter ---------------------------------------------------

/// Render `briefing` as compact Markdown for chat embedding.
///
/// The output leads with a one-line window header, then only the sections
/// that have content, so a quiet repo prints a short "all clear" line rather
/// than four empty headers.
#[must_use]
pub fn to_markdown(briefing: &Briefing) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("# 🌅 Architect Briefing\n\n");

    match &briefing.session_window {
        Some(w) => {
            let _ = writeln!(out, "_Since {} → {}_\n", fmt_ts(w.from), fmt_ts(w.to));
        }
        None => {
            let note = briefing
                .note
                .as_deref()
                .unwrap_or("no baseline session to compare against");
            let _ = writeln!(out, "_{note}_");
            return out;
        }
    }

    if briefing.is_empty() {
        out.push_str("✅ Nothing got worse since the last session.\n");
        if !briefing.coverage_comparable {
            out.push_str("\n_Coverage delta not shown — tests didn't run in both sessions._\n");
        }
        return out;
    }

    if !briefing.new_hotspots.is_empty() {
        out.push_str("## 🔥 New hotspots\n\n");
        for h in &briefing.new_hotspots {
            let _ = writeln!(
                out,
                "- **{}** — {:.0} (was {:.0}, +{:.0}) · _{}_",
                h.fqn, h.score_now, h.score_then, h.delta, h.reason
            );
        }
        out.push('\n');
    }

    if !briefing.pattern_drift.is_empty() {
        out.push_str("## ⚠️ Pattern drift\n\n");
        for d in &briefing.pattern_drift {
            let _ = writeln!(
                out,
                "- **{}**: {} → {} violations (+{})",
                d.pattern, d.count_then, d.count_now, d.delta
            );
        }
        out.push('\n');
    }

    if !briefing.risk_delta.up.is_empty() {
        out.push_str("## 📈 Risk up\n\n");
        for m in &briefing.risk_delta.up {
            let _ = writeln!(out, "- {} (+{:.1})", m.fqn, m.delta);
        }
        out.push('\n');
    }

    if !briefing.risk_delta.down.is_empty() {
        out.push_str("## 📉 Risk down\n\n");
        for m in &briefing.risk_delta.down {
            let _ = writeln!(out, "- {} ({:.1})", m.fqn, m.delta);
        }
        out.push('\n');
    }

    if !briefing.coverage_comparable {
        out.push_str("_Coverage delta not factored — tests didn't run in both sessions._\n");
    }

    out
}

/// Format a Unix-seconds timestamp as `YYYY-MM-DD HH:MM UTC` for the header.
fn fmt_ts(secs: u64) -> String {
    let days = i64::try_from(secs / DAY_SECS).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    let rem = secs % DAY_SECS;
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

/// Inverse of [`days_from_civil`] — days-since-epoch → `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scored(fqn: &str, score: f64, why: &str) -> ScoredClass {
        ScoredClass {
            fqn: fqn.into(),
            file: PathBuf::from(format!("{}.java", fqn.replace('.', "/"))),
            score,
            why: why.into(),
        }
    }

    fn record(ts: u64, classes: Vec<ScoredClass>, patterns: &[(&str, u32)]) -> SessionRecord {
        let pattern_violations = patterns
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect();
        SessionRecord {
            schema_version: SCHEMA_VERSION,
            ts,
            atlas_hash: atlas_hash(&classes),
            top_classes: classes,
            pattern_violations,
            tests_ran: false,
        }
    }

    // ---- Since parsing ---------------------------------------------------

    #[test]
    fn since_parses_all_forms() {
        assert_eq!(Since::parse(""), Some(Since::LastSession));
        assert_eq!(Since::parse("last_session"), Some(Since::LastSession));
        assert_eq!(Since::parse("1d"), Some(Since::Days(1)));
        assert_eq!(Since::parse("7D"), Some(Since::Days(7)));
        assert_eq!(Since::parse("1700000000"), Some(Since::At(1_700_000_000)));
        // 2021-01-01T00:00:00Z == 1609459200
        assert_eq!(
            Since::parse("2021-01-01T00:00:00Z"),
            Some(Since::At(1_609_459_200))
        );
        // Date-only.
        assert_eq!(Since::parse("2021-01-01"), Some(Since::At(1_609_459_200)));
        assert_eq!(Since::parse("garbage!!"), None);
    }

    #[test]
    fn iso8601_handles_offset() {
        // 2021-01-01T01:00:00+01:00 == 2021-01-01T00:00:00Z == 1609459200
        assert_eq!(
            parse_iso8601("2021-01-01T01:00:00+01:00"),
            Some(1_609_459_200)
        );
        // Negative offset.
        assert_eq!(
            parse_iso8601("2020-12-31T23:00:00-01:00"),
            Some(1_609_459_200)
        );
        // Garbage offset is clamped, not applied as a multi-day shift.
        assert!(parse_iso8601("2021-01-01T00:00:00+99:99").is_some());
    }

    #[test]
    fn iso8601_rejects_impossible_calendar_dates() {
        // Feb 31 must be rejected, not silently rolled into March.
        assert_eq!(parse_iso8601("2021-02-31"), None);
        assert_eq!(parse_iso8601("2021-04-31"), None); // April has 30 days
        assert_eq!(parse_iso8601("2021-02-29"), None); // 2021 not a leap year
        assert!(parse_iso8601("2020-02-29").is_some()); // 2020 is a leap year
        assert_eq!(parse_iso8601("2021-00-10"), None); // month 0
        assert_eq!(parse_iso8601("2021-01-00"), None); // day 0
    }

    #[test]
    fn since_days_saturates_on_absurd_input() {
        // A huge N must not overflow `n * DAY_SECS` — it clamps to epoch, so
        // the oldest session becomes the baseline instead of panicking.
        let hist = vec![
            record(1000, vec![scored("a.A", 20.0, "x")], &[]),
            record(2000, vec![scored("a.A", 90.0, "x")], &[]),
        ];
        let b = briefing(&hist, Since::Days(u64::MAX));
        let w = b.session_window.unwrap();
        assert_eq!(w.from, 1000, "oldest session is the baseline");
        assert_eq!(w.to, 2000);
    }

    #[test]
    fn civil_roundtrips() {
        // 2026-07-10
        let days = days_from_civil(2026, 7, 10);
        assert_eq!(civil_from_days(days), (2026, 7, 10));
    }

    // ---- briefing core ---------------------------------------------------

    #[test]
    fn empty_history_gives_empty_briefing() {
        let b = briefing(&[], Since::LastSession);
        assert!(b.is_empty());
        assert!(b.session_window.is_none());
        assert!(b.note.is_some());
    }

    #[test]
    fn single_session_has_no_baseline() {
        let hist = vec![record(1000, vec![scored("a.A", 50.0, "hot")], &[])];
        let b = briefing(&hist, Since::LastSession);
        assert!(b.is_empty());
        assert!(b.session_window.is_none());
        assert!(b.note.as_deref().unwrap().contains("one session"));
    }

    #[test]
    fn rising_score_becomes_new_hotspot() {
        // Class X climbs from 40 → 75: crosses the hot floor with a big jump.
        let hist = vec![
            record(1000, vec![scored("x.X", 40.0, "baseline")], &[]),
            record(2000, vec![scored("x.X", 75.0, "hot+complex")], &[]),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert_eq!(b.new_hotspots.len(), 1);
        let h = &b.new_hotspots[0];
        assert_eq!(h.fqn, "x.X");
        assert!((h.score_now - 75.0).abs() < 1e-9);
        assert!((h.score_then - 40.0).abs() < 1e-9);
        assert!((h.delta - 35.0).abs() < 1e-9);
        assert_eq!(h.reason, "hot+complex");
        // Also shows up in risk_delta.up.
        assert_eq!(b.risk_delta.up.len(), 1);
        assert_eq!(b.risk_delta.up[0].fqn, "x.X");
        assert!(b.risk_delta.down.is_empty());
    }

    #[test]
    fn brand_new_hot_class_is_flagged() {
        let hist = vec![
            record(1000, vec![scored("a.A", 30.0, "baseline")], &[]),
            record(
                2000,
                vec![scored("a.A", 30.0, "baseline"), scored("b.B", 80.0, "hot")],
                &[],
            ),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert_eq!(b.new_hotspots.len(), 1);
        assert_eq!(b.new_hotspots[0].fqn, "b.B");
        assert!(b.new_hotspots[0].score_then.abs() < 1e-9);
        assert!(b.new_hotspots[0].reason.contains("new class"));
    }

    #[test]
    fn score_drop_lands_in_risk_down_not_hotspots() {
        let hist = vec![
            record(1000, vec![scored("a.A", 70.0, "hot")], &[]),
            record(2000, vec![scored("a.A", 55.0, "cooling")], &[]),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert!(b.new_hotspots.is_empty());
        assert_eq!(b.risk_delta.down.len(), 1);
        assert!(b.risk_delta.down[0].delta < 0.0);
        assert!(b.risk_delta.up.is_empty());
    }

    #[test]
    fn small_moves_are_filtered_out() {
        // A 1-point move is below RISK_MOVE_FLOOR → nothing reported.
        let hist = vec![
            record(1000, vec![scored("a.A", 50.0, "x")], &[]),
            record(2000, vec![scored("a.A", 51.0, "x")], &[]),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert!(b.is_empty());
        // But the window is still populated (there *was* a baseline).
        assert!(b.session_window.is_some());
    }

    #[test]
    fn new_violation_is_pattern_drift() {
        let hist = vec![
            record(1000, vec![scored("a.A", 50.0, "x")], &[("Layered", 1)]),
            record(
                2000,
                vec![scored("a.A", 50.0, "x")],
                &[("Layered", 3), ("Repository", 2)],
            ),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert_eq!(b.pattern_drift.len(), 2);
        // Sorted by delta desc: Repository +2 ties with Layered +2 → tie
        // broken by pattern name (Layered < Repository).
        let layered = b
            .pattern_drift
            .iter()
            .find(|d| d.pattern == "Layered")
            .unwrap();
        assert_eq!(layered.count_then, 1);
        assert_eq!(layered.count_now, 3);
        assert_eq!(layered.delta, 2);
        let repo = b
            .pattern_drift
            .iter()
            .find(|d| d.pattern == "Repository")
            .unwrap();
        assert_eq!(repo.count_then, 0);
        assert_eq!(repo.delta, 2);
    }

    #[test]
    fn fewer_violations_is_not_drift() {
        let hist = vec![
            record(1000, vec![scored("a.A", 50.0, "x")], &[("Layered", 5)]),
            record(2000, vec![scored("a.A", 50.0, "x")], &[("Layered", 2)]),
        ];
        let b = briefing(&hist, Since::LastSession);
        assert!(b.pattern_drift.is_empty());
    }

    #[test]
    fn coverage_comparable_only_when_both_ran_tests() {
        let mut r1 = record(1000, vec![scored("a.A", 50.0, "x")], &[]);
        let mut r2 = record(2000, vec![scored("a.A", 60.0, "x")], &[]);
        r1.tests_ran = true;
        r2.tests_ran = true;
        let b = briefing(&[r1.clone(), r2.clone()], Since::LastSession);
        assert!(b.coverage_comparable);

        r1.tests_ran = false;
        let b2 = briefing(&[r1, r2], Since::LastSession);
        assert!(!b2.coverage_comparable);
    }

    // ---- since selection -------------------------------------------------

    #[test]
    fn since_days_picks_older_baseline() {
        let day = DAY_SECS;
        let hist = vec![
            record(0, vec![scored("a.A", 20.0, "x")], &[]), // 0d
            record(6 * day, vec![scored("a.A", 40.0, "x")], &[]), // recent-ish
            record(7 * day, vec![scored("a.A", 80.0, "x")], &[]), // current
        ];
        // since=7d from current (ts=7d) → cutoff = 0 → baseline is ts=0.
        let b = briefing(&hist, Since::Days(7));
        let w = b.session_window.unwrap();
        assert_eq!(w.from, 0);
        assert_eq!(w.to, 7 * day);
        // Score went 20 → 80.
        assert_eq!(b.new_hotspots.len(), 1);
        assert!((b.new_hotspots[0].delta - 60.0).abs() < 1e-9);
    }

    #[test]
    fn since_at_absolute_timestamp() {
        let hist = vec![
            record(1000, vec![scored("a.A", 20.0, "x")], &[]),
            record(2000, vec![scored("a.A", 40.0, "x")], &[]),
            record(3000, vec![scored("a.A", 90.0, "x")], &[]),
        ];
        // Baseline = most recent session <= 2000 → the ts=2000 record.
        let b = briefing(&hist, Since::At(2000));
        let w = b.session_window.unwrap();
        assert_eq!(w.from, 2000);
        assert_eq!(w.to, 3000);
    }

    // ---- persistence + trim ----------------------------------------------

    struct TempRepo(PathBuf);
    impl TempRepo {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "projectmind-session-{}-{}-{}",
                std::process::id(),
                tag,
                now_secs()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn append_and_read_roundtrip() {
        let repo = TempRepo::new("roundtrip");
        let r = record(
            now_secs(),
            vec![scored("a.A", 55.0, "hot")],
            &[("Layered", 2)],
        );
        append(repo.path(), &r).unwrap();
        let hist = load_history(repo.path()).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].top_classes[0].fqn, "a.A");
        assert_eq!(hist[0].pattern_violations.get("Layered"), Some(&2));
        // No temp file left behind.
        assert!(!log_path(repo.path()).with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn append_trims_entries_older_than_90_days() {
        let repo = TempRepo::new("trim");
        let now = 200 * DAY_SECS; // pretend "now" is day 200
                                  // Old entry (day 10, ~190 days ago) then a fresh one.
        let old = record(10 * DAY_SECS, vec![scored("a.A", 10.0, "x")], &[]);
        // Manually write the old record first without trimming relative to it.
        let path = log_path(repo.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", serde_json::to_string(&old).unwrap())).unwrap();

        let mut fresh = record(now, vec![scored("b.B", 90.0, "hot")], &[]);
        fresh.ts = now;
        append(repo.path(), &fresh).unwrap();

        let hist = load_history(repo.path()).unwrap();
        // The 190-day-old entry must be trimmed; only the fresh one remains.
        assert_eq!(hist.len(), 1, "old entry should be trimmed");
        assert_eq!(hist[0].top_classes[0].fqn, "b.B");
    }

    #[test]
    fn read_all_skips_unparseable_lines() {
        let repo = TempRepo::new("skip");
        let path = log_path(repo.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let good = record(1000, vec![scored("a.A", 50.0, "x")], &[]);
        std::fs::write(
            &path,
            format!(
                "not json\n{}\n{{\"garbage\":true}}\n",
                serde_json::to_string(&good).unwrap()
            ),
        )
        .unwrap();
        let hist = read_all(&path).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].top_classes[0].fqn, "a.A");
    }

    #[test]
    fn missing_log_reads_empty() {
        let repo = TempRepo::new("missing");
        assert!(load_history(repo.path()).unwrap().is_empty());
    }

    #[test]
    fn from_signals_caps_at_top_50() {
        let scores: Vec<RiskScore> = (0..80)
            .map(|i| RiskScore {
                fqn: format!("c.C{i}"),
                module: "m".into(),
                file: PathBuf::from(format!("C{i}.java")),
                score: 100.0 - f64::from(i),
                churn: 0,
                cx: 0,
                cov: None,
                fan_in: 0,
                fan_out: 0,
                sloc: 0,
                why: "x".into(),
            })
            .collect();
        let rec = SessionRecord::from_signals(&scores, HashMap::new(), false);
        assert_eq!(rec.top_classes.len(), TOP_CLASSES);
        assert_eq!(rec.top_classes[0].fqn, "c.C0");
    }

    // ---- markdown --------------------------------------------------------

    #[test]
    fn markdown_all_clear_when_empty() {
        let hist = vec![
            record(0, vec![scored("a.A", 50.0, "x")], &[]),
            record(DAY_SECS, vec![scored("a.A", 50.0, "x")], &[]),
        ];
        let b = briefing(&hist, Since::LastSession);
        let md = to_markdown(&b);
        assert!(md.contains("Architect Briefing"));
        assert!(md.contains("Nothing got worse"), "got: {md}");
    }

    #[test]
    fn markdown_lists_hotspots_and_drift() {
        let hist = vec![
            record(0, vec![scored("x.X", 40.0, "x")], &[("Layered", 1)]),
            record(
                DAY_SECS,
                vec![scored("x.X", 85.0, "hot+complex")],
                &[("Layered", 4)],
            ),
        ];
        let b = briefing(&hist, Since::LastSession);
        let md = to_markdown(&b);
        assert!(md.contains("New hotspots"), "got: {md}");
        assert!(md.contains("x.X"), "got: {md}");
        assert!(md.contains("Pattern drift"), "got: {md}");
        assert!(md.contains("Layered"), "got: {md}");
    }

    #[test]
    fn markdown_note_when_no_baseline() {
        let hist = vec![record(1000, vec![scored("a.A", 50.0, "x")], &[])];
        let b = briefing(&hist, Since::LastSession);
        let md = to_markdown(&b);
        assert!(md.contains("Architect Briefing"));
        assert!(md.contains("one session"), "got: {md}");
    }
}

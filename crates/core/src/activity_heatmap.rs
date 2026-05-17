// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! GitHub-style commit activity heatmap.
//!
//! Walks HEAD backwards for the last [`WINDOW_DAYS`] days and tallies
//! commits per (date, author). Output is a per-day bucket suitable for
//! a 7×N calendar grid in the GUI.
//!
//! Capped at [`MAX_COMMITS`] visited commits so a very large repo
//! can't stall the diagram.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use git2::Repository as GitRepo;
use serde::Serialize;

/// How many days back from today the heatmap covers.
pub const WINDOW_DAYS: i64 = 365;
/// Hard upper bound on commits we visit, no matter the window.
pub const MAX_COMMITS: usize = 20_000;
/// Hard upper bound on author records kept across the window.
pub const MAX_AUTHORS: usize = 5_000;

/// One day's worth of activity.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityDay {
    /// ISO date `YYYY-MM-DD` (local interpretation of the commit
    /// timestamp).
    pub date: String,
    /// Number of commits that day.
    pub commits: usize,
    /// Top-3 authors by commit count for the tooltip.
    pub top_authors: Vec<AuthorSlice>,
}

/// Author + commit count for tooltip rendering.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorSlice {
    /// Display name; falls back to email or `"unknown"`.
    pub name: String,
    /// Number of commits this day.
    pub commits: usize,
}

/// Author summary across the entire window.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorTotals {
    /// Display name.
    pub name: String,
    /// Total commits across the window.
    pub commits: usize,
}

/// Aggregated heatmap payload.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityHeatmap {
    /// Repository root for display.
    pub root: String,
    /// Window start (`YYYY-MM-DD`, inclusive).
    pub start_date: String,
    /// Window end (`YYYY-MM-DD`, inclusive, typically today).
    pub end_date: String,
    /// Per-day buckets in order from `start_date` to `end_date`. Days
    /// with no commits are still present so the grid renders without
    /// gaps.
    pub days: Vec<ActivityDay>,
    /// Total commits across the window.
    pub total_commits: usize,
    /// Number of distinct authors across the window.
    pub distinct_authors: usize,
    /// Top-10 authors by commit count for the side panel.
    pub top_authors: Vec<AuthorTotals>,
    /// Maximum commits in any single day. Used as the colour-ramp anchor.
    pub max_commits_per_day: usize,
    /// Longest run of consecutive days with at least one commit.
    pub longest_streak_days: usize,
    /// `true` when the walker hit `MAX_COMMITS` and stopped.
    pub truncated: bool,
    /// `true` when no git repository was found at `root`.
    pub no_git: bool,
}

/// Build the heatmap for the git repository at `root`.
///
/// Falls back to an empty payload (`no_git = true`) when the directory
/// isn't a git checkout, rather than failing — the caller (and the GUI)
/// shouldn't need an error path for "this is just not a git repo".
#[must_use]
pub fn build(root: &Path) -> ActivityHeatmap {
    let root_str = root.to_string_lossy().to_string();
    let Ok(repo) = GitRepo::discover(root) else {
        return empty(&root_str, true);
    };

    let now_secs = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);
    let window_start_secs = now_secs - WINDOW_DAYS * 86_400;
    let end_date = date_from_secs(now_secs);
    let start_date = date_from_secs(window_start_secs);

    let mut per_day: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut author_totals: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_commits = 0usize;
    let mut truncated = false;

    let walk = repo.revwalk();
    let Ok(mut walk) = walk else {
        return empty(&root_str, false);
    };
    if walk.push_head().is_err() {
        // Empty repo / no HEAD → no activity but still a git repo.
        return ActivityHeatmap {
            root: root_str,
            start_date,
            end_date,
            days: backfill_empty_window(window_start_secs, now_secs),
            total_commits: 0,
            distinct_authors: 0,
            top_authors: Vec::new(),
            max_commits_per_day: 0,
            longest_streak_days: 0,
            truncated: false,
            no_git: false,
        };
    }
    let _ = walk.set_sorting(git2::Sort::TIME);

    for (visited, oid_res) in walk.enumerate() {
        if visited >= MAX_COMMITS {
            truncated = true;
            break;
        }
        let Ok(oid) = oid_res else { continue };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let secs = commit.time().seconds();
        if secs < window_start_secs {
            break;
        }
        let date = date_from_secs(secs);
        let author = commit.author();
        let display = author
            .name()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| author.email().map(str::to_string).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "unknown".to_string());

        let by_author = per_day.entry(date).or_default();
        *by_author.entry(display.clone()).or_default() += 1;
        if author_totals.len() < MAX_AUTHORS || author_totals.contains_key(&display) {
            *author_totals.entry(display).or_default() += 1;
        }
        total_commits += 1;
    }

    // Materialise every day in the window, even empty ones.
    let mut days: Vec<ActivityDay> = Vec::with_capacity((WINDOW_DAYS as usize) + 1);
    let mut day_secs = window_start_secs;
    let mut max_per_day = 0usize;
    while day_secs <= now_secs {
        let date = date_from_secs(day_secs);
        let mut bucket = ActivityDay {
            date: date.clone(),
            commits: 0,
            top_authors: Vec::new(),
        };
        if let Some(by_author) = per_day.remove(&date) {
            bucket.commits = by_author.values().sum();
            let mut authors: Vec<AuthorSlice> = by_author
                .into_iter()
                .map(|(name, commits)| AuthorSlice { name, commits })
                .collect();
            authors.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.name.cmp(&b.name)));
            authors.truncate(3);
            bucket.top_authors = authors;
        }
        if bucket.commits > max_per_day {
            max_per_day = bucket.commits;
        }
        days.push(bucket);
        day_secs += 86_400;
    }

    // Longest streak across the full window.
    let mut longest = 0usize;
    let mut current = 0usize;
    for d in &days {
        if d.commits > 0 {
            current += 1;
            if current > longest {
                longest = current;
            }
        } else {
            current = 0;
        }
    }

    // Top-10 author summary.
    let mut top_authors: Vec<AuthorTotals> = author_totals
        .into_iter()
        .map(|(name, commits)| AuthorTotals { name, commits })
        .collect();
    let distinct_authors = top_authors.len();
    top_authors.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.name.cmp(&b.name)));
    top_authors.truncate(10);

    ActivityHeatmap {
        root: root_str,
        start_date,
        end_date,
        days,
        total_commits,
        distinct_authors,
        top_authors,
        max_commits_per_day: max_per_day,
        longest_streak_days: longest,
        truncated,
        no_git: false,
    }
}

fn empty(root: &str, no_git: bool) -> ActivityHeatmap {
    let now_secs = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);
    let window_start_secs = now_secs - WINDOW_DAYS * 86_400;
    ActivityHeatmap {
        root: root.to_string(),
        start_date: date_from_secs(window_start_secs),
        end_date: date_from_secs(now_secs),
        days: backfill_empty_window(window_start_secs, now_secs),
        total_commits: 0,
        distinct_authors: 0,
        top_authors: Vec::new(),
        max_commits_per_day: 0,
        longest_streak_days: 0,
        truncated: false,
        no_git,
    }
}

fn backfill_empty_window(start_secs: i64, end_secs: i64) -> Vec<ActivityDay> {
    let mut days = Vec::new();
    let mut t = start_secs;
    while t <= end_secs {
        days.push(ActivityDay {
            date: date_from_secs(t),
            commits: 0,
            top_authors: Vec::new(),
        });
        t += 86_400;
    }
    days
}

/// Convert a Unix timestamp (seconds, UTC) to `YYYY-MM-DD`.
///
/// We avoid pulling in `chrono` / `time` for one date format. The
/// algorithm is Howard Hinnant's days-from-civil routine — exact for
/// any Gregorian date.
fn date_from_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i32, u8, u8) {
    // Reference: http://howardhinnant.github.io/date_algorithms.html#civil_from_days
    // Everything below stays in i64 to avoid signed/unsigned casts; the
    // algorithm only ever produces non-negative intermediates within the
    // year range we care about (1970..=2100-ish).
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // 0..=146_096
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // 0..=399
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0..=365
    let mp = (5 * doy + 2) / 153; // 0..=11
    let d_raw = doy - (153 * mp + 2) / 5 + 1; // 1..=31
    let m_raw = if mp < 10 { mp + 3 } else { mp - 9 }; // 1..=12
    if m_raw <= 2 {
        y += 1;
    }
    let m = u8::try_from(m_raw).unwrap_or(0);
    let d = u8::try_from(d_raw).unwrap_or(0);
    let y = i32::try_from(y).unwrap_or(0);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_from_secs_known_anchors() {
        // 1970-01-01 00:00 UTC → epoch 0
        assert_eq!(date_from_secs(0), "1970-01-01");
        // 2000-01-01 00:00 UTC → 946684800
        assert_eq!(date_from_secs(946_684_800), "2000-01-01");
        // 2026-05-17 00:00 UTC → 1778976000
        assert_eq!(date_from_secs(1_778_976_000), "2026-05-17");
        // Leap day:
        assert_eq!(date_from_secs(1_709_164_800), "2024-02-29");
    }

    #[test]
    fn no_git_falls_back_to_empty_window() {
        let dir = std::env::temp_dir().join(format!(
            "projectmind-heatmap-nogit-{}-{}",
            std::process::id(),
            uniq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let out = build(&dir);
        assert!(out.no_git, "fresh dir is not a git repo");
        assert_eq!(out.total_commits, 0);
        assert!(!out.days.is_empty(), "window backfilled");
        // Always at least 366 days inclusive of both ends.
        assert_eq!(out.days.len(), (WINDOW_DAYS as usize) + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn uniq() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(1);
        C.fetch_add(1, Ordering::Relaxed)
    }
}

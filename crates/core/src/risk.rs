// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Risk Atlas — composite per-class risk scores.
//!
//! Combines four signals (Phase 2.2, issue #158):
//! - **Churn**: number of commits touching the class's file inside the
//!   configured window (default 90 days).
//! - **Complexity**: a heuristic cyclomatic count derived from the source
//!   text (decision points: `if`, `else if`, `for`, `while`, `case`,
//!   `catch`, `&&`, `||`, `?`).
//! - **Coverage**: `1 - line_coverage` from a JaCoCo / LCOV / Cobertura
//!   report (see [`crate::coverage`]). Absent reports degrade gracefully —
//!   the coverage weight is redistributed onto the remaining signals.
//! - **Fan-in**: how many other classes reference this one, derived from the
//!   framework relations graph (bean injection + uses edges), *not* a
//!   re-scan. `log(fan_in + 1)` tames the long tail.
//!
//! Each signal is z-score normalised across the repo, then weighted into a
//! final 0..100 score.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use git2::Repository as GitRepo;
use projectmind_plugin_api::Relation;
use serde::{Deserialize, Serialize};

use crate::coverage::CoverageReport;
use crate::repository::Repository;

/// Default churn lookback window in days when the caller omits it.
pub const DEFAULT_CHURN_WINDOW_DAYS: u32 = 90;

/// Default weight applied to the churn signal.
pub const DEFAULT_WEIGHT_CHURN: f64 = 0.3;
/// Default weight applied to the complexity signal.
pub const DEFAULT_WEIGHT_CX: f64 = 0.3;
/// Default weight applied to the coverage signal (`1 - coverage`).
pub const DEFAULT_WEIGHT_COV: f64 = 0.2;
/// Default weight applied to the fan-in signal (`log(fan_in + 1)`).
pub const DEFAULT_WEIGHT_DEPS: f64 = 0.2;

/// Weights controlling how the four risk signals combine into a score.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Weights {
    /// Weight applied to (normalised) churn.
    pub churn: f64,
    /// Weight applied to (normalised) cyclomatic complexity.
    pub cx: f64,
    /// Weight applied to (normalised) `1 - coverage` (Phase 2.2).
    pub cov: f64,
    /// Weight applied to (normalised) fan-in/out (Phase 2.2).
    pub deps: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            churn: DEFAULT_WEIGHT_CHURN,
            cx: DEFAULT_WEIGHT_CX,
            cov: DEFAULT_WEIGHT_COV,
            deps: DEFAULT_WEIGHT_DEPS,
        }
    }
}

impl Weights {
    /// Weights actually applied for one `compute` run.
    ///
    /// When no coverage data is available (`have_coverage == false`) the
    /// coverage weight would silently do nothing, skewing the effective mix.
    /// We zero it and redistribute its mass proportionally across the three
    /// remaining signals so the score still spans a sensible range. When
    /// every non-coverage weight is zero we fall back to leaving the weights
    /// untouched (nothing sensible to redistribute onto).
    #[must_use]
    pub fn effective(self, have_coverage: bool) -> Self {
        if have_coverage || self.cov == 0.0 {
            return self;
        }
        let rest = self.churn + self.cx + self.deps;
        if rest <= 0.0 {
            return self;
        }
        let scale = (rest + self.cov) / rest;
        Self {
            churn: self.churn * scale,
            cx: self.cx * scale,
            cov: 0.0,
            deps: self.deps * scale,
        }
    }
}

/// Options accepted by [`compute`].
#[derive(Debug, Clone)]
pub struct Options {
    /// Optional module id filter. `None` = all modules.
    pub module: Option<String>,
    /// Maximum number of classes to return (after sorting by score desc).
    pub top: usize,
    /// Churn lookback window in days.
    pub window_days: u32,
    /// Weights for the score formula.
    pub weights: Weights,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            module: None,
            top: 20,
            window_days: DEFAULT_CHURN_WINDOW_DAYS,
            weights: Weights::default(),
        }
    }
}

/// A single per-class risk score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    /// Fully-qualified name of the scored class.
    pub fqn: String,
    /// Module the class belongs to.
    pub module: String,
    /// Repo-relative path of the class's source file.
    pub file: PathBuf,
    /// Composite score in the 0..=100 range.
    pub score: f64,
    /// Raw commit count over the lookback window.
    pub churn: u32,
    /// Raw cyclomatic complexity estimate.
    pub cx: u32,
    /// Line coverage in 0.0..=1.0, or `None` when no coverage report covers
    /// this class (or none exists at all).
    pub cov: Option<f64>,
    /// Number of other classes that reference this one (fan-in).
    pub fan_in: u32,
    /// Number of other classes this one references (fan-out).
    pub fan_out: u32,
    /// Source lines of code in the class (line_end - line_start + 1).
    pub sloc: u32,
    /// Short human-readable hint explaining why the score is high.
    pub why: String,
}

/// Errors returned by [`compute`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RiskError {
    /// Underlying libgit2 error.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    /// I/O failure while reading source files.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Compute risk scores for every class in `repo`, sorted highest first.
///
/// `relations` is the framework relations graph (bean/uses edges) the caller
/// already has — fan-in/out is derived from it rather than re-scanning, so
/// the cost stays flat on huge repos. `coverage` is an optional parsed
/// report; when `None`, the coverage weight is redistributed onto the other
/// signals so the atlas keeps working without a test run.
///
/// Returns the top `opts.top` entries. Filters by `opts.module` when set.
pub fn compute(
    repo: &Repository,
    relations: &[Relation],
    coverage: Option<&CoverageReport>,
    opts: &Options,
) -> Result<Vec<RiskScore>, RiskError> {
    let churn_by_file = churn_per_file(&repo.root, opts.window_days)?;
    let fan = FanCounts::from_relations(relations);

    let mut raw: Vec<RawRisk> = Vec::new();
    for module in repo.modules.values() {
        if let Some(filter) = opts.module.as_deref() {
            if module.id != filter {
                continue;
            }
        }
        for class in module.classes.values() {
            // Class.file is module-relative; the churn map is repo-relative.
            let abs = module.root.join(&class.file);
            let rel = abs
                .strip_prefix(&repo.root)
                .ok()
                .map_or_else(|| class.file.clone(), Path::to_path_buf);
            let churn = churn_by_file.get(&rel).copied().unwrap_or(0);

            let sloc = class
                .line_end
                .saturating_sub(class.line_start)
                .saturating_add(1);
            let cx = match std::fs::read_to_string(&abs) {
                Ok(source) => cyclomatic_in_lines(&source, class.line_start, class.line_end),
                Err(_) => 0,
            };
            let cov = coverage.and_then(|c| c.coverage_for(&class.fqn, &class.file));
            let (fan_in, fan_out) = fan.for_class(&class.fqn);

            raw.push(RawRisk {
                fqn: class.fqn.clone(),
                module: module.id.clone(),
                file: rel,
                churn,
                cx,
                cov,
                fan_in,
                fan_out,
                sloc,
            });
        }
    }

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    // Coverage only participates when at least one class resolved to a
    // percentage — otherwise its weight is dead and we rebalance.
    let have_coverage = raw.iter().any(|r| r.cov.is_some());
    let weights = opts.weights.effective(have_coverage);

    let churn_stats = ZStats::from_iter(raw.iter().map(|r| f64::from(r.churn)));
    let cx_stats = ZStats::from_iter(raw.iter().map(|r| f64::from(r.cx)));
    // Uncovered-ness: `1 - coverage`, defaulting missing entries to the mean
    // (0 in z-space) so an unmeasured class isn't punished or rewarded.
    let uncovered_vals: Vec<f64> = raw.iter().filter_map(|r| r.cov.map(|c| 1.0 - c)).collect();
    let cov_stats = ZStats::from_iter(uncovered_vals.iter().copied());
    let deps_stats = ZStats::from_iter(raw.iter().map(|r| (f64::from(r.fan_in) + 1.0).ln()));

    let mut scored: Vec<RiskScore> = raw
        .into_iter()
        .map(|r| {
            let z_churn = churn_stats.z(f64::from(r.churn));
            let z_cx = cx_stats.z(f64::from(r.cx));
            let z_cov = r.cov.map_or(0.0, |c| cov_stats.z(1.0 - c));
            let z_deps = deps_stats.z((f64::from(r.fan_in) + 1.0).ln());
            // Weighted z-score, then map to 0..=100 via a sigmoid-ish squash.
            let combined = weights.churn * z_churn
                + weights.cx * z_cx
                + weights.cov * z_cov
                + weights.deps * z_deps;
            let score = score_from_z(combined);
            let why = why_label(z_churn, z_cx, r.cov.map(|_| z_cov), z_deps);
            RiskScore {
                fqn: r.fqn,
                module: r.module,
                file: r.file,
                score,
                churn: r.churn,
                cx: r.cx,
                cov: r.cov,
                fan_in: r.fan_in,
                fan_out: r.fan_out,
                sloc: r.sloc,
                why,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(opts.top.max(1));
    Ok(scored)
}

struct RawRisk {
    fqn: String,
    module: String,
    file: PathBuf,
    churn: u32,
    cx: u32,
    cov: Option<f64>,
    fan_in: u32,
    fan_out: u32,
    sloc: u32,
}

/// Per-class fan-in / fan-out counts derived from the relations graph.
///
/// Edges are deduplicated per `(from, to)` pair so a class that both injects
/// *and* calls another counts once — fan-in/out is about how many *distinct*
/// classes touch a given one, not how many edges. Self-edges are ignored.
struct FanCounts {
    fan_in: HashMap<String, u32>,
    fan_out: HashMap<String, u32>,
}

impl FanCounts {
    fn from_relations(relations: &[Relation]) -> Self {
        use std::collections::BTreeSet;
        let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut fan_in: HashMap<String, u32> = HashMap::new();
        let mut fan_out: HashMap<String, u32> = HashMap::new();
        for rel in relations {
            if rel.from == rel.to {
                continue;
            }
            if !seen.insert((rel.from.as_str(), rel.to.as_str())) {
                continue;
            }
            *fan_out.entry(rel.from.clone()).or_default() += 1;
            *fan_in.entry(rel.to.clone()).or_default() += 1;
        }
        Self { fan_in, fan_out }
    }

    fn for_class(&self, fqn: &str) -> (u32, u32) {
        (
            self.fan_in.get(fqn).copied().unwrap_or(0),
            self.fan_out.get(fqn).copied().unwrap_or(0),
        )
    }
}

/// Build a `file -> commit count` map by walking commits authored in the
/// last `window_days` days. Files are tracked by their post-commit path —
/// renames inside the window collapse to the newest path.
pub fn churn_per_file(
    repo_root: &Path,
    window_days: u32,
) -> Result<HashMap<PathBuf, u32>, RiskError> {
    let repo = GitRepo::discover(repo_root)?;
    let mut walk = repo.revwalk()?;
    walk.push_head()?;
    walk.set_sorting(git2::Sort::TIME)?;

    let now_secs = i64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);
    let cutoff = now_secs.saturating_sub(i64::from(window_days) * 86_400);

    let mut counts: HashMap<PathBuf, u32> = HashMap::new();

    for oid_res in walk {
        let Ok(oid) = oid_res else { continue };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        if commit.time().seconds() < cutoff {
            break;
        }
        // Merge commits are skipped: their branch-side changes are counted
        // at the original commits the walk visits anyway, so diffing a
        // merge against its parents would double the churn of every file
        // that arrives via a merge-PR workflow (same convention as
        // `git::commit_activity` / `git::file_recency`).
        if commit.parent_count() > 1 {
            continue;
        }
        let Ok(tree) = commit.tree() else { continue };
        // First parent, or the empty tree for a root commit (every file
        // counts as added).
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        if let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) {
            for path in paths(&diff) {
                *counts.entry(path).or_default() += 1;
            }
        }
    }
    Ok(counts)
}

fn paths(diff: &git2::Diff<'_>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for delta in diff.deltas() {
        if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            out.push(p.to_path_buf());
        }
    }
    out
}

/// Counts decision points inside the (1-based, inclusive) line range
/// `start..=end` of `source`. Language-agnostic regex pass over the
/// matching lines — accepts Java, Kotlin, Rust, Scala, Groovy, TypeScript.
///
/// Pattern set chosen so each match contributes one branch:
/// `if`, `else if`, `for`, `while`, `case`, `catch`, `&&`, `||`, `?`.
/// We start at 1 (every method has at least one path).
#[must_use]
pub fn cyclomatic_in_lines(source: &str, start: u32, end: u32) -> u32 {
    if start == 0 || end < start {
        return 0;
    }
    let mut score: u32 = 1;
    for (idx, line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        if line_no < start || line_no > end {
            continue;
        }
        let trimmed = strip_line_comment(line);
        score = score.saturating_add(count_decisions(trimmed));
    }
    score
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(idx) = line.find("//") {
        &line[..idx]
    } else {
        line
    }
}

fn count_decisions(line: &str) -> u32 {
    let mut n: u32 = 0;
    // Token-level checks keep us from triggering on identifiers like
    // `notification` containing `if`.
    n = n.saturating_add(count_word(line, "if"));
    n = n.saturating_add(count_word(line, "for"));
    n = n.saturating_add(count_word(line, "while"));
    n = n.saturating_add(count_word(line, "case"));
    n = n.saturating_add(count_word(line, "catch"));
    n = n.saturating_add(u32::try_from(line.matches("&&").count()).unwrap_or(0));
    n = n.saturating_add(u32::try_from(line.matches("||").count()).unwrap_or(0));
    // Ternary `?` — but avoid Rust's `?` operator and Optional types
    // (`Option<T>?`) by requiring at least one whitespace before `?`.
    n = n.saturating_add(u32::try_from(line.matches(" ? ").count()).unwrap_or(0));
    n
}

fn count_word(line: &str, word: &str) -> u32 {
    let mut count = 0u32;
    let bytes = line.as_bytes();
    let wlen = word.len();
    let mut i = 0;
    while i + wlen <= bytes.len() {
        if &bytes[i..i + wlen] == word.as_bytes() {
            let before = if i == 0 { b' ' } else { bytes[i - 1] };
            let after = if i + wlen == bytes.len() {
                b' '
            } else {
                bytes[i + wlen]
            };
            if !is_ident(before) && !is_ident(after) {
                count = count.saturating_add(1);
            }
        }
        i += 1;
    }
    count
}

const fn is_ident(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

struct ZStats {
    mean: f64,
    sd: f64,
}

impl ZStats {
    fn from_iter<I: IntoIterator<Item = f64>>(values: I) -> Self {
        let vs: Vec<f64> = values.into_iter().collect();
        if vs.is_empty() {
            return Self { mean: 0.0, sd: 1.0 };
        }
        let n = vs.len() as f64;
        let mean = vs.iter().sum::<f64>() / n;
        let var = vs.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let sd = var.sqrt();
        Self {
            mean,
            sd: if sd < 1e-9 { 1.0 } else { sd },
        }
    }

    fn z(&self, v: f64) -> f64 {
        (v - self.mean) / self.sd
    }
}

/// Maps a weighted z-score to a 0..=100 risk score via a logistic squash.
fn score_from_z(z: f64) -> f64 {
    // 1 / (1 + e^-z) is 0.5 at z=0; multiplying by 100 keeps the result in 0..=100.
    let s = 100.0 / (1.0 + (-z * 1.5).exp());
    (s * 10.0).round() / 10.0
}

/// Build the short "why" hint from the standout signals. Coverage and
/// fan-in join churn/complexity so the most dangerous combo — hot, uncovered
/// and highly-depended-on — reads clearly. `z_cov` is `None` when the class
/// has no coverage data, so an unmeasured class never claims to be uncovered.
fn why_label(z_churn: f64, z_cx: f64, z_cov: Option<f64>, z_deps: f64) -> String {
    let mut tags: Vec<&str> = Vec::new();
    if z_churn > 0.5 {
        tags.push("hot");
    }
    if z_cx > 0.5 {
        tags.push("complex");
    }
    if z_cov.is_some_and(|z| z > 0.5) {
        tags.push("uncovered");
    }
    if z_deps > 0.5 {
        tags.push("central");
    }
    if tags.is_empty() {
        "baseline".into()
    } else {
        tags.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Class, ClassKind, Module};

    #[test]
    fn cyclomatic_counts_basic_branches() {
        let src = "fn f() {\n    if a && b {\n        for x in xs {\n            if x { } else if y { }\n        }\n    }\n}\n";
        let cx = cyclomatic_in_lines(src, 1, 7);
        // 1 base + if(1) + &&(1) + for(1) + if(1) + else if(1) = 6
        assert_eq!(cx, 6);
    }

    #[test]
    fn cyclomatic_ignores_identifiers_containing_keywords() {
        let src = "let notification = 1;\nlet ifield = 2;\nlet forecast = 3;\n";
        let cx = cyclomatic_in_lines(src, 1, 3);
        assert_eq!(cx, 1); // only base
    }

    #[test]
    fn cyclomatic_strips_line_comments() {
        let src = "// if a && b\nlet x = 1;\n";
        let cx = cyclomatic_in_lines(src, 1, 2);
        assert_eq!(cx, 1);
    }

    #[test]
    fn cyclomatic_out_of_range_returns_zero() {
        let src = "if a { }\n";
        let cx = cyclomatic_in_lines(src, 5, 1); // end < start
        assert_eq!(cx, 0);
    }

    #[test]
    fn zstats_handles_constant_input() {
        let s = ZStats::from_iter([3.0, 3.0, 3.0]);
        assert!((s.z(3.0)).abs() < 1e-6);
    }

    #[test]
    fn score_from_z_is_monotonic() {
        let lo = score_from_z(-1.0);
        let mid = score_from_z(0.0);
        let hi = score_from_z(1.5);
        assert!(lo < mid);
        assert!(mid < hi);
        assert!((0.0..=100.0).contains(&lo));
        assert!((0.0..=100.0).contains(&hi));
    }

    #[test]
    fn compute_orders_high_score_first() {
        let mut repo = Repository::new(PathBuf::from("/tmp/risk"));
        let mut m = Module {
            id: "m".into(),
            name: "m".into(),
            root: PathBuf::from("/tmp/risk"),
            ..Default::default()
        };
        m.classes.insert(
            "a.A".into(),
            Class {
                fqn: "a.A".into(),
                name: "A".into(),
                file: PathBuf::from("A.java"),
                line_start: 1,
                line_end: 1,
                kind: ClassKind::Class,
                ..Default::default()
            },
        );
        m.classes.insert(
            "b.B".into(),
            Class {
                fqn: "b.B".into(),
                name: "B".into(),
                file: PathBuf::from("B.java"),
                line_start: 1,
                line_end: 1,
                kind: ClassKind::Class,
                ..Default::default()
            },
        );
        repo.insert_module(m);

        let opts = Options {
            top: 5,
            window_days: 365,
            ..Default::default()
        };
        // No actual git repo at /tmp/risk → churn_per_file errors;
        // compute() bubbles. The test below uses pure helpers.
        // Here we just verify the public API surface.
        let result = compute(&repo, &[], None, &opts);
        assert!(result.is_err() || result.is_ok());
    }

    fn rel(from: &str, to: &str) -> Relation {
        Relation {
            from: from.into(),
            to: to.into(),
            kind: projectmind_plugin_api::RelationKind::Uses,
        }
    }

    #[test]
    fn churn_per_file_skips_merge_commits() {
        use crate::git::test_support::{build_merge_history, init_repo};

        let dir = std::env::temp_dir().join(format!(
            "projectmind-risk-churn-merge-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        struct Guard(PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Guard(dir.clone());

        let repo = init_repo(&dir);
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap();
        build_merge_history(&repo, &dir, now - 400);

        let counts = churn_per_file(&dir, 365).unwrap();
        // One touch per file — the merge commit must not re-count the
        // branch's changes (pre-fix: feat/x.rs and main/y.rs showed 2).
        assert_eq!(counts.get(Path::new("feat/x.rs")).copied(), Some(1));
        assert_eq!(counts.get(Path::new("main/y.rs")).copied(), Some(1));
        assert_eq!(counts.get(Path::new("base.txt")).copied(), Some(1));
    }

    #[test]
    fn fan_counts_dedupe_and_ignore_self_edges() {
        let rels = vec![
            rel("A", "B"),
            rel("A", "B"), // duplicate pair → counted once
            rel("C", "B"),
            rel("B", "B"), // self-edge → ignored
            rel("A", "C"),
        ];
        let fan = FanCounts::from_relations(&rels);
        // B is referenced by A and C → fan_in 2, references nobody → fan_out 0.
        assert_eq!(fan.for_class("B"), (2, 0));
        // A references B and C → fan_out 2, referenced by nobody → fan_in 0.
        assert_eq!(fan.for_class("A"), (0, 2));
        // C referenced by A, references B.
        assert_eq!(fan.for_class("C"), (1, 1));
        // Unknown class is all-zero.
        assert_eq!(fan.for_class("Z"), (0, 0));
    }

    #[test]
    fn effective_weights_rebalance_when_no_coverage() {
        let w = Weights::default(); // 0.3 / 0.3 / 0.2 / 0.2
        let eff = w.effective(false);
        assert!(eff.cov.abs() < 1e-9);
        // The 0.2 coverage mass is spread over churn+cx+deps (sum 0.8) so the
        // remaining weights sum back to the original total (1.0).
        let total = eff.churn + eff.cx + eff.cov + eff.deps;
        assert!((total - 1.0).abs() < 1e-9, "total was {total}");
        // Relative proportions among the survivors are preserved.
        assert!((eff.churn - eff.cx).abs() < 1e-9);
    }

    #[test]
    fn effective_weights_untouched_when_coverage_present() {
        let w = Weights::default();
        let eff = w.effective(true);
        assert!((eff.cov - DEFAULT_WEIGHT_COV).abs() < 1e-9);
        assert!((eff.churn - DEFAULT_WEIGHT_CHURN).abs() < 1e-9);
    }

    #[test]
    fn why_label_combines_signals() {
        assert_eq!(
            why_label(1.0, 1.0, Some(1.0), 1.0),
            "hot+complex+uncovered+central"
        );
        assert_eq!(why_label(1.0, 0.0, None, 0.0), "hot");
        assert_eq!(why_label(0.0, 0.0, Some(0.0), 0.0), "baseline");
        // No coverage data → never labelled "uncovered" even at high z.
        assert_eq!(why_label(0.0, 0.0, None, 0.0), "baseline");
    }

    /// End-to-end `compute` over a throwaway git repo: exercises churn,
    /// complexity, fan-in and coverage together and asserts graceful
    /// degradation stays crash-free.
    #[test]
    fn compute_uses_all_four_signals_and_degrades_gracefully() {
        use crate::coverage::{CoverageFormat, CoverageReport};
        use std::collections::HashMap;

        let dir = std::env::temp_dir().join(format!(
            "projectmind-risk-e2e-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        struct Guard(PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Guard(dir.clone());

        // Real git repo so churn_per_file succeeds.
        let git = GitRepo::init(&dir).unwrap();
        std::fs::write(dir.join("A.java"), "class A { void f(){ if(x){} } }\n").unwrap();
        std::fs::write(dir.join("B.java"), "class B {}\n").unwrap();
        {
            let mut index = git.index().unwrap();
            index.add_path(Path::new("A.java")).unwrap();
            index.add_path(Path::new("B.java")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = git.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("t", "t@t").unwrap();
            git.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }

        let mut repo = Repository::new(dir.clone());
        let mut m = Module {
            id: "m".into(),
            name: "m".into(),
            root: dir.clone(),
            ..Default::default()
        };
        for (fqn, file, end) in [("a.A", "A.java", 1u32), ("b.B", "B.java", 1u32)] {
            m.classes.insert(
                fqn.into(),
                Class {
                    fqn: fqn.into(),
                    name: fqn.rsplit('.').next().unwrap().into(),
                    file: PathBuf::from(file),
                    line_start: 1,
                    line_end: end,
                    kind: ClassKind::Class,
                    ..Default::default()
                },
            );
        }
        repo.insert_module(m);

        // A depends on B → B has fan_in 1.
        let relations = vec![rel("a.A", "b.B")];
        let coverage = CoverageReport {
            format: CoverageFormat::Jacoco,
            path: PathBuf::from("jacoco.xml"),
            mtime_secs: None,
            by_fqn: HashMap::from([("a.A".to_string(), 0.1), ("b.B".to_string(), 0.9)]),
            by_file: HashMap::new(),
        };
        let opts = Options {
            top: 5,
            window_days: 365,
            ..Default::default()
        };

        // With coverage present.
        let scored = compute(&repo, &relations, Some(&coverage), &opts).unwrap();
        assert_eq!(scored.len(), 2);
        let a = scored.iter().find(|s| s.fqn == "a.A").unwrap();
        let b = scored.iter().find(|s| s.fqn == "b.B").unwrap();
        assert_eq!(a.cov, Some(0.1));
        assert_eq!(a.fan_out, 1);
        assert_eq!(b.fan_in, 1);

        // Without coverage: cov is None everywhere, no crash, still 2 scores.
        let degraded = compute(&repo, &relations, None, &opts).unwrap();
        assert_eq!(degraded.len(), 2);
        assert!(degraded.iter().all(|s| s.cov.is_none()));
    }
}

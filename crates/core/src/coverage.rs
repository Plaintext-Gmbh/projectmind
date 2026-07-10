// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Coverage loader — per-class line coverage from test reports.
//!
//! Detects and parses the coverage report a test run leaves behind, so the
//! Risk Atlas can fold "uncovered" into its composite score (Phase 2.2,
//! issue #158). Three formats are recognised, in priority order:
//!
//! | Format    | Detection glob                                            | Producer          |
//! |-----------|-----------------------------------------------------------|-------------------|
//! | JaCoCo    | `**/target/site/jacoco/jacoco.xml`                        | Maven (Java)      |
//! | LCOV      | `**/lcov.info`, `**/target/llvm-cov-target/lcov.info`     | cargo-llvm-cov/c8 |
//! | Cobertura | `**/cobertura.xml`                                         | generic           |
//!
//! Coverage is stored two ways so the scorer can resolve a class to a
//! percentage regardless of which report produced it:
//!
//! - `by_fqn`: keyed by fully-qualified class name (JaCoCo reports this
//!   directly).
//! - `by_file`: keyed by the report-relative source file path, matched
//!   against a class's source file by *suffix* (LCOV / Cobertura report
//!   file paths, not FQNs).
//!
//! Detection is a **single working-tree walk** — no re-scan per class. The
//! report's mtime is captured so the UI can flag stale data (`> 24h`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

/// How many seconds old a report may be before the UI calls it stale.
pub const STALE_AFTER_SECS: u64 = 24 * 60 * 60;

/// Which report format produced a [`CoverageReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageFormat {
    /// JaCoCo XML (`jacoco.xml`).
    Jacoco,
    /// LCOV tracefile (`lcov.info`).
    Lcov,
    /// Cobertura XML (`cobertura.xml`).
    Cobertura,
}

impl CoverageFormat {
    /// Stable lowercase id (for JSON payloads / the UI badge).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jacoco => "jacoco",
            Self::Lcov => "lcov",
            Self::Cobertura => "cobertura",
        }
    }
}

/// Parsed coverage for one repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Which format produced this report.
    pub format: CoverageFormat,
    /// Repo-relative path of the report file that was parsed.
    pub path: PathBuf,
    /// Report file mtime as seconds since the Unix epoch, when available.
    /// `None` when the platform / filesystem doesn't expose it.
    pub mtime_secs: Option<u64>,
    /// Line coverage keyed by fully-qualified class name (0.0..=1.0).
    pub by_fqn: HashMap<String, f64>,
    /// Line coverage keyed by report source-file path (0.0..=1.0).
    pub by_file: HashMap<String, f64>,
}

impl CoverageReport {
    /// Resolve line coverage (0.0..=1.0) for a class.
    ///
    /// Tries an exact FQN hit first (JaCoCo), then falls back to matching the
    /// class's `source_file` against the file-keyed map by path *suffix* —
    /// report paths and repo-relative paths rarely share a common prefix, but
    /// the tail (`.../com/acme/Foo.java`) is stable.
    #[must_use]
    pub fn coverage_for(&self, fqn: &str, source_file: &Path) -> Option<f64> {
        if let Some(pct) = self.by_fqn.get(fqn) {
            return Some(*pct);
        }
        if self.by_file.is_empty() {
            return None;
        }
        let needle = normalize_path(&source_file.to_string_lossy());
        // Prefer the longest matching suffix so `Foo.java` doesn't shadow
        // `com/acme/Foo.java` when both are present.
        let mut best: Option<f64> = None;
        let mut best_len = 0usize;
        for (file, pct) in &self.by_file {
            let cand = normalize_path(file);
            if path_suffix_match(&cand, &needle) {
                let len = cand.len().min(needle.len());
                if len >= best_len {
                    best_len = len;
                    best = Some(*pct);
                }
            }
        }
        best
    }

    /// Age of the report in seconds, relative to now. `None` when the mtime
    /// is unknown or lies in the future (clock skew).
    #[must_use]
    pub fn age_secs(&self) -> Option<u64> {
        let mtime = self.mtime_secs?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        now.checked_sub(mtime)
    }

    /// Whether the report is older than [`STALE_AFTER_SECS`].
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.age_secs().is_some_and(|age| age > STALE_AFTER_SECS)
    }
}

/// Detect and parse the best coverage report under `repo_root`.
///
/// Walks the working tree once (respecting `.gitignore` like the parse
/// pipeline does, but *keeping* `target/` since that's where JaCoCo and
/// llvm-cov write). Returns the first report found in format priority order:
/// JaCoCo → LCOV → Cobertura. `None` when no report exists — the caller
/// degrades gracefully.
#[must_use]
pub fn load(repo_root: &Path) -> Option<CoverageReport> {
    let mut jacoco: Option<PathBuf> = None;
    let mut lcov: Option<PathBuf> = None;
    let mut cobertura: Option<PathBuf> = None;

    let walker = WalkBuilder::new(repo_root)
        .standard_filters(false)
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .build();
    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        match name {
            "jacoco.xml" if jacoco.is_none() && in_jacoco_dir(path) => {
                jacoco = Some(path.to_path_buf());
            }
            "lcov.info" if lcov.is_none() => {
                lcov = Some(path.to_path_buf());
            }
            "cobertura.xml" if cobertura.is_none() => {
                cobertura = Some(path.to_path_buf());
            }
            _ => {}
        }
    }

    if let Some(p) = jacoco {
        if let Ok(text) = std::fs::read_to_string(&p) {
            return Some(finish(
                repo_root,
                &p,
                CoverageFormat::Jacoco,
                parse_jacoco(&text),
            ));
        }
    }
    if let Some(p) = lcov {
        if let Ok(text) = std::fs::read_to_string(&p) {
            return Some(finish(
                repo_root,
                &p,
                CoverageFormat::Lcov,
                parse_lcov(&text),
            ));
        }
    }
    if let Some(p) = cobertura {
        if let Ok(text) = std::fs::read_to_string(&p) {
            return Some(finish(
                repo_root,
                &p,
                CoverageFormat::Cobertura,
                parse_cobertura(&text),
            ));
        }
    }
    None
}

/// Only accept a `jacoco.xml` sitting in a `.../site/jacoco/` directory so a
/// stray file elsewhere doesn't masquerade as the Maven report.
fn in_jacoco_dir(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == "jacoco")
}

/// Assemble a [`CoverageReport`] from parsed maps, capturing mtime + a
/// repo-relative path for display.
fn finish(
    repo_root: &Path,
    report_path: &Path,
    format: CoverageFormat,
    parsed: Parsed,
) -> CoverageReport {
    let mtime_secs = std::fs::metadata(report_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let rel = report_path
        .strip_prefix(repo_root)
        .map_or_else(|_| report_path.to_path_buf(), Path::to_path_buf);
    CoverageReport {
        format,
        path: rel,
        mtime_secs,
        by_fqn: parsed.by_fqn,
        by_file: parsed.by_file,
    }
}

/// Intermediate parser output before mtime / path enrichment.
#[derive(Debug, Default)]
struct Parsed {
    by_fqn: HashMap<String, f64>,
    by_file: HashMap<String, f64>,
}

/// Parse a JaCoCo XML report into per-class line coverage.
///
/// JaCoCo nests `<class name="com/acme/Foo">` elements, each carrying a
/// `<counter type="LINE" missed=".." covered="..">`. We convert the slash
/// name to a dotted FQN (dropping any `$Inner` suffix so the score lands on
/// the top-level class the parser knows about).
#[must_use]
fn parse_jacoco(xml: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut current: Option<String> = None;
    let mut idx = 0;
    let bytes = xml.as_bytes();
    while idx < bytes.len() {
        if let Some(rel) = xml[idx..].find('<') {
            let start = idx + rel + 1;
            let Some(end_rel) = xml[start..].find('>') else {
                break;
            };
            let end = start + end_rel;
            let tag = &xml[start..end];
            idx = end + 1;
            if let Some(rest) = tag.strip_prefix("class ") {
                if let Some(name) = attr(rest, "name") {
                    current = Some(slash_to_fqn(&name));
                }
            } else if tag.starts_with("/class") {
                current = None;
            } else if tag.starts_with("counter ") {
                if let Some(fqn) = current.clone() {
                    if attr(tag, "type").as_deref() == Some("LINE") {
                        let covered = attr(tag, "covered")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let missed = attr(tag, "missed")
                            .and_then(|v| v.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let total = covered + missed;
                        if total > 0.0 {
                            // Only the class-level LINE counter matters; JaCoCo
                            // emits method-level counters too, but the class one
                            // comes first inside <class>, so keep the first hit.
                            out.by_fqn.entry(fqn).or_insert(covered / total);
                        }
                    }
                }
            }
        } else {
            break;
        }
    }
    out
}

/// Parse an LCOV tracefile into per-file line coverage.
///
/// LCOV groups records per source file:
/// ```text
/// SF:<path>
/// LH:<lines hit>
/// LF:<lines found>
/// end_of_record
/// ```
/// We prefer the `LH`/`LF` summary lines; when a producer omits them we fall
/// back to counting `DA:<line>,<hits>` records.
#[must_use]
fn parse_lcov(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut file: Option<String> = None;
    let mut lh: Option<f64> = None;
    let mut lf: Option<f64> = None;
    let mut da_hit = 0u32;
    let mut da_total = 0u32;
    for line in text.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            file = Some(path.to_string());
            lh = None;
            lf = None;
            da_hit = 0;
            da_total = 0;
        } else if let Some(v) = line.strip_prefix("LH:") {
            lh = v.trim().parse::<f64>().ok();
        } else if let Some(v) = line.strip_prefix("LF:") {
            lf = v.trim().parse::<f64>().ok();
        } else if let Some(v) = line.strip_prefix("DA:") {
            if let Some((_, hits)) = v.split_once(',') {
                da_total += 1;
                if hits.trim().parse::<u64>().unwrap_or(0) > 0 {
                    da_hit += 1;
                }
            }
        } else if line == "end_of_record" {
            if let Some(f) = file.take() {
                let pct = match (lh, lf) {
                    (Some(hit), Some(total)) if total > 0.0 => Some(hit / total),
                    _ if da_total > 0 => Some(f64::from(da_hit) / f64::from(da_total)),
                    _ => None,
                };
                if let Some(pct) = pct {
                    out.by_file.insert(f, pct);
                }
            }
            lh = None;
            lf = None;
            da_hit = 0;
            da_total = 0;
        }
    }
    out
}

/// Parse a Cobertura XML report into per-file line coverage.
///
/// Cobertura carries `<class filename="src/foo.py" line-rate="0.83">` — the
/// `line-rate` is already a 0..1 fraction, so we read it directly and key by
/// filename.
#[must_use]
fn parse_cobertura(xml: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut idx = 0;
    while let Some(rel) = xml[idx..].find("<class ") {
        let start = idx + rel + 1;
        let Some(end_rel) = xml[start..].find('>') else {
            break;
        };
        let end = start + end_rel;
        let tag = &xml[start..end];
        idx = end + 1;
        let (Some(file), Some(rate)) = (attr(tag, "filename"), attr(tag, "line-rate")) else {
            continue;
        };
        if let Ok(pct) = rate.parse::<f64>() {
            // Cobertura may repeat a filename across inner classes; keep the
            // first (top-level) entry to match how the parser sees the class.
            out.by_file.entry(file).or_insert(pct.clamp(0.0, 1.0));
        }
    }
    out
}

/// Read the value of a single XML attribute from a tag body (`name="value"`).
/// Deliberately tiny — coverage reports are machine-generated and
/// well-formed, so we avoid pulling in a full XML parser.
fn attr(tag: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// `com/acme/Foo` → `com.acme.Foo`, dropping any `$Inner` suffix.
fn slash_to_fqn(name: &str) -> String {
    let base = name.split('$').next().unwrap_or(name);
    base.replace('/', ".")
}

/// Normalise a path for suffix comparison: forward slashes, no leading `./`.
fn normalize_path(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

/// True when `a` and `b` share a path suffix on `/`-segment boundaries —
/// i.e. one ends with the other's tail. Guards against `Foo.java` matching
/// `BarFoo.java` by comparing whole segments.
fn path_suffix_match(a: &str, b: &str) -> bool {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if long == short {
        return true;
    }
    long.ends_with(short) && long.as_bytes()[long.len() - short.len() - 1] == b'/'
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Minimal scratch directory, cleaned up on drop. Mirrors the helper in
    /// `code_graph_sqlite` so the core crate needs no `tempfile` dev-dep.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-cov-test-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const JACOCO: &str = r#"<?xml version="1.0"?>
<report name="demo">
  <package name="com/acme">
    <class name="com/acme/Foo" sourcefilename="Foo.java">
      <counter type="LINE" missed="2" covered="8"/>
    </class>
    <class name="com/acme/Bar" sourcefilename="Bar.java">
      <counter type="LINE" missed="10" covered="0"/>
    </class>
    <class name="com/acme/Baz$Inner" sourcefilename="Baz.java">
      <counter type="LINE" missed="0" covered="4"/>
    </class>
  </package>
</report>"#;

    const LCOV: &str = "TN:\nSF:src/lib/foo.rs\nDA:1,1\nDA:2,0\nLF:2\nLH:1\nend_of_record\nSF:src/lib/bar.rs\nDA:1,1\nDA:2,1\nend_of_record\n";

    const COBERTURA: &str = r#"<?xml version="1.0"?>
<coverage>
  <packages>
    <package name="app">
      <classes>
        <class name="foo" filename="app/foo.py" line-rate="0.75"/>
        <class name="bar" filename="app/bar.py" line-rate="0.0"/>
      </classes>
    </package>
  </packages>
</coverage>"#;

    #[test]
    fn jacoco_parses_line_coverage_by_fqn() {
        let p = parse_jacoco(JACOCO);
        assert!((p.by_fqn["com.acme.Foo"] - 0.8).abs() < 1e-9);
        assert!((p.by_fqn["com.acme.Bar"] - 0.0).abs() < 1e-9);
        // Inner class folds onto the top-level Baz.
        assert!((p.by_fqn["com.acme.Baz"] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn lcov_prefers_lh_lf_summary_then_falls_back_to_da() {
        let p = parse_lcov(LCOV);
        // foo.rs: LH=1 LF=2 → 0.5
        assert!((p.by_file["src/lib/foo.rs"] - 0.5).abs() < 1e-9);
        // bar.rs: no LH/LF, DA 2/2 → 1.0
        assert!((p.by_file["src/lib/bar.rs"] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cobertura_reads_line_rate_by_file() {
        let p = parse_cobertura(COBERTURA);
        assert!((p.by_file["app/foo.py"] - 0.75).abs() < 1e-9);
        assert!((p.by_file["app/bar.py"] - 0.0).abs() < 1e-9);
    }

    #[test]
    fn coverage_for_matches_fqn_first() {
        let report = CoverageReport {
            format: CoverageFormat::Jacoco,
            path: PathBuf::from("jacoco.xml"),
            mtime_secs: None,
            by_fqn: HashMap::from([("com.acme.Foo".to_string(), 0.8)]),
            by_file: HashMap::new(),
        };
        let cov = report.coverage_for("com.acme.Foo", Path::new("Foo.java"));
        assert_eq!(cov, Some(0.8));
    }

    #[test]
    fn coverage_for_matches_file_by_suffix() {
        let report = CoverageReport {
            format: CoverageFormat::Lcov,
            path: PathBuf::from("lcov.info"),
            mtime_secs: None,
            by_fqn: HashMap::new(),
            by_file: HashMap::from([("crates/core/src/foo.rs".to_string(), 0.5)]),
        };
        // Class file is module-relative; suffix still resolves.
        let cov = report.coverage_for("foo", Path::new("src/foo.rs"));
        assert_eq!(cov, Some(0.5));
    }

    #[test]
    fn coverage_for_prefers_longest_suffix_match() {
        let report = CoverageReport {
            format: CoverageFormat::Lcov,
            path: PathBuf::from("lcov.info"),
            mtime_secs: None,
            by_fqn: HashMap::new(),
            by_file: HashMap::from([
                ("a/Foo.java".to_string(), 0.1),
                ("x/y/z/Foo.java".to_string(), 0.9),
            ]),
        };
        // Only the longer path shares the full `y/z/Foo.java` tail.
        let cov = report.coverage_for("Foo", Path::new("y/z/Foo.java"));
        assert_eq!(cov, Some(0.9));
    }

    #[test]
    fn suffix_match_respects_segment_boundaries() {
        assert!(path_suffix_match("com/acme/Foo.java", "Foo.java"));
        assert!(path_suffix_match("a/b/Foo.java", "b/Foo.java"));
        // `BarFoo.java` must NOT match `Foo.java`.
        assert!(!path_suffix_match("a/BarFoo.java", "Foo.java"));
        assert!(path_suffix_match("Foo.java", "Foo.java"));
    }

    #[test]
    fn stale_uses_mtime_age() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let fresh = CoverageReport {
            format: CoverageFormat::Jacoco,
            path: PathBuf::from("jacoco.xml"),
            mtime_secs: Some(now),
            by_fqn: HashMap::new(),
            by_file: HashMap::new(),
        };
        assert!(!fresh.is_stale());
        let old = CoverageReport {
            mtime_secs: Some(now.saturating_sub(STALE_AFTER_SECS + 60)),
            ..fresh.clone()
        };
        assert!(old.is_stale());
    }

    #[test]
    fn load_finds_jacoco_in_priority_order() {
        let dir = TempDir::new();
        let root = dir.path();
        // JaCoCo under target/site/jacoco.
        let jdir = root.join("target/site/jacoco");
        std::fs::create_dir_all(&jdir).unwrap();
        std::fs::write(jdir.join("jacoco.xml"), JACOCO).unwrap();
        // An LCOV file too — JaCoCo wins.
        std::fs::write(root.join("lcov.info"), LCOV).unwrap();

        let report = load(root).expect("a report is found");
        assert_eq!(report.format, CoverageFormat::Jacoco);
        assert!(report.by_fqn.contains_key("com.acme.Foo"));
    }

    #[test]
    fn load_returns_none_without_reports() {
        let dir = TempDir::new();
        assert!(load(dir.path()).is_none());
    }

    #[test]
    fn load_falls_back_to_lcov() {
        let dir = TempDir::new();
        let root = dir.path();
        std::fs::write(root.join("lcov.info"), LCOV).unwrap();
        let report = load(root).expect("lcov found");
        assert_eq!(report.format, CoverageFormat::Lcov);
        assert!(report.by_file.contains_key("src/lib/foo.rs"));
    }
}

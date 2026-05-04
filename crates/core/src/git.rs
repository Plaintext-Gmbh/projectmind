// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lightweight git helpers — list changed files, render diffs, build per-file
//! recency indexes for change-map visualisations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use git2::{Diff, DiffOptions, Repository as GitRepo};
use serde::{Deserialize, Serialize};

/// Status of a changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Newly added.
    Added,
    /// Modified.
    Modified,
    /// Deleted.
    Deleted,
    /// Renamed.
    Renamed,
    /// Type changed (e.g. file → symlink).
    TypeChange,
    /// Unknown / other.
    Other,
}

/// One entry in a list of changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    /// Repository-relative path.
    pub path: PathBuf,
    /// Status.
    pub status: FileStatus,
}

/// Errors from git operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GitError {
    /// Underlying libgit2 error.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    /// The provided git ref could not be resolved.
    #[error("ref not found: {0}")]
    RefNotFound(String),
}

/// List files that changed between `from_ref` and the working tree (or `to_ref` if provided).
pub fn list_changes_since(
    repo_root: &Path,
    from_ref: &str,
    to_ref: Option<&str>,
) -> Result<Vec<ChangedFile>, GitError> {
    let repo = GitRepo::discover(repo_root)?;
    let (from_ref, to_ref) = split_range(from_ref, to_ref);
    let from_tree = resolve_tree(&repo, from_ref)?;

    let diff = if let Some(to) = to_ref {
        let to_tree = resolve_tree(&repo, to)?;
        repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut diff_opts()))?
    } else {
        repo.diff_tree_to_workdir_with_index(Some(&from_tree), Some(&mut diff_opts()))?
    };

    Ok(collect_changes(&diff))
}

/// Render a unified diff between `from_ref` and `to_ref` (or working tree).
pub fn unified_diff(
    repo_root: &Path,
    from_ref: &str,
    to_ref: Option<&str>,
) -> Result<String, GitError> {
    let repo = GitRepo::discover(repo_root)?;
    let (from_ref, to_ref) = split_range(from_ref, to_ref);
    let from_tree = resolve_tree(&repo, from_ref)?;
    let diff = if let Some(to) = to_ref {
        let to_tree = resolve_tree(&repo, to)?;
        repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut diff_opts()))?
    } else {
        repo.diff_tree_to_workdir_with_index(Some(&from_tree), Some(&mut diff_opts()))?
    };

    let mut out = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let prefix = match line.origin() {
            '+' | '-' | ' ' => format!("{}", line.origin()),
            _ => String::new(),
        };
        out.push_str(&prefix);
        out.push_str(std::str::from_utf8(line.content()).unwrap_or(""));
        true
    })?;
    Ok(out)
}

/// Accept the git CLI shorthand `A..B` inside `from_ref` and split it into
/// `(A, Some(B))`. If `to_ref` is already set, it wins — the caller passed an
/// explicit second ref and we treat the inline range's tail as redundant.
/// `A...B` (three dots = symmetric difference) is treated like `A..B` here;
/// libgit2 doesn't model the merge base on this path either way.
fn split_range<'a>(from_ref: &'a str, to_ref: Option<&'a str>) -> (&'a str, Option<&'a str>) {
    if to_ref.is_some() {
        return (from_ref, to_ref);
    }
    let sep = if from_ref.contains("...") {
        "..."
    } else {
        ".."
    };
    if let Some((from, to)) = from_ref.split_once(sep) {
        if !from.is_empty() && !to.is_empty() {
            return (from, Some(to));
        }
    }
    (from_ref, to_ref)
}

fn resolve_tree<'a>(repo: &'a GitRepo, name: &str) -> Result<git2::Tree<'a>, GitError> {
    let obj = repo
        .revparse_single(name)
        .map_err(|_| GitError::RefNotFound(name.to_string()))?;
    let tree = obj
        .peel_to_tree()
        .map_err(|_| GitError::RefNotFound(format!("{name} (cannot peel to tree)")))?;
    Ok(tree)
}

fn diff_opts() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    opts
}

fn collect_changes(diff: &Diff<'_>) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    for delta in diff.deltas() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let status = match delta.status() {
            git2::Delta::Added | git2::Delta::Copied => FileStatus::Added,
            git2::Delta::Modified => FileStatus::Modified,
            git2::Delta::Deleted => FileStatus::Deleted,
            git2::Delta::Renamed => FileStatus::Renamed,
            git2::Delta::Typechange => FileStatus::TypeChange,
            _ => FileStatus::Other,
        };
        out.push(ChangedFile { path, status });
    }
    out
}

/// Per-file recency record. One entry per repo path that's been touched in
/// the visited commit window. The first commit that touches a path wins —
/// that's the "last edit", since revwalk visits newest-first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecency {
    /// Repository-relative path.
    pub path: PathBuf,
    /// Seconds since epoch when the most recent touching commit was authored.
    pub last_commit_secs: i64,
    /// Seconds elapsed between that commit and the time `file_recency` ran.
    /// Negative results (commits with future timestamps) are clamped to 0.
    pub secs_ago: u64,
    /// Short (7-char) commit hash of the most recent touching commit.
    pub sha: String,
    /// First line of that commit's message, trimmed.
    pub summary: String,
    /// Author display name (`commit.author().name()`), if present. Used by
    /// the GUI's author overlay; cheap proxy for "primary author per file"
    /// — strictly speaking this is "last toucher", but on most codebases
    /// the two correlate strongly.
    pub author_name: Option<String>,
    /// Author email, if present. Combined with `author_name` it gives the
    /// stable identity the overlay hashes onto a hue.
    pub author_email: Option<String>,
}

/// Cap on commits we'll walk before bailing. Realistic repos with a few
/// thousand commits visit every file in the first few hundred; the cap is a
/// pure safety belt for pathological histories.
const RECENCY_MAX_COMMITS: usize = 10_000;

/// Cap on distinct files we'll record. The frontend uses this to colour
/// folder maps; ~5k files is enough to render every leaf in a 20-module
/// monorepo without blowing up the JSON payload.
const RECENCY_MAX_FILES: usize = 5_000;

/// Build a per-file recency index for `repo_root` by walking commits from
/// HEAD backwards. The first commit (newest) that touches a path wins; once
/// we've seen [`RECENCY_MAX_FILES`] distinct paths or visited
/// [`RECENCY_MAX_COMMITS`] commits, we stop.
///
/// The result is sorted ascending by `secs_ago` (most recent first), which
/// is the order the heatmap layer wants.
pub fn file_recency(repo_root: &Path) -> Result<Vec<FileRecency>, GitError> {
    let repo = GitRepo::discover(repo_root)?;
    let mut walk = repo.revwalk()?;
    walk.push_head()?;
    walk.set_sorting(git2::Sort::TIME)?;

    // Cast through i64 deliberately: git2 reports commit times as i64 (signed
    // seconds since epoch), so we want the same domain for the subtraction
    // below. Year-2106 will be a problem here; that's fine.
    let now_secs = i64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);

    let mut recency: HashMap<PathBuf, FileRecency> = HashMap::new();

    for (commits_seen, oid_res) in walk.enumerate() {
        if recency.len() >= RECENCY_MAX_FILES {
            break;
        }
        if commits_seen >= RECENCY_MAX_COMMITS {
            break;
        }

        let Ok(oid) = oid_res else { continue };
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let commit_time = commit.time().seconds();
        let summary = commit
            .summary()
            .map(ToString::to_string)
            .unwrap_or_default();
        let sha = oid.to_string();
        let short_sha: String = sha.chars().take(7).collect();
        // git2 returns Option<&str> for both name and email; we promote
        // empty strings to `None` too so consumers don't have to special-
        // case malformed signatures.
        let author = commit.author();
        let author_name = author.name().map(str::to_string).filter(|s| !s.is_empty());
        let author_email = author.email().map(str::to_string).filter(|s| !s.is_empty());

        let Ok(tree) = commit.tree() else { continue };
        // Compare each parent against this commit's tree. For the root
        // commit (no parent) we diff against an empty tree so every file
        // gets an "added" delta.
        let parent_trees: Vec<git2::Tree<'_>> =
            commit.parents().filter_map(|p| p.tree().ok()).collect();

        let touched = if parent_trees.is_empty() {
            paths_in_diff(
                repo.diff_tree_to_tree(None, Some(&tree), None)
                    .ok()
                    .as_ref(),
            )
        } else {
            let mut paths: Vec<PathBuf> = Vec::new();
            for parent in &parent_trees {
                if let Ok(diff) = repo.diff_tree_to_tree(Some(parent), Some(&tree), None) {
                    paths.extend(paths_in_diff(Some(&diff)));
                }
            }
            paths
        };

        for path in touched {
            // First-write-wins: revwalk is newest-first, so the first
            // commit that touches a path is the "last edit".
            recency.entry(path.clone()).or_insert_with(|| {
                // Clamp negative deltas (commits with future timestamps) to
                // zero, then convert into the `u64` we expose. `try_from`
                // can't fail because we've just guaranteed the value is
                // non-negative — saturating fallback is a belt-and-braces.
                let secs_ago = u64::try_from((now_secs - commit_time).max(0)).unwrap_or(0);
                FileRecency {
                    path,
                    last_commit_secs: commit_time,
                    secs_ago,
                    sha: short_sha.clone(),
                    summary: summary.clone(),
                    author_name: author_name.clone(),
                    author_email: author_email.clone(),
                }
            });
            if recency.len() >= RECENCY_MAX_FILES {
                break;
            }
        }
    }

    let mut out: Vec<FileRecency> = recency.into_values().collect();
    out.sort_by_key(|r| r.secs_ago);
    Ok(out)
}

/// Helper: collect paths from a diff. Returns an empty list when `diff` is
/// `None` (let callers stay terse).
fn paths_in_diff(diff: Option<&Diff<'_>>) -> Vec<PathBuf> {
    let Some(diff) = diff else { return Vec::new() };
    let mut out = Vec::new();
    for delta in diff.deltas() {
        if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            out.push(path.to_path_buf());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Per-test temp dir, dropped recursively when it goes out of scope.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!("projectmind-git-test-{}-{}", std::process::id(), n));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn init_repo(dir: &Path) -> GitRepo {
        let repo = GitRepo::init(dir).unwrap();
        // libgit2 requires user.name + user.email for commits.
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        repo
    }

    fn commit_file(repo: &GitRepo, dir: &Path, rel: &str, content: &str, msg: &str, when: i64) {
        commit_file_as(
            repo,
            dir,
            rel,
            content,
            msg,
            when,
            "Test",
            "test@example.com",
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_file_as(
        repo: &GitRepo,
        dir: &Path,
        rel: &str,
        content: &str,
        msg: &str,
        when: i64,
        author_name: &str,
        author_email: &str,
    ) {
        fs::write(dir.join(rel), content).unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new(rel)).unwrap();
        idx.write().unwrap();
        let tree_id = idx.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig =
            git2::Signature::new(author_name, author_email, &git2::Time::new(when, 0)).unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    #[test]
    fn file_recency_picks_most_recent_commit_per_path() {
        let tmp = TempDir::new();
        let repo = init_repo(tmp.path());
        // Commit timestamps are ~UNIX epoch; the absolute value doesn't
        // matter for the test, only that commit B's timestamp > commit A's.
        commit_file(&repo, tmp.path(), "a.txt", "1", "first", 1_700_000_000);
        commit_file(&repo, tmp.path(), "b.txt", "1", "second", 1_700_000_100);
        commit_file(&repo, tmp.path(), "a.txt", "2", "third", 1_700_000_200);

        let result = file_recency(tmp.path()).unwrap();
        let by_path: HashMap<_, _> = result.iter().map(|r| (r.path.clone(), r)).collect();

        let a = by_path.get(&PathBuf::from("a.txt")).expect("a.txt present");
        let b = by_path.get(&PathBuf::from("b.txt")).expect("b.txt present");

        // a.txt was last touched in commit "third" (1_700_000_200), b.txt
        // in "second" (1_700_000_100).
        assert_eq!(a.last_commit_secs, 1_700_000_200);
        assert_eq!(a.summary, "third");
        assert_eq!(b.last_commit_secs, 1_700_000_100);
        assert_eq!(b.summary, "second");

        // Result is sorted ascending by `secs_ago` (most recent first), so
        // a.txt comes before b.txt.
        let pos_a = result
            .iter()
            .position(|r| r.path == Path::new("a.txt"))
            .unwrap();
        let pos_b = result
            .iter()
            .position(|r| r.path == Path::new("b.txt"))
            .unwrap();
        assert!(pos_a < pos_b, "a.txt (newer) should come before b.txt");
    }

    #[test]
    fn file_recency_records_short_sha_and_summary() {
        let tmp = TempDir::new();
        let repo = init_repo(tmp.path());
        commit_file(
            &repo,
            tmp.path(),
            "x.md",
            "hi",
            "Add x with details that get cut",
            1_700_000_000,
        );

        let result = file_recency(tmp.path()).unwrap();
        assert_eq!(result.len(), 1);
        let r = &result[0];
        assert_eq!(r.sha.len(), 7);
        // Summary is the full commit subject line; we don't trim it here.
        assert_eq!(r.summary, "Add x with details that get cut");
    }

    #[test]
    fn file_recency_records_author_of_most_recent_commit() {
        let tmp = TempDir::new();
        let repo = init_repo(tmp.path());
        // Two authors touch the same file in sequence; revwalk visits the
        // newest commit first so the second author wins.
        commit_file_as(
            &repo,
            tmp.path(),
            "x.md",
            "1",
            "one",
            1_700_000_000,
            "Alice",
            "alice@example.com",
        );
        commit_file_as(
            &repo,
            tmp.path(),
            "x.md",
            "2",
            "two",
            1_700_000_100,
            "Bob",
            "bob@example.com",
        );
        commit_file_as(
            &repo,
            tmp.path(),
            "y.md",
            "1",
            "y",
            1_700_000_050,
            "Alice",
            "alice@example.com",
        );

        let result = file_recency(tmp.path()).unwrap();
        let by_path: HashMap<_, _> = result.iter().map(|r| (r.path.clone(), r)).collect();
        let x = by_path.get(&PathBuf::from("x.md")).unwrap();
        assert_eq!(x.author_name.as_deref(), Some("Bob"));
        assert_eq!(x.author_email.as_deref(), Some("bob@example.com"));
        let y = by_path.get(&PathBuf::from("y.md")).unwrap();
        assert_eq!(y.author_name.as_deref(), Some("Alice"));
        assert_eq!(y.author_email.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn split_range_passes_plain_ref_through() {
        assert_eq!(split_range("HEAD", None), ("HEAD", None));
        assert_eq!(
            split_range("HEAD", Some("master")),
            ("HEAD", Some("master"))
        );
    }

    #[test]
    fn split_range_extracts_two_dot_range() {
        assert_eq!(
            split_range("origin/master..HEAD", None),
            ("origin/master", Some("HEAD"))
        );
    }

    #[test]
    fn split_range_extracts_three_dot_range() {
        assert_eq!(
            split_range("main...feature", None),
            ("main", Some("feature"))
        );
    }

    #[test]
    fn split_range_explicit_to_ref_wins_over_inline_range() {
        // When the caller passed an explicit `to_ref`, the inline range tail
        // is redundant — keep `from_ref` untouched and let libgit2 fail
        // loudly if it really is malformed.
        assert_eq!(
            split_range("origin/master..HEAD", Some("v1.0")),
            ("origin/master..HEAD", Some("v1.0"))
        );
    }

    #[test]
    fn split_range_ignores_empty_sides() {
        // `..HEAD` and `HEAD..` are valid git CLI shorthands but we have no
        // way to fill in the implied side without inventing repo state, so
        // pass them through and let `revparse_single` produce a clear error.
        assert_eq!(split_range("..HEAD", None), ("..HEAD", None));
        assert_eq!(split_range("HEAD..", None), ("HEAD..", None));
    }

    #[test]
    fn unified_diff_accepts_inline_range() {
        let tmp = TempDir::new();
        let repo = init_repo(tmp.path());
        commit_file(&repo, tmp.path(), "a.txt", "v1\n", "first", 1_700_000_000);
        commit_file(&repo, tmp.path(), "a.txt", "v2\n", "second", 1_700_000_100);

        // Caller passes an `A..B` shorthand in `from_ref` with no explicit
        // `to_ref` — the regression in issue #119.
        let out = unified_diff(tmp.path(), "HEAD~1..HEAD", None).expect("range diff");
        assert!(out.contains("-v1"), "diff should show old line: {out}");
        assert!(out.contains("+v2"), "diff should show new line: {out}");
    }
}

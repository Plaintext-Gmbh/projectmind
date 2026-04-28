// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lightweight git helpers — list changed files, render diffs.

use std::path::{Path, PathBuf};

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

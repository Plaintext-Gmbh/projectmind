// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Repo-scoped file access helpers.

use std::path::{Path, PathBuf};

/// Errors returned when a file access request violates the repo boundary or
/// cannot be served.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FileAccessError {
    /// The caller supplied a non-absolute path.
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(PathBuf),

    /// The repository root could not be canonicalized.
    #[error("invalid repository root {path}: {source}")]
    InvalidRepoRoot {
        /// Repository root path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The requested path could not be canonicalized.
    #[error("invalid file path {path}: {source}")]
    InvalidPath {
        /// Requested file path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The requested path resolves outside the opened repository.
    #[error("access denied: {path} is outside repository {repo_root}")]
    OutsideRepo {
        /// Canonical requested path.
        path: PathBuf,
        /// Canonical repository root.
        repo_root: PathBuf,
    },

    /// The path does not resolve to a regular file.
    #[error("path is not a file: {0}")]
    NotAFile(PathBuf),

    /// IO failure while reading the file.
    #[error("read {path}: {source}")]
    Read {
        /// Canonical requested path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// File exceeds the configured byte limit.
    #[error("file too large ({actual} bytes; limit {limit} bytes)")]
    FileTooLarge {
        /// Actual file size in bytes.
        actual: u64,
        /// Configured byte limit.
        limit: u64,
    },

    /// File bytes were not valid UTF-8.
    #[error("invalid UTF-8 in {path}: {source}")]
    InvalidUtf8 {
        /// Canonical requested path.
        path: PathBuf,
        /// Underlying UTF-8 error.
        source: std::string::FromUtf8Error,
    },
}

/// Canonicalize `path` and verify that it points to a regular file within
/// `repo_root`.
pub fn canonical_file_in_repo(repo_root: &Path, path: &Path) -> Result<PathBuf, FileAccessError> {
    if !path.is_absolute() {
        return Err(FileAccessError::PathNotAbsolute(path.to_path_buf()));
    }

    let repo_root =
        std::fs::canonicalize(repo_root).map_err(|source| FileAccessError::InvalidRepoRoot {
            path: repo_root.to_path_buf(),
            source,
        })?;
    let path = std::fs::canonicalize(path).map_err(|source| FileAccessError::InvalidPath {
        path: path.to_path_buf(),
        source,
    })?;

    if !path.starts_with(&repo_root) {
        return Err(FileAccessError::OutsideRepo { path, repo_root });
    }
    if !path.is_file() {
        return Err(FileAccessError::NotAFile(path));
    }
    Ok(path)
}

/// Read a UTF-8 file within `repo_root`, rejecting files above `limit_bytes`.
pub fn read_text_file_in_repo(
    repo_root: &Path,
    path: &Path,
    limit_bytes: u64,
) -> Result<String, FileAccessError> {
    let path = canonical_file_in_repo(repo_root, path)?;
    let metadata = std::fs::metadata(&path).map_err(|source| FileAccessError::Read {
        path: path.clone(),
        source,
    })?;
    if metadata.len() > limit_bytes {
        return Err(FileAccessError::FileTooLarge {
            actual: metadata.len(),
            limit: limit_bytes,
        });
    }
    let bytes = std::fs::read(&path).map_err(|source| FileAccessError::Read {
        path: path.clone(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|source| FileAccessError::InvalidUtf8 { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> TempDir {
        TempDir::new(name)
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!(
                "projectmind-file-access-{}-{}-{}",
                std::process::id(),
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
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

    #[test]
    fn reads_text_file_inside_repo() {
        let repo = tempdir("inside");
        let file = repo.path().join("README.md");
        std::fs::write(&file, "hello").unwrap();

        let text = read_text_file_in_repo(repo.path(), &file, 100).unwrap();

        assert_eq!(text, "hello");
    }

    #[test]
    fn rejects_file_outside_repo() {
        let repo = tempdir("repo");
        let outside = tempdir("outside");
        let file = outside.path().join("secret.txt");
        std::fs::write(&file, "secret").unwrap();

        let err = canonical_file_in_repo(repo.path(), &file).unwrap_err();

        assert!(matches!(err, FileAccessError::OutsideRepo { .. }));
    }

    #[test]
    fn rejects_non_absolute_path() {
        let repo = tempdir("relative");

        let err = canonical_file_in_repo(repo.path(), Path::new("README.md")).unwrap_err();

        assert!(matches!(err, FileAccessError::PathNotAbsolute(_)));
    }

    #[test]
    fn rejects_oversized_file() {
        let repo = tempdir("large");
        let file = repo.path().join("large.txt");
        std::fs::write(&file, "hello").unwrap();

        let err = read_text_file_in_repo(repo.path(), &file, 4).unwrap_err();

        assert!(matches!(err, FileAccessError::FileTooLarge { .. }));
    }
}

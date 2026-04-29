// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! File discovery helpers — currently just markdown listing.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::Serialize;

/// One markdown file found inside a repository.
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownFile {
    /// Absolute path on disk.
    pub abs: PathBuf,
    /// Path relative to the requested root, with `/` separators (always; we
    /// flatten Windows separators in the rare case this code runs there).
    pub rel: String,
    /// `Title` of the document — first H1 if present, otherwise the file stem.
    pub title: String,
    /// File size in bytes (so the UI can show large files differently).
    pub size: u64,
}

/// Walk `root` and return every `*.md`, `*.markdown`, and `*.mdx` it finds.
/// Honours `.gitignore`/`.ignore` (`ignore` crate default behaviour) and skips
/// the usual build-output noise even if the project lacks a gitignore.
///
/// The list is sorted by relative path so the UI gets a stable order without
/// re-sorting on every render.
#[must_use]
pub fn list_markdown_files(root: &Path) -> Vec<MarkdownFile> {
    let mut out: Vec<MarkdownFile> = Vec::new();

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "node_modules" | "target" | "dist" | "build" | ".git" | ".idea" | ".vscode"
            )
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let lower = ext.to_ascii_lowercase();
        if !matches!(lower.as_str(), "md" | "markdown" | "mdx") {
            continue;
        }
        let rel_buf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let rel = rel_buf.to_string_lossy().replace('\\', "/");
        let title = first_h1(path).unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });
        let size = entry.metadata().map_or(0, |m| m.len());
        out.push(MarkdownFile {
            abs: path.to_path_buf(),
            rel,
            title,
            size,
        });
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Read the first ATX H1 (`# Title`) from a markdown file. Returns `None` if
/// no H1 is found in the first 200 lines or the file isn't readable.
fn first_h1(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    for (i, line) in reader.lines().enumerate() {
        if i > 200 {
            break;
        }
        let line = line.ok()?;
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("projectmind-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_markdown_skipping_target() {
        let root = tmp_dir("listing");
        std::fs::write(root.join("README.md"), "# Hello\nbody").unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/guide.md"), "# Guide").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/leak.md"), "# Should not appear").unwrap();

        let files = list_markdown_files(&root);
        let names: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert!(names.contains(&"README.md"));
        assert!(names.contains(&"docs/guide.md"));
        assert!(!names.iter().any(|n| n.contains("target")));

        let readme = files.iter().find(|f| f.rel == "README.md").unwrap();
        assert_eq!(readme.title, "Hello");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn falls_back_to_stem_when_no_h1() {
        let root = tmp_dir("stem");
        std::fs::write(root.join("notes.md"), "no heading here").unwrap();
        let files = list_markdown_files(&root);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].title, "notes");
        std::fs::remove_dir_all(&root).ok();
    }
}

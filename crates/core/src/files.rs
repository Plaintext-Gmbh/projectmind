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

/// One non-source asset (PDF, image, …) found inside a module root.
///
/// Returned by [`list_module_files`] for the Code-tab sidebar so PDFs and
/// images can sit alongside the parsed class listing.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleFile {
    /// Absolute path on disk.
    pub abs: PathBuf,
    /// Path relative to the module root, with `/` separators.
    pub rel: String,
    /// Lowercase extension (e.g. `"pdf"`, `"png"`). Stored as `String` rather
    /// than `&'static str` so the JSON serialisation round-trips cleanly.
    pub kind: String,
    /// File size in bytes.
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

/// Walk `module_root` and return every file whose lowercase extension is in
/// `kinds`. Honours `.gitignore`/`.ignore` and skips the same build-output
/// directories as [`list_markdown_files`] (`target`, `node_modules`, `dist`,
/// `build`, `.git`, `.idea`, `.vscode`).
///
/// Source files (`.java`, `.rs`) are skipped explicitly even if the caller
/// passes them in `kinds` — those are the class listing's job and shouldn't
/// appear twice in the Code-tab sidebar.
///
/// Result is sorted alphabetically by relative path so the UI gets a stable
/// order without re-sorting on every render.
#[must_use]
pub fn list_module_files(module_root: &Path, kinds: &[&str]) -> Vec<ModuleFile> {
    let mut out: Vec<ModuleFile> = Vec::new();

    // Normalise the requested extension list once. Callers are expected to
    // pass lowercase already, but be tolerant.
    let wanted: Vec<String> = kinds.iter().map(|k| k.to_ascii_lowercase()).collect();

    let walker = WalkBuilder::new(module_root)
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
        // Source files are the class listing's job — never surface them here.
        if matches!(lower.as_str(), "java" | "rs") {
            continue;
        }
        if !wanted.iter().any(|k| k == &lower) {
            continue;
        }
        let rel_buf = path.strip_prefix(module_root).unwrap_or(path).to_path_buf();
        let rel = rel_buf.to_string_lossy().replace('\\', "/");
        let size = entry.metadata().map_or(0, |m| m.len());
        out.push(ModuleFile {
            abs: path.to_path_buf(),
            rel,
            kind: lower,
            size,
        });
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// One scored hit returned by [`search_markdown`].
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownHit {
    /// The matched file (same shape as [`MarkdownFile`]).
    pub file: MarkdownFile,
    /// Combined fuzzy score across title, path, and content. Higher is
    /// better; absolute values aren't meaningful — only ordering is.
    pub score: u32,
    /// Where the match landed: `title`, `path`, or `content`. The strongest
    /// of the three wins; we surface it so the UI can show context.
    pub matched_in: MatchKind,
    /// Optional snippet of the body around the matched substring. `None`
    /// when the hit didn't come from content.
    pub snippet: Option<String>,
}

/// Which field carried the strongest fuzzy match.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MatchKind {
    /// Match was strongest in the document title (first H1 / file stem).
    Title,
    /// Match was strongest in the relative path.
    Path,
    /// Match was strongest in the content body.
    Content,
}

/// Fuzzy-search markdown files under `root`. The query is matched against
/// each file's title, relative path, and a content snippet (first ~4 KB of
/// the file). Results are returned sorted by descending score, capped at
/// `limit`. With an empty query the behaviour is identical to
/// [`list_markdown_files`] — every file with score 0, in path order — so
/// the UI can use a single code path.
#[must_use]
pub fn search_markdown(root: &Path, query: &str, limit: usize) -> Vec<MarkdownHit> {
    use nucleo_matcher::{
        pattern::{CaseMatching, Normalization, Pattern},
        Config, Matcher, Utf32Str,
    };

    let files = list_markdown_files(root);

    if query.trim().is_empty() {
        return files
            .into_iter()
            .take(limit)
            .map(|f| MarkdownHit {
                file: f,
                score: 0,
                matched_in: MatchKind::Path,
                snippet: None,
            })
            .collect();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut buf_a: Vec<char> = Vec::new();
    let mut buf_b: Vec<char> = Vec::new();
    let mut buf_c: Vec<char> = Vec::new();

    let mut hits: Vec<MarkdownHit> = Vec::new();
    for f in files {
        let body = read_snippet(&f.abs);

        let title_str = Utf32Str::new(&f.title, &mut buf_a);
        let path_str = Utf32Str::new(&f.rel, &mut buf_b);
        let body_str = Utf32Str::new(&body, &mut buf_c);

        let title_score = pattern.score(title_str, &mut matcher).unwrap_or(0);
        let path_score = pattern.score(path_str, &mut matcher).unwrap_or(0);
        let body_score = pattern.score(body_str, &mut matcher).unwrap_or(0);

        // Bias title and path higher than content so a title-hit beats a
        // chance content-hit of the same raw score.
        let weighted_title = title_score.saturating_mul(3);
        let weighted_path = path_score.saturating_mul(2);
        let combined = weighted_title.max(weighted_path).max(body_score);
        if combined == 0 {
            continue;
        }

        let matched_in = if weighted_title >= weighted_path && weighted_title >= body_score {
            MatchKind::Title
        } else if weighted_path >= body_score {
            MatchKind::Path
        } else {
            MatchKind::Content
        };

        let snippet = if matched_in == MatchKind::Content {
            content_snippet(&body, query)
        } else {
            None
        };

        hits.push(MarkdownHit {
            file: f,
            score: combined,
            matched_in,
            snippet,
        });
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.file.rel.cmp(&b.file.rel)));
    hits.truncate(limit);
    hits
}

fn read_snippet(path: &Path) -> String {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; 4096];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    String::from_utf8(buf).unwrap_or_default()
}

/// Pick a ~120-char window around the first case-insensitive substring hit.
/// Falls back to the file head when nothing matches (e.g. fuzzy hit on
/// non-contiguous letters).
fn content_snippet(body: &str, query: &str) -> Option<String> {
    let needle = query.trim();
    if needle.is_empty() || body.is_empty() {
        return None;
    }
    let haystack_lower = body.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let center = haystack_lower.find(&needle_lower);
    let (start, end) = match center {
        Some(idx) => {
            let s = idx.saturating_sub(50);
            let e = (idx + needle.len() + 70).min(body.len());
            (s, e)
        }
        None => (0, body.len().min(120)),
    };
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(body[start..end].trim());
    if end < body.len() {
        out.push('…');
    }
    Some(out.replace('\n', " "))
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

    #[test]
    fn list_module_files_filters_kinds_and_skips_target() {
        let root = tmp_dir("module-files");
        std::fs::write(root.join("brochure.pdf"), b"%PDF-").unwrap();
        std::fs::write(root.join("logo.png"), b"\x89PNG").unwrap();
        // Source file — must NOT appear (class listing's job).
        std::fs::write(root.join("App.java"), "class App {}").unwrap();
        // Markdown — must NOT appear when caller doesn't ask for it.
        std::fs::write(root.join("README.md"), "# Hi").unwrap();
        // Nested PDF inside target/ — must be filtered.
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/leak.pdf"), b"%PDF-").unwrap();
        // Nested image inside docs/ — must appear.
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/photo.jpg"), b"\xff\xd8\xff").unwrap();

        let files = list_module_files(&root, &["pdf", "png", "jpg", "jpeg"]);
        let names: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();

        // Expected matches.
        assert!(names.contains(&"brochure.pdf"));
        assert!(names.contains(&"logo.png"));
        assert!(names.contains(&"docs/photo.jpg"));

        // Filtered out.
        assert!(!names.iter().any(|n| n.contains("target")));
        assert!(!names.iter().any(|n| std::path::Path::new(n)
            .extension()
            .is_some_and(|e| e == "java")));
        assert!(!names.iter().any(|n| std::path::Path::new(n)
            .extension()
            .is_some_and(|e| e == "md")));

        // Alphabetical ordering by `rel`.
        let sorted: Vec<&str> = {
            let mut v = names.clone();
            v.sort_unstable();
            v
        };
        assert_eq!(names, sorted);

        // `kind` field is the lowercase extension.
        let pdf = files.iter().find(|f| f.rel == "brochure.pdf").unwrap();
        assert_eq!(pdf.kind, "pdf");
        let jpg = files.iter().find(|f| f.rel == "docs/photo.jpg").unwrap();
        assert_eq!(jpg.kind, "jpg");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_module_files_skips_source_extensions_even_if_requested() {
        // Defensive: a buggy caller asks for `.java` / `.rs` — we still skip
        // them so they never collide with the class listing.
        let root = tmp_dir("module-files-defense");
        std::fs::write(root.join("App.java"), "class App {}").unwrap();
        std::fs::write(root.join("lib.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("notes.pdf"), b"%PDF-").unwrap();
        let files = list_module_files(&root, &["java", "rs", "pdf"]);
        let names: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(names, vec!["notes.pdf"]);
        std::fs::remove_dir_all(&root).ok();
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Language distribution statistics over a repository tree.
//!
//! Walks the repo with `WalkBuilder` (gitignore-aware), groups files by a
//! coarse "language" label derived from the file extension, and reports
//! file count + total byte size per language. The result drives the
//! "language-stats" diagram in the GUI and the same MCP tool.
//!
//! Cap defaults: 50 000 files walked, 100 distinct extensions tracked.
//! Above that we stop adding new buckets so a pathological repo can't
//! blow up memory. Files in standard build/output directories are
//! filtered out (same list as `files::list_module_files`).

use std::path::Path;

use ignore::WalkBuilder;
use serde::Serialize;

/// Maximum files walked. Past this we stop tallying — a repo with more
/// than this many source files probably wants a sampled view anyway.
const MAX_FILES: usize = 50_000;
/// Maximum number of distinct buckets we keep around. After the cap, new
/// extensions are folded into the `Other` bucket.
const MAX_BUCKETS: usize = 100;

/// One language bucket as exposed to the GUI.
#[derive(Debug, Clone, Serialize)]
pub struct LanguageBucket {
    /// Display label — "Rust", "Java", "Markdown", …
    pub language: String,
    /// Source extension (lowercase, no dot). `null` for the `Other` catch-all.
    pub extension: Option<String>,
    /// Number of files in this bucket.
    pub files: usize,
    /// Total size of those files in bytes.
    pub bytes: u64,
}

/// Aggregate result for the "language stats" diagram.
#[derive(Debug, Clone, Serialize)]
pub struct LanguageStats {
    /// Absolute repository root the stats were computed for.
    pub root: String,
    /// Number of files walked (after filters). Capped at [`MAX_FILES`].
    pub total_files: usize,
    /// Sum of `bytes` across every bucket.
    pub total_bytes: u64,
    /// Set to `true` when the walker hit [`MAX_FILES`] and stopped early.
    pub truncated: bool,
    /// Buckets sorted by `files` descending. `Other` (if present) always
    /// comes last regardless of count.
    pub buckets: Vec<LanguageBucket>,
}

/// Compute language statistics under `root`. Returns an empty result when
/// the path is missing or unreadable rather than failing — the diagram
/// surface can render "no data" without callers worrying about errors.
#[must_use]
pub fn build(root: &Path) -> LanguageStats {
    let mut buckets: std::collections::HashMap<String, LanguageBucket> =
        std::collections::HashMap::new();
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut truncated = false;

    let walker = WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | ".git"
                    | ".idea"
                    | ".vscode"
                    | "__pycache__"
            )
        })
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if total_files >= MAX_FILES {
            truncated = true;
            break;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let (language, extension): (&'static str, Option<String>) = if ext.is_empty() {
            ("No extension", None)
        } else {
            (label_for(&ext), Some(ext))
        };

        let size = entry.metadata().map_or(0, |m| m.len());
        total_files += 1;
        total_bytes = total_bytes.saturating_add(size);

        let key = extension.clone().unwrap_or_else(|| "_noext".to_string());
        if let Some(bucket) = buckets.get_mut(&key) {
            bucket.files += 1;
            bucket.bytes = bucket.bytes.saturating_add(size);
            continue;
        }
        if buckets.len() >= MAX_BUCKETS {
            let other = buckets
                .entry("__other__".to_string())
                .or_insert_with(|| LanguageBucket {
                    language: "Other".to_string(),
                    extension: None,
                    files: 0,
                    bytes: 0,
                });
            other.files += 1;
            other.bytes = other.bytes.saturating_add(size);
            continue;
        }
        buckets.insert(
            key,
            LanguageBucket {
                language: language.to_string(),
                extension,
                files: 1,
                bytes: size,
            },
        );
    }

    // Sort: files desc, with __other__ pinned to the end.
    let mut list: Vec<LanguageBucket> = buckets.into_values().collect();
    list.sort_by(|a, b| {
        let a_other = a.language == "Other";
        let b_other = b.language == "Other";
        match (a_other, b_other) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b.files.cmp(&a.files).then(a.language.cmp(&b.language)),
        }
    });

    LanguageStats {
        root: root.to_string_lossy().to_string(),
        total_files,
        total_bytes,
        truncated,
        buckets: list,
    }
}

/// Map a lowercase extension to a display label. Unknown extensions fall
/// back to the upper-cased extension itself ("FOO" for `.foo`), which is
/// good enough for the chart legend without an exhaustive language table.
fn label_for(ext: &str) -> &'static str {
    match ext {
        "rs" => "Rust",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "scala" | "sc" => "Scala",
        "groovy" | "gradle" => "Groovy",
        "py" | "pyi" => "Python",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "cjs" | "mjs" => "JavaScript",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "html" | "htm" | "xhtml" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "less" => "Less",
        "md" | "markdown" | "mdx" => "Markdown",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" | "xsd" | "xsl" => "XML",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        "fish" => "Fish",
        "ps1" => "PowerShell",
        "go" => "Go",
        "c" => "C",
        "h" | "hpp" | "hh" => "C/C++ header",
        "cc" | "cpp" | "cxx" => "C++",
        "cs" => "C#",
        "swift" => "Swift",
        "m" | "mm" => "Objective-C",
        "rb" => "Ruby",
        "php" => "PHP",
        "lua" => "Lua",
        "dart" => "Dart",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "clj" | "cljs" => "Clojure",
        "hs" => "Haskell",
        "ml" | "mli" => "OCaml",
        "r" => "R",
        "jl" => "Julia",
        "zig" => "Zig",
        "nim" => "Nim",
        "v" => "V",
        "dockerfile" => "Docker",
        "tf" | "tfvars" => "Terraform",
        "proto" => "Protobuf",
        "graphql" | "gql" => "GraphQL",
        "lock" => "Lockfile",
        "txt" => "Text",
        "csv" | "tsv" => "CSV",
        "pdf" => "PDF",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => "Image",
        "drawio" => "draw.io",
        "mp3" | "wav" | "flac" | "ogg" => "Audio",
        "mp4" | "mov" | "mkv" | "webm" => "Video",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => "Archive",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Per-test temp dir, dropped recursively when it goes out of scope.
    /// Local copy of the pattern used in `git.rs`'s tests so this module
    /// has no dev-dependency on `tempfile`.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut p = std::env::temp_dir();
            p.push(format!(
                "projectmind-langstats-test-{}-{n}",
                std::process::id()
            ));
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

    fn touch(p: &Path, bytes: &[u8]) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, bytes).unwrap();
    }

    #[test]
    fn empty_repo_returns_no_buckets() {
        let dir = TempDir::new();
        let stats = build(dir.path());
        assert_eq!(stats.total_files, 0);
        assert!(stats.buckets.is_empty());
        assert!(!stats.truncated);
    }

    #[test]
    fn groups_by_extension_label_and_sorts_by_files() {
        let dir = TempDir::new();
        let root = dir.path();
        touch(&root.join("a.rs"), b"fn main(){}");
        touch(&root.join("b.rs"), b"fn b(){}");
        touch(&root.join("c.rs"), b"fn c(){}");
        touch(&root.join("a.md"), b"# title");
        touch(&root.join("README"), b"plain");

        let stats = build(root);
        assert_eq!(stats.total_files, 5);
        // Rust bucket is biggest.
        assert_eq!(stats.buckets.first().unwrap().language, "Rust");
        assert_eq!(stats.buckets.first().unwrap().files, 3);
        // Markdown bucket exists.
        assert!(stats
            .buckets
            .iter()
            .any(|b| b.language == "Markdown" && b.files == 1));
        // No-extension bucket exists.
        assert!(stats
            .buckets
            .iter()
            .any(|b| b.language == "No extension" && b.files == 1));
    }

    #[test]
    fn skips_target_and_node_modules() {
        let dir = TempDir::new();
        let root = dir.path();
        touch(&root.join("src/lib.rs"), b"keep me");
        touch(&root.join("target/junk.rs"), b"skip me");
        touch(&root.join("node_modules/x/y.js"), b"also skip");

        let stats = build(root);
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.buckets[0].language, "Rust");
    }

    #[test]
    fn unknown_extensions_fall_into_other() {
        let dir = TempDir::new();
        let root = dir.path();
        touch(&root.join("foo.xyz"), b"x");
        touch(&root.join("bar.abc"), b"y");

        let stats = build(root);
        // Both unknowns label as "Other" but stay in separate extension buckets.
        assert_eq!(stats.total_files, 2);
        assert!(stats
            .buckets
            .iter()
            .all(|b| b.language == "Other" && b.files == 1));
    }
}

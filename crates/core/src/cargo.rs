// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cargo workspace detection.
//!
//! Mirrors [`crate::maven`] for Rust projects: every `Cargo.toml` whose `[package]` section
//! has a `name` becomes a module. Virtual-workspace `Cargo.toml`s (no `[package]`) are
//! treated as the umbrella and skipped — their members are surfaced individually instead.
//!
//! Phase 1 hand-rolls a tiny TOML scrape rather than pulling in a full parser dependency:
//! the only fields we care about are `[package].name` and `[package].version`, both
//! string literals, and the rest of TOML's grammar is irrelevant. This keeps the
//! `core` crate's dependency surface minimal.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// A discovered Cargo crate (a `Cargo.toml` with a `[package]` section).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoCrate {
    /// Crate root (directory containing the `Cargo.toml`).
    pub root: PathBuf,
    /// `[package] name` from the manifest.
    pub name: String,
    /// `[package] version`, when present.
    pub version: Option<String>,
}

impl CargoCrate {
    /// Coordinate string for the crate: name@version, or just the name.
    #[must_use]
    pub fn coordinate(&self) -> String {
        match &self.version {
            Some(v) => format!("{}@{v}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Discover all Cargo crates below `repo_root`.
///
/// Returned sorted by depth, deepest first — so [`attribute`] picks the most specific match.
#[must_use]
pub fn discover(repo_root: &Path) -> Vec<CargoCrate> {
    let mut crates = Vec::new();
    let walker = WalkBuilder::new(repo_root)
        .standard_filters(true)
        .hidden(false)
        .build();
    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) != Some("Cargo.toml") {
            continue;
        }
        match parse_manifest(path) {
            Ok(Some(parsed)) => {
                let root = path
                    .parent()
                    .map_or_else(|| repo_root.to_path_buf(), Path::to_path_buf);
                debug!(?root, name = %parsed.name, "discovered Cargo crate");
                crates.push(CargoCrate {
                    root,
                    name: parsed.name,
                    version: parsed.version,
                });
            }
            Ok(None) => {
                // Virtual workspace manifest (no `[package]` table) — skip.
            }
            Err(err) => {
                warn!(file = %path.display(), error = %err, "could not parse Cargo.toml");
            }
        }
    }
    crates.sort_by_key(|c| std::cmp::Reverse(c.root.components().count()));
    crates
}

/// Find the most specific crate that contains `file`.
#[must_use]
pub fn attribute<'a>(crates: &'a [CargoCrate], file: &Path) -> Option<&'a CargoCrate> {
    crates.iter().find(|c| file.starts_with(&c.root))
}

#[derive(Debug, Default)]
struct ParsedManifest {
    name: String,
    version: Option<String>,
}

fn parse_manifest(path: &Path) -> std::io::Result<Option<ParsedManifest>> {
    let text = std::fs::read_to_string(path)?;
    let mut current_section = String::new();
    let mut out = ParsedManifest::default();

    for raw in text.lines() {
        // Strip a `#` comment, but only outside string literals — the simple split is fine for the
        // narrow surface we look at (`[package]`, `name = "..."`, `version = "..."`).
        let line = raw.split('#').next().unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current_section = rest.trim().trim_start_matches('[').to_string();
            continue;
        }

        if current_section != "package" {
            continue;
        }

        if let Some(value) = strip_kv(line, "name") {
            if out.name.is_empty() {
                out.name = value;
            }
        } else if let Some(value) = strip_kv(line, "version") {
            if out.version.is_none() {
                out.version = Some(value);
            }
        }
    }

    if out.name.is_empty() {
        return Ok(None);
    }
    Ok(Some(out))
}

fn strip_kv(line: &str, key: &str) -> Option<String> {
    let stripped = line.strip_prefix(key)?;
    let after = stripped.trim_start();
    let after_eq = after.strip_prefix('=')?.trim();
    // Only handle simple string literals — workspace inheritance (`name.workspace = true`) and
    // arrays don't apply to `name`/`version` we care about.
    let unquoted = after_eq
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            after_eq
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })?;
    if unquoted.is_empty() {
        None
    } else {
        Some(unquoted.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), body).unwrap();
    }

    fn tmpdir() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "projectmind-cargo-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn discovers_single_crate() {
        let root = tmpdir();
        write_manifest(&root, "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
        let crates = discover(&root);
        assert_eq!(crates.len(), 1);
        assert_eq!(crates[0].name, "demo");
        assert_eq!(crates[0].version.as_deref(), Some("0.1.0"));
        assert_eq!(crates[0].coordinate(), "demo@0.1.0");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovers_workspace_members_and_skips_virtual_root() {
        let root = tmpdir();
        // Virtual workspace root: no [package] section.
        write_manifest(&root, "[workspace]\nmembers = [\"foo\", \"bar\"]\n");
        write_manifest(
            &root.join("foo"),
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        );
        write_manifest(
            &root.join("bar"),
            "[package]\nname = \"bar\"\nversion = \"0.2.0\"\n",
        );
        let crates = discover(&root);
        assert_eq!(crates.len(), 2);
        let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn attributes_files_to_deepest_containing_crate() {
        let root = tmpdir();
        // Hybrid: root has both [workspace] and [package].
        write_manifest(
            &root,
            "[package]\nname = \"top\"\nversion = \"0.1.0\"\n[workspace]\nmembers = [\"sub\"]\n",
        );
        write_manifest(
            &root.join("sub"),
            "[package]\nname = \"sub\"\nversion = \"0.0.1\"\n",
        );
        let crates = discover(&root);
        assert_eq!(crates.len(), 2);

        let inside_sub = attribute(&crates, &root.join("sub/src/lib.rs")).unwrap();
        assert_eq!(inside_sub.name, "sub");

        let at_top = attribute(&crates, &root.join("README.md")).unwrap();
        assert_eq!(at_top.name, "top");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignores_inherited_name_via_workspace() {
        // `name.workspace = true` is a different syntax we deliberately don't handle —
        // the crate is then effectively unnamed from our scrape's perspective and is skipped.
        let root = tmpdir();
        write_manifest(
            &root,
            "[package]\nname.workspace = true\nversion = \"0.1.0\"\n",
        );
        let crates = discover(&root);
        assert!(crates.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn handles_comments_and_extra_whitespace() {
        let root = tmpdir();
        write_manifest(
            &root,
            "# top-level comment\n[package]   # right after\n  name   =  \"thing\"  # trailing\nversion = \"0.1.0\"\n",
        );
        let crates = discover(&root);
        assert_eq!(crates.len(), 1);
        assert_eq!(crates[0].name, "thing");
        std::fs::remove_dir_all(&root).ok();
    }
}

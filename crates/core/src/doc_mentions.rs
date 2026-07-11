// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Doc-mention scan: which repo-internal Markdown documents talk about a class?
//!
//! The bridge from code to prose (#65, phase 1 — in-repo docs only). Given a
//! class (FQN, simple name, repo-relative source path), [`docs_for_class`]
//! walks every Markdown file [`crate::files::list_markdown_files`] finds and
//! ranks the documents that mention the class. Four rules, most precise first:
//!
//! 1. [`MentionKind::Link`] — the doc names the class's source file, either as
//!    a repo-relative path substring or via a relative Markdown link that
//!    resolves onto it.
//! 2. [`MentionKind::Fqn`] — the fully-qualified name appears verbatim
//!    (`com.foo.BarService`, `doc_graph::DocGraph`).
//! 3. [`MentionKind::Code`] — the simple name sits in an inline code span
//!    (`` `BarService` ``, `` `BarService.java` ``).
//! 4. [`MentionKind::Name`] — the bare name with word boundaries, only when
//!    the name is distinctive (>= 6 chars, >= 2 uppercase letters) so generic
//!    names like `Engine` or `Main` never produce noise.
//!
//! The matcher itself ([`scan_markdown`]) is pure — text in, match out — so
//! the ranking rules are unit-testable without touching the filesystem.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::doc_graph;
use crate::files;

/// Cap on how many bytes of a Markdown file are scanned (matches the spirit
/// of `search_markdown`'s head-read: docs beyond this size are almost
/// certainly generated dumps, not hand-written architecture prose).
const MAX_SCAN_BYTES: usize = 256 * 1024;

/// What we search for: the identity of one class.
#[derive(Debug, Clone)]
pub struct ClassNeedle {
    /// Fully-qualified name, verbatim (`com.foo.BarService` or `a::b::C`).
    pub fqn: String,
    /// Simple class name (`BarService`).
    pub name: String,
    /// Source file path relative to the repository root, `/`-separated.
    pub file_rel: String,
}

impl ClassNeedle {
    /// Build a needle for `class` inside a module rooted at `module_root`.
    ///
    /// The class file (stored relative to the module root) is resolved to a
    /// repo-root-relative path — the form docs link to. Falls back to the
    /// module-relative path when the module root is not under `repo_root`.
    #[must_use]
    pub fn for_class(
        repo_root: &Path,
        module_root: &Path,
        class: &projectmind_plugin_api::Class,
    ) -> Self {
        let abs = module_root.join(&class.file);
        let rel = abs.strip_prefix(repo_root).unwrap_or(class.file.as_path());
        Self {
            fqn: class.fqn.clone(),
            name: class.name.clone(),
            file_rel: rel.to_string_lossy().replace('\\', "/"),
        }
    }
}

/// Which rule produced a doc's best hit. Declaration order = descending rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MentionKind {
    /// The doc links to / names the class's source file (rank 4, strongest).
    Link,
    /// The fully-qualified name appears verbatim (rank 3).
    Fqn,
    /// The simple name appears in an inline code span (rank 2).
    Code,
    /// The bare (distinctive) name appears with word boundaries (rank 1).
    Name,
}

impl MentionKind {
    /// Numeric rank for sorting — higher is more precise.
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            MentionKind::Link => 4,
            MentionKind::Fqn => 3,
            MentionKind::Code => 2,
            MentionKind::Name => 1,
        }
    }
}

/// Result of scanning one Markdown text against a [`ClassNeedle`].
#[derive(Debug, Clone, Serialize)]
pub struct DocMatch {
    /// Best (highest-rank) rule that hit.
    pub kind: MentionKind,
    /// Total number of hits across all rules.
    pub count: u32,
    /// 1-based line of the first hit of the best rule.
    pub line: u32,
    /// ~120-char window around that hit, newlines flattened.
    pub snippet: String,
}

/// One Markdown document that mentions a class — a [`DocMatch`] joined with
/// the file identity from [`files::MarkdownFile`].
#[derive(Debug, Clone, Serialize)]
pub struct DocMention {
    /// Path relative to the repository root, `/`-separated.
    pub rel: String,
    /// Absolute path on disk (what the viewer opens).
    pub abs: PathBuf,
    /// Document title — first H1 or file stem.
    pub title: String,
    /// Best (highest-rank) rule that hit.
    pub kind: MentionKind,
    /// Total number of hits across all rules.
    pub count: u32,
    /// 1-based line of the first hit of the best rule.
    pub line: u32,
    /// ~120-char window around that hit, newlines flattened.
    pub snippet: String,
}

/// Scan every Markdown file under `root` for mentions of `needle`.
///
/// Reads at most 256 KiB per file (UTF-8 lossy). Results are sorted by rank
/// descending, then hit count descending, then relative path ascending, and
/// truncated to `limit`.
#[must_use]
pub fn docs_for_class(root: &Path, needle: &ClassNeedle, limit: usize) -> Vec<DocMention> {
    let mut out: Vec<DocMention> = Vec::new();
    for f in files::list_markdown_files(root) {
        let text = read_head(&f.abs);
        if text.is_empty() {
            continue;
        }
        if let Some(m) = scan_markdown(&text, &f.rel, needle) {
            out.push(DocMention {
                rel: f.rel,
                abs: f.abs,
                title: f.title,
                kind: m.kind,
                count: m.count,
                line: m.line,
                snippet: m.snippet,
            });
        }
    }
    out.sort_by(|a, b| {
        b.kind
            .rank()
            .cmp(&a.kind.rank())
            .then(b.count.cmp(&a.count))
            .then(a.rel.cmp(&b.rel))
    });
    out.truncate(limit);
    out
}

/// Pure matcher: does this Markdown text mention the class, and how strongly?
///
/// `doc_rel` is the document's own repo-relative path — needed to resolve
/// relative link hrefs against the document's directory. Returns `None` when
/// no rule hits.
#[must_use]
pub fn scan_markdown(text: &str, doc_rel: &str, needle: &ClassNeedle) -> Option<DocMatch> {
    // (idx, match_len) pairs per rule; idx is a byte offset into `text`.
    let link_hits = link_hits(text, doc_rel, &needle.file_rel);
    let fqn_hits = fqn_hits(text, &needle.fqn);
    let code_hits = code_span_hits(text, &needle.name);
    let name_hits = bare_name_hits(text, &needle.name);

    let total = link_hits.len() + fqn_hits.len() + code_hits.len() + name_hits.len();
    if total == 0 {
        return None;
    }

    let (kind, best) = [
        (MentionKind::Link, &link_hits),
        (MentionKind::Fqn, &fqn_hits),
        (MentionKind::Code, &code_hits),
        (MentionKind::Name, &name_hits),
    ]
    .into_iter()
    .find_map(|(kind, hits)| hits.iter().min_by_key(|(idx, _)| *idx).map(|h| (kind, *h)))?;

    Some(DocMatch {
        kind,
        count: u32::try_from(total).unwrap_or(u32::MAX),
        line: line_of(text, best.0),
        snippet: snippet_at(text, best.0, best.1),
    })
}

/// Rule 4 — the doc names the class's source file. Two ways to hit:
/// (a) the repo-relative path appears as a substring anywhere, or
/// (b) a relative Markdown link resolves (against the doc's directory) onto
/// the source file. Hrefs that already contain the full path are skipped in
/// (b) so one link never counts twice.
fn link_hits(text: &str, doc_rel: &str, file_rel: &str) -> Vec<(usize, usize)> {
    if file_rel.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(usize, usize)> = text
        .match_indices(file_rel)
        .map(|(idx, m)| (idx, m.len()))
        .collect();
    for (idx, link) in doc_graph::markdown_links_indexed(text) {
        if link.href.contains(file_rel) {
            continue; // already counted by the substring rule above
        }
        if resolve_relative_href(doc_rel, &link.href).is_some_and(|target| target == file_rel) {
            hits.push((idx, link.href.len()));
        }
    }
    hits
}

/// Resolve a Markdown link href against the linking document's directory,
/// yielding a repo-relative `/`-separated path. External links, absolute
/// paths, and pure-fragment links resolve to `None`. Unlike the doc-graph
/// resolver this keeps every extension — the target here is a source file.
fn resolve_relative_href(doc_rel: &str, href: &str) -> Option<String> {
    if href.starts_with('#') || href.starts_with('/') || doc_graph::is_external(href) {
        return None;
    }
    let without_fragment = href.split_once('#').map_or(href, |(path, _)| path);
    if without_fragment.is_empty() {
        return None;
    }
    let base = Path::new(doc_rel).parent().unwrap_or_else(|| Path::new(""));
    let joined = doc_graph::normalize_relative(base.join(without_fragment));
    Some(joined.to_string_lossy().replace('\\', "/"))
}

/// Rule 3 — the FQN verbatim, case-sensitive, with word boundaries on both
/// ends (so `com.foo.BarService` never hits inside `com.foo.BarServiceImpl`).
/// Only applies when the FQN actually carries a separator — a bare name is
/// rule 1's job.
fn fqn_hits(text: &str, fqn: &str) -> Vec<(usize, usize)> {
    if !(fqn.contains('.') || fqn.contains("::")) {
        return Vec::new();
    }
    word_bounded_hits(text, fqn)
}

/// Rule 2 — the simple name inside an inline code span: `` `BarService` ``
/// exactly, or a filename-style mention like `` `BarService.java` ``.
fn code_span_hits(text: &str, name: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    for (idx, span) in inline_code_spans(text) {
        if code_span_matches(span, name) {
            hits.push((idx, span.len()));
        }
    }
    hits
}

/// Collect inline code spans (single-backtick, single-line) as
/// `(content_byte_idx, content)` pairs.
fn inline_code_spans(text: &str) -> Vec<(usize, &str)> {
    let re = regex::Regex::new(r"`([^`\n]+)`").expect("valid regex");
    re.captures_iter(text)
        .filter_map(|caps| {
            let m = caps.get(1)?;
            Some((m.start(), m.as_str()))
        })
        .collect()
}

/// True when a code span is exactly the class name or the class name plus a
/// single dot-suffix of word characters (`BarService.java`, `BarService.rs`,
/// `BarService.doIt`).
fn code_span_matches(span: &str, name: &str) -> bool {
    let s = span.trim();
    if s == name {
        return true;
    }
    match s.strip_prefix(name) {
        Some(rest) => {
            rest.len() > 1
                && rest.starts_with('.')
                && rest[1..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Rule 1 — the bare name with word boundaries, but only for distinctive
/// CamelCase compounds: at least 6 chars and at least 2 uppercase letters.
/// `BarService` qualifies; `Engine`, `Main`, `Utils` do not.
fn bare_name_hits(text: &str, name: &str) -> Vec<(usize, usize)> {
    let distinctive = name.len() >= 6 && name.chars().filter(|c| c.is_uppercase()).count() >= 2;
    if !distinctive {
        return Vec::new();
    }
    word_bounded_hits(text, name)
}

/// All occurrences of `word` in `text` whose neighbouring characters are not
/// word characters (`[A-Za-z0-9_]`). Case-sensitive.
fn word_bounded_hits(text: &str, word: &str) -> Vec<(usize, usize)> {
    if word.is_empty() {
        return Vec::new();
    }
    text.match_indices(word)
        .filter(|(idx, m)| {
            let before_ok = !text[..*idx].chars().next_back().is_some_and(is_word_char);
            let after_ok = !text[idx + m.len()..]
                .chars()
                .next()
                .is_some_and(is_word_char);
            before_ok && after_ok
        })
        .map(|(idx, m)| (idx, m.len()))
        .collect()
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 1-based line number of the byte offset `idx`. Hit offsets always sit on
/// char boundaries (they come from `match_indices` / regex match starts).
fn line_of(text: &str, idx: usize) -> u32 {
    let head = text.get(..idx).unwrap_or(text);
    u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX)
}

/// ~120-char window around the hit at byte offset `idx` with length
/// `match_len`, adjusted to char boundaries, newlines flattened to spaces.
fn snippet_at(text: &str, idx: usize, match_len: usize) -> String {
    let mut start = idx.saturating_sub(50);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (idx + match_len + 70).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push('…');
    }
    out.replace('\n', " ")
}

/// Read up to [`MAX_SCAN_BYTES`] from a file, UTF-8 lossy. Errors degrade to
/// an empty string — an unreadable doc is simply not a mention.
fn read_head(path: &Path) -> String {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; MAX_SCAN_BYTES];
    let mut filled = 0;
    // `Read::read` may return short counts; loop until the cap or EOF.
    loop {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return String::new(),
        }
        if filled == buf.len() {
            break;
        }
    }
    buf.truncate(filled);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needle() -> ClassNeedle {
        ClassNeedle {
            fqn: "com.foo.BarService".into(),
            name: "BarService".into(),
            file_rel: "core/src/main/java/com/foo/BarService.java".into(),
        }
    }

    #[test]
    fn link_match_via_rel_path_substring() {
        let text =
            "# ADR\n\nThe logic lives in core/src/main/java/com/foo/BarService.java today.\n";
        let m = scan_markdown(text, "docs/adr.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Link);
        assert_eq!(m.line, 3);
        assert!(m.snippet.contains("BarService.java"));
    }

    #[test]
    fn link_match_via_relative_markdown_link() {
        // The href climbs out of `core/docs/` and does NOT contain the full
        // repo-relative path — only the resolver can connect it to the class.
        let text = "# Guide\n\nSee [the service](../src/main/java/com/foo/BarService.java#L10).\n";
        let m = scan_markdown(text, "core/docs/guide.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Link);
        // Exactly 1 link hit + 1 bare-name hit inside the href — were the
        // resolved href double-counted against the substring rule, this
        // would be 3.
        assert_eq!(m.count, 2);
        assert_eq!(m.line, 3);
    }

    #[test]
    fn fqn_match_java_dots_and_rust_colons() {
        let java = scan_markdown("uses com.foo.BarService here", "a.md", &needle()).unwrap();
        assert_eq!(java.kind, MentionKind::Fqn);

        let rust_needle = ClassNeedle {
            fqn: "doc_graph::DocGraph".into(),
            name: "DocGraph".into(),
            file_rel: "crates/core/src/doc_graph.rs".into(),
        };
        let rust = scan_markdown("built by doc_graph::DocGraph.", "a.md", &rust_needle).unwrap();
        assert_eq!(rust.kind, MentionKind::Fqn);
    }

    #[test]
    fn fqn_does_not_match_longer_identifier() {
        assert!(scan_markdown("see com.foo.BarServiceImpl only", "a.md", &needle()).is_none());
    }

    #[test]
    fn code_span_matches_name_and_filename() {
        let m = scan_markdown("wired via `BarService` at boot", "a.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Code);

        let f = scan_markdown("open `BarService.java` first", "a.md", &needle()).unwrap();
        assert_eq!(f.kind, MentionKind::Code);
    }

    #[test]
    fn code_span_rejects_other_classes() {
        // `BarServiceImpl` is its own class; `myBarService` is a field.
        assert!(scan_markdown("see `BarServiceImpl`", "a.md", &needle()).is_none());
        assert!(scan_markdown("see `myBarService`", "a.md", &needle()).is_none());
    }

    #[test]
    fn bare_name_needs_distinctive_camel_case() {
        // Distinctive: >= 6 chars, >= 2 uppercase.
        let m = scan_markdown("the BarService owns retries", "a.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Name);

        // `Engine` — 6 chars but a single hump: bare mention must NOT hit …
        let engine = ClassNeedle {
            fqn: "com.foo.Engine".into(),
            name: "Engine".into(),
            file_rel: "core/Engine.java".into(),
        };
        assert!(scan_markdown("restart the Engine daily", "a.md", &engine).is_none());
        // … but a code span still counts.
        let span = scan_markdown("restart `Engine` daily", "a.md", &engine).unwrap();
        assert_eq!(span.kind, MentionKind::Code);
    }

    #[test]
    fn bare_name_respects_word_boundaries() {
        // `FooServiceImpl` / `myBarService` must not leak a `BarService` hit.
        assert!(scan_markdown("uses BarServiceImpl and myBarService", "a.md", &needle()).is_none());
    }

    #[test]
    fn best_rank_wins_and_count_sums_all_rules() {
        let text = "com.foo.BarService is `BarService`, linked from core/src/main/java/com/foo/BarService.java";
        let m = scan_markdown(text, "a.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Link);
        // fqn(1) + code(1) + path substring(1) + bare name inside the FQN is
        // boundary-blocked by the dot… the FQN's own `BarService` tail *does*
        // count as a bare-name hit (preceded by `.`, followed by ` `).
        assert!(m.count >= 3, "count sums every rule, got {}", m.count);
    }

    #[test]
    fn ranking_orders_by_kind_count_then_rel() {
        let root = tmp_root("ranking");
        std::fs::create_dir_all(root.join("core/src/main/java/com/foo")).unwrap();
        std::fs::write(
            root.join("core/src/main/java/com/foo/BarService.java"),
            "package com.foo; class BarService {}",
        )
        .unwrap();
        std::fs::write(
            root.join("a-name.md"),
            "# A\nBarService twice: BarService\n",
        )
        .unwrap();
        std::fs::write(root.join("b-code.md"), "# B\nsee `BarService`\n").unwrap();
        std::fs::write(root.join("c-fqn.md"), "# C\nsee com.foo.BarService\n").unwrap();
        std::fs::write(
            root.join("d-link.md"),
            "# D\nsee core/src/main/java/com/foo/BarService.java\n",
        )
        .unwrap();
        std::fs::write(root.join("e-name.md"), "# E\nBarService once\n").unwrap();

        let out = docs_for_class(&root, &needle(), 10);
        let rels: Vec<&str> = out.iter().map(|m| m.rel.as_str()).collect();
        // link > fqn > code > name; among the two name-docs the higher count
        // wins, then rel breaks the tie.
        assert_eq!(
            rels,
            vec![
                "d-link.md",
                "c-fqn.md",
                "b-code.md",
                "a-name.md",
                "e-name.md"
            ]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn limit_caps_results_and_target_dir_is_ignored() {
        let root = tmp_root("limit");
        for i in 0..5 {
            std::fs::write(root.join(format!("doc{i}.md")), "# D\n`BarService`\n").unwrap();
        }
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/gen.md"), "# G\n`BarService`\n").unwrap();

        let out = docs_for_class(&root, &needle(), 3);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|m| !m.rel.contains("target")));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snippet_is_char_boundary_safe() {
        // Multi-byte chars right around the 50-char window edge must not panic.
        let text = format!("{}BarService{}", "ä".repeat(60), "ü".repeat(60));
        let m = scan_markdown(&text, "a.md", &needle()).unwrap();
        assert_eq!(m.kind, MentionKind::Name);
        assert!(m.snippet.contains("BarService"));
    }

    fn tmp_root(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "projectmind-doc-mentions-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

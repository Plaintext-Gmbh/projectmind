// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML discovery — full files (`.html`, `.xhtml`, `.htm`, `.jsp`, `.vm`,
//! `.ftl`) plus heuristic snippet extraction from string literals in source
//! code (`.java`, `.kt`, `.kts`, `.groovy`, `.scala`, `.xml`, `.properties`).
//!
//! The snippet detection is deliberately conservative: a literal must contain
//! at least two HTML-ish tag opens to be considered a snippet. This filters
//! out things like XML namespaces (`xmlns="..."`) and short error messages
//! that happen to contain a `<`.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;

/// One HTML/XHTML file found inside a repository.
#[derive(Debug, Clone, Serialize)]
pub struct HtmlFile {
    /// Absolute path on disk.
    pub abs: PathBuf,
    /// Path relative to the requested root, with `/` separators.
    pub rel: String,
    /// Detected dialect — used by the UI to pick syntax-highlight mode.
    pub kind: HtmlKind,
    /// File size in bytes.
    pub size: u64,
}

/// One HTML snippet extracted from a source file's string literal.
#[derive(Debug, Clone, Serialize)]
pub struct HtmlSnippet {
    /// Absolute path of the source file.
    pub abs: PathBuf,
    /// Path relative to the requested root, with `/` separators.
    pub rel: String,
    /// 1-based line where the literal starts.
    pub line: u32,
    /// Source-language hint for the UI (java, kotlin, xml, …).
    pub lang: String,
    /// Decoded HTML content (escape sequences resolved for Java/Kotlin).
    pub content: String,
    /// Number of distinct opening tags detected (used as a quality score).
    pub tag_count: u32,
}

/// HTML dialect classification.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HtmlKind {
    /// Plain HTML (`.html`, `.htm`).
    Html,
    /// Facelets / JSF (`.xhtml`).
    Xhtml,
    /// JSP — server-rendered, may contain scriptlets.
    Jsp,
    /// Velocity template.
    Velocity,
    /// FreeMarker template.
    Freemarker,
}

impl HtmlKind {
    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "html" | "htm" => Some(Self::Html),
            "xhtml" => Some(Self::Xhtml),
            "jsp" => Some(Self::Jsp),
            "vm" => Some(Self::Velocity),
            "ftl" | "ftlh" => Some(Self::Freemarker),
            _ => None,
        }
    }
}

fn build_walker(root: &Path) -> ignore::Walk {
    WalkBuilder::new(root)
        .standard_filters(true)
        .hidden(true)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "node_modules" | "target" | "dist" | "build" | ".git" | ".idea" | ".vscode"
            )
        })
        .build()
}

/// Walk `root` and return every recognised HTML/template file. List is sorted
/// by relative path for stable ordering in the UI.
#[must_use]
pub fn list_html_files(root: &Path) -> Vec<HtmlFile> {
    let mut out = Vec::new();
    for entry in build_walker(root).flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(kind) = HtmlKind::from_extension(ext) else {
            continue;
        };
        let rel_buf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let rel = rel_buf.to_string_lossy().replace('\\', "/");
        let size = entry.metadata().map_or(0, |m| m.len());
        out.push(HtmlFile {
            abs: path.to_path_buf(),
            rel,
            kind,
            size,
        });
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Walk `root` and return HTML snippets embedded in source-code string
/// literals. Heuristic: any literal containing at least two distinct opening
/// tags (`<tag>` or `<ns:tag>`) qualifies. Java/Kotlin escape sequences in
/// the literal are decoded so the rendered output is faithful.
#[must_use]
pub fn find_html_snippets(root: &Path) -> Vec<HtmlSnippet> {
    let mut out = Vec::new();
    for entry in build_walker(root).flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let lang = match ext.to_ascii_lowercase().as_str() {
            "java" => "java",
            "kt" | "kts" => "kotlin",
            "groovy" => "groovy",
            "scala" => "scala",
            "xml" => "xml",
            "properties" => "properties",
            _ => continue,
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // Skip mega-files — keeping them as-is would dominate the scan time
        // and they almost never contain HTML.
        if text.len() > 2_000_000 {
            continue;
        }
        let rel_buf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let rel = rel_buf.to_string_lossy().replace('\\', "/");
        for sn in scan_text(&text, lang) {
            out.push(HtmlSnippet {
                abs: path.to_path_buf(),
                rel: rel.clone(),
                line: sn.line,
                lang: lang.to_string(),
                content: sn.content,
                tag_count: sn.tag_count,
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel).then(a.line.cmp(&b.line)));
    out
}

struct ScannedSnippet {
    line: u32,
    content: String,
    tag_count: u32,
}

fn scan_text(text: &str, lang: &str) -> Vec<ScannedSnippet> {
    match lang {
        "java" | "kotlin" | "groovy" | "scala" => scan_double_quoted(text),
        // XML/properties files use plain `<…>` tags directly — treating the
        // whole file as one snippet would be misleading. Skip for now; the
        // file scanner already lists XHTML separately.
        _ => Vec::new(),
    }
}

/// Scan a Java/Kotlin-style source for double-quoted string literals that
/// look like HTML. Handles `"…"`, `\"` escapes, and Java 15+ text blocks
/// (`"""…"""`).
fn scan_double_quoted(text: &str) -> Vec<ScannedSnippet> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut line = 1u32;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        // Line comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Block comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    line += 1;
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // Text block """…"""
        if b == b'"' && i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
            let start_line = line;
            i += 3;
            let lit_start = i;
            let mut lit_end = i;
            while i + 2 < bytes.len()
                && !(bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"')
            {
                if bytes[i] == b'\n' {
                    line += 1;
                }
                i += 1;
                lit_end = i;
            }
            i = (i + 3).min(bytes.len());
            if let Some(slice) = text.get(lit_start..lit_end) {
                consider(&decode_escapes(slice), start_line, &mut out);
            }
            continue;
        }
        // Regular "…"
        if b == b'"' {
            let start_line = line;
            i += 1;
            let lit_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    if bytes[i + 1] == b'\n' {
                        line += 1;
                    }
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\n' {
                    // unterminated literal across newline — bail
                    line += 1;
                    break;
                }
                i += 1;
            }
            let lit_end = i.min(bytes.len());
            i = (i + 1).min(bytes.len());
            if let Some(slice) = text.get(lit_start..lit_end) {
                consider(&decode_escapes(slice), start_line, &mut out);
            }
            continue;
        }
        i += 1;
    }
    out
}

fn decode_escapes(s: &str) -> String {
    // Cheap-but-good-enough Java/Kotlin escape decoder.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') | None => out.push('\\'),
            Some('\'') => out.push('\''),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

fn consider(content: &str, line: u32, out: &mut Vec<ScannedSnippet>) {
    let count = count_tags(content);
    if count < 2 {
        return;
    }
    if content.trim().is_empty() {
        return;
    }
    out.push(ScannedSnippet {
        line,
        content: content.to_string(),
        tag_count: count,
    });
}

fn count_tags(content: &str) -> u32 {
    // Match `<tag>` and `<ns:tag>` openings; ignores closings and self-close.
    static_re().find_iter(content).count() as u32
}

fn static_re() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"<([a-zA-Z][a-zA-Z0-9]*:)?[a-zA-Z][a-zA-Z0-9-]*(\s|>|/)").unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_ext(rel: &str, ext: &str) -> bool {
        std::path::Path::new(rel)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("plaintext-ide-html-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_html_xhtml_jsp() {
        let root = tmp_dir("files");
        std::fs::write(root.join("index.html"), "<html><body>x</body></html>").unwrap();
        std::fs::create_dir_all(root.join("WEB-INF")).unwrap();
        std::fs::write(root.join("WEB-INF/page.xhtml"), "<ui:composition/>").unwrap();
        std::fs::write(root.join("legacy.jsp"), "<%@ page %><html/>").unwrap();
        std::fs::write(root.join("note.txt"), "not html").unwrap();
        // Build dir should be skipped.
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/leak.html"), "<html/>").unwrap();

        let files = list_html_files(&root);
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert!(rels.contains(&"index.html"));
        assert!(rels.contains(&"WEB-INF/page.xhtml"));
        assert!(rels.contains(&"legacy.jsp"));
        assert!(!rels.iter().any(|r| r.contains("target")));
        assert!(!rels.iter().any(|r| has_ext(r, "txt")));

        let xhtml = files.iter().find(|f| has_ext(&f.rel, "xhtml")).unwrap();
        assert_eq!(xhtml.kind, HtmlKind::Xhtml);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extracts_html_from_java_string() {
        let java = r#"
            class Foo {
                String tpl = "<div class=\"x\"><p>hi</p></div>";
                String not = "just a < single";
                String mail = """
                    <html>
                        <body><p>multi</p></body>
                    </html>
                    """;
            }
        "#;
        let root = tmp_dir("snippets");
        std::fs::write(root.join("Foo.java"), java).unwrap();

        let snippets = find_html_snippets(&root);
        assert!(snippets.iter().any(|s| s.content.contains("<div")));
        assert!(snippets.iter().any(|s| s.content.contains("<body>")));
        assert!(!snippets.iter().any(|s| s.content.contains("just a <")));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignores_xmlns_only_strings() {
        let java = r#"
            class Foo {
                String ns = "xmlns=\"http://java.sun.com/xml/ns/javaee\"";
                String single = "<just one tag>";
            }
        "#;
        let root = tmp_dir("xmlns");
        std::fs::write(root.join("F.java"), java).unwrap();
        let snippets = find_html_snippets(&root);
        assert!(snippets.is_empty(), "should not match: {snippets:?}");
        std::fs::remove_dir_all(&root).ok();
    }
}

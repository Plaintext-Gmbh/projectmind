// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Code ↔ doc bridge, external half of
//! [#65](https://github.com/Plaintext-Gmbh/projectmind/issues/65): the
//! Confluence pages, Jira tickets, issue-tracker items and documentation URLs
//! a class's source refers to.
//!
//! [`crate::doc_mentions`] answers "which repo-internal Markdown talks about
//! this class?". This module answers the mirror question for *external*
//! documentation — "what does the code itself point at?" — with a single,
//! cheap regex sweep over the class's source file:
//!
//! - **URLs** are classified by shape into Confluence, Jira, issue tracker
//!   (GitHub / GitLab issues and pull requests) or plain documentation link.
//!   Confluence / Jira / issue URLs are unambiguous and count anywhere in the
//!   file, string literals included. Plain URLs count only on **comment
//!   lines** (`//`, `/* … */`, `///`, `//!`, `#`, `<!-- -->`), because URLs in
//!   code are overwhelmingly endpoints, XML namespaces and licence headers,
//!   not documentation; a small deny-list drops the licence / schema hosts
//!   that live in every file header.
//! - **Jira keys** (`PAY-1234`) are matched on comment lines. Without
//!   configuration a deny-list of look-alikes (`UTF-8`, `SHA-256`, `ISO-8859`,
//!   `CVE-2024-…`, …) keeps the noise out; with `jira_projects` configured in
//!   `.projectmind/config.toml` only those prefixes count, and they count
//!   everywhere — including string literals. With `jira_base` configured a
//!   key becomes a clickable URL.
//!
//! The sweep is O(lines) with no per-class state; hosts call it on demand for
//! the open class, exactly like `docs_for_class`. The live-preview idea from
//! the original sketch (hover → fetch the Confluence page through an MCP
//! bridge) is deliberately out of scope: it needs an authenticated bridge
//! server, and the side-bar here already gives the navigation that most users
//! wanted from it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::persistence::ExternalDocsConfig;

/// Cap on how much of a source file is scanned (generated giants are not
/// where hand-written references live).
const MAX_SCAN_BYTES: usize = 2 * 1024 * 1024;
/// Length cap for the context line shown as a tooltip.
const CONTEXT_CHARS: usize = 160;

/// What kind of external reference a link is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodeLinkKind {
    /// A Confluence page URL.
    Confluence,
    /// A Jira ticket — a key like `PAY-1234`, or a Jira browse URL.
    Jira,
    /// An issue-tracker item: GitHub / GitLab issue or pull request URL.
    Issue,
    /// Any other documentation URL found on a comment line.
    Url,
}

/// One external reference found in a class's source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeLink {
    /// Classification.
    pub kind: CodeLinkKind,
    /// What to show: the ticket key, or the URL with its scheme stripped.
    pub label: String,
    /// Where to go. `None` for a Jira key when no `jira_base` is configured —
    /// the GUI then jumps to the source line instead.
    pub url: Option<String>,
    /// 1-based line of the first occurrence.
    pub line: u32,
    /// The (trimmed, capped) source line of that occurrence.
    pub context: String,
    /// Occurrences in the file (same kind + label).
    pub count: u32,
    /// `true` when the first occurrence sits inside the class's own line
    /// range (as opposed to the file header or a sibling type).
    pub in_class: bool,
}

/// Scan a class's source file for external references.
///
/// `class.file` is relative to `module_root` (the plugin convention);
/// `class.line_start..=line_end` marks the class body for the `in_class`
/// flag. Files above 2 MiB are truncated to their head; unreadable files
/// yield an empty list.
#[must_use]
pub fn code_links_for_class(
    module_root: &Path,
    class: &projectmind_plugin_api::Class,
    config: &ExternalDocsConfig,
) -> Vec<CodeLink> {
    let path = module_root.join(&class.file);
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let head = &bytes[..bytes.len().min(MAX_SCAN_BYTES)];
    let text = String::from_utf8_lossy(head);
    let range = (class.line_start > 0).then_some((class.line_start, class.line_end));
    code_links_in_text(&text, config, range)
}

/// Pure sweep over `text`. `class_range` (1-based, inclusive) drives the
/// `in_class` flag. Results are ordered by first occurrence, deduplicated by
/// (kind, label) with a running `count`.
#[must_use]
pub fn code_links_in_text(
    text: &str,
    config: &ExternalDocsConfig,
    class_range: Option<(u32, u32)>,
) -> Vec<CodeLink> {
    let mut order: Vec<(CodeLinkKind, String)> = Vec::new();
    let mut links: BTreeMap<(CodeLinkKind, String), CodeLink> = BTreeMap::new();
    let jira_projects: Vec<String> = config
        .jira_projects
        .iter()
        .map(|p| p.trim().to_ascii_uppercase())
        .filter(|p| !p.is_empty())
        .collect();
    let jira_base = config
        .jira_base
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| {
            if b.ends_with('/') {
                b.to_string()
            } else {
                format!("{b}/")
            }
        });

    let mut in_block_comment = false;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        let trimmed = raw.trim();
        let (comment, next_in_block) = comment_state(raw, trimmed, in_block_comment);
        in_block_comment = next_in_block;
        let in_class = class_range.is_some_and(|(from, to)| line_no >= from && line_no <= to);

        let mut push = |kind: CodeLinkKind, label: String, url: Option<String>| {
            let key = (kind, label.clone());
            match links.get_mut(&key) {
                Some(existing) => existing.count += 1,
                None => {
                    order.push(key.clone());
                    links.insert(
                        key,
                        CodeLink {
                            kind,
                            label,
                            url,
                            line: line_no,
                            context: context_of(trimmed),
                            count: 1,
                            in_class,
                        },
                    );
                }
            }
        };

        // URLs first, and remember their spans so a Jira key inside a browse
        // URL is not reported twice.
        let mut url_spans: Vec<(usize, usize)> = Vec::new();
        for m in url_regex().find_iter(raw) {
            let url = trim_url(m.as_str());
            if url.is_empty() {
                continue;
            }
            url_spans.push((m.start(), m.start() + url.len()));
            let Some(kind) = classify_url(url, comment.covers(m.start())) else {
                continue;
            };
            let label = match kind {
                CodeLinkKind::Jira => {
                    jira_key_from_browse_url(url).unwrap_or_else(|| strip_scheme(url).to_string())
                }
                _ => strip_scheme(url).to_string(),
            };
            push(kind, label, Some(url.to_string()));
        }

        // Jira keys.
        for m in jira_key_regex().find_iter(raw) {
            if url_spans
                .iter()
                .any(|(s, e)| m.start() >= *s && m.end() <= *e)
            {
                continue;
            }
            let key = m.as_str();
            let prefix = key.split('-').next().unwrap_or(key);
            let accepted = if jira_projects.is_empty() {
                comment.covers(m.start()) && !JIRA_PREFIX_DENYLIST.contains(&prefix)
            } else {
                jira_projects.iter().any(|p| p == prefix)
            };
            if !accepted {
                continue;
            }
            let url = jira_base.as_ref().map(|b| format!("{b}{key}"));
            push(CodeLinkKind::Jira, key.to_string(), url);
        }
    }

    order
        .into_iter()
        .filter_map(|key| links.remove(&key))
        .collect()
}

/// Which part of a line is comment text.
#[derive(Debug, Clone, Copy)]
enum CommentSpan {
    /// No comment on this line.
    None,
    /// The whole line is a comment.
    Full,
    /// A trailing `// …` comment starting at this byte offset of the raw line.
    From(usize),
}

impl CommentSpan {
    fn covers(self, at: usize) -> bool {
        match self {
            CommentSpan::None => false,
            CommentSpan::Full => true,
            CommentSpan::From(start) => at >= start,
        }
    }
}

/// Where the comment text of this line is, and whether we are inside a block
/// comment afterwards. Line-based on purpose — good enough for Java, Kotlin,
/// Rust, C-family, Python/Ruby/shell `#`, SQL `--`, and HTML/XML comments.
fn comment_state(raw: &str, trimmed: &str, in_block: bool) -> (CommentSpan, bool) {
    if in_block {
        let closes = trimmed.contains("*/") || trimmed.contains("-->");
        return (CommentSpan::Full, !closes);
    }
    let opens_block = trimmed.starts_with("/*") || trimmed.starts_with("<!--");
    if opens_block {
        let closes = trimmed.contains("*/") || trimmed.contains("-->");
        return (CommentSpan::Full, !closes);
    }
    let line_comment = trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with('#')
        || trimmed.starts_with("--")
        || trimmed.starts_with("'''")
        || trimmed.starts_with("\"\"\"");
    if line_comment {
        return (CommentSpan::Full, false);
    }
    // A code line with a trailing `// …` comment: only the part after the
    // marker is comment text (`://` inside a URL never matches — it has no
    // whitespace in front of the slashes).
    let trailing = raw.find(" //").or_else(|| raw.find("\t//"));
    match trailing {
        Some(i) => (CommentSpan::From(i), false),
        None => (CommentSpan::None, false),
    }
}

fn context_of(trimmed: &str) -> String {
    if trimmed.chars().count() <= CONTEXT_CHARS {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(CONTEXT_CHARS - 1).collect();
    format!("{cut}…")
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"https?://[^\s<>"'`)\]}]+"#).expect("valid regex"))
}

fn jira_key_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Prefix: 2–10 chars, uppercase letters/digits, starting with a letter.
    // Number: 1–6 digits. Word boundaries both ends so `XPAY-12` and
    // `PAY-123a` don't match.
    RE.get_or_init(|| Regex::new(r"\b[A-Z][A-Z0-9]{1,9}-\d{1,6}\b").expect("valid regex"))
}

/// Ticket-key look-alikes that show up in ordinary source comments.
const JIRA_PREFIX_DENYLIST: &[&str] = &[
    "UTF", "ISO", "SHA", "MD", "RFC", "CVE", "CWE", "HTTP", "TLS", "SSL", "RSA", "AES", "IEEE",
    "ANSI", "ASCII", "DIN", "EN", "ID", "PK", "FK", "X", "UTC", "GMT", "IEC", "ECMA", "JDK", "JEP",
    "JSR", "PEP", "GB", "MB", "KB", "TB", "PS", "IPV", "IP", "HMAC", "PBKDF", "ES", "AZ", "SDK",
    "API", "V", "IPHONE", "A", "B", "C", "P", "S", "T",
];

/// Hosts / path fragments that are never documentation: licence headers,
/// XML schemas, local endpoints, placeholder domains.
const URL_DENYLIST: &[&str] = &[
    "mozilla.org/MPL",
    "apache.org/licenses",
    "opensource.org/licenses",
    "gnu.org/licenses",
    "creativecommons.org/licenses",
    "www.w3.org/",
    "schemas.xmlsoap.org",
    "xmlns.jcp.org",
    "java.sun.com/xml",
    "jakarta.ee/xml",
    "springframework.org/schema",
    "maven.apache.org/POM",
    "maven.apache.org/xsd",
    "localhost",
    "127.0.0.1",
    "0.0.0.0",
    "example.com",
    "example.org",
    "schema.org",
];

/// Classify a URL, or `None` when it should not be reported.
fn classify_url(url: &str, on_comment_line: bool) -> Option<CodeLinkKind> {
    let lower = url.to_ascii_lowercase();
    if URL_DENYLIST
        .iter()
        .any(|d| lower.contains(&d.to_ascii_lowercase()))
    {
        return None;
    }
    let (host, path) = split_host_path(&lower);
    let atlassian = host.ends_with(".atlassian.net");
    if path.contains("/wiki/")
        || path.contains("/confluence/")
        || path.contains("/display/")
        || path.contains("/pages/viewpage.action")
        || host.contains("confluence")
    {
        return Some(CodeLinkKind::Confluence);
    }
    if path.contains("/browse/") || (atlassian && path.contains("/jira/")) || host.contains("jira")
    {
        return Some(CodeLinkKind::Jira);
    }
    if (host == "github.com" && (path.contains("/issues/") || path.contains("/pull/")))
        || (path.contains("/-/issues/") || path.contains("/-/merge_requests/"))
        || (host.contains("gitlab") && path.contains("/issues/"))
    {
        return Some(CodeLinkKind::Issue);
    }
    on_comment_line.then_some(CodeLinkKind::Url)
}

fn split_host_path(lower: &str) -> (&str, &str) {
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(lower);
    match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    }
}

fn strip_scheme(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// `…/browse/PAY-1234` → `PAY-1234`.
fn jira_key_from_browse_url(url: &str) -> Option<String> {
    let i = url.find("/browse/")? + "/browse/".len();
    let key: String = url[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    jira_key_regex().is_match(&key).then_some(key)
}

/// Drop trailing punctuation a sentence or Javadoc tag leaves glued to a URL.
fn trim_url(raw: &str) -> &str {
    let mut s = raw;
    loop {
        let trimmed = s.trim_end_matches(['.', ',', ';', ':', '!', '?', '*']);
        // A `)` only closes the URL when the URL has no matching `(`.
        let trimmed = if trimmed.ends_with(')') && !trimmed.contains('(') {
            &trimmed[..trimmed.len() - 1]
        } else {
            trimmed
        };
        if trimmed.len() == s.len() {
            return s;
        }
        s = trimmed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ExternalDocsConfig {
        ExternalDocsConfig::default()
    }

    fn kinds(links: &[CodeLink]) -> Vec<(CodeLinkKind, &str)> {
        links.iter().map(|l| (l.kind, l.label.as_str())).collect()
    }

    #[test]
    fn confluence_and_jira_urls_count_anywhere_even_in_strings() {
        let src = r#"
package a;
/** Settlement flow.
 *  @see https://acme.atlassian.net/wiki/spaces/PAY/pages/123/Settlement
 */
public class PaymentService {
    static final String TICKET = "https://acme.atlassian.net/browse/PAY-1234";
    static final String ISSUE = "https://github.com/acme/shop/issues/42";
}
"#;
        let links = code_links_in_text(src, &cfg(), Some((6, 9)));
        assert_eq!(
            kinds(&links),
            vec![
                (
                    CodeLinkKind::Confluence,
                    "acme.atlassian.net/wiki/spaces/PAY/pages/123/Settlement"
                ),
                (CodeLinkKind::Jira, "PAY-1234"),
                (CodeLinkKind::Issue, "github.com/acme/shop/issues/42"),
            ]
        );
        assert_eq!(links[0].line, 4);
        assert!(!links[0].in_class);
        assert!(links[1].in_class);
        assert_eq!(
            links[1].url.as_deref(),
            Some("https://acme.atlassian.net/browse/PAY-1234")
        );
        // The key inside the browse URL is not reported a second time.
        assert_eq!(links.iter().filter(|l| l.label == "PAY-1234").count(), 1);
    }

    #[test]
    fn plain_urls_only_count_on_comment_lines() {
        let src = "// docs: https://docs.acme.io/guide/settlement.\nString ep = \"https://api.acme.io/v1/settle\";\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(
            kinds(&links),
            vec![(CodeLinkKind::Url, "docs.acme.io/guide/settlement")]
        );
        assert_eq!(
            links[0].url.as_deref(),
            Some("https://docs.acme.io/guide/settlement")
        );
    }

    #[test]
    fn licence_headers_and_schemas_are_ignored() {
        let src = "// This Source Code Form is subject to the terms of the Mozilla Public\n// License, v. 2.0. If a copy of the MPL was not distributed with this\n// file, You can obtain one at https://mozilla.org/MPL/2.0/.\n// http://www.apache.org/licenses/LICENSE-2.0\n/* xmlns=\"http://www.w3.org/2001/XMLSchema\" */\n// http://localhost:8080/health\n";
        assert!(code_links_in_text(src, &cfg(), None).is_empty());
    }

    #[test]
    fn jira_keys_on_comment_lines_with_lookalikes_filtered() {
        let src = "// PAY-1289: retry on 503\n// decode as UTF-8, hash with SHA-256, see ISO-8859-1 and CVE-2024-1234\nint x = 1; // MD-5 is not a ticket\nString s = \"PAY-1500\"; // in code, no allow-list → ignored\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(kinds(&links), vec![(CodeLinkKind::Jira, "PAY-1289")]);
        assert!(links[0].url.is_none(), "no jira_base → no URL");
    }

    #[test]
    fn jira_allowlist_counts_everywhere_and_base_makes_urls() {
        let config = ExternalDocsConfig {
            jira_base: Some("https://acme.atlassian.net/browse".into()),
            jira_projects: vec!["pay".into(), "OPS".into()],
        };
        let src = "String s = \"PAY-1500\";\n// UTF-8 and ABC-1 are not ours; OPS-7 is\n";
        let links = code_links_in_text(src, &config, None);
        assert_eq!(
            kinds(&links),
            vec![
                (CodeLinkKind::Jira, "PAY-1500"),
                (CodeLinkKind::Jira, "OPS-7")
            ]
        );
        assert_eq!(
            links[0].url.as_deref(),
            Some("https://acme.atlassian.net/browse/PAY-1500")
        );
    }

    #[test]
    fn repeated_references_are_counted_once_with_first_line() {
        let src = "// PAY-1: a\n// PAY-1: b\n/* PAY-1 again */\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].count, 3);
        assert_eq!(links[0].line, 1);
        assert_eq!(links[0].context, "// PAY-1: a");
    }

    #[test]
    fn trailing_punctuation_and_javadoc_stars_are_trimmed() {
        let src = " * See https://acme.atlassian.net/wiki/x/Y).\n * (https://intra.acme.io/confluence/display/DOC/Home;\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(
            links
                .iter()
                .map(|l| l.url.clone().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "https://acme.atlassian.net/wiki/x/Y".to_string(),
                "https://intra.acme.io/confluence/display/DOC/Home".to_string()
            ]
        );
        assert!(links.iter().all(|l| l.kind == CodeLinkKind::Confluence));
    }

    #[test]
    fn block_comments_span_lines() {
        let src = "/*\n  handbook: https://handbook.acme.io/ops\n*/\nString x = \"https://handbook.acme.io/ignored\";\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(
            kinds(&links),
            vec![(CodeLinkKind::Url, "handbook.acme.io/ops")]
        );
    }

    #[test]
    fn gitlab_issues_and_merge_requests_are_issues() {
        let src = "// https://gitlab.acme.io/shop/core/-/issues/9 and https://gitlab.acme.io/shop/core/-/merge_requests/3\n";
        let links = code_links_in_text(src, &cfg(), None);
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.kind == CodeLinkKind::Issue));
    }

    #[test]
    fn long_context_is_capped() {
        let long = format!("// PAY-9 {}", "x".repeat(400));
        let links = code_links_in_text(&long, &cfg(), None);
        assert_eq!(links[0].context.chars().count(), CONTEXT_CHARS);
        assert!(links[0].context.ends_with('…'));
    }

    #[test]
    fn class_wrapper_reads_file_relative_to_module_root() {
        let dir = std::env::temp_dir().join(format!("pm-code-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src_dir = dir.join("src/main/java/a");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            src_dir.join("Foo.java"),
            "package a;\n// PAY-77\npublic class Foo {}\n",
        )
        .unwrap();
        let class = projectmind_plugin_api::Class {
            fqn: "a.Foo".into(),
            name: "Foo".into(),
            file: "src/main/java/a/Foo.java".into(),
            line_start: 3,
            line_end: 3,
            ..Default::default()
        };
        let links = code_links_for_class(&dir, &class, &cfg());
        assert_eq!(kinds(&links), vec![(CodeLinkKind::Jira, "PAY-77")]);
        assert!(!links[0].in_class);
        // Missing file → empty, never an error.
        let ghost = projectmind_plugin_api::Class {
            file: "nope.java".into(),
            ..Default::default()
        };
        assert!(code_links_for_class(&dir, &ghost, &cfg()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

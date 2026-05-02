// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Markdown documentation graph extraction.
//!
//! The graph is intentionally repository-local: markdown files are nodes,
//! relative markdown links are edges, external links are counted on the source
//! node, and broken relative markdown links are returned as dangling entries.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use serde::Serialize;

use crate::files;

#[derive(Debug, Clone, Serialize)]
/// Repository-local graph of Markdown documentation links.
pub struct DocGraph {
    /// Repository root used to build the graph.
    pub root: PathBuf,
    /// Markdown documents in stable relative-path order.
    pub nodes: Vec<DocNode>,
    /// Resolved internal Markdown links.
    pub edges: Vec<DocEdge>,
    /// Relative Markdown links that point at missing files.
    pub dangling: Vec<DanglingDocLink>,
    /// Number of documents with no inbound internal links.
    pub orphan_count: usize,
    /// Number of dangling relative Markdown links.
    pub dangling_count: usize,
    /// Number of external links encountered across all documents.
    pub external_count: usize,
}

#[derive(Debug, Clone, Serialize)]
/// One Markdown document in the documentation graph.
pub struct DocNode {
    /// Stable node id; currently the repository-relative path.
    pub id: String,
    /// Absolute path on disk.
    pub abs: PathBuf,
    /// Repository-relative path with `/` separators.
    pub rel: String,
    /// First H1 title or file stem.
    pub title: String,
    /// Count of resolved internal links pointing at this document.
    pub inbound: u32,
    /// Count of resolved internal links originating from this document.
    pub outbound: u32,
    /// Count of external links originating from this document.
    pub external: u32,
    /// True when no internal links point at this document.
    pub orphan: bool,
}

#[derive(Debug, Clone, Serialize)]
/// One resolved internal Markdown link.
pub struct DocEdge {
    /// Source document id.
    pub from: String,
    /// Target document id.
    pub to: String,
    /// Link label from the source Markdown.
    pub label: String,
    /// Original href text from the source Markdown.
    pub href: String,
}

#[derive(Debug, Clone, Serialize)]
/// One relative Markdown link whose target file does not exist.
pub struct DanglingDocLink {
    /// Source document id.
    pub from: String,
    /// Link label from the source Markdown.
    pub label: String,
    /// Original href text from the source Markdown.
    pub href: String,
    /// Absolute path the href resolved to.
    pub resolved: PathBuf,
}

#[must_use]
/// Build a documentation graph for all Markdown files under `root`.
pub fn build(root: &Path) -> DocGraph {
    let files = files::list_markdown_files(root);
    let known: BTreeMap<String, &files::MarkdownFile> =
        files.iter().map(|f| (f.rel.clone(), f)).collect();
    let mut inbound: BTreeMap<String, u32> = BTreeMap::new();
    let mut outbound: BTreeMap<String, u32> = BTreeMap::new();
    let mut external: BTreeMap<String, u32> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut dangling = Vec::new();
    let mut seen_edges: BTreeSet<(String, String, String)> = BTreeSet::new();

    for f in &files {
        let Ok(markdown) = std::fs::read_to_string(&f.abs) else {
            continue;
        };
        for link in markdown_links(&markdown) {
            if is_external(&link.href) {
                *external.entry(f.rel.clone()).or_default() += 1;
                continue;
            }
            let Some(target) = resolve_markdown_href(root, &f.abs, &link.href) else {
                continue;
            };
            let target_rel = target.to_string_lossy().replace('\\', "/");
            if known.contains_key(&target_rel) {
                let key = (f.rel.clone(), target_rel.clone(), link.href.clone());
                if seen_edges.insert(key) {
                    *outbound.entry(f.rel.clone()).or_default() += 1;
                    *inbound.entry(target_rel.clone()).or_default() += 1;
                    edges.push(DocEdge {
                        from: f.rel.clone(),
                        to: target_rel,
                        label: link.label,
                        href: link.href,
                    });
                }
            } else {
                dangling.push(DanglingDocLink {
                    from: f.rel.clone(),
                    label: link.label,
                    href: link.href,
                    resolved: root.join(target),
                });
            }
        }
    }

    let nodes: Vec<DocNode> = files
        .into_iter()
        .map(|f| {
            let inbound_count = inbound.get(&f.rel).copied().unwrap_or(0);
            let outbound_count = outbound.get(&f.rel).copied().unwrap_or(0);
            let external_count = external.get(&f.rel).copied().unwrap_or(0);
            DocNode {
                id: f.rel.clone(),
                abs: f.abs,
                rel: f.rel,
                title: f.title,
                inbound: inbound_count,
                outbound: outbound_count,
                external: external_count,
                orphan: inbound_count == 0,
            }
        })
        .collect();

    DocGraph {
        root: root.to_path_buf(),
        orphan_count: nodes.iter().filter(|n| n.orphan).count(),
        dangling_count: dangling.len(),
        external_count: nodes.iter().map(|n| n.external as usize).sum(),
        nodes,
        edges,
        dangling,
    }
}

#[derive(Debug)]
struct MarkdownLink {
    label: String,
    href: String,
}

fn markdown_links(markdown: &str) -> Vec<MarkdownLink> {
    let link_re =
        Regex::new(r#"!?\[([^\]\n]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).expect("valid regex");
    link_re
        .captures_iter(markdown)
        .filter_map(|caps| {
            let whole = caps.get(0)?.as_str();
            if whole.starts_with('!') {
                return None;
            }
            Some(MarkdownLink {
                label: caps.get(1)?.as_str().trim().to_string(),
                href: caps.get(2)?.as_str().trim().to_string(),
            })
        })
        .collect()
}

fn is_external(href: &str) -> bool {
    let lower = href.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
}

fn resolve_markdown_href(root: &Path, source_abs: &Path, href: &str) -> Option<PathBuf> {
    if href.starts_with('#') || href.starts_with('/') {
        return None;
    }
    let without_fragment = href.split_once('#').map_or(href, |(path, _)| path);
    if without_fragment.is_empty() {
        return None;
    }
    let ext = Path::new(without_fragment)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "md" | "markdown" | "mdx") {
        return None;
    }
    let base = source_abs.parent().unwrap_or(root);
    let joined = normalize_relative(base.join(without_fragment));
    joined.strip_prefix(root).ok().map(Path::to_path_buf)
}

fn normalize_relative(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "projectmind-doc-graph-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn links_docs_and_reports_orphans_and_dangling() {
        let root = tmp_root("basic");
        std::fs::write(
            root.join("README.md"),
            "# Readme\n\nSee [Guide](docs/guide.md) and [Missing](docs/missing.md).\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/guide.md"),
            "# Guide\n\nBack to [home](../README.md#readme). External [site](https://example.com).\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/orphan.md"), "# Orphan\n").unwrap();

        let graph = build(&root);

        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 2);
        assert_eq!(graph.dangling_count, 1);
        assert_eq!(graph.external_count, 1);
        assert!(graph
            .nodes
            .iter()
            .any(|n| n.rel == "docs/orphan.md" && n.orphan));
        assert!(graph
            .edges
            .iter()
            .any(|e| e.from == "README.md" && e.to == "docs/guide.md"));

        let _ = std::fs::remove_dir_all(root);
    }
}

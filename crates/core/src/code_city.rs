// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Data payload for the 3D code city (`code-city`, #66).
//!
//! One walk over the working tree (same filters as the folder map) plus two
//! optional joins:
//!
//! - **bytes** from filesystem metadata — the universal base height, so
//!   `.svelte`/`.ts`/`.md` files get buildings even though no language
//!   plugin parses them.
//! - **risk** via [`risk::compute`] — per-file `sloc` (precise height) and
//!   `score` (facade colour) for parsed Java/Rust classes. Degrades to
//!   `None` per node when the repo has no git history or no parsed classes.
//! - **recency** via [`git::file_recency`] — "freshly built" glow for files
//!   touched by recent commits. Degrades the same way.
//!
//! Served through its own endpoint like `render_bean_graph_data` —
//! `show_diagram` does not know this kind. The folder-map payload stays
//! untouched; this module owns its own walk so the caps can differ (an
//! `InstancedMesh` renders 2000 boxes effortlessly, while the 2D solar view
//! stays readable only up to ~420 nodes).

use std::collections::HashMap;
use std::path::Path;

use ignore::WalkBuilder;
use projectmind_plugin_api::Relation;
use serde::Serialize;

use crate::repository::Repository;
use crate::{coverage, git, risk};

/// Depth cap, mirrors the folder map so the two views agree on granularity.
const MAX_DEPTH: usize = 5;
/// Node cap. Higher than the folder map's 420: readability there is a 2D
/// concern; the 3D city renders thousands of instanced boxes effortlessly.
const MAX_NODES: usize = 2000;

/// JSON payload for the 3D code city (`code-city`, #66). Own endpoint like
/// `BeanGraphData` — `show_diagram` does not serve it.
#[derive(Debug, Clone, Serialize)]
pub struct CodeCityData {
    /// Repository root, for display only.
    pub root: String,
    /// Walk depth cap (= 5, mirrors the folder map).
    pub max_depth: usize,
    /// `true` when the walk hit [`MAX_NODES`] and stopped early.
    pub truncated: bool,
    /// `false` when the risk join produced nothing (no git history or no
    /// parsed classes) — the frontend hides the risk legend then.
    pub has_risk: bool,
    /// Every walked node; the root comes first, the rest in walk order.
    pub nodes: Vec<CityNode>,
}

/// One walked filesystem node — a future district (folder) or building (file).
#[derive(Debug, Clone, Serialize)]
pub struct CityNode {
    /// Repo-relative forward-slashed path; `"."` for the root.
    pub id: String,
    /// Parent id; `None` only for the root.
    pub parent: Option<String>,
    /// File or directory name.
    pub label: String,
    /// `"root"` | `"folder"` | `"file"`.
    pub kind: &'static str,
    /// Path depth below the root, `0..=5`.
    pub depth: usize,
    /// File: size from fs metadata. Folder/root: subtree sum, min 1 so an
    /// empty district still occupies a sliver of ground.
    pub bytes: u64,
    /// Sum of `sloc` over the parsed classes in this file (risk join).
    pub sloc: Option<u32>,
    /// Max risk score (0..=100) over the classes in this file (risk join).
    pub risk_score: Option<f64>,
    /// Max churn over the classes in this file (risk join).
    pub churn: Option<u32>,
    /// FQN of the highest-scored class in this file — the drill target.
    pub fqn: Option<String>,
    /// Module of that class (the `ClassViewer` drill needs both).
    pub module: Option<String>,
    /// Seconds since the last commit touching this file (recency join) —
    /// drives the "freshly built" glow.
    pub recency_secs_ago: Option<u64>,
}

/// Per-file aggregate of the risk join (folded from per-class scores).
struct FileRisk {
    sloc: u32,
    score: f64,
    churn: u32,
    fqn: String,
    module: String,
}

/// Build the city payload: one walk + risk join + recency join.
///
/// `relations` comes from the caller's engine (`Engine::relations`) so this
/// never re-scans the repo — the same contract `risk_atlas` uses. Both joins
/// degrade gracefully: without git history (or without parsed classes) the
/// per-node options stay `None` and `has_risk` turns `false`.
#[must_use]
pub fn build(repo: &Repository, relations: &[Relation]) -> CodeCityData {
    let root = repo.root.clone();
    let mut nodes = vec![CityNode {
        id: ".".into(),
        parent: None,
        label: root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo")
            .to_string(),
        kind: "root",
        depth: 0,
        bytes: 0,
        sloc: None,
        risk_score: None,
        churn: None,
        fqn: None,
        module: None,
        recency_secs_ago: None,
    }];
    let mut truncated = false;

    // Walk configured identically to `render_folder_map` (same skips, same
    // depth cap) so city and folder map show the same tree.
    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .parents(true)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .max_depth(Some(MAX_DEPTH))
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | "target" | "node_modules" | "dist" | ".svelte-kit"
            )
        })
        .build();

    for entry in walker.flatten() {
        if entry.path() == root {
            continue;
        }
        if nodes.len() >= MAX_NODES {
            truncated = true;
            break;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let depth = rel.components().count();
        let id = rel_id(rel);
        let parent = rel
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| ".".to_string(), rel_id);
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let bytes = if is_dir {
            0 // aggregated below
        } else {
            entry.metadata().map_or(0, |m| m.len())
        };
        nodes.push(CityNode {
            id,
            parent: Some(parent),
            label: rel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string(),
            kind: if is_dir { "folder" } else { "file" },
            depth,
            bytes,
            sloc: None,
            risk_score: None,
            churn: None,
            fqn: None,
            module: None,
            recency_secs_ago: None,
        });
    }

    // Aggregate file bytes up the folder chain (same walk-up the folder map
    // uses for its counts, only summing u64 sizes instead).
    let parent_by_id: HashMap<String, Option<String>> = nodes
        .iter()
        .map(|n| (n.id.clone(), n.parent.clone()))
        .collect();
    let mut folder_bytes: HashMap<String, u64> = HashMap::new();
    for (id, bytes) in nodes
        .iter()
        .filter(|n| n.kind == "file")
        .map(|n| (n.id.clone(), n.bytes))
    {
        let mut cur = parent_by_id.get(&id).cloned().flatten();
        while let Some(node_id) = cur {
            *folder_bytes.entry(node_id.clone()).or_default() += bytes;
            cur = parent_by_id.get(&node_id).cloned().flatten();
        }
    }
    for node in &mut nodes {
        if node.kind != "file" {
            node.bytes = folder_bytes.get(&node.id).copied().unwrap_or(0).max(1);
        }
    }

    // Risk join. `top` must cover every class — the default 20 would give a
    // city with only 20 coloured towers (#66). Err (no git) → empty join.
    // Ok-but-empty (no parsed classes) also reads as "no risk data": nothing
    // would be coloured, so the legend would only mislead.
    let opts = risk::Options {
        top: repo.class_count().max(1),
        ..risk::Options::default()
    };
    let cov = coverage::load(&repo.root);
    let mut risk_by_file: HashMap<String, FileRisk> = HashMap::new();
    if let Ok(scores) = risk::compute(repo, relations, cov.as_ref(), &opts) {
        for s in scores {
            let key = s.file.to_string_lossy().replace('\\', "/");
            risk_by_file
                .entry(key)
                .and_modify(|agg| {
                    agg.sloc = agg.sloc.saturating_add(s.sloc);
                    agg.churn = agg.churn.max(s.churn);
                    if s.score > agg.score {
                        agg.score = s.score;
                        agg.fqn.clone_from(&s.fqn);
                        agg.module.clone_from(&s.module);
                    }
                })
                .or_insert_with(|| FileRisk {
                    sloc: s.sloc,
                    score: s.score,
                    churn: s.churn,
                    fqn: s.fqn,
                    module: s.module,
                });
        }
    }
    let has_risk = !risk_by_file.is_empty();

    // Recency join — the "freshly built" glow. No git → no glow, no error.
    let recency_by_path: HashMap<String, u64> = git::file_recency(&repo.root)
        .map(|entries| {
            entries
                .into_iter()
                .map(|r| (r.path.to_string_lossy().replace('\\', "/"), r.secs_ago))
                .collect()
        })
        .unwrap_or_default();

    for node in &mut nodes {
        if node.kind != "file" {
            continue;
        }
        if let Some(fr) = risk_by_file.get(&node.id) {
            node.sloc = Some(fr.sloc);
            node.risk_score = Some(fr.score);
            node.churn = Some(fr.churn);
            node.fqn = Some(fr.fqn.clone());
            node.module = Some(fr.module.clone());
        }
        node.recency_secs_ago = recency_by_path.get(&node.id).copied();
    }

    CodeCityData {
        root: root.to_string_lossy().into_owned(),
        max_depth: MAX_DEPTH,
        truncated,
        has_risk,
        nodes,
    }
}

/// Forward-slashed repo-relative id — join key shared with the risk and
/// recency maps (mirrors `diagram::rel_id`).
fn rel_id(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Class, ClassKind, Module, RelationKind};
    use std::fs;
    use std::path::PathBuf;
    use std::time::SystemTime;

    /// Throwaway temp dir with drop-cleanup (same pattern as the risk e2e test).
    struct Guard(PathBuf);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmpdir(tag: &str) -> (PathBuf, Guard) {
        let dir = std::env::temp_dir().join(format!(
            "projectmind-city-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        (dir.clone(), Guard(dir))
    }

    fn node<'a>(data: &'a CodeCityData, id: &str) -> &'a CityNode {
        data.nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("node {id} missing"))
    }

    #[test]
    fn aggregates_folder_bytes_and_degrades_without_git() {
        let (dir, _guard) = tmpdir("bytes");
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("root.md"), b"12").unwrap();
        fs::write(dir.join("a/one.txt"), b"123").unwrap();
        fs::write(dir.join("a/b/two.txt"), b"12345").unwrap();

        let repo = Repository::new(dir);
        let data = build(&repo, &[]);

        // No git history → both joins degrade, has_risk goes off.
        assert!(!data.has_risk);
        assert!(!data.truncated);
        assert_eq!(data.max_depth, 5);

        // Folder bytes = subtree sums; files keep their fs sizes.
        assert_eq!(node(&data, ".").bytes, 10);
        assert_eq!(node(&data, "a").bytes, 8);
        assert_eq!(node(&data, "a/b").bytes, 5);
        assert_eq!(node(&data, "a/b/two.txt").bytes, 5);
        assert_eq!(node(&data, "root.md").bytes, 2);

        // Shape: forward-slashed ids, parent chain, kinds, depths.
        assert_eq!(node(&data, "a/b/two.txt").parent.as_deref(), Some("a/b"));
        assert_eq!(node(&data, "a").parent.as_deref(), Some("."));
        assert_eq!(node(&data, ".").kind, "root");
        assert_eq!(node(&data, "a").kind, "folder");
        assert_eq!(node(&data, "root.md").kind, "file");
        assert_eq!(node(&data, "a/b/two.txt").depth, 3);

        // Every optional join field stays None.
        assert!(data.nodes.iter().all(|n| n.sloc.is_none()
            && n.risk_score.is_none()
            && n.churn.is_none()
            && n.fqn.is_none()
            && n.module.is_none()
            && n.recency_secs_ago.is_none()));
    }

    #[test]
    fn empty_folder_gets_min_one_byte() {
        let (dir, _guard) = tmpdir("empty");
        fs::create_dir_all(dir.join("hollow")).unwrap();

        let repo = Repository::new(dir);
        let data = build(&repo, &[]);
        // Min 1 so an empty district still occupies a sliver of ground.
        assert_eq!(node(&data, "hollow").bytes, 1);
        assert_eq!(node(&data, ".").bytes, 1);
    }

    #[test]
    fn joins_risk_and_recency_per_file() {
        let (dir, _guard) = tmpdir("risk");

        // Real git repo so risk churn + file_recency succeed.
        let git = git2::Repository::init(&dir).unwrap();
        fs::write(dir.join("A.java"), "class A { void f(){ if(x){} } }\n").unwrap();
        fs::write(dir.join("plain.md"), b"# hi\n").unwrap();
        {
            let mut index = git.index().unwrap();
            index.add_path(Path::new("A.java")).unwrap();
            index.add_path(Path::new("plain.md")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = git.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("t", "t@t").unwrap();
            git.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }

        let mut repo = Repository::new(dir.clone());
        let mut m = Module {
            id: "m".into(),
            name: "m".into(),
            root: dir,
            ..Default::default()
        };
        m.classes.insert(
            "a.A".into(),
            Class {
                fqn: "a.A".into(),
                name: "A".into(),
                file: PathBuf::from("A.java"),
                line_start: 1,
                line_end: 1,
                kind: ClassKind::Class,
                ..Default::default()
            },
        );
        repo.insert_module(m);

        let relations = vec![Relation {
            from: "a.A".into(),
            to: "b.B".into(),
            kind: RelationKind::Uses,
        }];
        let data = build(&repo, &relations);

        assert!(data.has_risk);
        let a = node(&data, "A.java");
        assert_eq!(a.sloc, Some(1));
        assert_eq!(a.fqn.as_deref(), Some("a.A"));
        assert_eq!(a.module.as_deref(), Some("m"));
        assert!(a.risk_score.is_some());
        assert!(a.churn.is_some());
        // Both committed files carry recency; the freshly committed file is
        // "now"-ish (well under a day).
        assert!(a.recency_secs_ago.is_some_and(|s| s < 86_400));
        // The unparsed markdown file gets bytes + recency but no risk join.
        let md = node(&data, "plain.md");
        assert!(md.risk_score.is_none() && md.fqn.is_none());
        assert!(md.recency_secs_ago.is_some());
    }

    #[test]
    fn truncates_at_node_cap() {
        let (dir, _guard) = tmpdir("cap");
        for i in 0..(MAX_NODES + 100) {
            fs::write(dir.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let repo = Repository::new(dir);
        let data = build(&repo, &[]);
        assert!(data.truncated);
        assert!(data.nodes.len() <= MAX_NODES);
    }
}

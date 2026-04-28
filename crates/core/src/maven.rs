// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Maven multi-module detection.
//!
//! Phase 1 keeps this minimal: discover every `pom.xml` under the repo, extract its
//! `<artifactId>` (and optionally `<groupId>`), and treat each pom directory as a module root.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// A discovered Maven module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenModule {
    /// Module root (directory containing the `pom.xml`).
    pub root: PathBuf,
    /// `<artifactId>` from the pom.
    pub artifact_id: String,
    /// `<groupId>` from the pom (or inherited from parent — best effort).
    pub group_id: Option<String>,
}

impl MavenModule {
    /// Coordinate string for the module: `groupId:artifactId` or just `artifactId`.
    #[must_use]
    pub fn coordinate(&self) -> String {
        match &self.group_id {
            Some(g) => format!("{g}:{}", self.artifact_id),
            None => self.artifact_id.clone(),
        }
    }
}

/// Discover all Maven modules below `repo_root`.
///
/// Modules are returned sorted by the depth of their root, deepest first — so a file walker can
/// iterate them and attribute each Java file to the most specific containing module.
#[must_use]
pub fn discover(repo_root: &Path) -> Vec<MavenModule> {
    let mut modules = Vec::new();
    let walker = WalkBuilder::new(repo_root)
        .standard_filters(true)
        .hidden(false)
        .build();
    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) != Some("pom.xml") {
            continue;
        }
        match parse_pom(path) {
            Ok(parsed) => {
                let module_root = path
                    .parent()
                    .map_or_else(|| repo_root.to_path_buf(), Path::to_path_buf);
                debug!(?module_root, artifact = %parsed.artifact_id, "discovered Maven module");
                modules.push(MavenModule {
                    root: module_root,
                    artifact_id: parsed.artifact_id,
                    group_id: parsed.group_id,
                });
            }
            Err(err) => {
                warn!(file = %path.display(), error = %err, "could not parse pom.xml");
            }
        }
    }
    // Deepest first — used by attribute()
    modules.sort_by_key(|m| std::cmp::Reverse(m.root.components().count()));
    modules
}

/// Find the most specific module that contains `file`.
#[must_use]
pub fn attribute<'a>(modules: &'a [MavenModule], file: &Path) -> Option<&'a MavenModule> {
    modules.iter().find(|m| file.starts_with(&m.root))
}

#[derive(Debug, Default)]
struct ParsedPom {
    artifact_id: String,
    group_id: Option<String>,
}

fn parse_pom(path: &Path) -> std::io::Result<ParsedPom> {
    let bytes = std::fs::read(path)?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut path_stack: Vec<String> = Vec::new();
    let mut out = ParsedPom::default();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e
                    .name()
                    .as_ref()
                    .iter()
                    .map(|b| char::from(*b))
                    .collect::<String>();
                path_stack.push(name);
            }
            Ok(Event::End(_)) => {
                path_stack.pop();
            }
            Ok(Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default().to_string();
                let depth = path_stack.len();
                let leaf = path_stack.last().map_or("", String::as_str);
                let parent = if depth >= 2 {
                    path_stack.get(depth - 2).map_or("", String::as_str)
                } else {
                    ""
                };
                if parent == "project" {
                    match leaf {
                        "artifactId" if out.artifact_id.is_empty() => out.artifact_id = text,
                        "groupId" if out.group_id.is_none() => out.group_id = Some(text),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, err));
            }
        }
        buf.clear();
    }

    if out.artifact_id.is_empty() {
        // Fall back to the directory name so the module is at least listed.
        if let Some(parent_dir) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str())
        {
            out.artifact_id = parent_dir.to_string();
        } else {
            out.artifact_id = "unknown".to_string();
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pom(dir: &Path, artifact_id: &str, group_id: Option<&str>) {
        use std::fmt::Write as _;
        std::fs::create_dir_all(dir).unwrap();
        let mut xml = String::from(r#"<?xml version="1.0"?><project>"#);
        if let Some(g) = group_id {
            let _ = write!(xml, "<groupId>{g}</groupId>");
        }
        let _ = write!(xml, "<artifactId>{artifact_id}</artifactId>");
        xml.push_str("</project>");
        std::fs::write(dir.join("pom.xml"), xml).unwrap();
    }

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "plaintext-ide-mvn-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn discovers_single_module() {
        let root = tmpdir();
        write_pom(&root, "demo", Some("com.example"));
        let mods = discover(&root);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].artifact_id, "demo");
        assert_eq!(mods[0].group_id.as_deref(), Some("com.example"));
        assert_eq!(mods[0].coordinate(), "com.example:demo");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discovers_multi_module_and_attributes_files() {
        let root = tmpdir();
        write_pom(&root, "parent", Some("com.example"));
        write_pom(&root.join("module-a"), "module-a", None);
        write_pom(&root.join("module-b"), "module-b", None);
        let mods = discover(&root);
        assert_eq!(mods.len(), 3);
        // Deepest two first, parent last (order between siblings is not guaranteed).
        let depth0_ids: Vec<&str> = mods[..2].iter().map(|m| m.artifact_id.as_str()).collect();
        assert!(depth0_ids.contains(&"module-a"));
        assert!(depth0_ids.contains(&"module-b"));
        assert_eq!(mods[2].artifact_id, "parent");

        let attributed = attribute(&mods, &root.join("module-a/src/Main.java")).unwrap();
        assert_eq!(attributed.artifact_id, "module-a");

        let attributed = attribute(&mods, &root.join("README.md")).unwrap();
        assert_eq!(attributed.artifact_id, "parent");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn falls_back_to_directory_name_when_artifact_id_missing() {
        let root = tmpdir();
        let dir = root.join("weird");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pom.xml"), "<project></project>").unwrap();
        let mods = discover(&root);
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].artifact_id, "weird");
        std::fs::remove_dir_all(&root).ok();
    }
}

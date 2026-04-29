// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tree-sitter Java parser.

use std::path::Path;

use projectmind_plugin_api::{
    Annotation, Class, ClassKind, Field, Method, Module, Result, Visibility,
};
use tree_sitter::{Node, Parser, Tree};

/// Parse a single `.java` file and append its classes to `module`.
pub(crate) fn parse(file: &Path, source: &str, module: &mut Module) -> Result<()> {
    let tree = build_tree(source)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let package = find_package(root, bytes);

    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if let Some(class) = build_class(child, bytes, package.as_deref(), file, module) {
            module.classes.insert(class.fqn.clone(), class);
        }
    }
    Ok(())
}

fn build_tree(source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::language())
        .map_err(|e| projectmind_plugin_api::Error::Parse(e.to_string()))?;
    parser
        .parse(source, None)
        .ok_or_else(|| projectmind_plugin_api::Error::Parse("tree-sitter returned None".into()))
}

fn find_package(root: Node<'_>, bytes: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "package_declaration" {
            // package_declaration -> scoped_identifier | identifier
            let mut sub = child.walk();
            for sub_child in child.named_children(&mut sub) {
                let kind = sub_child.kind();
                if kind == "scoped_identifier" || kind == "identifier" {
                    return Some(node_text(sub_child, bytes).to_string());
                }
            }
        }
    }
    None
}

fn build_class(
    node: Node<'_>,
    bytes: &[u8],
    package: Option<&str>,
    file: &Path,
    module: &Module,
) -> Option<Class> {
    let kind = match node.kind() {
        "class_declaration" => ClassKind::Class,
        "interface_declaration" => ClassKind::Interface,
        "enum_declaration" => ClassKind::Enum,
        "record_declaration" => ClassKind::Record,
        "annotation_type_declaration" => ClassKind::Annotation,
        _ => return None,
    };

    let name = field_text(node, "name", bytes)?;
    let fqn = match package {
        Some(p) => format!("{p}.{name}"),
        None => name.to_owned(),
    };

    let modifiers = collect_modifiers(node, bytes);

    let mut class = Class {
        fqn,
        name: name.to_string(),
        file: relative(file, &module.root),
        line_start: line_of(node.start_position().row),
        line_end: line_of(node.end_position().row),
        kind,
        visibility: visibility_from(&modifiers),
        annotations: collect_annotations(node, bytes),
        methods: Vec::new(),
        fields: Vec::new(),
        stereotypes: Vec::new(),
        extras: std::collections::BTreeMap::default(),
    };

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            match member.kind() {
                "method_declaration" | "constructor_declaration" => {
                    if let Some(m) = build_method(member, bytes) {
                        class.methods.push(m);
                    }
                }
                "field_declaration" => {
                    class.fields.extend(build_fields(member, bytes));
                }
                _ => {}
            }
        }
    }

    Some(class)
}

fn build_method(node: Node<'_>, bytes: &[u8]) -> Option<Method> {
    let name = field_text(node, "name", bytes)
        .unwrap_or("<init>")
        .to_string();
    let modifiers = collect_modifiers(node, bytes);
    Some(Method {
        name,
        line_start: line_of(node.start_position().row),
        line_end: line_of(node.end_position().row),
        visibility: visibility_from(&modifiers),
        annotations: collect_annotations(node, bytes),
        is_static: modifiers.iter().any(|m| m == "static"),
    })
}

fn build_fields(node: Node<'_>, bytes: &[u8]) -> Vec<Field> {
    let mut out = Vec::new();
    let modifiers = collect_modifiers(node, bytes);
    let type_text = field_text(node, "type", bytes)
        .unwrap_or_default()
        .to_string();
    let line = line_of(node.start_position().row);
    let annotations = collect_annotations(node, bytes);

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            if let Some(name) = field_text(child, "name", bytes) {
                out.push(Field {
                    name: name.to_string(),
                    type_text: type_text.clone(),
                    line,
                    visibility: visibility_from(&modifiers),
                    annotations: annotations.clone(),
                    is_static: modifiers.iter().any(|m| m == "static"),
                });
            }
        }
    }
    out
}

fn collect_annotations(node: Node<'_>, bytes: &[u8]) -> Vec<Annotation> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if let Some(modifiers) = node.children(&mut cursor).find(|c| c.kind() == "modifiers") {
        let mut sub = modifiers.walk();
        for child in modifiers.children(&mut sub) {
            match child.kind() {
                "annotation" | "marker_annotation" => {
                    let name_node = child
                        .child_by_field_name("name")
                        .or_else(|| child.named_child(0));
                    if let Some(n) = name_node {
                        let raw = node_text(n, bytes).to_string();
                        let simple = raw.rsplit('.').next().unwrap_or(&raw).to_string();
                        let raw_args = child
                            .child_by_field_name("arguments")
                            .map(|a| node_text(a, bytes).to_string());
                        out.push(Annotation {
                            name: simple,
                            fqn: Some(raw),
                            raw_args,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn collect_modifiers(node: Node<'_>, bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if let Some(modifiers) = node.children(&mut cursor).find(|c| c.kind() == "modifiers") {
        let mut sub = modifiers.walk();
        for child in modifiers.children(&mut sub) {
            // Skip annotations here — they're collected separately.
            if child.kind() == "annotation" || child.kind() == "marker_annotation" {
                continue;
            }
            out.push(node_text(child, bytes).to_string());
        }
    }
    out
}

fn visibility_from(modifiers: &[String]) -> Visibility {
    if modifiers.iter().any(|m| m == "public") {
        Visibility::Public
    } else if modifiers.iter().any(|m| m == "protected") {
        Visibility::Protected
    } else if modifiers.iter().any(|m| m == "private") {
        Visibility::Private
    } else {
        Visibility::PackagePrivate
    }
}

fn field_text<'a>(node: Node<'_>, field: &str, bytes: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name(field).map(|n| node_text(n, bytes))
}

fn node_text<'a>(node: Node<'_>, bytes: &'a [u8]) -> &'a str {
    std::str::from_utf8(&bytes[node.byte_range()]).unwrap_or("")
}

fn line_of(row: usize) -> u32 {
    u32::try_from(row + 1).unwrap_or(u32::MAX)
}

fn relative(file: &Path, root: &Path) -> std::path::PathBuf {
    file.strip_prefix(root)
        .map_or_else(|_| file.to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_one(src: &str) -> Module {
        let mut module = Module {
            id: "test".into(),
            name: "test".into(),
            root: PathBuf::from("/"),
            ..Default::default()
        };
        parse(Path::new("/Test.java"), src, &mut module).expect("parse");
        module
    }

    #[test]
    fn extracts_simple_class() {
        let src = r"
            package com.example;
            public class Hello {
                private int field;
                public String greet(String name) { return name; }
            }
        ";
        let m = parse_one(src);
        let class = m.classes.get("com.example.Hello").expect("class");
        assert_eq!(class.name, "Hello");
        assert_eq!(class.kind, ClassKind::Class);
        assert_eq!(class.visibility, Visibility::Public);
        assert_eq!(class.fields.len(), 1);
        assert_eq!(class.fields[0].name, "field");
        assert_eq!(class.methods.len(), 1);
        assert_eq!(class.methods[0].name, "greet");
        assert_eq!(class.methods[0].visibility, Visibility::Public);
    }

    #[test]
    fn extracts_interface_and_record() {
        let src = r"
            package com.example;
            public interface Repo {}
            public record Coord(int x, int y) {}
        ";
        let m = parse_one(src);
        assert_eq!(
            m.classes.get("com.example.Repo").unwrap().kind,
            ClassKind::Interface
        );
        assert_eq!(
            m.classes.get("com.example.Coord").unwrap().kind,
            ClassKind::Record
        );
    }

    #[test]
    fn extracts_annotations() {
        let src = r"
            package com.example;
            @Service
            public class UserService {
                @Autowired private UserRepo repo;
                @Transactional
                public void doIt() {}
            }
        ";
        let m = parse_one(src);
        let c = m.classes.get("com.example.UserService").unwrap();
        assert!(c.annotations.iter().any(|a| a.is("Service")));
        assert!(c.fields[0].annotations.iter().any(|a| a.is("Autowired")));
        assert!(c.methods[0]
            .annotations
            .iter()
            .any(|a| a.is("Transactional")));
    }

    #[test]
    fn handles_default_package() {
        let src = r"public class Top {}";
        let m = parse_one(src);
        assert!(m.classes.contains_key("Top"));
    }
}

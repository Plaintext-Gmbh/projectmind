// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Tree-sitter Rust parser.
//!
//! Walks the AST in two passes:
//! - **Pass 1** registers `struct_item`, `enum_item`, `trait_item`, and `union_item`
//!   declarations as [`Class`]es indexed by their FQN.
//! - **Pass 2** walks `impl_item` blocks and attaches their methods (free `function_item`
//!   children of the impl body) to the matching class. Trait impls (`impl T for S`)
//!   contribute their methods *and* a `t::Trait` annotation on the implementing class
//!   so that the class browser surfaces "this struct implements that trait" without
//!   needing a separate framework plugin.
//!
//! Attribute macros — `#[derive(Debug, Clone)]`, `#[serde(skip)]`, `#[tokio::main]` and
//! friends — are lifted to [`Annotation`]s. `#[derive(A, B, …)]` is exploded into one
//! annotation per derived trait so that downstream consumers can match them by simple
//! name (the same way Java framework plugins match `@Service`).
//!
//! [`Class`]: projectmind_plugin_api::Class
//! [`Annotation`]: projectmind_plugin_api::Annotation

use std::path::{Path, PathBuf};

use projectmind_plugin_api::{
    Annotation, Class, ClassKind, Field, Method, Module, Result, Visibility,
};
use tree_sitter::{Node, Parser, Tree};

/// Parse a single `.rs` file and append its items to `module`.
pub(crate) fn parse(file: &Path, source: &str, module: &mut Module) -> Result<()> {
    let tree = build_tree(source)?;
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let namespace = derive_namespace(file, &module.root);
    let rel_file = relative(file, &module.root);

    // Pass 1: collect classes (struct/enum/trait/union) keyed by simple name.
    let mut by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if let Some(class) = build_class(child, bytes, namespace.as_deref(), &rel_file) {
            by_name.insert(class.name.clone(), class.fqn.clone());
            module.classes.insert(class.fqn.clone(), class);
        }
    }

    // Pass 2: attach impl-block methods + trait stereotype to the matching class.
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if child.kind() == "impl_item" {
            apply_impl(child, bytes, &by_name, module);
        }
    }

    Ok(())
}

fn build_tree(source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::language())
        .map_err(|e| projectmind_plugin_api::Error::Parse(e.to_string()))?;
    parser
        .parse(source, None)
        .ok_or_else(|| projectmind_plugin_api::Error::Parse("tree-sitter returned None".into()))
}

fn build_class(
    node: Node<'_>,
    bytes: &[u8],
    namespace: Option<&str>,
    rel_file: &Path,
) -> Option<Class> {
    let kind = match node.kind() {
        "struct_item" | "union_item" => ClassKind::Class,
        "enum_item" => ClassKind::Enum,
        "trait_item" => ClassKind::Interface,
        _ => return None,
    };

    let name = field_text(node, "name", bytes)?.to_string();
    let fqn = match namespace {
        Some(ns) if !ns.is_empty() => format!("{ns}::{name}"),
        _ => name.clone(),
    };

    let visibility = item_visibility(node, bytes);
    let annotations = collect_outer_attributes(node, bytes);

    let mut class = Class {
        fqn,
        name,
        file: rel_file.to_path_buf(),
        line_start: line_of(node.start_position().row),
        line_end: line_of(node.end_position().row),
        kind,
        visibility,
        annotations,
        methods: Vec::new(),
        fields: collect_fields(node, bytes),
        stereotypes: Vec::new(),
        extras: std::collections::BTreeMap::default(),
    };

    // Trait body: `trait T { fn foo(); fn bar(&self); }` — the prototypes are methods too.
    if class.kind == ClassKind::Interface {
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for member in body.named_children(&mut cursor) {
                if member.kind() == "function_signature_item" || member.kind() == "function_item" {
                    if let Some(m) = build_method(member, bytes) {
                        class.methods.push(m);
                    }
                }
            }
        }
    }

    Some(class)
}

fn collect_fields(node: Node<'_>, bytes: &[u8]) -> Vec<Field> {
    let Some(body) = node.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if member.kind() != "field_declaration" {
            continue;
        }
        let name = match field_text(member, "name", bytes) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let type_text = field_text(member, "type", bytes)
            .unwrap_or_default()
            .to_string();
        out.push(Field {
            name,
            type_text,
            line: line_of(member.start_position().row),
            visibility: item_visibility(member, bytes),
            annotations: collect_outer_attributes(member, bytes),
            is_static: false,
        });
    }
    out
}

fn build_method(node: Node<'_>, bytes: &[u8]) -> Option<Method> {
    let name = field_text(node, "name", bytes)?.to_string();
    let visibility = item_visibility(node, bytes);
    let annotations = collect_outer_attributes(node, bytes);
    Some(Method {
        name,
        line_start: line_of(node.start_position().row),
        line_end: line_of(node.end_position().row),
        visibility,
        annotations,
        // Rust associated functions (no `self` param) are conceptually static.
        is_static: !has_self_parameter(node),
    })
}

fn has_self_parameter(fn_node: Node<'_>) -> bool {
    let Some(params) = fn_node.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = params.walk();
    let found = params
        .named_children(&mut cursor)
        .any(|c| c.kind() == "self_parameter");
    found
}

fn apply_impl(
    impl_node: Node<'_>,
    bytes: &[u8],
    by_name: &std::collections::HashMap<String, String>,
    module: &mut Module,
) {
    let Some(type_node) = impl_node.child_by_field_name("type") else {
        return;
    };
    let type_simple = simple_type_name(type_node, bytes);
    let Some(fqn) = by_name.get(&type_simple) else {
        return;
    };
    let Some(class) = module.classes.get_mut(fqn) else {
        return;
    };

    // `impl Trait for Type` — record the trait as an annotation on the type.
    if let Some(trait_node) = impl_node.child_by_field_name("trait") {
        let trait_simple = simple_type_name(trait_node, bytes);
        let raw = node_text(trait_node, bytes).to_string();
        class.annotations.push(Annotation {
            name: trait_simple,
            fqn: Some(raw),
            raw_args: None,
        });
    }

    let Some(body) = impl_node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if member.kind() == "function_item" {
            if let Some(m) = build_method(member, bytes) {
                class.methods.push(m);
            }
        }
    }
}

fn simple_type_name(type_node: Node<'_>, bytes: &[u8]) -> String {
    // Walk down through generic_type / scoped_type_identifier to find the bare identifier.
    let mut node = type_node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" => {
                return node_text(node, bytes).to_string();
            }
            "generic_type" => {
                if let Some(t) = node.child_by_field_name("type") {
                    node = t;
                    continue;
                }
            }
            "scoped_type_identifier" | "scoped_identifier" => {
                if let Some(t) = node.child_by_field_name("name") {
                    return node_text(t, bytes).to_string();
                }
            }
            _ => {}
        }
        // Fallback: return the whole text.
        return node_text(node, bytes).to_string();
    }
}

fn collect_outer_attributes(node: Node<'_>, bytes: &[u8]) -> Vec<Annotation> {
    // Outer attributes precede the item as siblings under the same parent in tree-sitter-rust;
    // they are NOT children of the item itself. So we walk backwards from the item node.
    let Some(parent) = node.parent() else {
        return Vec::new();
    };

    // Find this node's index among the parent's children.
    let mut cursor = parent.walk();
    let children: Vec<Node<'_>> = parent.children(&mut cursor).collect();
    let Some(idx) = children.iter().position(|c| c.id() == node.id()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    // Walk backwards collecting consecutive attribute_item siblings.
    for i in (0..idx).rev() {
        let sib = children[i];
        match sib.kind() {
            "attribute_item" => {
                expand_attribute(sib, bytes, &mut out);
            }
            "line_comment" | "block_comment" | "inner_attribute_item" => {}
            _ => break,
        }
    }
    // We collected in reverse order (closest attribute first); flip so source order is preserved.
    out.reverse();
    out
}

fn expand_attribute(attr_item: Node<'_>, bytes: &[u8], out: &mut Vec<Annotation>) {
    // tree-sitter-rust shape: `attribute_item` → `attribute` whose children are an
    // unnamed-field `identifier`/`scoped_identifier` (the path) followed optionally by a
    // `token_tree` (the arguments). The grammar exposes no `path` / `arguments` field
    // names here, so walk children by kind instead.
    let Some(attr) = attr_item.named_child(0).filter(|n| n.kind() == "attribute") else {
        return;
    };

    let mut path_node: Option<Node<'_>> = None;
    let mut args: Option<Node<'_>> = None;
    let mut cursor = attr.walk();
    for c in attr.named_children(&mut cursor) {
        match c.kind() {
            "identifier" | "scoped_identifier" if path_node.is_none() => path_node = Some(c),
            "token_tree" if args.is_none() => args = Some(c),
            _ => {}
        }
    }

    let path_text = path_node
        .map(|n| node_text(n, bytes).to_string())
        .unwrap_or_default();
    let simple = path_text
        .rsplit("::")
        .next()
        .unwrap_or(&path_text)
        .to_string();

    let raw_args = args.map(|n| node_text(n, bytes).to_string());

    // `#[derive(A, B, C)]` -> emit one annotation per trait, keeping the derive itself implicit.
    if simple == "derive" {
        if let Some(args_node) = args {
            for ident in extract_identifiers(args_node, bytes) {
                out.push(Annotation {
                    name: ident.clone(),
                    fqn: Some(format!("derive::{ident}")),
                    raw_args: None,
                });
            }
            return;
        }
    }

    out.push(Annotation {
        name: simple,
        fqn: Some(path_text),
        raw_args,
    });
}

fn extract_identifiers(node: Node<'_>, bytes: &[u8]) -> Vec<String> {
    // tree-sitter-rust represents attribute arguments as a `token_tree` whose contents
    // are mostly anonymous tokens (identifiers, commas, parens), so walking only
    // `named_children` would miss the trait names. Walk *all* children, and accept
    // anonymous identifier-shaped tokens by their text content.
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let kind = n.kind();
        let txt = node_text(n, bytes);
        if matches!(kind, "identifier" | "type_identifier")
            || (n.named_child_count() == 0
                && !txt.is_empty()
                && txt
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                && txt
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ':'))
        {
            // Skip the path-component punctuation `::` itself.
            let final_segment = txt.rsplit("::").next().unwrap_or(txt);
            if !final_segment.is_empty() {
                out.push(final_segment.to_string());
            }
            continue;
        }
        let mut cursor = n.walk();
        for c in n.children(&mut cursor) {
            stack.push(c);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn item_visibility(node: Node<'_>, bytes: &[u8]) -> Visibility {
    // visibility_modifier is a named child of struct/enum/trait/fn/field items.
    let mut cursor = node.walk();
    for c in node.children(&mut cursor) {
        if c.kind() == "visibility_modifier" {
            let txt = node_text(c, bytes);
            if txt.starts_with("pub") {
                // pub | pub(crate) | pub(super) | pub(in path) — for Phase 1 collapse
                // anything more restrictive than bare `pub` to package-private since the
                // domain model has no native concept for crate-restricted visibility.
                return if txt == "pub" {
                    Visibility::Public
                } else {
                    Visibility::PackagePrivate
                };
            }
        }
    }
    Visibility::Private
}

fn derive_namespace(file: &Path, module_root: &Path) -> Option<String> {
    let stem = file.file_stem().and_then(|s| s.to_str())?;
    let parent_name = file
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .filter(|s| !["src", "tests", "benches", "examples"].contains(s));

    // `mod.rs` / `lib.rs` / `main.rs` collapse onto the directory name above them. When
    // the directory is the conventional `src` (filtered to None), we drop the head — the
    // crate name alone is the right namespace, since `lib.rs` *is* the crate root.
    let head: Option<String> = match stem {
        "mod" | "lib" | "main" => parent_name.map(str::to_string),
        other => Some(other.to_string()),
    };

    let crate_name = find_crate_name(file, module_root);
    match (crate_name, head) {
        (Some(cr), Some(h)) if cr != h => Some(format!("{cr}::{h}")),
        (Some(cr), _) => Some(cr),
        (None, Some(h)) => Some(h),
        (None, None) => None,
    }
}

fn find_crate_name(file: &Path, module_root: &Path) -> Option<String> {
    let mut dir = file.parent()?;
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(text) = std::fs::read_to_string(&cargo) {
                // Simple textual scrape — avoids pulling in a TOML parser for one field.
                for line in text.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("name") {
                        let after = rest.trim_start();
                        if let Some(after_eq) = after.strip_prefix('=') {
                            let val = after_eq.trim().trim_matches('"').trim_matches('\'');
                            if !val.is_empty() {
                                return Some(val.to_string());
                            }
                        }
                    }
                }
            }
            // Cargo.toml without a name field (workspace root) — keep walking up.
        }
        if dir == module_root {
            return None;
        }
        dir = dir.parent()?;
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

fn relative(file: &Path, root: &Path) -> PathBuf {
    file.strip_prefix(root)
        .map_or_else(|_| file.to_path_buf(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_one(src: &str) -> Module {
        parse_at(src, "/repo/src/lib.rs", "/repo")
    }

    fn parse_at(src: &str, file: &str, root: &str) -> Module {
        let mut module = Module {
            id: "test".into(),
            name: "test".into(),
            root: PathBuf::from(root),
            ..Default::default()
        };
        parse(Path::new(file), src, &mut module).expect("parse");
        module
    }

    #[test]
    fn extracts_struct_with_field_and_impl_methods() {
        let src = r"
            pub struct Counter {
                pub value: i32,
                step: u32,
            }

            impl Counter {
                pub fn new(initial: i32) -> Self { Self { value: initial, step: 1 } }
                pub fn tick(&mut self) { self.value += self.step as i32; }
            }
        ";
        let m = parse_one(src);
        let class = m
            .classes
            .values()
            .find(|c| c.name == "Counter")
            .expect("Counter");
        assert_eq!(class.kind, ClassKind::Class);
        assert_eq!(class.visibility, Visibility::Public);
        assert_eq!(class.fields.len(), 2);
        assert_eq!(class.fields[0].name, "value");
        assert_eq!(class.fields[0].visibility, Visibility::Public);
        assert_eq!(class.fields[1].visibility, Visibility::Private);
        let method_names: Vec<_> = class.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(method_names.contains(&"new"));
        assert!(method_names.contains(&"tick"));
        let new_method = class.methods.iter().find(|m| m.name == "new").unwrap();
        assert!(new_method.is_static, "associated fn `new` should be static");
        let tick = class.methods.iter().find(|m| m.name == "tick").unwrap();
        assert!(
            !tick.is_static,
            "fn taking `&mut self` should not be static"
        );
    }

    #[test]
    fn enum_and_trait_kinds() {
        let src = r"
            pub enum Color { Red, Green, Blue }
            pub trait Greet { fn hello(&self); }
        ";
        let m = parse_one(src);
        let color = m.classes.values().find(|c| c.name == "Color").unwrap();
        assert_eq!(color.kind, ClassKind::Enum);
        let greet = m.classes.values().find(|c| c.name == "Greet").unwrap();
        assert_eq!(greet.kind, ClassKind::Interface);
        // Trait body methods become Method entries.
        assert_eq!(greet.methods.len(), 1);
        assert_eq!(greet.methods[0].name, "hello");
    }

    #[test]
    fn derive_explodes_to_individual_annotations() {
        let src = r"
            #[derive(Debug, Clone, PartialEq)]
            pub struct Coord { pub x: i32, pub y: i32 }
        ";
        let m = parse_one(src);
        let coord = m.classes.values().find(|c| c.name == "Coord").unwrap();
        let names: Vec<_> = coord.annotations.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"Debug"));
        assert!(names.contains(&"Clone"));
        assert!(names.contains(&"PartialEq"));
    }

    #[test]
    fn impl_trait_for_type_lifts_trait_to_annotation() {
        let src = r"
            pub struct Foo;
            pub trait Speak { fn say(&self); }
            impl Speak for Foo { fn say(&self) {} }
        ";
        let m = parse_one(src);
        let foo = m.classes.values().find(|c| c.name == "Foo").unwrap();
        assert!(foo.annotations.iter().any(|a| a.name == "Speak"));
        assert!(foo.methods.iter().any(|m| m.name == "say"));
    }

    #[test]
    fn namespace_is_crate_plus_file_stem() {
        // Pretend there's a Cargo.toml at /repo/Cargo.toml with name = "myapp".
        let dir =
            std::env::temp_dir().join(format!("projectmind-lang-rust-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let file = dir.join("src/widget.rs");
        std::fs::write(&file, "pub struct Widget;").unwrap();

        let mut module = Module {
            id: "test".into(),
            name: "test".into(),
            root: dir.clone(),
            ..Default::default()
        };
        parse(&file, "pub struct Widget;", &mut module).unwrap();
        assert!(
            module.classes.contains_key("myapp::widget::Widget"),
            "got keys: {:?}",
            module.classes.keys().collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn private_struct_defaults_to_private_visibility() {
        let src = "struct Hidden { value: i32 }";
        let m = parse_one(src);
        let hidden = m.classes.values().find(|c| c.name == "Hidden").unwrap();
        assert_eq!(hidden.visibility, Visibility::Private);
    }
}

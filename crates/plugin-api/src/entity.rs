// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Domain entities describing source code.
//!
//! These types are language-agnostic: a Java class, a Kotlin class and a TypeScript class all
//! become a [`Class`]. Language-specific details that matter for visualisation are kept in
//! [`Class::stereotypes`] and [`Class::extras`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A parsed module of source code (a Maven module, an npm package, …).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Module {
    /// Stable identifier (typically the module's coordinate or directory name).
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Absolute root directory of the module.
    pub root: PathBuf,

    /// Classes parsed from this module, indexed by fully-qualified name.
    pub classes: BTreeMap<String, Class>,
}

/// A class, interface, enum, or record.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Class {
    /// Fully-qualified name (e.g. `com.example.UserService`).
    pub fqn: String,

    /// Simple name (e.g. `UserService`).
    pub name: String,

    /// Source file (relative to the module root) and 1-based line range.
    pub file: PathBuf,
    /// Inclusive start line, 1-based.
    pub line_start: u32,
    /// Inclusive end line, 1-based.
    pub line_end: u32,

    /// `class`, `interface`, `enum`, `record`, …
    pub kind: ClassKind,

    /// Visibility modifier.
    pub visibility: Visibility,

    /// Annotations declared on the class.
    pub annotations: Vec<Annotation>,

    /// Methods declared on the class.
    pub methods: Vec<Method>,

    /// Fields declared on the class.
    pub fields: Vec<Field>,

    /// Stereotypes attached by framework plugins (e.g. `service`, `controller`, `dto`).
    pub stereotypes: Vec<String>,

    /// Super types declared on this class — `extends` targets (typically
    /// just one in Java; none on Rust types) and `implements` / Rust trait-
    /// `impl` targets. Names are kept as written in source: language plugins
    /// don't try to resolve to fully-qualified names since imports may live
    /// in other files. Consumers that need FQN resolution can do their own
    /// pass against the parsed [`Module`].
    #[serde(default)]
    pub super_types: Vec<TypeRef>,

    /// Free-form metadata that framework plugins may attach.
    #[serde(default)]
    pub extras: BTreeMap<String, serde_json::Value>,
}

/// A reference to another type, declared as a parent or implemented interface.
/// Plain simple-or-qualified name as written in source — no FQN resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeRef {
    /// Type name as written in source (`AbstractEntity`, `java.io.Serializable`, `Display`).
    pub name: String,
    /// Whether this is an `extends` target or an `implements` / trait-impl target.
    pub kind: TypeRefKind,
}

/// How a [`TypeRef`] relates to its bearing class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeRefKind {
    /// Java `extends`. The bearing class inherits from this type.
    #[default]
    Extends,
    /// Java `implements` or Rust `impl Trait for T` — the bearing class
    /// satisfies this interface / trait without inheriting from it.
    Implements,
}

/// Kind of class-like entity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassKind {
    /// A Java/Kotlin/etc. class.
    #[default]
    Class,
    /// An interface.
    Interface,
    /// An enum.
    Enum,
    /// A record / data class.
    Record,
    /// An annotation type.
    Annotation,
}

/// Visibility / access modifier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Public visibility.
    Public,
    /// Protected visibility.
    Protected,
    /// Package-private (default in Java).
    #[default]
    PackagePrivate,
    /// Private visibility.
    Private,
}

/// A method, function, or constructor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Method {
    /// Method name.
    pub name: String,
    /// 1-based line start.
    pub line_start: u32,
    /// 1-based line end.
    pub line_end: u32,
    /// Visibility.
    pub visibility: Visibility,
    /// Annotations declared on the method.
    pub annotations: Vec<Annotation>,
    /// Whether the method is static.
    pub is_static: bool,
}

/// A field on a class.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Field {
    /// Field name.
    pub name: String,
    /// Type as written in source (best effort, language-specific).
    pub type_text: String,
    /// 1-based line.
    pub line: u32,
    /// Visibility.
    pub visibility: Visibility,
    /// Annotations declared on the field.
    pub annotations: Vec<Annotation>,
    /// Whether the field is static.
    pub is_static: bool,
}

/// An annotation as written in source (`@Service`, `@Bean(name = "x")`, …).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Annotation {
    /// Simple name without the `@` (`Service`, `Bean`, …).
    pub name: String,
    /// Optional fully-qualified name when known.
    pub fqn: Option<String>,
    /// Raw argument text inside the parentheses, if present.
    pub raw_args: Option<String>,
}

impl Annotation {
    /// Returns true if the annotation matches the given simple name (case-sensitive).
    #[must_use]
    pub fn is(&self, name: &str) -> bool {
        self.name == name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_matches_by_simple_name() {
        let a = Annotation {
            name: "Service".into(),
            fqn: None,
            raw_args: None,
        };
        assert!(a.is("Service"));
        assert!(!a.is("Controller"));
    }

    #[test]
    fn class_defaults_are_empty() {
        let c = Class::default();
        assert!(c.fqn.is_empty());
        assert!(c.methods.is_empty());
        assert_eq!(c.kind, ClassKind::Class);
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pattern Lens — architectural-pattern compliance detectors.
//!
//! Each detector returns a [`PatternResult`] describing how often the rule
//! holds versus how often it drifts. The detectors read from the already-
//! parsed [`Repository`] (classes, annotations, fields), so they're cheap
//! to run repeatedly — no source-text scanning beyond what the language
//! plugins already did.
//!
//! v1 detectors (#159):
//! - [`Pattern::NoStaticState`] — Spring components must not own non-final
//!   `static` fields (mutable shared state breaks lifecycle assumptions).
//! - [`Pattern::TxOnService`] — `@Transactional` must live on `@Service`,
//!   not on `@Controller` / `@RestController`.
//! - [`Pattern::Layered`] — classes whose FQN contains `.controller.` or
//!   `.web.` must not depend (via field types or super-types) on FQNs
//!   matching `.repository.` or `.entity.` directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use projectmind_plugin_api::Class;
use serde::{Deserialize, Serialize};

use crate::repository::Repository;

/// Patterns understood by [`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// No non-final `static` fields on Spring `@Component`-typed classes.
    NoStaticState,
    /// `@Transactional` only on `@Service` (not on controllers).
    TxOnService,
    /// Controllers / web classes must not reference repositories / entities directly.
    Layered,
}

impl Pattern {
    /// Parse a snake-cased pattern name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "no_static_state" | "nostaticstate" | "NoStaticState" => Some(Self::NoStaticState),
            "tx_on_service" | "txonservice" | "TxOnService" => Some(Self::TxOnService),
            "layered" | "Layered" => Some(Self::Layered),
            _ => None,
        }
    }

    /// Stable snake-case name for serialisation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoStaticState => "no_static_state",
            Self::TxOnService => "tx_on_service",
            Self::Layered => "layered",
        }
    }
}

/// One module's compliance count: classes that fully satisfy the rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleHold {
    /// Module id.
    pub module: String,
    /// Classes inside this module that satisfy the rule.
    pub count: u32,
}

/// One concrete violation of a pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Module id where the violation lives.
    pub module: String,
    /// Repo-relative file path (best effort — falls back to module-relative).
    pub file: PathBuf,
    /// 1-based line. `0` when the offending element has no line attached.
    pub line: u32,
    /// FQN of the offending class.
    pub fqn: String,
    /// Human-readable summary of the violation.
    pub message: String,
    /// Severity, 1=info, 2=warn, 3=critical. Reserved for future use.
    pub severity: u8,
}

/// Aggregate result returned by [`check`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternResult {
    /// Pattern that was checked.
    pub pattern: Pattern,
    /// Per-module count of classes that satisfy the rule.
    pub holds: Vec<ModuleHold>,
    /// All violations found, grouped per module in `holds` order.
    pub violations: Vec<Violation>,
    /// Confidence in the detector (0.0..=1.0). v1 detectors are 0.85.
    pub confidence: f64,
}

/// Optional scope passed to [`check`].
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Filter to a single module id when set.
    pub module: Option<String>,
}

/// Run a pattern check against `repo`.
#[must_use]
pub fn check(repo: &Repository, pattern: Pattern, scope: &Scope) -> PatternResult {
    match pattern {
        Pattern::NoStaticState => check_no_static_state(repo, scope),
        Pattern::TxOnService => check_tx_on_service(repo, scope),
        Pattern::Layered => check_layered(repo, scope),
    }
}

fn each_class<'a>(
    repo: &'a Repository,
    scope: &'a Scope,
) -> impl Iterator<Item = (&'a str, &'a Path, &'a Class)> + 'a {
    repo.modules
        .values()
        .filter(move |m| scope.module.as_deref().map_or(true, |f| f == m.id))
        .flat_map(|m| {
            m.classes
                .values()
                .map(move |c| (m.id.as_str(), m.root.as_path(), c))
        })
}

fn rel_file(_module_root: &Path, class_file: &Path) -> PathBuf {
    class_file.to_path_buf()
}

fn check_no_static_state(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        if !is_spring_component(class) {
            continue;
        }
        let bad_fields: Vec<&projectmind_plugin_api::Field> = class
            .fields
            .iter()
            .filter(|f| f.is_static && !is_final_field(f))
            .collect();

        if bad_fields.is_empty() {
            *holds.entry(module_id.to_string()).or_default() += 1;
        } else {
            for f in bad_fields {
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line: f.line,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "Spring component `{}` owns mutable static field `{}` — replace with bean state or a singleton",
                        class.name, f.name
                    ),
                    severity: 2,
                });
            }
        }
    }

    PatternResult {
        pattern: Pattern::NoStaticState,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.85,
    }
}

fn check_tx_on_service(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        let on_class = class.annotations.iter().any(|a| a.is("Transactional"));
        let methods_with_tx: Vec<&projectmind_plugin_api::Method> = class
            .methods
            .iter()
            .filter(|m| m.annotations.iter().any(|a| a.is("Transactional")))
            .collect();

        let has_tx_anywhere = on_class || !methods_with_tx.is_empty();
        if !has_tx_anywhere {
            continue;
        }

        if is_spring_service_or_repo(class) {
            *holds.entry(module_id.to_string()).or_default() += 1;
            continue;
        }

        if is_spring_controller(class) {
            let line = if on_class {
                class.line_start
            } else {
                methods_with_tx
                    .first()
                    .map_or(class.line_start, |m| m.line_start)
            };
            violations.push(Violation {
                module: module_id.to_string(),
                file: rel_file(module_root, &class.file),
                line,
                fqn: class.fqn.clone(),
                message: format!(
                    "Controller `{}` carries @Transactional — move boundary to @Service",
                    class.name
                ),
                severity: 3,
            });
        }
    }

    PatternResult {
        pattern: Pattern::TxOnService,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.85,
    }
}

fn check_layered(repo: &Repository, scope: &Scope) -> PatternResult {
    let mut holds: BTreeMap<String, u32> = BTreeMap::new();
    let mut violations: Vec<Violation> = Vec::new();

    for (module_id, module_root, class) in each_class(repo, scope) {
        if !is_web_layer(&class.fqn) {
            continue;
        }
        let mut bad_refs: Vec<String> = Vec::new();
        for f in &class.fields {
            if is_persistence_ref(&f.type_text) {
                bad_refs.push(format!("field `{}: {}`", f.name, f.type_text));
            }
        }
        for s in &class.super_types {
            if is_persistence_ref(&s.name) {
                bad_refs.push(format!("super-type `{}`", s.name));
            }
        }

        if bad_refs.is_empty() {
            *holds.entry(module_id.to_string()).or_default() += 1;
        } else {
            for r in bad_refs {
                violations.push(Violation {
                    module: module_id.to_string(),
                    file: rel_file(module_root, &class.file),
                    line: class.line_start,
                    fqn: class.fqn.clone(),
                    message: format!(
                        "Web class `{}` reaches into persistence layer ({r}) — go through a service",
                        class.name
                    ),
                    severity: 2,
                });
            }
        }
    }

    PatternResult {
        pattern: Pattern::Layered,
        holds: holds_to_vec(holds),
        violations,
        confidence: 0.8,
    }
}

fn holds_to_vec(holds: BTreeMap<String, u32>) -> Vec<ModuleHold> {
    holds
        .into_iter()
        .map(|(module, count)| ModuleHold { module, count })
        .collect()
}

fn is_spring_component(class: &Class) -> bool {
    const TAGS: &[&str] = &[
        "Component",
        "Service",
        "Repository",
        "Controller",
        "RestController",
        "Configuration",
    ];
    class
        .annotations
        .iter()
        .any(|a| TAGS.iter().any(|t| a.is(t)))
        || class.stereotypes.iter().any(|s| {
            matches!(
                s.as_str(),
                "component" | "service" | "repository" | "controller" | "configuration"
            )
        })
}

fn is_spring_service_or_repo(class: &Class) -> bool {
    class
        .annotations
        .iter()
        .any(|a| a.is("Service") || a.is("Repository"))
        || class
            .stereotypes
            .iter()
            .any(|s| s == "service" || s == "repository")
}

fn is_spring_controller(class: &Class) -> bool {
    class
        .annotations
        .iter()
        .any(|a| a.is("Controller") || a.is("RestController"))
        || class.stereotypes.iter().any(|s| s == "controller")
}

fn is_final_field(field: &projectmind_plugin_api::Field) -> bool {
    // Primary signal: the parser extracted the `final` modifier. The
    // type_text checks stay as a fallback for parsers that fold modifiers
    // into the type text; `final` itself is a keyword, never an annotation.
    field.is_final
        || field
            .annotations
            .iter()
            .any(|a| a.is("Value") || a.is("ConfigurationProperty"))
        || field.type_text.starts_with("final ")
        || field.type_text.contains(" final ")
}

fn is_web_layer(fqn: &str) -> bool {
    let lower = fqn.to_ascii_lowercase();
    lower.contains(".controller.")
        || lower.contains(".web.")
        || lower.contains(".rest.")
        || lower.contains(".api.")
}

fn is_persistence_ref(type_text: &str) -> bool {
    let lower = type_text.to_ascii_lowercase();
    lower.contains(".repository.")
        || lower.contains(".entity.")
        || lower.contains(".dao.")
        || lower.ends_with("repository")
        || lower.ends_with("entity")
        || lower.ends_with("entitymanager")
        || lower == "entitymanager"
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Annotation, Class, ClassKind, Field, Method, Module, TypeRef};

    fn mk_repo(modules: Vec<Module>) -> Repository {
        let mut r = Repository::new(PathBuf::from("/tmp/patterns"));
        for m in modules {
            r.insert_module(m);
        }
        r
    }

    fn make_module(id: &str, classes: Vec<Class>) -> Module {
        let mut m = Module {
            id: id.into(),
            name: id.into(),
            root: PathBuf::from(format!("/tmp/{id}")),
            ..Default::default()
        };
        for c in classes {
            m.classes.insert(c.fqn.clone(), c);
        }
        m
    }

    fn annot(name: &str) -> Annotation {
        Annotation {
            name: name.into(),
            fqn: None,
            raw_args: None,
        }
    }

    fn cls(fqn: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
            file: PathBuf::from(format!("{}.java", fqn.rsplit('.').next().unwrap_or("Cls"))),
            kind: ClassKind::Class,
            line_start: 10,
            line_end: 50,
            ..Default::default()
        }
    }

    #[test]
    fn no_static_state_flags_mutable_static_in_component() {
        let mut c = cls("a.b.Cache");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "MAP".into(),
            type_text: "Map<String,String>".into(),
            line: 12,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].line, 12);
        assert!(res.violations[0].message.contains("MAP"));
    }

    #[test]
    fn no_static_state_ignores_final_static() {
        let mut c = cls("a.b.K");
        c.annotations.push(annot("Component"));
        c.fields.push(Field {
            name: "MAX".into(),
            type_text: "final int".into(),
            line: 11,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds.len(), 1);
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn no_static_state_ignores_static_final_via_is_final_flag() {
        // Real Java-parser shape: `private static final Logger LOG = ...`
        // yields type_text "Logger" (modifiers never land in type_text) and
        // is_final: true. Must not be flagged.
        let mut c = cls("a.b.Svc");
        c.annotations.push(annot("Service"));
        c.fields.push(Field {
            name: "LOG".into(),
            type_text: "Logger".into(),
            line: 7,
            is_static: true,
            is_final: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds.len(), 1);
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn no_static_state_ignores_non_components() {
        let mut c = cls("a.b.Util");
        c.fields.push(Field {
            name: "X".into(),
            type_text: "int".into(),
            line: 1,
            is_static: true,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::NoStaticState, &Scope::default());
        assert!(res.violations.is_empty());
    }

    #[test]
    fn tx_on_service_flags_controller_with_tx() {
        let mut c = cls("a.web.UserCtrl");
        c.annotations.push(annot("RestController"));
        let m = Method {
            name: "update".into(),
            line_start: 25,
            annotations: vec![annot("Transactional")],
            ..Default::default()
        };
        c.methods.push(m);
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::TxOnService, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].line, 25);
    }

    #[test]
    fn tx_on_service_accepts_service_with_tx() {
        let mut c = cls("a.svc.UserSvc");
        c.annotations.push(annot("Service"));
        c.annotations.push(annot("Transactional"));
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::TxOnService, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn layered_flags_controller_with_repository_field() {
        let mut c = cls("a.controller.UserCtrl");
        c.annotations.push(annot("RestController"));
        c.fields.push(Field {
            name: "repo".into(),
            type_text: "a.repository.UserRepository".into(),
            line: 14,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert_eq!(res.violations.len(), 1);
        assert!(res.violations[0].message.contains("persistence"));
    }

    #[test]
    fn layered_flags_controller_extending_entity() {
        let mut c = cls("a.web.AdminCtrl");
        c.annotations.push(annot("Controller"));
        c.super_types.push(TypeRef {
            name: "a.entity.User".into(),
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert_eq!(res.violations.len(), 1);
    }

    #[test]
    fn layered_holds_for_clean_controller() {
        let mut c = cls("a.controller.UserCtrl");
        c.annotations.push(annot("Controller"));
        c.fields.push(Field {
            name: "svc".into(),
            type_text: "a.service.UserService".into(),
            line: 14,
            ..Default::default()
        });
        let repo = mk_repo(vec![make_module("m", vec![c])]);
        let res = check(&repo, Pattern::Layered, &Scope::default());
        assert!(res.violations.is_empty());
        assert_eq!(res.holds[0].count, 1);
    }

    #[test]
    fn scope_filter_limits_to_module() {
        let mut c1 = cls("x.web.Ctl");
        c1.annotations.push(annot("Controller"));
        c1.fields.push(Field {
            name: "r".into(),
            type_text: "x.repository.Repo".into(),
            line: 5,
            ..Default::default()
        });
        let mut c2 = cls("y.web.Ctl");
        c2.annotations.push(annot("Controller"));
        c2.fields.push(Field {
            name: "r".into(),
            type_text: "y.repository.Repo".into(),
            line: 5,
            ..Default::default()
        });
        let repo = mk_repo(vec![
            make_module("alpha", vec![c1]),
            make_module("beta", vec![c2]),
        ]);
        let res = check(
            &repo,
            Pattern::Layered,
            &Scope {
                module: Some("alpha".into()),
            },
        );
        assert_eq!(res.violations.len(), 1);
        assert_eq!(res.violations[0].module, "alpha");
    }

    #[test]
    fn pattern_parses_aliases() {
        assert_eq!(Pattern::parse("layered"), Some(Pattern::Layered));
        assert_eq!(Pattern::parse("Layered"), Some(Pattern::Layered));
        assert_eq!(
            Pattern::parse("no_static_state"),
            Some(Pattern::NoStaticState)
        );
        assert_eq!(Pattern::parse("tx_on_service"), Some(Pattern::TxOnService));
        assert!(Pattern::parse("unknown").is_none());
    }
}

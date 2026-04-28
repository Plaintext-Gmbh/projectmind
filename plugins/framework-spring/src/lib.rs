// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Spring framework plugin.
//!
//! Recognises common Spring stereotypes (`@Service`, `@Controller`, …) and DI
//! annotations (`@Autowired`, `@Inject`, …). Builds a bean graph as relations
//! between classes.

#![warn(missing_docs)]

use plaintext_ide_plugin_api::{
    Annotation, Class, FrameworkPlugin, Module, PluginInfo, Relation, RelationKind, Result,
};

/// Stereotypes recognised by this plugin.
const STEREOTYPES: &[(&str, &str)] = &[
    ("Service", "service"),
    ("Component", "component"),
    ("Controller", "controller"),
    ("RestController", "rest-controller"),
    ("Repository", "repository"),
    ("Configuration", "configuration"),
];

const INJECT_ANNOTATIONS: &[&str] = &["Autowired", "Inject", "Resource"];

/// Spring framework plugin.
#[derive(Debug, Default)]
pub struct SpringPlugin;

impl SpringPlugin {
    /// Construct a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl FrameworkPlugin for SpringPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "framework-spring",
            name: "Spring",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn supported_languages(&self) -> &[&'static str] {
        &["lang-java", "lang-kotlin"]
    }

    fn enrich(&self, module: &mut Module) -> Result<()> {
        for class in module.classes.values_mut() {
            attach_stereotype(class);
        }
        Ok(())
    }

    fn relations(&self, module: &Module) -> Vec<Relation> {
        let mut out = Vec::new();
        let bean_fqns: Vec<&str> = module
            .classes
            .values()
            .filter(|c| !c.stereotypes.is_empty())
            .map(|c| c.fqn.as_str())
            .collect();

        for class in module.classes.values() {
            // Field injection
            for field in &class.fields {
                if has_inject_annotation(&field.annotations) {
                    if let Some(target) = resolve_type(&field.type_text, &bean_fqns, module) {
                        out.push(Relation {
                            from: class.fqn.clone(),
                            to: target,
                            kind: RelationKind::Injects,
                        });
                    }
                }
            }
        }
        out
    }
}

fn attach_stereotype(class: &mut Class) {
    for (annotation, stereotype) in STEREOTYPES {
        if class.annotations.iter().any(|a| a.is(annotation))
            && !class.stereotypes.iter().any(|s| s == stereotype)
        {
            class.stereotypes.push((*stereotype).to_string());
        }
    }
}

fn has_inject_annotation(annotations: &[Annotation]) -> bool {
    annotations
        .iter()
        .any(|a| INJECT_ANNOTATIONS.contains(&a.name.as_str()))
}

/// Try to find a class FQN whose simple name matches the field's type text.
///
/// Field type text is produced by tree-sitter as written in the source (e.g. `UserRepo`,
/// `org.foo.Bar`). For Phase 1 we match by simple name within the same module.
fn resolve_type(type_text: &str, bean_fqns: &[&str], module: &Module) -> Option<String> {
    let simple = type_text.rsplit_once('.').map_or(type_text, |(_, s)| s);
    // Prefer beans (annotated classes) but fall back to any class with a matching simple name.
    if let Some(fqn) = bean_fqns.iter().find(|fqn| simple_name(fqn) == simple) {
        return Some((*fqn).to_string());
    }
    module
        .classes
        .keys()
        .find(|fqn| simple_name(fqn) == simple)
        .cloned()
}

fn simple_name(fqn: &str) -> &str {
    fqn.rsplit_once('.').map_or(fqn, |(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plaintext_ide_plugin_api::{Annotation, Class, Field, Module, Visibility};

    fn class_with_annotation(fqn: &str, annot: &str) -> Class {
        Class {
            fqn: fqn.into(),
            name: simple_name(fqn).into(),
            annotations: vec![Annotation {
                name: annot.into(),
                fqn: None,
                raw_args: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn enrich_attaches_stereotype() {
        let mut module = Module::default();
        module
            .classes
            .insert("a.S".into(), class_with_annotation("a.S", "Service"));
        SpringPlugin.enrich(&mut module).unwrap();
        assert_eq!(
            module.classes["a.S"].stereotypes,
            vec!["service".to_string()]
        );
    }

    #[test]
    fn relations_capture_field_injection() {
        let mut module = Module::default();
        module.classes.insert(
            "a.UserService".into(),
            class_with_annotation("a.UserService", "Service"),
        );
        let mut consumer = class_with_annotation("a.UserController", "RestController");
        consumer.fields.push(Field {
            name: "service".into(),
            type_text: "UserService".into(),
            line: 1,
            visibility: Visibility::default(),
            annotations: vec![Annotation {
                name: "Autowired".into(),
                fqn: None,
                raw_args: None,
            }],
            is_static: false,
        });
        module.classes.insert(consumer.fqn.clone(), consumer);
        SpringPlugin.enrich(&mut module).unwrap();
        let rels = SpringPlugin.relations(&module);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].from, "a.UserController");
        assert_eq!(rels[0].to, "a.UserService");
        assert_eq!(rels[0].kind, RelationKind::Injects);
    }

    #[test]
    fn rest_controller_gets_distinct_stereotype() {
        let mut module = Module::default();
        module
            .classes
            .insert("a.X".into(), class_with_annotation("a.X", "RestController"));
        SpringPlugin.enrich(&mut module).unwrap();
        assert_eq!(
            module.classes["a.X"].stereotypes,
            vec!["rest-controller".to_string()]
        );
    }
}

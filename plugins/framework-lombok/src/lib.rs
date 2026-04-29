// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Project Lombok plugin.
//!
//! Recognises common Lombok annotations and exposes them as `class.extras["lombok"]` so
//! visualisations can show "this DTO has @Data, you'll see synthetic getters/setters here".
//!
//! Phase 1 keeps this static — we don't synthesise virtual methods yet. Once a method-level
//! visualiser exists we can flesh those out.

#![warn(missing_docs)]

use projectmind_plugin_api::{FrameworkPlugin, Module, PluginInfo, Relation, Result};

/// Lombok annotations that touch class-level structure.
const CLASS_ANNOTATIONS: &[&str] = &[
    "Data",
    "Value",
    "Builder",
    "SuperBuilder",
    "AllArgsConstructor",
    "NoArgsConstructor",
    "RequiredArgsConstructor",
    "ToString",
    "EqualsAndHashCode",
    "Slf4j",
    "Log",
    "Log4j2",
    "CommonsLog",
    "JBossLog",
    "FieldDefaults",
];

/// Lombok annotations that touch field/method-level structure.
const MEMBER_ANNOTATIONS: &[&str] = &["Getter", "Setter", "With", "Synchronized", "Cleanup"];

/// Lombok plugin.
#[derive(Debug, Default)]
pub struct LombokPlugin;

impl LombokPlugin {
    /// Construct a new instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl FrameworkPlugin for LombokPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "framework-lombok",
            name: "Lombok",
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn supported_languages(&self) -> &[&'static str] {
        &["lang-java", "lang-kotlin"]
    }

    fn enrich(&self, module: &mut Module) -> Result<()> {
        for class in module.classes.values_mut() {
            let mut detected: Vec<String> = Vec::new();
            for ann in &class.annotations {
                if CLASS_ANNOTATIONS.contains(&ann.name.as_str()) {
                    detected.push(ann.name.clone());
                }
            }
            for field in &class.fields {
                for ann in &field.annotations {
                    if MEMBER_ANNOTATIONS.contains(&ann.name.as_str())
                        && !detected.iter().any(|d| d == &ann.name)
                    {
                        detected.push(ann.name.clone());
                    }
                }
            }

            if !detected.is_empty() {
                if !class.stereotypes.iter().any(|s| s == "lombok") {
                    class.stereotypes.push("lombok".to_string());
                }
                class
                    .extras
                    .insert("lombok".to_string(), serde_json::json!(detected));
            }
        }
        Ok(())
    }

    fn relations(&self, _module: &Module) -> Vec<Relation> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_plugin_api::{Annotation, Class, Field};

    fn class_with_class_anns(fqn: &str, anns: &[&str]) -> Class {
        Class {
            fqn: fqn.into(),
            name: fqn.rsplit_once('.').map_or(fqn, |(_, s)| s).to_string(),
            annotations: anns
                .iter()
                .map(|n| Annotation {
                    name: (*n).to_string(),
                    fqn: None,
                    raw_args: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn data_annotation_marks_class_as_lombok() {
        let mut module = Module::default();
        module
            .classes
            .insert("a.Foo".into(), class_with_class_anns("a.Foo", &["Data"]));
        LombokPlugin.enrich(&mut module).unwrap();
        let class = &module.classes["a.Foo"];
        assert_eq!(class.stereotypes, vec!["lombok".to_string()]);
        assert_eq!(
            class
                .extras
                .get("lombok")
                .and_then(|v| v.as_array().map(Vec::len)),
            Some(1)
        );
    }

    #[test]
    fn no_lombok_stereotype_when_no_lombok_annotation() {
        let mut module = Module::default();
        module
            .classes
            .insert("a.Bar".into(), class_with_class_anns("a.Bar", &["Service"]));
        LombokPlugin.enrich(&mut module).unwrap();
        assert!(module.classes["a.Bar"].stereotypes.is_empty());
        assert!(!module.classes["a.Bar"].extras.contains_key("lombok"));
    }

    #[test]
    fn field_level_getter_counts() {
        let mut module = Module::default();
        let mut class = class_with_class_anns("a.Pojo", &[]);
        class.fields.push(Field {
            name: "x".into(),
            annotations: vec![Annotation {
                name: "Getter".into(),
                fqn: None,
                raw_args: None,
            }],
            ..Default::default()
        });
        module.classes.insert(class.fqn.clone(), class);
        LombokPlugin.enrich(&mut module).unwrap();
        let class = &module.classes["a.Pojo"];
        assert_eq!(class.stereotypes, vec!["lombok".to_string()]);
        assert!(class
            .extras
            .get("lombok")
            .and_then(|v| v.as_array())
            .is_some_and(|a| a.iter().any(|x| x == "Getter")));
    }
}

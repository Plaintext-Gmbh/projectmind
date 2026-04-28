// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Relationships between code entities.

use serde::{Deserialize, Serialize};

/// A directed relation between two entities, identified by their FQNs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    /// FQN of the source entity.
    pub from: String,
    /// FQN of the target entity.
    pub to: String,
    /// Kind of relation.
    pub kind: RelationKind,
}

/// Kinds of relations between entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// `A extends B`.
    Extends,
    /// `A implements B`.
    Implements,
    /// `A` declares a field/parameter of type `B`.
    Uses,
    /// `A` injects `B` (Spring `@Autowired`, constructor injection, …).
    Injects,
    /// `A` calls a method on `B`.
    Calls,
    /// `A` is annotated with `B`.
    Annotated,
    /// Catch-all for plugin-defined relations.
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_round_trip() {
        let r = Relation {
            from: "a".into(),
            to: "b".into(),
            kind: RelationKind::Injects,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Relation = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}

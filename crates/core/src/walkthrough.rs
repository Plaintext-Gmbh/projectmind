// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Walk-through body & feedback storage.
//!
//! A *walk-through* is a sequenced tour authored by an LLM:
//! ordered steps with narration, each pointing at one of the existing
//! viewers (class / file / markdown / diff / note). The LLM drives via
//! the MCP tools `walkthrough_start / append / set_step / clear`; the
//! GUI displays the current step and lets the user acknowledge or ask
//! for more detail. Those user actions get appended to a separate
//! feedback file, which the LLM polls to know what the user wants.
//!
//! The body and feedback live next to the statefile:
//!
//! ```text
//! $cache/projectmind/
//!   current.json            # existing — UiState; pointer is in here
//!   ui-heartbeat.json       # existing
//!   walkthrough.json        # ← body (this module)
//!   walkthrough-feedback.json   # ← user → LLM channel (this module)
//! ```
//!
//! Splitting body from `current.json` keeps the high-traffic statefile
//! tiny: every `walkthrough_set_step` is a `seq` bump on `current.json`,
//! never a re-write of the (potentially large) narration.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// `schemaVersion` of the very first walk-through format (pre-Cockpit-2.4).
/// Tours authored before the field existed deserialize to this value.
pub const SCHEMA_VERSION_V1: u32 = 1;

/// `schemaVersion` stamped on tours authored with the Cockpit 2.4
/// (`risk` / `pattern` / `atlas`) step kinds and auto-annotation.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// serde default for the [`Walkthrough::schema_version`] field: a missing
/// `schemaVersion` means a legacy v1 tour.
const fn schema_version_v1() -> u32 {
    SCHEMA_VERSION_V1
}

/// One stop in the tour. The `target` field decides which viewer
/// renders the step; the rest is metadata + the LLM's narration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkthroughStep {
    /// Short, human-readable step title (sidebar entry).
    pub title: String,
    /// Optional markdown narration shown alongside the target.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub narration: String,
    /// What to render in the main pane.
    pub target: WalkthroughTarget,
}

/// What kind of thing the step is pointing at. Each variant maps 1:1
/// to a viewer the GUI already has, plus a `note` mode that's narration-
/// only (useful for "let me explain the context first" steps).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WalkthroughTarget {
    /// A class in the open repository.
    Class {
        /// Fully-qualified class name.
        fqn: String,
        /// Line ranges to highlight (1-based, inclusive).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        highlight: Vec<LineRange>,
    },
    /// An arbitrary file (markdown is rendered, others as plain source).
    File {
        /// Absolute path on disk.
        path: PathBuf,
        /// Heading slug (markdown only).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
        /// Line ranges to highlight (non-markdown only; markdown ignores it).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        highlight: Vec<LineRange>,
    },
    /// A unified diff between two refs (or `ref` vs working tree).
    Diff {
        /// Base ref (e.g. `HEAD~5`, branch name).
        reference: String,
        /// Optional target ref. `None` means working tree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        /// Optional focus inside the diff (#126). When present the GUI
        /// scrolls + pulses the matching hunk; old tour payloads that
        /// don't set it render the diff exactly like before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<DiffFocus>,
    },
    /// An AI-generated artifact (HTML or Markdown), rendered by the same
    /// viewer as the standalone `present_artifact` push. The body lives in
    /// [`crate::artifact::artifact_path`].
    Artifact {
        /// Artifact handle. Matches `Artifact::id` in the body file.
        id: String,
    },
    /// A risk-scored class (Cockpit 2.4, #160). Renders the class viewer
    /// topped by a risk-score header bar. The `show` list selects which
    /// signals the header emphasises; an empty list means "show every
    /// signal that has data".
    Risk {
        /// Fully-qualified class name to score + display.
        fqn: String,
        /// Optional member (method/field) name to scroll to inside the class.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus: Option<String>,
        /// Which risk signals to surface in the header bar. Empty = all
        /// available. Unknown entries are ignored by the renderer.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        show: Vec<RiskSignal>,
    },
    /// A single architecture-drift pattern scoped to a module or the whole
    /// repo (Cockpit 2.4, #160). Renders the violation list with
    /// `file:line` jumps.
    Pattern {
        /// Pattern id — `repository` | `layered` | `di_only` |
        /// `tx_on_service` | `no_static_state` (`PascalCase` accepted too).
        pattern: String,
        /// Optional scope. `module:<id>` narrows to one module; `all`
        /// (or omitted) checks the whole repo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
        /// What to render. Only `violations` is defined today; other
        /// values fall back to the violation list.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<String>,
    },
    /// The Risk Atlas treemap (Cockpit 2.4, #160), optionally scoped to a
    /// single module, with named hotspots ringed. Renders the same treemap
    /// as the standalone Risk Atlas view.
    Atlas {
        /// Optional module id filter. Omitted = the whole repo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        module: Option<String>,
        /// Fully-qualified class names to ring as named hotspots. Entries
        /// that don't resolve to a tile are ignored by the renderer.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        highlight_fqns: Vec<String>,
    },
    /// A before/after snapshot of one diagram between two refs (#125).
    /// The GUI renders the `from` and `to` states of `diagram` with
    /// before / after / changed-only toggles; changed nodes pulse once.
    /// Old tours never emit this variant, so their rendering is unchanged.
    DiagramDiff {
        /// Which diagram kind to snapshot (e.g. `folder-map`). Omitted
        /// means the GUI's currently selected diagram.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diagram: Option<String>,
        /// Base ref for the "before" state (e.g. `HEAD~5`, a branch name).
        from: String,
        /// Optional target ref for the "after" state. `None` means the
        /// current working tree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    /// Pure narration; nothing rendered in the target pane.
    Note,
}

/// A risk signal the `risk` step's header bar can surface (Cockpit 2.4).
///
/// The names mirror the fields of [`crate::risk::RiskScore`] so a step's
/// `show: ["churn", "cx", "cov"]` maps 1:1 onto the atlas data. Unknown
/// strings are rejected at parse time, keeping the union narrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskSignal {
    /// Commit count over the churn window.
    Churn,
    /// Cyclomatic-complexity estimate.
    Cx,
    /// Line coverage fraction.
    Cov,
    /// Fan-in (incoming references).
    FanIn,
    /// Fan-out (outgoing references).
    FanOut,
}

/// Optional spotlight inside a diff target (#126).
///
/// All fields are independent: `file` alone scrolls the diff to the first
/// hunk of that file; `file + hunk` jumps to a specific hunk inside it;
/// `file + line` jumps to a particular line. The GUI tolerates any
/// combination — a `line` without a `file` falls back to "first hunk
/// containing that line", which is usually unique inside a single
/// commit-sized patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffFocus {
    /// Repository-relative path of the file to scroll to. Match is
    /// substring on the `+++ b/<path>` header so callers can pass
    /// either the relative path or just the basename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 0-based hunk index *within `file`* (or the whole diff when no
    /// file is set). Ignored when out of range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk: Option<u32>,
    /// 1-based line number in the new file (i.e. the right side of the
    /// diff). Ignored when out of range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// 1-based inclusive line range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LineRange {
    /// First line of the range (1-based, inclusive).
    pub from: u32,
    /// Last line of the range (1-based, inclusive).
    pub to: u32,
}

/// Full walk-through body. The current step pointer lives in
/// [`crate::state::UiState`] / [`crate::state::ViewIntent::Walkthrough`],
/// not here — bumping the pointer must not require rewriting the
/// (potentially large) body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Walkthrough {
    /// On-disk schema version (Cockpit 2.4, #160). v1 tours predate the
    /// `risk` / `pattern` / `atlas` step kinds and omit this field, so a
    /// missing value deserializes to [`SCHEMA_VERSION_V1`] and old tours
    /// load unchanged. New tours written by `walkthrough_start` stamp
    /// [`CURRENT_SCHEMA_VERSION`].
    #[serde(default = "schema_version_v1", rename = "schemaVersion")]
    pub schema_version: u32,
    /// Stable handle. Generated by `walkthrough_start` if not provided.
    pub id: String,
    /// Tour title (header + sidebar caption).
    pub title: String,
    /// Optional one-paragraph intro shown above step 1.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Ordered steps. Length ≥ 1 once the tour has started.
    pub steps: Vec<WalkthroughStep>,
    /// Optional end-of-tour learning quiz (#124). Empty means the GUI
    /// shows the existing "Tour finished" card without quiz UI; tours
    /// authored before this field existed deserialize cleanly because
    /// of the `default`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quiz: Vec<QuizQuestion>,
    /// Unix-seconds timestamp of last write — coarse `mtime` so the GUI
    /// can detect stale tours without consulting the FS.
    #[serde(default)]
    pub updated_at: u64,
}

/// One end-of-tour multiple-choice question.
///
/// The data shape mirrors the sketch in
/// [#124](https://github.com/Plaintext-Gmbh/projectmind/issues/124):
/// a prompt, a small list of choices, the index of the correct answer,
/// and an optional list of step indices the user can replay if they
/// got the question wrong.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuizQuestion {
    /// The question text.
    pub prompt: String,
    /// Possible answers in the order they will be rendered. The GUI
    /// expects 2-5 choices; tours that send more get rendered as-is
    /// but become hard to scan.
    pub choices: Vec<String>,
    /// 0-based index into `choices` of the correct answer.
    pub answer: usize,
    /// Optional 0-based step indices that explain this question. The
    /// GUI shows them as "replay these steps" links when the user gets
    /// the question wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_refs: Vec<u32>,
    /// Optional one-line explanation shown after the user answers.
    /// Renders as plain text — markdown intentionally not supported
    /// here so the explanation reads identically across viewers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub explanation: String,
}

impl QuizQuestion {
    /// `true` when `answer` points at a valid index in `choices`.
    /// Tours with malformed quiz entries are rendered without scoring.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.choices.is_empty() && self.answer < self.choices.len()
    }
}

/// One feedback event from the user, tied to a step. Append-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FeedbackEvent {
    /// Walk-through id this belongs to.
    pub walkthrough_id: String,
    /// 0-based step index when the event happened.
    pub step: u32,
    /// What the user did.
    pub kind: FeedbackKind,
    /// Optional free-text note (only for `more_detail` today).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Unix-seconds timestamp.
    pub ts: u64,
}

/// Categorical user action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    /// "I read & understood this step." Advances the pointer.
    Understood,
    /// "Please explain this in more detail." Pointer stays put; the
    /// LLM is expected to amend the step's narration (or insert a new
    /// step before continuing).
    MoreDetail,
}

/// Feedback log — the whole file, replayed each time. Tiny; it's a few
/// kilobytes per tour at most.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeedbackLog {
    /// All events in arrival order.
    pub events: Vec<FeedbackEvent>,
}

// ----- Paths ---------------------------------------------------------------

/// Path of the body file. Always next to the statefile.
#[must_use]
pub fn body_path() -> PathBuf {
    sibling("walkthrough.json")
}

/// Path of the feedback log.
#[must_use]
pub fn feedback_path() -> PathBuf {
    sibling("walkthrough-feedback.json")
}

fn sibling(name: &str) -> PathBuf {
    let state = crate::state::statefile_path();
    let parent = state
        .parent()
        .map_or_else(std::env::temp_dir, Path::to_path_buf);
    parent.join(name)
}

// ----- IO ------------------------------------------------------------------

/// Read the current walk-through body, or `None` if no tour is active.
pub fn read_body() -> std::io::Result<Option<Walkthrough>> {
    read_at::<Walkthrough>(&body_path())
}

/// Read the feedback log. Returns an empty log if the file is missing.
pub fn read_feedback() -> std::io::Result<FeedbackLog> {
    Ok(read_at::<FeedbackLog>(&feedback_path())?.unwrap_or_default())
}

/// Write the body atomically. Stamps `updated_at` automatically.
pub fn write_body(mut body: Walkthrough) -> std::io::Result<Walkthrough> {
    body.updated_at = now_secs();
    write_atomic(&body_path(), &body)?;
    Ok(body)
}

/// Append one event to the feedback log atomically.
pub fn append_feedback(event: FeedbackEvent) -> std::io::Result<FeedbackLog> {
    let mut log = read_feedback()?;
    log.events.push(event);
    write_atomic(&feedback_path(), &log)?;
    Ok(log)
}

/// Remove body & feedback. No-op if either is already absent.
pub fn clear() -> std::io::Result<()> {
    let body = body_path();
    let fb = feedback_path();
    for p in [body, fb] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn read_at<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s)
            .map(Some)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn write_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- ID generation -------------------------------------------------------

/// Make a slug-friendly id from a tour title. Falls back to a timestamp-
/// based handle if the title slug would be empty.
#[must_use]
pub fn slugify_id(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("wt-{}", now_secs())
    } else {
        format!("{}-{}", &slug[..slug.len().min(40)], now_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock;

    fn override_state(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("projectmind-wt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("current.json");
        std::env::set_var("PROJECTMIND_STATE", &p);
        p
    }

    #[test]
    fn body_round_trip() {
        let _g = test_lock();
        let _ = override_state("body");
        let _ = clear();
        let body = Walkthrough {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "demo".into(),
            title: "Demo tour".into(),
            summary: "What changed".into(),
            steps: vec![WalkthroughStep {
                title: "First".into(),
                narration: "hello".into(),
                target: WalkthroughTarget::Note,
            }],
            quiz: vec![],
            updated_at: 0,
        };
        let written = write_body(body).unwrap();
        assert!(written.updated_at > 0);
        let read = read_body().unwrap().expect("body present");
        assert_eq!(read.id, "demo");
        assert_eq!(read.steps.len(), 1);
    }

    #[test]
    fn feedback_log_appends() {
        let _g = test_lock();
        let _ = override_state("fb");
        let _ = clear();
        let log = read_feedback().unwrap();
        assert!(log.events.is_empty());

        let ev1 = FeedbackEvent {
            walkthrough_id: "t".into(),
            step: 0,
            kind: FeedbackKind::Understood,
            comment: None,
            ts: 1,
        };
        let log = append_feedback(ev1).unwrap();
        assert_eq!(log.events.len(), 1);

        let ev2 = FeedbackEvent {
            walkthrough_id: "t".into(),
            step: 1,
            kind: FeedbackKind::MoreDetail,
            comment: Some("zoom in on the highlight".into()),
            ts: 2,
        };
        let log = append_feedback(ev2).unwrap();
        assert_eq!(log.events.len(), 2);
        assert_eq!(log.events[1].kind, FeedbackKind::MoreDetail);
    }

    #[test]
    fn clear_removes_both_files() {
        let _g = test_lock();
        let _ = override_state("clr");
        let _ = clear();
        write_body(Walkthrough {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "x".into(),
            title: "x".into(),
            summary: String::new(),
            steps: vec![],
            quiz: vec![],
            updated_at: 0,
        })
        .unwrap();
        append_feedback(FeedbackEvent {
            walkthrough_id: "x".into(),
            step: 0,
            kind: FeedbackKind::Understood,
            comment: None,
            ts: 0,
        })
        .unwrap();
        clear().unwrap();
        assert!(read_body().unwrap().is_none());
        assert!(read_feedback().unwrap().events.is_empty());
    }

    #[test]
    fn artifact_target_deserializes_from_kebab_tag() {
        let json = r#"{"kind":"artifact","id":"my-report"}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::Artifact { id } => assert_eq!(id, "my-report"),
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn diagram_diff_target_parses_diagram_from_and_to() {
        let json = r#"{"kind":"diagram-diff","diagram":"folder-map","from":"HEAD~5","to":"HEAD"}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::DiagramDiff { diagram, from, to } => {
                assert_eq!(diagram.as_deref(), Some("folder-map"));
                assert_eq!(from, "HEAD~5");
                assert_eq!(to.as_deref(), Some("HEAD"));
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn diagram_diff_target_defaults_diagram_and_to() {
        // A minimal `diagram-diff` step: only `from`. diagram/to default to None.
        let json = r#"{"kind":"diagram-diff","from":"main"}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::DiagramDiff { diagram, from, to } => {
                assert!(diagram.is_none());
                assert_eq!(from, "main");
                assert!(to.is_none());
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn risk_target_parses_fqn_focus_and_show() {
        let json =
            r#"{"kind":"risk","fqn":"a.b.C","focus":"validateToken","show":["churn","cx","cov"]}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::Risk { fqn, focus, show } => {
                assert_eq!(fqn, "a.b.C");
                assert_eq!(focus.as_deref(), Some("validateToken"));
                assert_eq!(
                    show,
                    vec![RiskSignal::Churn, RiskSignal::Cx, RiskSignal::Cov]
                );
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn risk_target_defaults_focus_and_show() {
        // A minimal `risk` step: only the fqn. focus/show default to empty.
        let json = r#"{"kind":"risk","fqn":"a.b.C"}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::Risk { fqn, focus, show } => {
                assert_eq!(fqn, "a.b.C");
                assert!(focus.is_none());
                assert!(show.is_empty());
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn risk_target_rejects_unknown_signal() {
        // Unknown show-entries are a hard parse error so the union stays narrow.
        let json = r#"{"kind":"risk","fqn":"a.b.C","show":["bogus"]}"#;
        assert!(serde_json::from_str::<WalkthroughTarget>(json).is_err());
    }

    #[test]
    fn pattern_target_parses_pattern_scope_and_view() {
        let json = r#"{"kind":"pattern","pattern":"Repository","scope":"module:auth","view":"violations"}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::Pattern {
                pattern,
                scope,
                view,
            } => {
                assert_eq!(pattern, "Repository");
                assert_eq!(scope.as_deref(), Some("module:auth"));
                assert_eq!(view.as_deref(), Some("violations"));
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn atlas_target_parses_module_and_highlights() {
        let json = r#"{"kind":"atlas","module":"auth","highlight_fqns":["a.b.C","a.b.D"]}"#;
        let target: WalkthroughTarget = serde_json::from_str(json).unwrap();
        match target {
            WalkthroughTarget::Atlas {
                module,
                highlight_fqns,
            } => {
                assert_eq!(module.as_deref(), Some("auth"));
                assert_eq!(highlight_fqns, vec!["a.b.C", "a.b.D"]);
            }
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn v1_tour_without_schema_version_loads_as_v1() {
        // A tour authored before Cockpit 2.4: no `schemaVersion` field, only
        // the classic `class` kind. Must load unchanged as schema v1.
        let json = r#"{
            "id":"legacy","title":"Legacy tour",
            "steps":[{"title":"S1","target":{"kind":"class","fqn":"a.b.C"}}]
        }"#;
        let body: Walkthrough = serde_json::from_str(json).unwrap();
        assert_eq!(body.schema_version, SCHEMA_VERSION_V1);
        assert_eq!(body.steps.len(), 1);
        match &body.steps[0].target {
            WalkthroughTarget::Class { fqn, .. } => assert_eq!(fqn, "a.b.C"),
            other => panic!("wrong target: {other:?}"),
        }
    }

    #[test]
    fn v2_tour_round_trips_schema_version() {
        let json = r#"{
            "schemaVersion":2,"id":"new","title":"New tour",
            "steps":[{"title":"S1","target":{"kind":"risk","fqn":"a.b.C"}}]
        }"#;
        let body: Walkthrough = serde_json::from_str(json).unwrap();
        assert_eq!(body.schema_version, 2);
        let serialized = serde_json::to_string(&body).unwrap();
        assert!(
            serialized.contains("\"schemaVersion\":2"),
            "schemaVersion should serialize back out, got: {serialized}"
        );
    }

    #[test]
    fn slugify_id_handles_punctuation() {
        let id = slugify_id("Markdown Viewer: rollout — phase 1");
        assert!(id.starts_with("markdown-viewer-rollout-phase-1-"));
    }

    #[test]
    fn slugify_id_falls_back_when_empty() {
        let id = slugify_id("!!! ??? ###");
        assert!(id.starts_with("wt-"));
    }

    #[test]
    fn quiz_question_well_formed_when_answer_in_range() {
        let q = QuizQuestion {
            prompt: "Which?".into(),
            choices: vec!["A".into(), "B".into()],
            answer: 1,
            step_refs: vec![],
            explanation: String::new(),
        };
        assert!(q.is_well_formed());
    }

    #[test]
    fn quiz_question_malformed_when_answer_out_of_range() {
        let q = QuizQuestion {
            prompt: "Which?".into(),
            choices: vec!["A".into(), "B".into()],
            answer: 9,
            step_refs: vec![],
            explanation: String::new(),
        };
        assert!(!q.is_well_formed());
    }

    #[test]
    fn walkthrough_quiz_field_is_optional_in_json() {
        // Tour authored before quiz existed: the field is missing entirely.
        let json = r#"{"id":"old","title":"old","steps":[]}"#;
        let body: Walkthrough = serde_json::from_str(json).unwrap();
        assert!(body.quiz.is_empty());

        // Tour with quiz: round-trip preserves it; missing optional fields
        // (`step_refs`, `explanation`) deserialize to defaults.
        let with_quiz = r#"{
            "id":"new","title":"new","steps":[],
            "quiz":[{"prompt":"P","choices":["A","B"],"answer":1}]
        }"#;
        let body: Walkthrough = serde_json::from_str(with_quiz).unwrap();
        assert_eq!(body.quiz.len(), 1);
        assert_eq!(body.quiz[0].answer, 1);
        assert!(body.quiz[0].step_refs.is_empty());
        assert!(body.quiz[0].explanation.is_empty());
    }

    #[test]
    fn walkthrough_quiz_field_is_omitted_when_empty() {
        // Empty quiz round-trips without polluting the on-disk JSON.
        let body = Walkthrough {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "x".into(),
            title: "x".into(),
            summary: String::new(),
            steps: vec![],
            quiz: vec![],
            updated_at: 0,
        };
        let serialized = serde_json::to_string(&body).unwrap();
        assert!(
            !serialized.contains("\"quiz\""),
            "empty quiz should be skipped in serialization, got: {serialized}"
        );
    }
}

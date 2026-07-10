// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `projectmind briefing [--repo <path>] [--since <spec>] [--format text|markdown|json]`
//! — the CLI face of `architect_briefing` (Cockpit 2.7, #163).
//!
//! Prints "what got worse since I last looked" as plain text (default),
//! Markdown, or JSON — designed for cron jobs and Slack bots that want the
//! morning briefing without an MCP round-trip.
//!
//! # Behaviour
//!
//! Opening the repo appends a fresh health snapshot to
//! `.projectmind/state/sessions.jsonl` (exactly as the MCP `open_repo` tool
//! does), so the newest entry is "now" and the briefing diffs it against the
//! baseline chosen by `--since`. Running `briefing` twice in a row therefore
//! compares the two runs — matching the interactive tool's semantics. The
//! `--no-record` flag suppresses that write for a read-only peek.

use std::path::PathBuf;

use anyhow::{Context, Result};
use projectmind_core::session::{self, Briefing, Since};
use projectmind_core::Engine;
use projectmind_framework_lombok::LombokPlugin;
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;
use projectmind_lang_rust::RustPlugin;

/// Output format for the `briefing` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// Compact plain text (the default; cron/Slack friendly).
    Text,
    /// The Markdown chat embed produced by [`session::to_markdown`].
    Markdown,
    /// The raw [`Briefing`] JSON payload.
    Json,
}

impl Format {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "text" | "txt" | "plain" => Some(Self::Text),
            "markdown" | "md" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Parsed `briefing` invocation.
#[derive(Debug)]
pub(crate) struct BriefingArgs {
    /// Repository root. Falls back to the statefile's recorded repo.
    pub(crate) repo: Option<PathBuf>,
    /// Baseline selector (`last_session`, `1d`, `7d`, ISO-8601, unix-seconds).
    pub(crate) since: String,
    /// Output format.
    pub(crate) format: Format,
    /// Skip appending a fresh snapshot before computing the delta.
    pub(crate) no_record: bool,
}

/// Build the same statically-linked engine the MCP server / `record` use, so
/// the briefing resolves risk + patterns identically to the live tools.
fn default_engine() -> Engine {
    let mut engine = Engine::new();
    engine.register_language(Box::new(JavaPlugin::new()));
    engine.register_language(Box::new(RustPlugin::new()));
    engine.register_framework(Box::new(SpringPlugin::new()));
    engine.register_framework(Box::new(LombokPlugin::new()));
    engine
}

/// Run the `briefing` command. Returns the formatted output to print.
pub(crate) fn run(args: &BriefingArgs) -> Result<String> {
    let since = Since::parse(&args.since).with_context(|| {
        format!(
            "unrecognised --since value `{}` (expected last_session | Nd | ISO-8601 | unix-seconds)",
            args.since
        )
    })?;

    let root =
        args.repo.clone().or_else(repo_from_statefile).context(
            "no repository given and none recorded in the statefile — pass --repo <path>",
        )?;

    let engine = default_engine();
    let repo = engine
        .open_repo(&root)
        .with_context(|| format!("open repo {}", root.display()))?;

    // Record the current snapshot unless suppressed, mirroring `open_repo`.
    if !args.no_record {
        let relations = engine.relations(&repo);
        if let Err(err) = session::snapshot_and_log(&repo, &relations) {
            // A read-only home shouldn't make the CLI fail — warn and diff the
            // history as it stands.
            tracing::warn!(error = %err, "briefing: could not append session snapshot");
        }
    }

    let history = session::load_history(&repo.root).context("read session history")?;
    let briefing = session::briefing(&history, since);

    Ok(match args.format {
        Format::Markdown => session::to_markdown(&briefing),
        Format::Json => serde_json::to_string_pretty(&briefing).context("serialise briefing")?,
        Format::Text => to_text(&briefing, history.len()),
    })
}

/// Best-effort read of the repo root recorded in the statefile.
fn repo_from_statefile() -> Option<PathBuf> {
    projectmind_core::state::read()
        .ok()
        .flatten()
        .and_then(|s| s.repo_root)
}

/// Render the briefing as compact plain text (no Markdown decoration) so it
/// pastes cleanly into a terminal, a cron log, or a plain Slack message.
fn to_text(briefing: &Briefing, sessions: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();

    if let Some(w) = &briefing.session_window {
        let _ = writeln!(out, "Architect briefing — {sessions} session(s) on record");
        let _ = writeln!(out, "Window: {} .. {}", w.from, w.to);
    } else {
        let note = briefing
            .note
            .as_deref()
            .unwrap_or("no baseline session to compare against");
        let _ = writeln!(out, "Architect briefing: {note}");
        return out;
    }
    out.push('\n');

    if briefing.is_empty() {
        out.push_str("Nothing got worse since the last session.\n");
        if !briefing.coverage_comparable {
            out.push_str("(coverage delta not shown — tests didn't run in both sessions)\n");
        }
        return out;
    }

    if !briefing.new_hotspots.is_empty() {
        out.push_str("New hotspots:\n");
        for h in &briefing.new_hotspots {
            let _ = writeln!(
                out,
                "  {} : {:.0} (was {:.0}, +{:.0}) [{}]",
                h.fqn, h.score_now, h.score_then, h.delta, h.reason
            );
        }
        out.push('\n');
    }

    if !briefing.pattern_drift.is_empty() {
        out.push_str("Pattern drift:\n");
        for d in &briefing.pattern_drift {
            let _ = writeln!(
                out,
                "  {} : {} -> {} violations (+{})",
                d.pattern, d.count_then, d.count_now, d.delta
            );
        }
        out.push('\n');
    }

    if !briefing.risk_delta.up.is_empty() {
        out.push_str("Risk up:\n");
        for m in &briefing.risk_delta.up {
            let _ = writeln!(out, "  {} (+{:.1})", m.fqn, m.delta);
        }
        out.push('\n');
    }

    if !briefing.risk_delta.down.is_empty() {
        out.push_str("Risk down:\n");
        for m in &briefing.risk_delta.down {
            let _ = writeln!(out, "  {} ({:.1})", m.fqn, m.delta);
        }
        out.push('\n');
    }

    if !briefing.coverage_comparable {
        out.push_str("(coverage delta not factored — tests didn't run in both sessions)\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_core::session::{NewHotspot, PatternDrift, RiskDelta, RiskMove, SessionWindow};

    #[test]
    fn format_parse_accepts_aliases() {
        assert_eq!(Format::parse("text"), Some(Format::Text));
        assert_eq!(Format::parse("MD"), Some(Format::Markdown));
        assert_eq!(Format::parse("json"), Some(Format::Json));
        assert_eq!(Format::parse("yaml"), None);
    }

    #[test]
    fn text_note_when_no_window() {
        let b = Briefing {
            note: Some("only one session on record".into()),
            ..Briefing::default()
        };
        let text = to_text(&b, 1);
        assert!(text.contains("only one session"), "got: {text}");
    }

    #[test]
    fn text_all_clear_when_empty() {
        let b = Briefing {
            session_window: Some(SessionWindow { from: 10, to: 20 }),
            ..Briefing::default()
        };
        let text = to_text(&b, 2);
        assert!(text.contains("Nothing got worse"), "got: {text}");
        assert!(text.contains("Window: 10 .. 20"), "got: {text}");
    }

    #[test]
    fn text_lists_all_sections() {
        let b = Briefing {
            new_hotspots: vec![NewHotspot {
                fqn: "a.A".into(),
                score_now: 80.0,
                score_then: 40.0,
                delta: 40.0,
                reason: "hot".into(),
            }],
            pattern_drift: vec![PatternDrift {
                pattern: "Layered".into(),
                count_now: 3,
                count_then: 1,
                delta: 2,
            }],
            risk_delta: RiskDelta {
                up: vec![RiskMove {
                    fqn: "a.A".into(),
                    delta: 40.0,
                }],
                down: vec![RiskMove {
                    fqn: "b.B".into(),
                    delta: -5.0,
                }],
            },
            session_window: Some(SessionWindow { from: 1, to: 2 }),
            coverage_comparable: false,
            note: None,
        };
        let text = to_text(&b, 2);
        assert!(text.contains("New hotspots:"), "got: {text}");
        assert!(text.contains("a.A : 80"), "got: {text}");
        assert!(text.contains("Pattern drift:"), "got: {text}");
        assert!(text.contains("Layered : 1 -> 3"), "got: {text}");
        assert!(text.contains("Risk up:"), "got: {text}");
        assert!(text.contains("Risk down:"), "got: {text}");
        assert!(text.contains("coverage delta not factored"), "got: {text}");
    }
}

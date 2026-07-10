// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `projectmind record <tour-id> --output tour.pdf` — export a tour.
//!
//! Cockpit 2.6 (#162). Turns the active walk-through into a self-contained
//! deliverable that reads without `ProjectMind` installed:
//!
//! - `.pdf` (default): a structured page per step — title, `file:line`, the
//!   highlighted code snippet, narration, and the Cockpit 2.4 risk / pattern
//!   annotations. Pure Rust (`printpdf`); no headless browser, no `FFmpeg`.
//! - `.mp4`: gated behind the `record-mp4` cargo feature. Without it, an
//!   `.mp4` output is rejected with a clear "enable record-mp4 feature"
//!   error (see [`crate::record::mp4`]).
//!
//! # Tour resolution
//!
//! `ProjectMind` keeps a single *active* tour on disk (next to the statefile,
//! written by `walkthrough_start`) — there is no tour *library* yet, so
//! `record` exports that active tour. The `<tour-id>` argument is validated
//! against the active tour's id unless it is `active` / `-`, which always
//! selects whatever is live. A future tour library only needs to widen
//! [`load_tour`].
//!
//! # Signal resolution
//!
//! When `--repo` is given (or the statefile records one), the command opens
//! it so `class` / `risk` steps can embed the real source lines plus the
//! risk-atlas badges, and `pattern` steps can embed the drift violations.
//! Without a repo it still exports titles + narration, so a note-only tour
//! records fine offline.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use projectmind_core::risk::{self, Options as RiskOptions};
use projectmind_core::tour_pdf::{render_pdf, RenderStep, RenderTour};
use projectmind_core::walkthrough::{LineRange, Walkthrough, WalkthroughStep, WalkthroughTarget};
use projectmind_core::{coverage, patterns as core_patterns, Engine, Repository};
use projectmind_framework_lombok::LombokPlugin;
use projectmind_framework_spring::SpringPlugin;
use projectmind_lang_java::JavaPlugin;
use projectmind_lang_rust::RustPlugin;

/// Build an [`Engine`] with the same statically-linked plugin set the MCP
/// server registers (see [`crate::handler::ServerState`]). Kept here so the
/// `record` command resolves classes / relations exactly like the live tools.
fn default_engine() -> Engine {
    let mut engine = Engine::new();
    engine.register_language(Box::new(JavaPlugin::new()));
    engine.register_language(Box::new(RustPlugin::new()));
    engine.register_framework(Box::new(SpringPlugin::new()));
    engine.register_framework(Box::new(LombokPlugin::new()));
    engine
}

/// Parsed `record` invocation.
#[derive(Debug)]
pub(crate) struct RecordArgs {
    /// Tour id to export. `active` or `-` always selects the live tour.
    pub(crate) tour_id: String,
    /// Output file. Extension picks the format (`.pdf` / `.mp4`).
    pub(crate) output: PathBuf,
    /// Repository root for source / signal resolution. Falls back to the
    /// statefile's recorded repo when omitted.
    pub(crate) repo: Option<PathBuf>,
    /// Embed narration as an audio track (MP4 only; ignored for PDF).
    pub(crate) narrate: bool,
}

/// Run the `record` command. Returns a short success message to print.
pub(crate) fn run(args: &RecordArgs) -> Result<String> {
    let tour = load_tour(&args.tour_id)?;
    let repo = open_repo_for(args);
    let render = build_render_tour(&tour, repo.as_ref());

    match output_kind(&args.output) {
        OutputKind::Pdf => {
            if args.narrate {
                // `--narrate` only embeds audio into an MP4 track; the PDF is
                // silent by construction. Surface that rather than silently
                // dropping the flag.
                tracing::info!("--narrate has no effect on PDF output (audio is MP4-only)");
            }
            let bytes = render_pdf(&render).context("render tour PDF")?;
            std::fs::write(&args.output, &bytes)
                .with_context(|| format!("write {}", args.output.display()))?;
            Ok(format!(
                "Wrote {} ({} steps, {} bytes) to {}",
                tour.title,
                tour.steps.len(),
                bytes.len(),
                args.output.display()
            ))
        }
        OutputKind::Mp4 => mp4::record(&render, args),
        OutputKind::Unknown(ext) => bail!(
            "unsupported output extension `.{ext}` — use .pdf (default) or .mp4 (needs the record-mp4 feature)"
        ),
    }
}

/// Which exporter an output path selects.
enum OutputKind {
    /// Structured PDF — the default, pure-Rust deliverable.
    Pdf,
    /// MP4 video — only real behind the `record-mp4` feature.
    Mp4,
    /// Anything else — rejected.
    Unknown(String),
}

fn output_kind(path: &Path) -> OutputKind {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => OutputKind::Pdf,
        Some("mp4") => OutputKind::Mp4,
        Some(other) => OutputKind::Unknown(other.to_string()),
        None => OutputKind::Unknown(String::new()),
    }
}

/// Load the tour to export. Today the only source is the active tour on disk.
fn load_tour(tour_id: &str) -> Result<Walkthrough> {
    let body = projectmind_core::walkthrough::read_body()
        .context("read active tour")?
        .context("no active tour to record — start one with walkthrough_start first")?;
    let selects_active = tour_id == "active" || tour_id == "-" || tour_id.is_empty();
    if !selects_active && body.id != tour_id {
        bail!(
            "active tour is `{}`, not `{tour_id}` — pass that id, or `active` to record whatever is live",
            body.id
        );
    }
    Ok(body)
}

/// Open the repo for signal resolution, if we can find one. A failure to
/// open is downgraded to `None` with a warning: the export still runs on
/// titles + narration alone.
fn open_repo_for(args: &RecordArgs) -> Option<Repository> {
    let root = args.repo.clone().or_else(repo_from_statefile)?;
    match default_engine().open_repo(&root) {
        Ok(repo) => Some(repo),
        Err(err) => {
            tracing::warn!(error = %err, root = %root.display(), "record: could not open repo — exporting without signals");
            None
        }
    }
}

/// Best-effort read of the repo root recorded in the statefile.
fn repo_from_statefile() -> Option<PathBuf> {
    projectmind_core::state::read()
        .ok()
        .flatten()
        .and_then(|s| s.repo_root)
}

/// Flatten a [`Walkthrough`] into a [`RenderTour`], resolving source and
/// signals from `repo` where possible.
fn build_render_tour(tour: &Walkthrough, repo: Option<&Repository>) -> RenderTour {
    let steps = tour
        .steps
        .iter()
        .map(|s| build_render_step(s, repo))
        .collect();
    RenderTour {
        title: tour.title.clone(),
        summary: tour.summary.clone(),
        steps,
    }
}

fn build_render_step(step: &WalkthroughStep, repo: Option<&Repository>) -> RenderStep {
    let mut out = RenderStep {
        title: step.title.clone(),
        narration: step.narration.clone(),
        ..RenderStep::default()
    };
    match &step.target {
        WalkthroughTarget::Class { fqn, highlight } => {
            out.target = format!("class {fqn}");
            resolve_class(&mut out, repo, fqn, highlight);
        }
        WalkthroughTarget::Risk { fqn, focus, .. } => {
            out.target = match focus {
                Some(f) => format!("risk {fqn} · {f}"),
                None => format!("risk {fqn}"),
            };
            resolve_class(&mut out, repo, fqn, &[]);
            resolve_risk_badge(&mut out, repo, fqn);
        }
        WalkthroughTarget::File {
            path, highlight, ..
        } => {
            out.target = format!("file {}", path.display());
            resolve_file(&mut out, path, highlight);
        }
        WalkthroughTarget::Diff { reference, to, .. } => {
            out.target = match to {
                Some(t) => format!("diff {reference}..{t}"),
                None => format!("diff {reference} → working tree"),
            };
        }
        WalkthroughTarget::Pattern { pattern, scope, .. } => {
            out.target = match scope {
                Some(s) => format!("pattern {pattern} · {s}"),
                None => format!("pattern {pattern}"),
            };
            resolve_pattern(&mut out, repo, pattern, scope.as_deref());
        }
        WalkthroughTarget::Atlas { module, .. } => {
            out.target = match module {
                Some(m) => format!("atlas · {m}"),
                None => "atlas · repo".to_string(),
            };
        }
        WalkthroughTarget::Artifact { id } => {
            out.target = format!("artifact {id}");
        }
        WalkthroughTarget::Note => {
            out.target = "note".to_string();
        }
    }
    out
}

/// Slice out the highlighted lines of a class into the render step. When the
/// step has no explicit highlight, use the class's own line span so the
/// snippet is always something concrete rather than the whole file.
fn resolve_class(
    out: &mut RenderStep,
    repo: Option<&Repository>,
    fqn: &str,
    highlight: &[LineRange],
) {
    let Some(repo) = repo else { return };
    let Some((module, class)) = repo.find_class(fqn) else {
        return;
    };
    let abs = module.root.join(&class.file);
    let Ok(source) = std::fs::read_to_string(&abs) else {
        return;
    };
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        // Empty source file: clamp_range would floor len to 1 and the slice
        // below would index a zero-length vec and panic. Nothing to show.
        return;
    }
    let (from, to) = if let Some(first) = highlight.first() {
        // Union of all highlight ranges, clamped to the file.
        let lo = highlight.iter().map(|r| r.from).min().unwrap_or(first.from);
        let hi = highlight.iter().map(|r| r.to).max().unwrap_or(first.to);
        (lo, hi)
    } else {
        (class.line_start, class.line_end)
    };
    let (from, to) = clamp_range(from, to, lines.len());
    out.location = format!("{}:{}-{}", class.file.display(), from, to);
    out.code = lines[(from as usize - 1)..(to as usize)]
        .iter()
        .map(|l| (*l).to_string())
        .collect();
    out.code_start_line = from;
}

/// Slice a plain (non-class) file target's highlighted lines.
fn resolve_file(out: &mut RenderStep, path: &Path, highlight: &[LineRange]) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return;
    }
    let (from, to) = match highlight.first() {
        Some(_) => {
            let lo = highlight.iter().map(|r| r.from).min().unwrap_or(1);
            let hi = highlight.iter().map(|r| r.to).max().unwrap_or(lo);
            clamp_range(lo, hi, lines.len())
        }
        // No highlight: cap at the first chunk so a huge file doesn't bloat
        // the PDF.
        None => clamp_range(1, 40, lines.len()),
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    out.location = format!("{name}:{from}-{to}");
    out.code = lines[(from as usize - 1)..(to as usize)]
        .iter()
        .map(|l| (*l).to_string())
        .collect();
    out.code_start_line = from;
}

/// Attach the risk-atlas badge line (`risk N/100 · churn.. · cov.. · cx..`)
/// for a class, using the same [`risk::compute`] the `risk_atlas` tool runs.
fn resolve_risk_badge(out: &mut RenderStep, repo: Option<&Repository>, fqn: &str) {
    let Some(repo) = repo else { return };
    let engine = default_engine();
    let relations = engine.relations(repo);
    let cov = coverage::load(&repo.root);
    let opts = RiskOptions {
        top: usize::MAX,
        ..RiskOptions::default()
    };
    let Ok(scores) = risk::compute(repo, &relations, cov.as_ref(), &opts) else {
        return;
    };
    let Some(s) = scores.iter().find(|s| s.fqn == fqn) else {
        return;
    };
    let cov_str = s.cov.map_or_else(
        || "n/a".to_string(),
        |c| format!("{:.0}%", (c * 100.0).round()),
    );
    out.badges.push(format!(
        "risk {:.0}/100 · churn {} · cov {} · cx {} · fan-in {} · fan-out {}",
        s.score.round(),
        s.churn,
        cov_str,
        s.cx,
        s.fan_in,
        s.fan_out
    ));
}

/// Attach a pattern-check summary line + the top drift violations for a
/// `pattern` step, mirroring the `pattern_check` tool.
fn resolve_pattern(
    out: &mut RenderStep,
    repo: Option<&Repository>,
    pattern: &str,
    scope: Option<&str>,
) {
    let Some(repo) = repo else { return };
    let Some(pat) = core_patterns::Pattern::parse(pattern) else {
        return;
    };
    let module = scope.and_then(|s| s.strip_prefix("module:").map(str::to_string));
    let pat_scope = core_patterns::Scope { module };
    let config = core_patterns::PatternConfig::load(&repo.root);
    let result = core_patterns::check_with_config(repo, pat, &pat_scope, &config);
    let visible = result.visible_violations();
    out.badges.push(format!(
        "pattern {pattern}: {} violation(s), confidence {:.2}",
        visible.len(),
        result.confidence
    ));
    // Fold the first handful of violations into the narration-free code
    // area so they read like a checklist without a repo open.
    for v in visible.iter().take(8) {
        out.code
            .push(format!("{}:{}  {}", v.file.display(), v.line, v.message));
    }
}

/// Clamp a 1-based inclusive `[from, to]` line range into `[1, len]`.
fn clamp_range(from: u32, to: u32, len: usize) -> (u32, u32) {
    // Files with more than `u32::MAX` lines don't exist in practice; saturate
    // rather than truncate so the clamp stays sane.
    let len = u32::try_from(len.max(1)).unwrap_or(u32::MAX);
    let from = from.clamp(1, len);
    let to = to.clamp(from, len);
    (from, to)
}

// ----- MP4 path (feature-gated) --------------------------------------------

/// MP4 recording. The real encoder lives behind the `record-mp4` feature;
/// the default build ships only the "feature disabled" stub so `cargo build`
/// never needs `FFmpeg`.
mod mp4 {
    use super::{RecordArgs, RenderTour, Result};

    #[cfg(not(feature = "record-mp4"))]
    pub(super) fn record(_render: &RenderTour, _args: &RecordArgs) -> Result<String> {
        anyhow::bail!(
            "MP4 export is disabled in this build — rebuild with `--features record-mp4` \
             (needs the system FFmpeg libraries), or export a .pdf instead (the default)."
        )
    }

    #[cfg(feature = "record-mp4")]
    pub(super) fn record(render: &RenderTour, args: &RecordArgs) -> Result<String> {
        // Minimal, intentionally thin FFmpeg path. Rasterising each PDF page
        // to a frame is out of scope for the default deliverable; this proves
        // the feature-gated seam compiles and links against ffmpeg-next, and
        // is where a full slideshow encoder would grow. Documented as a
        // deliberate stretch path in the PR.
        ffmpeg_next::init().map_err(|e| anyhow::anyhow!("ffmpeg init: {e}"))?;
        let _ = args.narrate; // audio-track embedding would consume this
        anyhow::bail!(
            "record-mp4 is compiled but the slideshow encoder is not implemented yet \
             ({} steps requested for {}). Use PDF export for now.",
            render.steps.len(),
            args.output.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectmind_core::walkthrough::WalkthroughStep;

    fn tour(id: &str) -> Walkthrough {
        Walkthrough {
            schema_version: 2,
            id: id.into(),
            title: "Demo".into(),
            summary: "s".into(),
            steps: vec![WalkthroughStep {
                title: "Intro".into(),
                narration: "hi".into(),
                target: WalkthroughTarget::Note,
            }],
            quiz: vec![],
            updated_at: 0,
        }
    }

    #[test]
    fn output_kind_from_extension() {
        assert!(matches!(output_kind(Path::new("a.pdf")), OutputKind::Pdf));
        assert!(matches!(output_kind(Path::new("a.PDF")), OutputKind::Pdf));
        assert!(matches!(output_kind(Path::new("a.mp4")), OutputKind::Mp4));
        assert!(matches!(
            output_kind(Path::new("a.gif")),
            OutputKind::Unknown(_)
        ));
        assert!(matches!(
            output_kind(Path::new("noext")),
            OutputKind::Unknown(_)
        ));
    }

    #[test]
    fn build_render_tour_maps_note_step() {
        let t = tour("t1");
        let render = build_render_tour(&t, None);
        assert_eq!(render.title, "Demo");
        assert_eq!(render.steps.len(), 1);
        assert_eq!(render.steps[0].target, "note");
        assert_eq!(render.steps[0].narration, "hi");
        // No repo → no code resolved.
        assert!(render.steps[0].code.is_empty());
    }

    #[test]
    fn build_render_step_labels_targets() {
        let class_step = WalkthroughStep {
            title: "c".into(),
            narration: String::new(),
            target: WalkthroughTarget::Class {
                fqn: "a.b.C".into(),
                highlight: vec![],
            },
        };
        assert_eq!(build_render_step(&class_step, None).target, "class a.b.C");

        let pattern_step = WalkthroughStep {
            title: "p".into(),
            narration: String::new(),
            target: WalkthroughTarget::Pattern {
                pattern: "layered".into(),
                scope: Some("module:web".into()),
                view: None,
            },
        };
        assert_eq!(
            build_render_step(&pattern_step, None).target,
            "pattern layered · module:web"
        );
    }

    #[test]
    fn clamp_range_stays_in_bounds() {
        assert_eq!(clamp_range(1, 10, 5), (1, 5));
        assert_eq!(clamp_range(0, 3, 5), (1, 3));
        assert_eq!(clamp_range(8, 20, 5), (5, 5));
        assert_eq!(clamp_range(3, 2, 5), (3, 3));
    }

    #[test]
    fn resolve_file_on_empty_file_does_not_panic() {
        // A zero-length file must not panic the line slice (clamp_range
        // floors len to 1, so the guard in resolve_file is what saves us).
        let dir = std::env::temp_dir().join(format!("pm-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, "").unwrap();
        let mut out = RenderStep::default();
        resolve_file(&mut out, &path, &[LineRange { from: 1, to: 5 }]);
        assert!(out.code.is_empty());
        assert!(out.location.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(not(feature = "record-mp4"))]
    #[test]
    fn mp4_disabled_errors_clearly() {
        let render = build_render_tour(&tour("t"), None);
        let args = RecordArgs {
            tour_id: "t".into(),
            output: PathBuf::from("out.mp4"),
            repo: None,
            narrate: false,
        };
        let err = mp4::record(&render, &args).unwrap_err().to_string();
        assert!(err.contains("record-mp4"), "got: {err}");
    }
}

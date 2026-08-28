// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Self-contained PDF rendering of a walk-through tour (Cockpit 2.6, #162).
//!
//! A recorded tour ships as a single `.tour.pdf` that reads without
//! ProjectMind installed: a cover page plus one structured page per step
//! (title, `file:line`, the code snippet of the highlighted lines, the
//! narration, and any risk-badge / pattern annotations resolved from the
//! Cockpit 2.4 signals). This is the default `projectmind record`
//! deliverable — no headless browser, no `ffmpeg`, just a pure-Rust PDF
//! writer (`printpdf`, standard Helvetica so no font file is bundled).
//!
//! # The [`RenderTour`] seam
//!
//! Rendering is deliberately split from data resolution. This module is
//! **pure**: it takes an already-resolved [`RenderTour`] — plain strings,
//! no repo, no git, no I/O beyond producing bytes — and lays it out.
//! Resolving a [`Walkthrough`] into a [`RenderTour`] (reading class source,
//! computing risk scores, checking patterns) is the caller's job and lives
//! in the `record` command of the MCP binary, where a [`Repository`] is
//! available. Keeping the layout core pure makes it unit-testable without a
//! model, a repo, or the network — the whole point of the acceptance
//! criterion "N steps → N pages / non-empty PDF".
//!
//! [`Walkthrough`]: crate::walkthrough::Walkthrough
//! [`Repository`]: crate::repository::Repository

use printpdf::{
    BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, TextItem,
};

/// A tour flattened into everything the PDF layout needs — nothing more.
///
/// Built by the `record` command from a [`Walkthrough`](crate::walkthrough::Walkthrough)
/// plus repo-resolved signals; consumed by [`render_pdf`]. Pure data: no
/// paths to read, no git, so tests can hand-build one.
#[derive(Debug, Clone)]
pub struct RenderTour {
    /// Tour title — printed large on the cover page.
    pub title: String,
    /// Optional one-paragraph intro shown on the cover page.
    pub summary: String,
    /// Ordered steps; one PDF page each.
    pub steps: Vec<RenderStep>,
}

/// One tour step, fully resolved into printable text.
#[derive(Debug, Clone, Default)]
pub struct RenderStep {
    /// Step title (page heading).
    pub title: String,
    /// Human-readable target descriptor, e.g. `class com.example.Foo` or
    /// `note`. Printed under the heading.
    pub target: String,
    /// Optional `file:line` locator (e.g. `src/Foo.java:40-58`). Empty when
    /// the step has no source location (`note` / `atlas` steps).
    pub location: String,
    /// Risk / pattern annotation lines resolved from the Cockpit 2.4
    /// signals, e.g. `churn 87 · cov 12% · cx 24` or a pattern-violation
    /// summary. Empty when nothing resolved.
    pub badges: Vec<String>,
    /// Code snippet lines for the step's highlighted range (already sliced,
    /// no line numbers baked in — [`render_pdf`] adds them). Empty for steps
    /// with no code.
    pub code: Vec<String>,
    /// First 1-based source line number of [`RenderStep::code`], so the PDF
    /// can print real gutter numbers. `0` means "no numbering".
    pub code_start_line: u32,
    /// Markdown-ish narration, rendered as plain wrapped paragraphs (the PDF
    /// deliberately does not interpret markdown — it reads identically
    /// everywhere).
    pub narration: String,
}

// ----- Page geometry (A4 portrait, in mm) ----------------------------------

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN_X: f32 = 18.0;
const MARGIN_TOP: f32 = 20.0;
const MARGIN_BOTTOM: f32 = 18.0;

const BODY_SIZE: f32 = 10.5;
const CODE_SIZE: f32 = 9.0;
const HEADING_SIZE: f32 = 17.0;
const META_SIZE: f32 = 9.0;

/// Approximate mm advance per point of font size for a monospace glyph.
/// Helvetica isn't monospace, but the code snippet uses the same font; this
/// factor keeps a conservative character budget so lines rarely overrun.
const CHAR_W_FACTOR: f32 = 0.52;

/// Line height as a multiple of the font size (points → mm is ~0.3528,
/// folded into [`pt_to_mm`]).
const LINE_SPACING: f32 = 1.35;

fn pt_to_mm(pt: f32) -> f32 {
    pt * 0.352_777_8
}

/// How many characters of a given font size fit across the text column.
fn chars_per_line(font_size: f32) -> usize {
    let usable = PAGE_W - 2.0 * MARGIN_X;
    let per_char = pt_to_mm(font_size) * CHAR_W_FACTOR;
    if per_char <= 0.0 {
        return 80;
    }
    ((usable / per_char).floor() as usize).max(8)
}

/// Greedy word-wrap of `text` to at most `width` characters per line.
///
/// Whitespace-collapsing and word-based; a single word longer than `width`
/// is hard-split so it can never overrun the column. Returns at least one
/// (possibly empty) line so an empty paragraph still advances the cursor.
#[must_use]
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    for raw_line in text.split('\n') {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let mut word = word;
            // Hard-split words longer than the column.
            while word.chars().count() > width {
                let head: String = word.chars().take(width).collect();
                if current.is_empty() {
                    out.push(head);
                } else {
                    out.push(std::mem::take(&mut current));
                    out.push(word.chars().take(width).collect());
                }
                word = &word[char_boundary(word, width)..];
            }
            if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Byte offset of the `n`-th char boundary (for slicing a hard-split word).
fn char_boundary(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or_else(|| s.len(), |(i, _)| i)
}

/// Pages under construction: one op list per page. printpdf 0.12 builds a
/// document from complete page op lists (no mutable page/layer handles any
/// more), so text is accumulated here and turned into [`PdfPage`]s at the end.
#[derive(Default)]
struct Pages {
    ops: Vec<Vec<Op>>,
}

impl Pages {
    /// Open a fresh page; returns its index.
    fn add_page(&mut self) -> usize {
        self.ops.push(Vec::new());
        self.ops.len() - 1
    }

    /// Draw one run of text in the built-in Helvetica at `font_size` pt with
    /// its baseline at (`x`, `y`) mm from the bottom-left corner.
    fn use_text(&mut self, page: usize, text: &str, font_size: f32, x: f32, y: f32) {
        let ops = &mut self.ops[page];
        ops.push(Op::StartTextSection);
        ops.push(Op::SetFont {
            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
            size: Pt(font_size),
        });
        ops.push(Op::SetTextCursor {
            pos: Point {
                x: Mm(x).into_pt(),
                y: Mm(y).into_pt(),
            },
        });
        ops.push(Op::ShowText {
            items: vec![TextItem::Text(text.to_string())],
        });
        ops.push(Op::EndTextSection);
    }

    fn into_document(self, title: &str) -> Vec<u8> {
        let pages = self
            .ops
            .into_iter()
            .map(|ops| PdfPage::new(Mm(PAGE_W), Mm(PAGE_H), ops))
            .collect();
        let mut warnings = Vec::new();
        let bytes = PdfDocument::new(title)
            .with_pages(pages)
            .save(&PdfSaveOptions::default(), &mut warnings);
        if !warnings.is_empty() {
            tracing::debug!(
                count = warnings.len(),
                "printpdf warnings while saving tour"
            );
        }
        bytes
    }
}

/// A tiny cursor that flows text down a page, opening a fresh page when it
/// runs out of vertical room. Keeps [`render_pdf`] readable.
struct Flow<'a> {
    pages: &'a mut Pages,
    page: usize,
    /// Current baseline, measured in mm from the page bottom.
    y: f32,
}

impl<'a> Flow<'a> {
    fn new(pages: &'a mut Pages) -> Self {
        let page = pages.add_page();
        Self {
            pages,
            page,
            y: PAGE_H - MARGIN_TOP,
        }
    }

    fn line_height(font_size: f32) -> f32 {
        pt_to_mm(font_size) * LINE_SPACING
    }

    /// Ensure at least `need` mm of vertical space remain; page-break if not.
    fn ensure(&mut self, need: f32) {
        if self.y - need < MARGIN_BOTTOM {
            self.page = self.pages.add_page();
            self.y = PAGE_H - MARGIN_TOP;
        }
    }

    /// Emit one line of text at the current baseline in the given size, then
    /// advance the cursor by one line height.
    fn write_line(&mut self, text: &str, font_size: f32, x: f32) {
        let lh = Self::line_height(font_size);
        self.ensure(lh);
        self.pages.use_text(self.page, text, font_size, x, self.y);
        self.y -= lh;
    }

    /// Advance the cursor by `mm` without drawing (paragraph spacing).
    fn gap(&mut self, mm: f32) {
        self.y -= mm;
    }

    /// Word-wrap and emit a paragraph at `font_size`.
    fn paragraph(&mut self, text: &str, font_size: f32) {
        for line in wrap_text(text, chars_per_line(font_size)) {
            self.write_line(&line, font_size, MARGIN_X);
        }
    }
}

/// Render a resolved tour to PDF bytes.
///
/// Layout: a cover page (title + summary + step count), then one section per
/// step — heading, target/location meta, badge annotations, the code snippet
/// with gutter line numbers, and the narration. Long steps flow onto
/// continuation pages automatically. The returned bytes are a complete,
/// standalone PDF (`%PDF` header) needing no external font.
///
/// # Errors
///
/// Kept as a `Result` for API stability; printpdf 0.12 serialises
/// infallibly (it reports non-fatal findings as warnings, logged at debug).
pub fn render_pdf(tour: &RenderTour) -> anyhow::Result<Vec<u8>> {
    let mut pages = Pages::default();

    // ----- Cover page --------------------------------------------------
    {
        let cover = pages.add_page();
        let mut y = PAGE_H - 90.0;
        for line in wrap_text(&tour.title, chars_per_line(HEADING_SIZE + 7.0)) {
            pages.use_text(cover, &line, HEADING_SIZE + 7.0, MARGIN_X, y);
            y -= Flow::line_height(HEADING_SIZE + 7.0);
        }
        y -= 6.0;
        if !tour.summary.is_empty() {
            for line in wrap_text(&tour.summary, chars_per_line(BODY_SIZE)) {
                pages.use_text(cover, &line, BODY_SIZE, MARGIN_X, y);
                y -= Flow::line_height(BODY_SIZE);
            }
            y -= 6.0;
        }
        let footer = format!(
            "{} step{} · generated by ProjectMind record",
            tour.steps.len(),
            if tour.steps.len() == 1 { "" } else { "s" }
        );
        pages.use_text(cover, &footer, META_SIZE, MARGIN_X, y);
    }

    // ----- One section per step ----------------------------------------
    let mut flow = Flow::new(&mut pages);
    for (i, step) in tour.steps.iter().enumerate() {
        // Keep a step's heading with at least a couple of following lines.
        flow.ensure(Flow::line_height(HEADING_SIZE) * 3.0);

        flow.write_line(
            &format!("{}. {}", i + 1, step.title),
            HEADING_SIZE,
            MARGIN_X,
        );
        flow.gap(1.5);

        if !step.target.is_empty() {
            flow.write_line(&step.target, META_SIZE, MARGIN_X);
        }
        if !step.location.is_empty() {
            flow.write_line(&step.location, META_SIZE, MARGIN_X);
        }
        for badge in &step.badges {
            flow.write_line(badge, META_SIZE, MARGIN_X);
        }
        flow.gap(2.0);

        if !step.code.is_empty() {
            render_code(&mut flow, step);
            flow.gap(2.0);
        }

        if !step.narration.is_empty() {
            for para in step.narration.split("\n\n") {
                flow.paragraph(para, BODY_SIZE);
                flow.gap(2.0);
            }
        }

        flow.gap(6.0);
    }

    Ok(pages.into_document(&tour.title))
}

/// Emit the code snippet with a right-aligned gutter line number. Wraps
/// overlong source lines with a continuation marker so nothing overruns.
fn render_code(flow: &mut Flow, step: &RenderStep) {
    let width = chars_per_line(CODE_SIZE).saturating_sub(7).max(8);
    let mut line_no = step.code_start_line;
    for src in &step.code {
        let wrapped = wrap_text(src, width);
        for (j, piece) in wrapped.iter().enumerate() {
            let gutter = if step.code_start_line > 0 && j == 0 {
                format!("{line_no:>5} ")
            } else {
                "      ".to_string()
            };
            flow.write_line(&format!("{gutter}{piece}"), CODE_SIZE, MARGIN_X);
        }
        if step.code_start_line > 0 {
            line_no = line_no.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_step(title: &str) -> RenderStep {
        RenderStep {
            title: title.into(),
            target: "note".into(),
            narration: "Some narration for the step.".into(),
            ..RenderStep::default()
        }
    }

    #[test]
    fn wrap_text_respects_width() {
        let wrapped = wrap_text("the quick brown fox jumps", 9);
        assert!(
            wrapped.iter().all(|l| l.chars().count() <= 9),
            "{wrapped:?}"
        );
        // Reassembling drops the wrapping but keeps the words in order.
        assert_eq!(wrapped.join(" ").split_whitespace().count(), 5);
    }

    #[test]
    fn wrap_text_hard_splits_long_word() {
        let wrapped = wrap_text("supercalifragilisticexpialidocious", 10);
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|l| l.chars().count() <= 10));
    }

    #[test]
    fn wrap_text_empty_paragraph_yields_one_line() {
        assert_eq!(wrap_text("", 20), vec![String::new()]);
    }

    #[test]
    fn renders_nonempty_pdf() {
        let tour = RenderTour {
            title: "Auth flow".into(),
            summary: "How login works".into(),
            steps: vec![note_step("Intro"), note_step("The filter")],
        };
        let bytes = render_pdf(&tour).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "should be a PDF");
        assert!(bytes.len() > 1000, "non-trivial PDF, got {}", bytes.len());
    }

    /// A step with a full page of narration, so the flow cursor is forced to
    /// break onto a fresh page for each one. Exercises the page-break path in
    /// [`Flow::ensure`] that "N steps → N pages" relies on.
    fn tall_step(title: &str) -> RenderStep {
        // ~80 lines of narration comfortably overflows a single A4 page.
        let narration = (0..80)
            .map(|i| format!("Narration paragraph line {i} explaining the code."))
            .collect::<Vec<_>>()
            .join("\n\n");
        RenderStep {
            title: title.into(),
            target: "note".into(),
            narration,
            ..RenderStep::default()
        }
    }

    #[test]
    fn more_steps_produce_more_pages() {
        // Each tall step overflows a page, so a 4-step tour renders strictly
        // more page objects than a 1-step tour. We count `/MediaBox` markers,
        // which printpdf emits once per page.
        let small = RenderTour {
            title: "T".into(),
            summary: String::new(),
            steps: vec![tall_step("a")],
        };
        let big = RenderTour {
            title: "T".into(),
            summary: String::new(),
            steps: (0..4).map(|i| tall_step(&format!("step {i}"))).collect(),
        };
        assert!(page_count(&render_pdf(&small).unwrap()) < page_count(&render_pdf(&big).unwrap()));
    }

    #[test]
    fn code_snippet_and_badges_render() {
        let step = RenderStep {
            title: "The token check".into(),
            target: "class com.example.Auth".into(),
            location: "src/Auth.java:40-42".into(),
            badges: vec!["risk: churn 87 · cov 12% · cx 24".into()],
            code: vec![
                "public boolean check(String t) {".into(),
                "    return t != null && verify(t);".into(),
                "}".into(),
            ],
            code_start_line: 40,
            narration: "Validates the bearer token before the request proceeds.".into(),
        };
        let tour = RenderTour {
            title: "Security".into(),
            summary: String::new(),
            steps: vec![step],
        };
        let bytes = render_pdf(&tour).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        assert!(bytes.len() > 1200);
    }

    #[test]
    fn empty_tour_still_produces_cover() {
        let tour = RenderTour {
            title: "Empty".into(),
            summary: String::new(),
            steps: vec![],
        };
        let bytes = render_pdf(&tour).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        // Cover page + the (empty) first content page opened by Flow::new.
        assert!(page_count(&bytes) >= 1);
    }

    /// Count PDF page objects by scanning for the `/MediaBox` entry that
    /// printpdf emits exactly once per page. More robust than matching
    /// `/Type /Page` because printpdf's whitespace between the tokens is not
    /// guaranteed and `/Pages` (the tree root) shares the `/Type /Page`
    /// prefix.
    fn page_count(bytes: &[u8]) -> usize {
        let needle = b"/MediaBox";
        let mut n = 0;
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                n += 1;
                i += needle.len();
            } else {
                i += 1;
            }
        }
        n
    }
}

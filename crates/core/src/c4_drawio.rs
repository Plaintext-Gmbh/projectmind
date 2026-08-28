// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! draw.io / diagrams.net export of the C4 model — the *visual review surface*
//! of [#142](https://github.com/Plaintext-Gmbh/projectmind/issues/142).
//!
//! The editable model (`docs/architecture.dsl`, see [`crate::c4_dsl`]) is the
//! semantic source of truth. This module turns that model into a hand-editable
//! `.drawio` file so architects can polish the picture for meetings, reviews
//! and onboarding without leaving the repo:
//!
//! 1. [`c4_model_to_drawio`] renders a [`C4Model`] as an uncompressed `mxfile`
//!    document using draw.io's built-in **C4 shape library** (`mxgraph.c4.*`,
//!    `c4Name` / `c4Type` / `c4Description` placeholders) — the same shapes the
//!    diagrams.net "C4" palette inserts, so the result looks and edits like a
//!    hand-drawn C4 diagram. Page 1 is the container view (persons, one
//!    boundary per software system, containers, cross-container
//!    relationships); every container that owns components gets its own
//!    component page.
//! 2. Every generated cell carries the **model identity** in its metadata
//!    (`pmId` = the DSL id, `pmKind` = person / system / container / component /
//!    boundary / relationship). That is what makes the file *maintainable*
//!    rather than a one-off drawing: [`merge_c4_drawio`] can re-export later
//!    and only **add** the elements the diagram is missing — existing cells,
//!    and with them every manual layout tweak, colour change or annotation,
//!    are preserved byte-for-byte, exactly like [`crate::c4_dsl::merge_c4_dsl`]
//!    treats the DSL. New elements land in a row *below* the existing drawing
//!    so they never overlap what the user arranged.
//! 3. [`export_c4_drawio`] is the host-facing entry point (MCP tool, Tauri
//!    command, browser-host route): read `docs/architecture.dsl` when present
//!    (fall back to the generated model otherwise), then create or merge
//!    `docs/architecture.drawio`.
//! 4. [`save_drawio`] is the write path behind the in-app editor: the GUI hands
//!    the XML the embedded diagrams.net editor produced back to the host, and
//!    this function checks it is a `.drawio` file inside the open repo and a
//!    plausible draw.io document before writing it atomically.
//!
//! Existing files may store pages **compressed** (draw.io's classic
//! `base64(deflate(urlencode(xml)))` page encoding); [`merge_c4_drawio`]
//! understands both forms on read and always writes plain XML, so the file
//! stays diffable in Git.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use projectmind_plugin_api::FrameworkPlugin;
use serde::{Deserialize, Serialize};

use crate::c4_dsl::{self, C4Model};
use crate::file_access::{self, FileAccessError};
use crate::Repository;

/// Repo-relative location of the exported draw.io diagram.
pub const C4_DRAWIO_REL_PATH: &str = "docs/architecture.drawio";

/// Hard cap on the XML the in-app editor may hand back — a C4 diagram is a
/// few hundred KB at most; anything larger is almost certainly not a diagram.
pub const MAX_DRAWIO_BYTES: usize = 20_000_000;

/// Result of [`export_c4_drawio`]: where the file is and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawioExportResult {
    /// Absolute path to `docs/architecture.drawio`.
    pub path: PathBuf,
    /// `true` when this call wrote the file from scratch; `false` when an
    /// existing file was merged in place (or was already complete).
    pub created: bool,
    /// Cells (shapes, boundaries, edges) added to an existing file. `0` when
    /// created fresh or already up to date.
    pub added: usize,
    /// Cells that already existed and were left untouched (layout preserved).
    pub kept: usize,
    /// Pages the file now has (container page + one per container with
    /// components).
    pub pages: usize,
}

/// Outcome of [`merge_c4_drawio`]: the merged document plus what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawioMerge {
    /// The merged `mxfile` XML. Equals the input byte-for-byte when nothing
    /// had to be added (`added == 0`).
    pub xml: String,
    /// Cells inserted from the model.
    pub added: usize,
    /// Cells that already existed (matched by cell id) and were kept as-is.
    pub kept: usize,
    /// Pages in the merged document.
    pub pages: usize,
}

/// Result of [`save_drawio`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveDrawioResult {
    /// Canonical absolute path that was written.
    pub path: PathBuf,
    /// Bytes written.
    pub bytes: usize,
}

/// Why [`save_drawio`] refused to write.
#[derive(Debug, thiserror::Error)]
pub enum SaveDrawioError {
    /// The target is not a `.drawio` file, is outside the repo, or does not
    /// exist yet (the editor only ever rewrites a file it opened).
    #[error("{0}")]
    Access(#[from] FileAccessError),
    /// The file name does not end in `.drawio`.
    #[error("not a .drawio file: {0}")]
    NotDrawio(PathBuf),
    /// The payload is not a draw.io document.
    #[error("not a draw.io document (expected <mxfile> or <mxGraphModel> root)")]
    NotADiagram,
    /// The payload exceeds [`MAX_DRAWIO_BYTES`].
    #[error("diagram too large ({actual} bytes; limit {limit})")]
    TooLarge {
        /// Actual size.
        actual: usize,
        /// Configured cap.
        limit: usize,
    },
    /// Writing failed.
    #[error("write {path}: {source}")]
    Write {
        /// Target path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// Resolve the absolute path of the draw.io export for `repo`.
#[must_use]
pub fn drawio_path(repo: &Repository) -> PathBuf {
    repo.root.join(C4_DRAWIO_REL_PATH)
}

// ---------------------------------------------------------------------------
// Export (host entry point)
// ---------------------------------------------------------------------------

/// Export the C4 model of `repo` to `docs/architecture.drawio`.
///
/// The model comes from `docs/architecture.dsl` when that file exists (so
/// hand-added actors, external systems and rewritten descriptions show up in
/// the drawing); otherwise it is generated from the code exactly like
/// [`crate::c4_dsl::scaffold_c4_model`] would. When the `.drawio` file does not
/// exist it is written fresh (`created: true`); when it does, it is merged with
/// [`merge_c4_drawio`] and only rewritten if something was added.
///
/// # Errors
/// Returns an [`std::io::Error`] if the directory or file cannot be created,
/// read or written.
pub fn export_c4_drawio(
    repo: &Repository,
    framework: &dyn FrameworkPlugin,
) -> std::io::Result<DrawioExportResult> {
    let dsl = match std::fs::read_to_string(c4_dsl::c4_model_path(repo)) {
        Ok(text) => text,
        Err(_) => c4_dsl::generate_c4_dsl(repo, framework),
    };
    let model = c4_dsl::parse_c4_dsl(&dsl);
    let path = drawio_path(repo);

    if let Ok(existing) = std::fs::read_to_string(&path) {
        let merged = merge_c4_drawio(&existing, &model);
        if merged.added > 0 {
            write_atomic(&path, &merged.xml)?;
        }
        return Ok(DrawioExportResult {
            path,
            created: false,
            added: merged.added,
            kept: merged.kept,
            pages: merged.pages,
        });
    }

    let doc = build_document(&model);
    write_atomic(&path, &doc.to_xml())?;
    Ok(DrawioExportResult {
        path,
        created: true,
        added: 0,
        kept: 0,
        pages: doc.pages.len(),
    })
}

/// Write the XML the in-app diagrams.net editor produced back to `path`.
///
/// Guards, in order: the path must be an existing regular `.drawio` file
/// inside `repo_root` (canonicalised — symlinks cannot escape the repo), the
/// payload must be at most [`MAX_DRAWIO_BYTES`], and it must be a draw.io
/// document (`<mxfile>` or `<mxGraphModel>` root, an optional XML declaration
/// before it). The write is atomic (temp file + rename) so a concurrent reader
/// never sees a half-written diagram.
///
/// # Errors
/// See [`SaveDrawioError`].
pub fn save_drawio(
    repo_root: &Path,
    path: &Path,
    xml: &str,
) -> Result<SaveDrawioResult, SaveDrawioError> {
    let is_drawio = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("drawio"));
    if !is_drawio {
        return Err(SaveDrawioError::NotDrawio(path.to_path_buf()));
    }
    let canonical = file_access::canonical_file_in_repo(repo_root, path)?;
    if xml.len() > MAX_DRAWIO_BYTES {
        return Err(SaveDrawioError::TooLarge {
            actual: xml.len(),
            limit: MAX_DRAWIO_BYTES,
        });
    }
    if !looks_like_drawio(xml) {
        return Err(SaveDrawioError::NotADiagram);
    }
    write_atomic(&canonical, xml).map_err(|source| SaveDrawioError::Write {
        path: canonical.clone(),
        source,
    })?;
    Ok(SaveDrawioResult {
        path: canonical,
        bytes: xml.len(),
    })
}

/// `true` when `xml` is (after an optional `<?xml …?>` prolog) an `<mxfile>`
/// or bare `<mxGraphModel>` document.
fn looks_like_drawio(xml: &str) -> bool {
    let mut body = xml.trim_start();
    if let Some(rest) = body.strip_prefix("<?xml") {
        match rest.find("?>") {
            Some(end) => body = rest[end + 2..].trim_start(),
            None => return false,
        }
    }
    body.starts_with("<mxfile") || body.starts_with("<mxGraphModel")
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("drawio.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render `model` as a fresh, uncompressed draw.io `mxfile` document.
///
/// Deterministic: the same model always yields byte-identical XML, so a
/// regenerated diagram diffs cleanly in Git. See the module docs for the page
/// layout and the identity metadata every cell carries.
#[must_use]
pub fn c4_model_to_drawio(model: &C4Model) -> String {
    build_document(model).to_xml()
}

/// Fold the elements of `model` that `existing` (an `mxfile` document) lacks
/// into it, **preserving every existing cell byte-for-byte**.
///
/// Identity is the generated cell id (`pm-<dsl id>` for elements,
/// `pm-boundary-<id>` for boundaries, `pm-rel-<from>-<to>` for
/// relationships), per page. Cells the user added by hand have different ids
/// and are simply kept; cells the user *deleted* are not resurrected only if
/// they also left the model — like the DSL merge, this is additive, so prune
/// by editing the model rather than the drawing. Pages the file lacks (a new
/// container that gained components) are appended whole. Vertices added to an
/// existing page are laid out in rows **below** the current drawing's bounding
/// box so they never cover the user's layout.
///
/// Compressed pages (draw.io's `base64(deflate(urlencode(xml)))`) are
/// decompressed on read; the merged document is always plain XML. Input that is
/// not an `mxfile` at all is returned unchanged with `added == 0`.
#[must_use]
pub fn merge_c4_drawio(existing: &str, model: &C4Model) -> DrawioMerge {
    let doc = build_document(model);
    let Some(mut file) = MxFile::parse(existing) else {
        return DrawioMerge {
            xml: existing.to_string(),
            added: 0,
            kept: 0,
            pages: 0,
        };
    };

    let mut added = 0;
    let mut kept = 0;
    for page in &doc.pages {
        match file.pages.iter_mut().find(|p| p.id == page.id) {
            Some(existing_page) => {
                let ids = existing_page.cell_ids();
                let missing: Vec<&Cell> =
                    page.cells.iter().filter(|c| !ids.contains(&c.id)).collect();
                kept += page.cells.len() - missing.len();
                if missing.is_empty() {
                    continue;
                }
                let mut y = existing_page.bottom() + GAP_Y;
                let mut x = MARGIN;
                let mut inserted = String::new();
                for cell in missing {
                    match cell.kind {
                        CellKind::Vertex { width, height } => {
                            if x + width > ROW_WIDTH {
                                x = MARGIN;
                                y += height + GAP_Y;
                            }
                            inserted.push_str(&cell.emit(x, y));
                            x += width + GAP_X;
                        }
                        CellKind::Edge => inserted.push_str(&cell.emit(0, 0)),
                    }
                    added += 1;
                }
                existing_page.insert_before_root_end(&inserted);
            }
            None => {
                added += page.cells.len();
                file.append_page(page);
            }
        }
    }

    let pages = file.pages.len();
    let xml = if added == 0 {
        existing.to_string()
    } else {
        file.to_xml()
    };
    DrawioMerge {
        xml,
        added,
        kept,
        pages,
    }
}

// ---------------------------------------------------------------------------
// Document model
// ---------------------------------------------------------------------------

const NODE_W: i32 = 240;
const NODE_H: i32 = 120;
const PERSON_W: i32 = 200;
const PERSON_H: i32 = 180;
const GAP_X: i32 = 60;
const GAP_Y: i32 = 80;
const PAD: i32 = 40;
const MARGIN: i32 = 40;
/// Boundary label sits at the bottom; reserve room for it.
const BOUNDARY_LABEL_H: i32 = 40;
/// Row wrap width for merged-in vertices.
const ROW_WIDTH: i32 = 1400;

/// A generated page (`<diagram>`).
#[derive(Debug, Clone)]
struct Page {
    id: String,
    name: String,
    cells: Vec<Cell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Vertex { width: i32, height: i32 },
    Edge,
}

/// One generated cell with its position tokens still unresolved — so the same
/// cell can be emitted at its designed spot (fresh export) or below an
/// existing drawing (merge).
#[derive(Debug, Clone)]
struct Cell {
    id: String,
    kind: CellKind,
    /// XML with `{X}` / `{Y}` placeholders in the geometry.
    template: String,
    /// Designed position for a fresh export.
    x: i32,
    y: i32,
}

impl Cell {
    fn emit(&self, x: i32, y: i32) -> String {
        self.template
            .replace("{X}", &x.to_string())
            .replace("{Y}", &y.to_string())
    }
}

#[derive(Debug, Clone)]
struct Document {
    pages: Vec<Page>,
}

impl Document {
    fn to_xml(&self) -> String {
        let mut out = String::from(
            "<mxfile host=\"ProjectMind\" agent=\"projectmind\" type=\"device\" compressed=\"false\">\n",
        );
        for page in &self.pages {
            out.push_str(&page.to_xml());
        }
        out.push_str("</mxfile>\n");
        out
    }
}

impl Page {
    fn to_xml(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "  <diagram id=\"{}\" name=\"{}\">",
            xml_attr(&self.id),
            xml_attr(&self.name)
        );
        out.push_str(
            "    <mxGraphModel dx=\"1200\" dy=\"800\" grid=\"1\" gridSize=\"10\" guides=\"1\" tooltips=\"1\" connect=\"1\" arrows=\"1\" fold=\"1\" page=\"1\" pageScale=\"1\" pageWidth=\"1169\" pageHeight=\"827\" math=\"0\" shadow=\"0\">\n",
        );
        out.push_str("      <root>\n");
        out.push_str("        <mxCell id=\"0\"/>\n");
        out.push_str("        <mxCell id=\"1\" parent=\"0\"/>\n");
        for cell in &self.cells {
            out.push_str(&cell.emit(cell.x, cell.y));
        }
        out.push_str("      </root>\n");
        out.push_str("    </mxGraphModel>\n");
        out.push_str("  </diagram>\n");
        out
    }
}

/// What a model element renders as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Person,
    System,
    Container,
    Component,
}

impl Shape {
    fn c4_type(self) -> &'static str {
        match self {
            Shape::Person => "Person",
            Shape::System => "Software System",
            Shape::Container => "Container",
            Shape::Component => "Component",
        }
    }

    fn pm_kind(self) -> &'static str {
        match self {
            Shape::Person => "person",
            Shape::System => "system",
            Shape::Container => "container",
            Shape::Component => "component",
        }
    }

    fn size(self) -> (i32, i32) {
        match self {
            Shape::Person => (PERSON_W, PERSON_H),
            _ => (NODE_W, NODE_H),
        }
    }

    /// draw.io C4 library styles (the "C4" palette), so the file edits exactly
    /// like a hand-made C4 diagram. Colours are the library defaults.
    fn style(self) -> &'static str {
        match self {
            Shape::Person => "html=1;fontSize=11;dashed=0;whiteSpace=wrap;fillColor=#083F75;strokeColor=#06315C;fontColor=#ffffff;shape=mxgraph.c4.person2;align=center;metaEdit=1;points=[[0.5,0,0,0],[1,0.5,0,0],[1,0.75,0,0],[0.75,1,0,0],[0.5,1,0,0],[0.25,1,0,0],[0,0.75,0,0],[0,0.5,0,0]];resizable=0;",
            Shape::System => "rounded=1;whiteSpace=wrap;html=1;labelBackgroundColor=none;fillColor=#1061B0;fontColor=#ffffff;align=center;arcSize=10;strokeColor=#0D5091;metaEdit=1;resizable=0;points=[[0.25,0,0],[0.5,0,0],[0.75,0,0],[1,0.25,0],[1,0.5,0],[1,0.75,0],[0.75,1,0],[0.5,1,0],[0.25,1,0],[0,0.75,0],[0,0.5,0],[0,0.25,0]];",
            Shape::Container => "rounded=1;whiteSpace=wrap;html=1;labelBackgroundColor=none;fillColor=#23A2D9;fontColor=#ffffff;align=center;arcSize=10;strokeColor=#0E7DAD;metaEdit=1;resizable=0;points=[[0.25,0,0],[0.5,0,0],[0.75,0,0],[1,0.25,0],[1,0.5,0],[1,0.75,0],[0.75,1,0],[0.5,1,0],[0.25,1,0],[0,0.75,0],[0,0.5,0],[0,0.25,0]];",
            Shape::Component => "rounded=1;whiteSpace=wrap;html=1;labelBackgroundColor=none;fillColor=#63BEF2;fontColor=#ffffff;align=center;arcSize=10;strokeColor=#2086C9;metaEdit=1;resizable=0;points=[[0.25,0,0],[0.5,0,0],[0.75,0,0],[1,0.25,0],[1,0.5,0],[1,0.75,0],[0.75,1,0],[0.5,1,0],[0.25,1,0],[0,0.75,0],[0,0.5,0],[0,0.25,0]];",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Shape::Person => "%c4Name%<div>[%c4Type%]</div><br><div><font style=\"font-size: 11px\"><font color=\"#cccccc\">%c4Description%</font></div>",
            _ => "<b>%c4Name%</b><div>[%c4Type%]</div><br><div><font style=\"font-size: 11px\"><font color=\"#E6E6E6\">%c4Description%</font></div>",
        }
    }
}

const BOUNDARY_STYLE: &str = "rounded=1;fontSize=11;whiteSpace=wrap;html=1;dashed=1;arcSize=20;fillColor=none;strokeColor=#666666;fontColor=#333333;labelBackgroundColor=none;align=left;verticalAlign=bottom;labelBorderColor=none;spacingTop=0;spacing=10;dashPattern=8 4;metaEdit=1;rotatable=0;perimeter=rectanglePerimeter;noLabel=0;labelPadding=0;allowArrows=0;connectable=0;expand=0;recursiveResize=0;editable=1;pointerEvents=0;absoluteArcSize=1;points=[[0.25,0,0],[0.5,0,0],[0.75,0,0],[1,0.25,0],[1,0.5,0],[1,0.75,0],[0.75,1,0],[0.5,1,0],[0.25,1,0],[0,0.75,0],[0,0.5,0],[0,0.25,0]];";
const BOUNDARY_LABEL: &str =
    "<font style=\"font-size: 16px\"><b>%c4Name%</b></font><div>[%c4Type%]</div>";
const REL_STYLE: &str = "endArrow=blockThin;html=1;fontSize=10;fontColor=#404040;strokeWidth=1;endFill=1;strokeColor=#828282;elbow=vertical;metaEdit=1;endSize=14;startSize=14;jumpStyle=arc;jumpSize=16;rounded=0;edgeStyle=orthogonalEdgeStyle;";
const REL_LABEL: &str = "<div style=\"text-align: left\"><div style=\"text-align: center\"><b>%c4Description%</b></div></div>";

fn cell_id(model_id: &str) -> String {
    format!("pm-{}", safe_id(model_id))
}

fn boundary_id(model_id: &str) -> String {
    format!("pm-boundary-{}", safe_id(model_id))
}

fn rel_id(from: &str, to: &str) -> String {
    format!("pm-rel-{}-{}", safe_id(from), safe_id(to))
}

/// Cell ids must be safe as XML attribute values and stable; DSL ids are
/// already `[A-Za-z0-9_]` but hand-edited DSL may carry anything.
fn safe_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn vertex(id: &str, shape: Shape, name: &str, description: &str, x: i32, y: i32) -> Cell {
    let (width, height) = shape.size();
    let template = format!(
        "        <object placeholders=\"1\" c4Name=\"{name}\" c4Type=\"{c4type}\" c4Description=\"{desc}\" label=\"{label}\" pmId=\"{pm_id}\" pmKind=\"{pm_kind}\" id=\"{cell}\">\n          <mxCell style=\"{style}\" vertex=\"1\" parent=\"1\">\n            <mxGeometry x=\"{{X}}\" y=\"{{Y}}\" width=\"{width}\" height=\"{height}\" as=\"geometry\"/>\n          </mxCell>\n        </object>\n",
        name = xml_attr(name),
        c4type = shape.c4_type(),
        desc = xml_attr(description),
        label = xml_attr(shape.label()),
        pm_id = xml_attr(id),
        pm_kind = shape.pm_kind(),
        cell = xml_attr(&cell_id(id)),
        style = shape.style(),
    );
    Cell {
        id: cell_id(id),
        kind: CellKind::Vertex { width, height },
        template,
        x,
        y,
    }
}

fn boundary(id: &str, c4_type: &str, name: &str, x: i32, y: i32, width: i32, height: i32) -> Cell {
    let template = format!(
        "        <object placeholders=\"1\" c4Name=\"{name}\" c4Type=\"{c4type}\" label=\"{label}\" pmId=\"{pm_id}\" pmKind=\"boundary\" id=\"{cell}\">\n          <mxCell style=\"{style}\" vertex=\"1\" parent=\"1\">\n            <mxGeometry x=\"{{X}}\" y=\"{{Y}}\" width=\"{width}\" height=\"{height}\" as=\"geometry\"/>\n          </mxCell>\n        </object>\n",
        name = xml_attr(name),
        c4type = c4_type,
        label = xml_attr(BOUNDARY_LABEL),
        pm_id = xml_attr(id),
        cell = xml_attr(&boundary_id(id)),
        style = BOUNDARY_STYLE,
    );
    Cell {
        id: boundary_id(id),
        kind: CellKind::Vertex { width, height },
        template,
        x,
        y,
    }
}

fn edge(from: &str, to: &str, description: &str) -> Cell {
    let id = rel_id(from, to);
    let template = format!(
        "        <object placeholders=\"1\" c4Type=\"Relationship\" c4Description=\"{desc}\" label=\"{label}\" pmId=\"{pm_id}\" pmKind=\"relationship\" id=\"{cell}\">\n          <mxCell style=\"{style}\" edge=\"1\" parent=\"1\" source=\"{src}\" target=\"{dst}\">\n            <mxGeometry relative=\"1\" as=\"geometry\"/>\n          </mxCell>\n        </object>\n",
        desc = xml_attr(description),
        label = xml_attr(REL_LABEL),
        pm_id = xml_attr(&format!("{from}->{to}")),
        cell = xml_attr(&id),
        style = REL_STYLE,
        src = xml_attr(&cell_id(from)),
        dst = xml_attr(&cell_id(to)),
    );
    Cell {
        id,
        kind: CellKind::Edge,
        template,
        x: 0,
        y: 0,
    }
}

/// Where every model element lives: which shape it is and, for components,
/// which container owns it. Drives the id→page-level mapping of relationships.
struct Index<'a> {
    shape: BTreeMap<&'a str, Shape>,
    /// component id → container id
    owner: BTreeMap<&'a str, &'a str>,
    name: BTreeMap<&'a str, &'a str>,
    description: BTreeMap<&'a str, &'a str>,
}

impl<'a> Index<'a> {
    fn build(model: &'a C4Model) -> Self {
        let mut idx = Index {
            shape: BTreeMap::new(),
            owner: BTreeMap::new(),
            name: BTreeMap::new(),
            description: BTreeMap::new(),
        };
        for p in &model.persons {
            idx.shape.insert(&p.id, Shape::Person);
            idx.name.insert(&p.id, &p.name);
            idx.description.insert(&p.id, &p.description);
        }
        for s in &model.systems {
            idx.shape.insert(&s.id, Shape::System);
            idx.name.insert(&s.id, &s.name);
            idx.description.insert(&s.id, &s.description);
            for c in &s.containers {
                idx.shape.insert(&c.id, Shape::Container);
                idx.name.insert(&c.id, &c.name);
                idx.description.insert(&c.id, &c.description);
                for comp in &c.components {
                    idx.shape.insert(&comp.id, Shape::Component);
                    idx.owner.insert(&comp.id, &c.id);
                    idx.name.insert(&comp.id, &comp.name);
                    idx.description.insert(&comp.id, &comp.description);
                }
            }
        }
        idx
    }

    /// Lift a component id to its container; everything else maps to itself.
    fn container_level<'b>(&self, id: &'b str) -> &'b str
    where
        'a: 'b,
    {
        self.owner.get(id).copied().unwrap_or(id)
    }
}

fn build_document(model: &C4Model) -> Document {
    let idx = Index::build(model);
    let mut pages = vec![containers_page(model, &idx)];
    for system in &model.systems {
        for container in &system.containers {
            if container.components.is_empty() {
                continue;
            }
            pages.push(components_page(model, &idx, &container.id));
        }
    }
    Document { pages }
}

/// Grid counts are tiny (≤ 4 columns, a few dozen rows); saturate rather
/// than wrap in the theoretical overflow case.
fn i(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

fn grid_cols(n: usize) -> usize {
    // 1..=4 columns, square-ish.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cols = (n as f64).sqrt().ceil() as usize;
    cols.clamp(1, 4)
}

/// Page 1: persons, one boundary per system holding its containers (a system
/// without containers — typically a hand-added external system — is a plain
/// Software System box), and every relationship lifted to container level.
fn containers_page(model: &C4Model, idx: &Index<'_>) -> Page {
    let mut cells = Vec::new();
    let mut y = MARGIN;

    // Persons in a row on top.
    let mut x = MARGIN;
    for p in &model.persons {
        cells.push(vertex(&p.id, Shape::Person, &p.name, &p.description, x, y));
        x += PERSON_W + GAP_X;
    }
    if !model.persons.is_empty() {
        y += PERSON_H + GAP_Y;
    }

    // Systems: a boundary with a container grid, or a plain box. Container-less
    // systems (hand-added external ones) go in a row below all boundaries,
    // whose total height is only known after this loop.
    let has_external = model.systems.iter().any(|s| s.containers.is_empty());
    for system in &model.systems {
        if system.containers.is_empty() {
            continue;
        }
        let n = system.containers.len();
        let cols = grid_cols(n);
        let rows = n.div_ceil(cols);
        let cols_i = i(cols);
        let rows_i = i(rows);
        let width = PAD * 2 + cols_i * NODE_W + (cols_i - 1) * GAP_X;
        let height = PAD * 2 + rows_i * NODE_H + (rows_i - 1) * GAP_Y + BOUNDARY_LABEL_H;
        cells.push(boundary(
            &system.id,
            "SystemScopeBoundary",
            &system.name,
            MARGIN,
            y,
            width,
            height,
        ));
        for (idx_, c) in system.containers.iter().enumerate() {
            let col = i(idx_ % cols);
            let row = i(idx_ / cols);
            let cx = MARGIN + PAD + col * (NODE_W + GAP_X);
            let cy = y + PAD + row * (NODE_H + GAP_Y);
            cells.push(vertex(
                &c.id,
                Shape::Container,
                &c.name,
                &c.description,
                cx,
                cy,
            ));
        }
        y += height + GAP_Y;
    }
    if has_external {
        let mut x = MARGIN;
        for system in model.systems.iter().filter(|s| s.containers.is_empty()) {
            cells.push(vertex(
                &system.id,
                Shape::System,
                &system.name,
                &system.description,
                x,
                y,
            ));
            x += NODE_W + GAP_X;
        }
    }

    // Relationships lifted to container level, deduplicated, self-loops
    // dropped (a component talking to a sibling component is intra-container).
    let mut seen = BTreeSet::new();
    for rel in &model.relationships {
        let from = idx.container_level(&rel.from);
        let to = idx.container_level(&rel.to);
        if from == to || !idx.shape.contains_key(from) || !idx.shape.contains_key(to) {
            continue;
        }
        if seen.insert((from, to)) {
            cells.push(edge(from, to, &rel.description));
        }
    }

    Page {
        id: "c4-containers".to_string(),
        name: "Containers".to_string(),
        cells,
    }
}

/// One page per container with components: a container boundary holding the
/// component grid, plus every *outside* element the components talk to (or
/// that talks to them) as a plain box in a column on the right.
fn components_page(model: &C4Model, idx: &Index<'_>, container_id: &str) -> Page {
    let container = model
        .systems
        .iter()
        .flat_map(|s| s.containers.iter())
        .find(|c| c.id == container_id)
        .expect("components_page called for a known container");
    let mut cells = Vec::new();

    let n = container.components.len();
    let cols = grid_cols(n);
    let rows = n.div_ceil(cols);
    let cols_i = i(cols);
    let rows_i = i(rows);
    let width = PAD * 2 + cols_i * NODE_W + (cols_i - 1) * GAP_X;
    let height = PAD * 2 + rows_i * NODE_H + (rows_i - 1) * GAP_Y + BOUNDARY_LABEL_H;
    cells.push(boundary(
        &container.id,
        "ContainerScopeBoundary",
        &container.name,
        MARGIN,
        MARGIN,
        width,
        height,
    ));
    let mine: BTreeSet<&str> = container.components.iter().map(|c| c.id.as_str()).collect();
    for (idx_, comp) in container.components.iter().enumerate() {
        let col = i(idx_ % cols);
        let row = i(idx_ / cols);
        let cx = MARGIN + PAD + col * (NODE_W + GAP_X);
        let cy = MARGIN + PAD + row * (NODE_H + GAP_Y);
        cells.push(vertex(
            &comp.id,
            Shape::Component,
            &comp.name,
            &comp.description,
            cx,
            cy,
        ));
    }

    // Relationships touching this container's components. The far end is
    // lifted to container level when it is a component of another container.
    let mut externals: Vec<&str> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for rel in &model.relationships {
        let from_in = mine.contains(rel.from.as_str());
        let to_in = mine.contains(rel.to.as_str());
        if !from_in && !to_in {
            continue;
        }
        let from = if from_in {
            rel.from.as_str()
        } else {
            idx.container_level(&rel.from)
        };
        let to = if to_in {
            rel.to.as_str()
        } else {
            idx.container_level(&rel.to)
        };
        if from == to || !idx.shape.contains_key(from) || !idx.shape.contains_key(to) {
            continue;
        }
        for ext in [from, to] {
            if !mine.contains(ext) && ext != container_id && !externals.contains(&ext) {
                externals.push(ext);
            }
        }
        if seen.insert((from, to)) {
            edges.push(edge(from, to, &rel.description));
        }
    }
    let ext_x = MARGIN + width + GAP_X * 2;
    let mut ext_y = MARGIN;
    for ext in externals {
        let shape = idx.shape[ext];
        cells.push(vertex(
            ext,
            shape,
            idx.name[ext],
            idx.description[ext],
            ext_x,
            ext_y,
        ));
        ext_y += shape.size().1 + GAP_Y;
    }
    cells.extend(edges);

    Page {
        id: format!("c4-components-{}", safe_id(container_id)),
        name: format!("Components: {}", container.name),
        cells,
    }
}

// ---------------------------------------------------------------------------
// Existing-file model (merge)
// ---------------------------------------------------------------------------

/// A page of an existing file, kept as raw XML so untouched content
/// round-trips byte-for-byte.
#[derive(Debug, Clone)]
struct RawPage {
    id: String,
    /// Whitespace between the previous page (or the `<mxfile>` tag) and this
    /// page's `<diagram`, kept so indentation survives the round-trip.
    gap: String,
    /// The full `<diagram …>…</diagram>` element, with the page body
    /// decompressed to plain `<mxGraphModel>` XML.
    xml: String,
}

impl RawPage {
    fn cell_ids(&self) -> BTreeSet<String> {
        // Any element carrying an id="…" attribute: mxCell, object, UserObject.
        let re = cell_id_regex();
        re.captures_iter(&self.xml)
            .map(|c| c[1].to_string())
            .collect()
    }

    /// Bottom-most edge of any absolutely positioned geometry on the page.
    fn bottom(&self) -> i32 {
        let re = geometry_regex();
        re.captures_iter(&self.xml)
            .filter_map(|c| {
                let attrs = &c[1];
                let y = attr_num(attrs, "y")?;
                let h = attr_num(attrs, "height").unwrap_or(0.0);
                #[allow(clippy::cast_possible_truncation)]
                Some((y + h).ceil() as i32)
            })
            .max()
            .unwrap_or(MARGIN)
    }

    fn insert_before_root_end(&mut self, cells: &str) {
        if let Some(pos) = self.xml.rfind("</root>") {
            self.xml.insert_str(pos, cells);
        } else if let Some(pos) = self.xml.rfind("</mxGraphModel>") {
            // Degenerate page without <root>; still keep the XML well-formed.
            self.xml
                .insert_str(pos, &format!("<root>\n{cells}      </root>\n"));
        }
    }
}

fn cell_id_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r#"<(?:mxCell|object|UserObject)\b[^>]*?\sid="([^"]*)""#)
            .expect("valid regex")
    })
}

fn geometry_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"<mxGeometry\b([^>]*)>").expect("valid regex"))
}

fn attr_num(attrs: &str, name: &str) -> Option<f64> {
    let needle = format!(" {name}=\"");
    let start = attrs.find(&needle)? + needle.len();
    let end = attrs[start..].find('"')? + start;
    attrs[start..end].trim().parse().ok()
}

/// A parsed `mxfile`: the opening tag verbatim, its pages, and any trailing
/// text after the last page.
#[derive(Debug, Clone)]
struct MxFile {
    /// Everything before the first `<diagram` (the `<mxfile …>` tag and
    /// whitespace).
    head: String,
    pages: Vec<RawPage>,
    /// Everything after the last `</diagram>` (usually `</mxfile>`).
    tail: String,
}

impl MxFile {
    fn parse(text: &str) -> Option<Self> {
        let mxfile_start = text.find("<mxfile")?;
        let first_diagram = text[mxfile_start..].find("<diagram")? + mxfile_start;
        let mut pages = Vec::new();
        let mut cursor = first_diagram;
        let mut last_end = first_diagram;
        while let Some(rel) = text[cursor..].find("<diagram") {
            let start = cursor + rel;
            let end_rel = text[start..].find("</diagram>")?;
            let end = start + end_rel + "</diagram>".len();
            let raw = &text[start..end];
            let id = attr_value(raw, "id").unwrap_or_default();
            pages.push(RawPage {
                id,
                // The first page's leading whitespace is part of `head`.
                gap: text[last_end..start].to_string(),
                xml: decompress_page(raw),
            });
            cursor = end;
            last_end = end;
        }
        Some(MxFile {
            head: text[..first_diagram].to_string(),
            pages,
            tail: text[last_end..].to_string(),
        })
    }

    /// Whitespace to put in front of an appended page: whatever separates the
    /// existing pages, else the indentation the first page had after the
    /// `<mxfile>` tag.
    fn page_gap(&self) -> String {
        if let Some(gap) = self
            .pages
            .iter()
            .rev()
            .map(|p| &p.gap)
            .find(|g| !g.is_empty())
        {
            return gap.clone();
        }
        let after_tag = self.head.rfind('>').map_or(0, |i| i + 1);
        let indent = &self.head[after_tag..];
        if indent.trim().is_empty() && !indent.is_empty() {
            indent.to_string()
        } else {
            "\n  ".to_string()
        }
    }

    fn append_page(&mut self, page: &Page) {
        let gap = self.page_gap();
        self.pages.push(RawPage {
            id: page.id.clone(),
            gap,
            xml: page.to_xml().trim().to_string(),
        });
    }

    fn to_xml(&self) -> String {
        let mut out = self.head.clone();
        // A file we decompressed must not claim to be compressed any more;
        // draw.io reads either form, so only flip the attribute when present.
        out = out.replace("compressed=\"true\"", "compressed=\"false\"");
        for page in &self.pages {
            out.push_str(&page.gap);
            out.push_str(&page.xml);
        }
        out.push_str(&self.tail);
        out
    }
}

/// First `name="…"` attribute value inside the opening tag of `element`.
fn attr_value(element: &str, name: &str) -> Option<String> {
    let tag_end = element.find('>')?;
    let tag = &element[..tag_end];
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// If `diagram` (a full `<diagram …>…</diagram>` element) carries a compressed
/// body, replace it with the decoded `<mxGraphModel>` XML; otherwise return it
/// unchanged. Bodies that fail to decode are left alone (merging then simply
/// appends, which is still a valid file).
fn decompress_page(diagram: &str) -> String {
    let Some(open_end) = diagram.find('>') else {
        return diagram.to_string();
    };
    let Some(close) = diagram.rfind("</diagram>") else {
        return diagram.to_string();
    };
    let body = &diagram[open_end + 1..close];
    if body.contains("<mxGraphModel") {
        return diagram.to_string();
    }
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return diagram.to_string();
    }
    match decode_compressed(&compact) {
        Some(xml) if xml.contains("<mxGraphModel") => {
            format!(
                "{}\n    {}\n  </diagram>",
                &diagram[..=open_end],
                xml.trim()
            )
        }
        _ => diagram.to_string(),
    }
}

/// draw.io's classic page encoding: `base64(deflate-raw(percent-encode(xml)))`.
fn decode_compressed(b64: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let mut inflated = String::new();
    flate2::read::DeflateDecoder::new(bytes.as_slice())
        .read_to_string(&mut inflated)
        .ok()?;
    Some(percent_decode(&inflated))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escape a string for use inside a double-quoted XML attribute.
fn xml_attr(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c4_dsl::{C4Component, C4Container, C4Person, C4Relationship, C4System};
    use std::io::Write as _;

    fn sample_model() -> C4Model {
        C4Model {
            persons: vec![C4Person {
                id: "developer".into(),
                name: "Developer".into(),
                description: "Browses architecture".into(),
            }],
            systems: vec![
                C4System {
                    id: "shop".into(),
                    name: "Shop".into(),
                    description: "The shop".into(),
                    containers: vec![
                        C4Container {
                            id: "web".into(),
                            name: "web".into(),
                            description: "3 classes".into(),
                            components: vec![
                                C4Component {
                                    id: "web_CartController".into(),
                                    name: "CartController".into(),
                                    description: "rest-controller".into(),
                                },
                                C4Component {
                                    id: "web_CartView".into(),
                                    name: "CartView".into(),
                                    description: "controller".into(),
                                },
                            ],
                        },
                        C4Container {
                            id: "core".into(),
                            name: "core".into(),
                            description: "12 classes".into(),
                            components: vec![C4Component {
                                id: "core_CartService".into(),
                                name: "CartService".into(),
                                description: "service".into(),
                            }],
                        },
                        C4Container {
                            id: "db".into(),
                            name: "db \"quoted\" & <odd>".into(),
                            description: "1 class".into(),
                            components: vec![],
                        },
                    ],
                },
                C4System {
                    id: "payment".into(),
                    name: "Payment API".into(),
                    description: "external".into(),
                    containers: vec![],
                },
            ],
            relationships: vec![
                C4Relationship {
                    from: "developer".into(),
                    to: "web".into(),
                    description: "explores".into(),
                },
                C4Relationship {
                    from: "web".into(),
                    to: "core".into(),
                    description: "uses".into(),
                },
                C4Relationship {
                    from: "web_CartController".into(),
                    to: "core_CartService".into(),
                    description: "calls".into(),
                },
                C4Relationship {
                    from: "web_CartController".into(),
                    to: "web_CartView".into(),
                    description: "renders".into(),
                },
                C4Relationship {
                    from: "core".into(),
                    to: "payment".into(),
                    description: "charges".into(),
                },
                C4Relationship {
                    from: "core".into(),
                    to: "ghost".into(),
                    description: "dangling".into(),
                },
            ],
        }
    }

    #[test]
    fn export_has_container_page_and_one_component_page_per_container_with_components() {
        let xml = c4_model_to_drawio(&sample_model());
        assert!(xml.starts_with("<mxfile "));
        assert!(xml.contains("<diagram id=\"c4-containers\" name=\"Containers\">"));
        assert!(xml.contains("<diagram id=\"c4-components-web\" name=\"Components: web\">"));
        assert!(xml.contains("<diagram id=\"c4-components-core\" name=\"Components: core\">"));
        // `db` has no components → no page.
        assert!(!xml.contains("c4-components-db"));
        assert_eq!(xml.matches("<diagram ").count(), 3);
    }

    #[test]
    fn export_uses_c4_shapes_and_identity_metadata() {
        let xml = c4_model_to_drawio(&sample_model());
        assert!(xml.contains("shape=mxgraph.c4.person2"));
        assert!(xml.contains("c4Type=\"Container\""));
        assert!(xml.contains("c4Type=\"Component\""));
        assert!(xml.contains("c4Type=\"SystemScopeBoundary\""));
        assert!(xml.contains("c4Type=\"ContainerScopeBoundary\""));
        assert!(xml.contains("c4Type=\"Software System\""));
        assert!(xml.contains("pmId=\"web\" pmKind=\"container\" id=\"pm-web\""));
        assert!(xml.contains("pmId=\"shop\" pmKind=\"boundary\" id=\"pm-boundary-shop\""));
        assert!(
            xml.contains("pmId=\"web-&gt;core\" pmKind=\"relationship\" id=\"pm-rel-web-core\"")
        );
        assert!(xml.contains("source=\"pm-web\" target=\"pm-core\""));
        assert!(xml.contains("placeholders=\"1\""));
    }

    #[test]
    fn export_escapes_attribute_values() {
        let xml = c4_model_to_drawio(&sample_model());
        assert!(xml.contains("c4Name=\"db &quot;quoted&quot; &amp; &lt;odd&gt;\""));
        assert!(!xml.contains("c4Name=\"db \"quoted\""));
    }

    #[test]
    fn container_page_lifts_component_relationships_and_drops_dangling_and_self_edges() {
        let xml = c4_model_to_drawio(&sample_model());
        let containers = page(&xml, "c4-containers");
        // web_CartController -> core_CartService lifts to web -> core, which
        // already exists: exactly one edge.
        assert_eq!(containers.matches("id=\"pm-rel-web-core\"").count(), 1);
        // Intra-container component edge lifts to web -> web: dropped.
        assert!(!containers.contains("pm-rel-web-web"));
        // Dangling target never becomes an edge.
        assert!(!containers.contains("ghost"));
        // External system drawn as a plain box, with its edge.
        assert!(containers.contains("id=\"pm-payment\""));
        assert!(containers.contains("id=\"pm-rel-core-payment\""));
    }

    #[test]
    fn component_page_shows_components_externals_and_their_edges() {
        let xml = c4_model_to_drawio(&sample_model());
        let web = page(&xml, "c4-components-web");
        assert!(web.contains("id=\"pm-web_CartController\""));
        assert!(web.contains("id=\"pm-web_CartView\""));
        assert!(web.contains("id=\"pm-rel-web_CartController-web_CartView\""));
        // Far end core_CartService lifts to its container `core`, drawn as an
        // external Container box on this page.
        assert!(web.contains("pmId=\"core\" pmKind=\"container\" id=\"pm-core\""));
        assert!(web.contains("id=\"pm-rel-web_CartController-core\""));
        // Container-level edges that don't touch a component stay off the page.
        assert!(!web.contains("pm-rel-core-payment"));
        assert!(!web.contains("id=\"pm-developer\""));
    }

    #[test]
    fn export_is_deterministic() {
        let a = c4_model_to_drawio(&sample_model());
        let b = c4_model_to_drawio(&sample_model());
        assert_eq!(a, b);
    }

    #[test]
    fn empty_model_still_yields_a_valid_file() {
        let xml = c4_model_to_drawio(&C4Model::default());
        assert!(xml.contains("<diagram id=\"c4-containers\""));
        assert!(xml.contains("<mxCell id=\"0\"/>"));
        assert!(xml.ends_with("</mxfile>\n"));
    }

    #[test]
    fn merge_keeps_existing_cells_byte_identical_and_adds_missing_ones() {
        let mut small = sample_model();
        // Start from a model without `db` and without the payment system.
        small.systems[0].containers.retain(|c| c.id != "db");
        small.systems.retain(|s| s.id != "payment");
        small.relationships.retain(|r| r.to != "payment");
        let existing = c4_model_to_drawio(&small);
        // Simulate a user layout tweak: move `web` somewhere else.
        let existing = move_cell(&existing, "pm-web", 999, 1234);
        assert!(
            existing.contains("x=\"999\" y=\"1234\""),
            "fixture must have moved web"
        );

        let merged = merge_c4_drawio(&existing, &sample_model());
        // db, payment, core->payment edge on the container page.
        assert!(merged.added >= 3);
        assert!(merged.kept > 0);
        assert_eq!(merged.pages, 3);
        assert!(
            merged.xml.contains("x=\"999\" y=\"1234\""),
            "user layout preserved"
        );
        assert!(merged.xml.contains("id=\"pm-db\""));
        assert!(merged.xml.contains("id=\"pm-payment\""));
        assert!(merged.xml.contains("id=\"pm-rel-core-payment\""));
        // Every original line survives verbatim.
        for line in existing.lines() {
            assert!(merged.xml.contains(line), "lost line: {line}");
        }
        // New vertices land below the existing drawing (y > old bottom).
        let db_pos = merged.xml.find("id=\"pm-db\"").unwrap();
        let geom = &merged.xml[db_pos..];
        let y = attr_num(
            &geom[geom.find("<mxGeometry").unwrap()..geom.find("as=\"geometry\"").unwrap()],
            "y",
        )
        .unwrap();
        assert!(
            y > 1234.0,
            "new cell placed below existing bottom, got y={y}"
        );
    }

    #[test]
    fn merge_is_idempotent_and_leaves_complete_files_untouched() {
        let xml = c4_model_to_drawio(&sample_model());
        let merged = merge_c4_drawio(&xml, &sample_model());
        assert_eq!(merged.added, 0);
        assert_eq!(merged.xml, xml);
        assert_eq!(merged.pages, 3);
    }

    #[test]
    fn merge_appends_whole_page_when_container_gained_components() {
        let mut model = sample_model();
        model.systems[0].containers[2].components.push(C4Component {
            id: "db_Schema".into(),
            name: "Schema".into(),
            description: "class".into(),
        });
        let existing = c4_model_to_drawio(&sample_model());
        let merged = merge_c4_drawio(&existing, &model);
        assert_eq!(merged.pages, 4);
        assert!(merged.xml.contains("<diagram id=\"c4-components-db\""));
        assert!(merged.xml.trim_end().ends_with("</mxfile>"));
    }

    #[test]
    fn merge_decompresses_legacy_compressed_pages() {
        // Build a compressed page the way draw.io does: percent-encode →
        // raw deflate → base64.
        let fresh = c4_model_to_drawio(&sample_model());
        let inner_start = fresh.find("<mxGraphModel").unwrap();
        let inner_end = fresh.find("</mxGraphModel>").unwrap() + "</mxGraphModel>".len();
        let inner = &fresh[inner_start..inner_end];
        let encoded: String = inner
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(encoded.as_bytes()).unwrap();
        let deflated = enc.finish().unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(deflated);
        let compressed = format!(
            "<mxfile host=\"app.diagrams.net\" compressed=\"true\">\n  <diagram id=\"c4-containers\" name=\"Containers\">{b64}</diagram>\n</mxfile>\n"
        );

        let merged = merge_c4_drawio(&compressed, &sample_model());
        // Container page was complete; component pages are new.
        assert_eq!(merged.pages, 3);
        assert!(merged.added > 0);
        assert!(merged.xml.contains("compressed=\"false\""));
        assert!(merged.xml.contains("<mxGraphModel"));
        assert!(!merged.xml.contains(&b64));
        // Existing cells recognised through the compression → not duplicated
        // on the container page (the `core` component page legitimately draws
        // `web` again as an external box).
        assert_eq!(
            page(&merged.xml, "c4-containers")
                .matches("id=\"pm-web\"")
                .count(),
            1
        );
    }

    #[test]
    fn merge_returns_non_mxfile_input_unchanged() {
        let merged = merge_c4_drawio("not xml at all", &sample_model());
        assert_eq!(merged.added, 0);
        assert_eq!(merged.xml, "not xml at all");
    }

    #[test]
    fn save_drawio_guards_extension_repo_boundary_and_payload() {
        let dir = scratch_dir("save");
        let root = dir.as_path();
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let target = docs.join("architecture.drawio");
        std::fs::write(&target, "<mxfile></mxfile>").unwrap();

        // Wrong extension.
        let md = docs.join("notes.md");
        std::fs::write(&md, "x").unwrap();
        assert!(matches!(
            save_drawio(root, &md, "<mxfile/>"),
            Err(SaveDrawioError::NotDrawio(_))
        ));

        // Outside the repo.
        let outside = scratch_dir("save-outside");
        let foreign = outside.join("x.drawio");
        std::fs::write(&foreign, "<mxfile/>").unwrap();
        assert!(matches!(
            save_drawio(root, &foreign, "<mxfile/>"),
            Err(SaveDrawioError::Access(FileAccessError::OutsideRepo { .. }))
        ));

        // Not a diagram.
        assert!(matches!(
            save_drawio(root, &target, "<html>nope</html>"),
            Err(SaveDrawioError::NotADiagram)
        ));

        // Happy path, with an XML prolog.
        let payload = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mxfile host=\"x\"><diagram id=\"a\" name=\"A\"><mxGraphModel><root><mxCell id=\"0\"/></root></mxGraphModel></diagram></mxfile>";
        let saved = save_drawio(root, &target, payload).unwrap();
        assert_eq!(saved.bytes, payload.len());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), payload);
        assert!(!docs.join("architecture.drawio.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn export_creates_then_merges_on_disk() {
        use projectmind_plugin_api::{
            Class, FrameworkPlugin, Module, PluginInfo, Relation, Result as PiResult,
        };
        struct NoRel;
        impl FrameworkPlugin for NoRel {
            fn info(&self) -> PluginInfo {
                PluginInfo {
                    id: "norel",
                    name: "NoRel",
                    version: "0",
                }
            }
            fn supported_languages(&self) -> &[&'static str] {
                &["lang-java"]
            }
            fn enrich(&self, _m: &mut Module) -> PiResult<()> {
                Ok(())
            }
            fn relations(&self, _m: &Module) -> Vec<Relation> {
                Vec::new()
            }
        }
        let dir = scratch_dir("export");
        let mut modules = BTreeMap::new();
        let mut classes = BTreeMap::new();
        classes.insert(
            "a.Foo".to_string(),
            Class {
                fqn: "a.Foo".into(),
                name: "Foo".into(),
                ..Default::default()
            },
        );
        modules.insert(
            "g:app".to_string(),
            Module {
                id: "g:app".into(),
                name: "app".into(),
                root: dir.clone(),
                classes,
            },
        );
        let repo = Repository {
            root: dir.clone(),
            modules,
        };

        let first = export_c4_drawio(&repo, &NoRel).unwrap();
        assert!(first.created);
        assert_eq!(first.path, dir.join(C4_DRAWIO_REL_PATH));
        assert!(first.path.exists());
        assert_eq!(first.pages, 2); // containers + one component page

        let second = export_c4_drawio(&repo, &NoRel).unwrap();
        assert!(!second.created);
        assert_eq!(second.added, 0);
        assert!(second.kept > 0);

        // A hand-written DSL takes precedence over the generated model.
        std::fs::write(
            dir.join(c4_dsl::C4_MODEL_REL_PATH),
            "workspace {\n model {\n  ops = person \"Ops\" \"runs it\"\n  s = softwareSystem \"S\" {\n   g_app = container \"app\" \"1 class\"\n   billing = container \"billing\" \"external\"\n  }\n  ops -> billing \"pays\"\n }\n}\n",
        )
        .unwrap();
        let third = export_c4_drawio(&repo, &NoRel).unwrap();
        assert!(!third.created);
        assert!(third.added >= 3, "ops, billing and the edge: {third:?}");
        let on_disk = std::fs::read_to_string(&third.path).unwrap();
        assert!(on_disk.contains("id=\"pm-ops\""));
        assert!(on_disk.contains("id=\"pm-rel-ops-billing\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fresh scratch directory per test, following the crate's convention
    /// (`std::env::temp_dir()` + pid) — no tempfile dev-dependency.
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-c4-drawio-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Rewrite the geometry of the first cell with `id` to (`x`, `y`) — the
    /// user dragging a shape in draw.io.
    fn move_cell(xml: &str, id: &str, x: i32, y: i32) -> String {
        let cell = xml.find(&format!("id=\"{id}\"")).expect("cell present");
        let geo = xml[cell..].find("<mxGeometry x=\"").unwrap() + cell;
        let end = xml[geo..].find(" width=").unwrap() + geo;
        format!(
            "{}<mxGeometry x=\"{x}\" y=\"{y}\"{}",
            &xml[..geo],
            &xml[end..]
        )
    }

    /// Extract one `<diagram id="…">…</diagram>` block.
    fn page<'a>(xml: &'a str, id: &str) -> &'a str {
        let start = xml
            .find(&format!("<diagram id=\"{id}\""))
            .expect("page present");
        let end = xml[start..].find("</diagram>").unwrap() + start;
        &xml[start..end]
    }
}

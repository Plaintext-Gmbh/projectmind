// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Semantic tour lookup (Cockpit 2.5, #161).
//!
//! Curated walk-throughs are the AI's *preferred* code-navigation
//! interface: an architect builds a tour once ("auth flow", "request
//! lifecycle") and every future session queries it instead of grepping
//! the repo. A ten-step tour of ten lines each answers "explain X" in
//! ~500 lines where reading the underlying files would be 5000-20000.
//!
//! This module is the engine behind the `walkthrough_query` MCP tool:
//!
//! 1. **Indexing.** For every tour step, the searchable text is
//!    `title + narration + target fqn`. Those strings are embedded once
//!    (at [`open_repo`](crate::Engine::open_repo) time) into fixed-width
//!    vectors and persisted to `.projectmind/cache/tours.idx`.
//! 2. **Querying.** The question is embedded, cosine-ranked against every
//!    step vector, grouped by tour, and the best tour's top steps are
//!    returned with a `confidence` score.
//! 3. **Fallback.** When the best similarity is below
//!    [`FALLBACK_THRESHOLD`] (0.45) the query returns `fallback: "grep"`
//!    so the agent falls back to the existing search tools rather than
//!    trusting a weak semantic match.
//!
//! # Architecture: the [`Embedder`] seam
//!
//! The whole ranking core is written against the [`Embedder`] trait and
//! is **pure** — no model, no I/O, no global state. That makes it unit-
//! testable with an injected fake embedder that assigns deterministic
//! vectors, so the default test suite never needs a real model or the
//! network. The real embedder (`fastembed`, `all-MiniLM-L6-v2`) lives
//! behind the `embed` cargo feature; without it, `walkthrough_query`
//! degrades gracefully to `fallback: "grep"` and never crashes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::persistence::CONFIG_DIR;
use crate::walkthrough::{Walkthrough, WalkthroughTarget};

/// Similarity below which a query is judged too weak to trust and the
/// caller is told to `fallback: "grep"` (per #161).
pub const FALLBACK_THRESHOLD: f32 = 0.45;

/// On-disk format version for `tours.idx`. Bumped when the serialized
/// shape changes; a mismatch forces a rebuild rather than a mis-parse.
pub const INDEX_FORMAT_VERSION: u32 = 1;

/// File name of the persisted tour index inside `.projectmind/cache/`.
pub const INDEX_FILE: &str = "tours.idx";

/// Turns text into fixed-width embedding vectors.
///
/// The ranking core only ever talks to this trait, so tests can inject a
/// deterministic fake and production can plug in `fastembed` behind the
/// `embed` feature. Implementations must return one vector per input, in
/// order, each of length [`Embedder::dim`].
pub trait Embedder {
    /// Embed a batch of texts. The returned outer `Vec` has the same
    /// length and order as `texts`; each inner `Vec` has length
    /// [`Embedder::dim`].
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Dimensionality of the vectors this embedder produces. Stamped into
    /// the persisted index so a model swap (different `dim`) forces a
    /// rebuild instead of comparing incompatible vectors.
    fn dim(&self) -> usize;
}

/// One indexed step: its embedding plus the metadata a query answer needs.
///
/// The metadata is denormalised into the index so a query can be answered
/// without re-reading the tour body — `title`, `fqn`, `file`, `lines` and
/// `narration` are exactly the fields `walkthrough_query` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedStep {
    /// Id of the tour this step belongs to.
    pub tour_id: String,
    /// Title of the owning tour (for display / grouping).
    pub tour_title: String,
    /// 0-based index of the step within its tour.
    pub step: u32,
    /// Step title.
    pub title: String,
    /// Step narration (markdown, may be empty).
    pub narration: String,
    /// Fully-qualified name of the step's target, when it has one
    /// (`class` / `risk` targets). `None` for `note` / `atlas` / …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fqn: Option<String>,
    /// Repo-relative (or absolute, for `file` targets) path the step
    /// points at, when resolvable. `None` for narration-only steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based inclusive `[from, to]` line span, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<[u32; 2]>,
    /// The embedding of `title + narration + fqn`.
    pub vector: Vec<f32>,
}

/// The persisted, queryable tour index.
///
/// Serialized to `.projectmind/cache/tours.idx`. The `format_version` and
/// `dim` guard against loading an index written by an incompatible build
/// or a different embedding model — a mismatch triggers a rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourIndex {
    /// On-disk format version (see [`INDEX_FORMAT_VERSION`]).
    pub format_version: u32,
    /// Embedding dimensionality this index was built with.
    pub dim: usize,
    /// Every indexed step across every tour.
    pub steps: Vec<IndexedStep>,
}

impl TourIndex {
    /// Path of the persisted index for `repo_root`:
    /// `<repo>/.projectmind/cache/tours.idx`.
    #[must_use]
    pub fn index_path(repo_root: &Path) -> PathBuf {
        repo_root.join(CONFIG_DIR).join("cache").join(INDEX_FILE)
    }

    /// Build an index from a set of tours using `embedder`.
    ///
    /// The searchable text of a step is `title + narration + fqn` (see
    /// [`step_text`]). Steps with no text at all (empty title, no
    /// narration, no fqn) are skipped — they can never match a query and
    /// would only add a zero vector.
    pub fn build(tours: &[Walkthrough], embedder: &dyn Embedder) -> anyhow::Result<Self> {
        let mut metas: Vec<IndexedStep> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        for tour in tours {
            for (i, step) in tour.steps.iter().enumerate() {
                let (fqn, file, lines) = target_locator(&step.target);
                let text = step_text(&step.title, &step.narration, fqn.as_deref());
                if text.trim().is_empty() {
                    continue;
                }
                texts.push(text);
                metas.push(IndexedStep {
                    tour_id: tour.id.clone(),
                    tour_title: tour.title.clone(),
                    step: i as u32,
                    title: step.title.clone(),
                    narration: step.narration.clone(),
                    fqn,
                    file,
                    lines,
                    vector: Vec::new(),
                });
            }
        }
        let vectors = if texts.is_empty() {
            Vec::new()
        } else {
            embedder.embed(&texts)?
        };
        for (meta, vector) in metas.iter_mut().zip(vectors) {
            meta.vector = vector;
        }
        Ok(Self {
            format_version: INDEX_FORMAT_VERSION,
            dim: embedder.dim(),
            steps: metas,
        })
    }

    /// Build an index from `tours` and persist it to
    /// [`TourIndex::index_path`]. Returns the built index. Best-effort
    /// writer: the caller decides whether a write failure is fatal (it
    /// isn't for `open_repo`).
    pub fn build_and_save(
        repo_root: &Path,
        tours: &[Walkthrough],
        embedder: &dyn Embedder,
    ) -> anyhow::Result<Self> {
        let index = Self::build(tours, embedder)?;
        index.save(repo_root)?;
        Ok(index)
    }

    /// Persist the index atomically to `.projectmind/cache/tours.idx`.
    pub fn save(&self, repo_root: &Path) -> anyhow::Result<()> {
        let path = Self::index_path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(self)?;
        let tmp = path.with_extension("idx.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Load the persisted index for `repo_root`, if present and usable.
    ///
    /// Returns `Ok(None)` when the file is absent. A file that exists but
    /// is unparseable, has a wrong `format_version`, or was built with a
    /// different embedding `dim` than `expected_dim` is treated as stale
    /// (also `Ok(None)`): the caller rebuilds rather than trusting a
    /// mismatched index.
    pub fn load(repo_root: &Path, expected_dim: usize) -> anyhow::Result<Option<Self>> {
        let path = Self::index_path(repo_root);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let index: Self = match serde_json::from_slice(&bytes) {
            Ok(i) => i,
            // Corrupt / older-shape index: treat as absent, rebuild.
            Err(_) => return Ok(None),
        };
        if index.format_version != INDEX_FORMAT_VERSION || index.dim != expected_dim {
            return Ok(None);
        }
        Ok(Some(index))
    }

    /// Answer a `walkthrough_query`: rank steps against `question`, group
    /// by tour, and return the best tour's top steps.
    ///
    /// `prefer_tours` biases matching towards the named tour ids (a small
    /// additive nudge, never overriding a clearly better match). `top_k`
    /// caps how many steps come back for the winning tour.
    ///
    /// This is the **pure ranking core**: it takes an [`Embedder`] so it
    /// can be exercised with a fake embedder in tests, and it never
    /// touches disk. When the best similarity is below
    /// [`FALLBACK_THRESHOLD`] the result's `fallback` is `Some("grep")`.
    pub fn query(
        &self,
        question: &str,
        prefer_tours: &[String],
        top_k: usize,
        embedder: &dyn Embedder,
    ) -> anyhow::Result<QueryResult> {
        if self.steps.is_empty() {
            return Ok(QueryResult::fallback());
        }
        let qvec = embedder
            .embed(std::slice::from_ref(&question.to_string()))?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("embedder returned no vector for the query"))?;

        // Score every step; a small preference bonus nudges (but cannot
        // dominate) tours the caller asked to bias toward.
        let mut scored: Vec<(usize, f32)> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, step)| {
                let mut sim = cosine(&qvec, &step.vector);
                if prefer_tours.iter().any(|t| t == &step.tour_id) {
                    sim += PREFER_BONUS;
                }
                (i, sim)
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));

        // The winning tour is the one owning the single best-scoring step.
        let (best_idx, best_sim) = scored[0];
        let winning_tour = self.steps[best_idx].tour_id.clone();

        // Collect that tour's steps in score order, dedup by step index,
        // capped at `top_k`. Carry the tour-order step index alongside so
        // the final ordering is unambiguous even if two steps share a title.
        let mut seen: Vec<u32> = Vec::new();
        let mut picked: Vec<(u32, QueryStep)> = Vec::new();
        for (idx, sim) in &scored {
            let step = &self.steps[*idx];
            if step.tour_id != winning_tour {
                continue;
            }
            if seen.contains(&step.step) {
                continue;
            }
            seen.push(step.step);
            picked.push((
                step.step,
                QueryStep {
                    title: step.title.clone(),
                    fqn: step.fqn.clone(),
                    file: step.file.clone(),
                    lines: step.lines,
                    narration: step.narration.clone(),
                    score: clamp01(*sim),
                },
            ));
            if picked.len() >= top_k {
                break;
            }
        }
        // Present steps in tour order, not score order — a tour reads best
        // front-to-back.
        picked.sort_by_key(|(order, _)| *order);
        let steps: Vec<QueryStep> = picked.into_iter().map(|(_, s)| s).collect();

        let confidence = clamp01(best_sim);
        let fallback = if best_sim < FALLBACK_THRESHOLD {
            Some("grep".to_string())
        } else {
            None
        };
        Ok(QueryResult {
            tour_id: Some(winning_tour),
            steps,
            confidence,
            fallback,
        })
    }
}

/// Additive similarity bonus for a step whose tour was named in
/// `prefer_tours`. Small on purpose: a hint, not an override.
const PREFER_BONUS: f32 = 0.05;

/// Collect the tours available to index for a repo.
///
/// Today ProjectMind keeps a single *active* walk-through (the one an
/// author most recently built via `walkthrough_start`), stored next to
/// the statefile — there is no on-disk tour *library* yet. So the indexed
/// corpus is "the active tour, if any". This function is the single seam
/// a future tour library would extend: return more `Walkthrough`s here and
/// the whole index / query path picks them up unchanged.
#[must_use]
pub fn available_tours() -> Vec<Walkthrough> {
    match crate::walkthrough::read_body() {
        Ok(Some(body)) => vec![body],
        _ => Vec::new(),
    }
}

/// Build the tour index for `repo_root` and persist it — best effort.
///
/// Called at [`open_repo`](crate::Engine::open_repo) time. Without the
/// `embed` feature (no local model) this is a no-op returning `Ok(None)`.
/// Any failure (no model, no network, unwritable cache) is swallowed into
/// `Ok(None)` / a logged warning: indexing tours must never block opening
/// a repo, exactly like the code-graph cache.
pub fn build_index_on_open(repo_root: &Path) -> anyhow::Result<Option<TourIndex>> {
    let Some(embedder) = crate::tour_embed::default_embedder()? else {
        return Ok(None);
    };
    let tours = available_tours();
    let index = TourIndex::build_and_save(repo_root, &tours, embedder.as_ref())?;
    Ok(Some(index))
}

/// Answer a `walkthrough_query` for `repo_root`.
///
/// This is the runtime entry point the MCP tool / Tauri command / browser
/// route all call. It:
///
/// 1. Picks the [`Embedder`]. Without the `embed` feature (or if the model
///    can't load) there is none, and the answer is
///    [`QueryResult::fallback`] — the agent falls back to grep.
/// 2. Loads the persisted index, rebuilding it from
///    [`available_tours`] when it's missing, stale, or built for a
///    different embedding `dim`.
/// 3. Runs the pure [`TourIndex::query`] ranking core.
pub fn query_repo(
    repo_root: &Path,
    question: &str,
    prefer_tours: &[String],
    top_k: usize,
) -> anyhow::Result<QueryResult> {
    // No model available (default build) or the model failed to load
    // (offline): grep is the honest answer, never an error to the AI.
    let Ok(Some(embedder)) = crate::tour_embed::default_embedder() else {
        return Ok(QueryResult::fallback());
    };
    let index = match TourIndex::load(repo_root, embedder.dim())? {
        Some(index) => index,
        None => {
            let tours = available_tours();
            TourIndex::build_and_save(repo_root, &tours, embedder.as_ref())?
        }
    };
    index.query(question, prefer_tours, top_k.max(1), embedder.as_ref())
}

/// Answer to a `walkthrough_query`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryResult {
    /// Winning tour id, or `None` when nothing was indexed / matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tour_id: Option<String>,
    /// Best steps of the winning tour, in tour order.
    pub steps: Vec<QueryStep>,
    /// Max similarity found, clamped to `0.0..=1.0`.
    pub confidence: f32,
    /// `Some("grep")` when the match is too weak to trust (or nothing was
    /// indexed) and the agent should fall back to search; `None` when the
    /// answer is good.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

impl QueryResult {
    /// A "nothing useful here, go grep" answer: no tour, no steps, zero
    /// confidence, `fallback: "grep"`.
    #[must_use]
    pub fn fallback() -> Self {
        Self {
            tour_id: None,
            steps: Vec::new(),
            confidence: 0.0,
            fallback: Some("grep".to_string()),
        }
    }
}

/// One step in a [`QueryResult`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryStep {
    /// Step title.
    pub title: String,
    /// Target fully-qualified name, when the step has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fqn: Option<String>,
    /// Target file path, when the step points at one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 1-based inclusive line span `[from, to]`, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<[u32; 2]>,
    /// Step narration.
    pub narration: String,
    /// This step's similarity to the question, clamped to `0.0..=1.0`.
    pub score: f32,
}

/// The searchable text of a step: `title + narration + fqn`, joined by
/// newlines and skipping empty parts. This is the exact recipe #161
/// specifies, and it is what an author should keep in mind when writing
/// tours that answer well.
#[must_use]
pub fn step_text(title: &str, narration: &str, fqn: Option<&str>) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !title.trim().is_empty() {
        parts.push(title.trim());
    }
    if !narration.trim().is_empty() {
        parts.push(narration.trim());
    }
    if let Some(f) = fqn {
        if !f.trim().is_empty() {
            parts.push(f.trim());
        }
    }
    parts.join("\n")
}

/// Pull the `(fqn, file, lines)` locator out of a step target, when it
/// carries one. Only the shapes that name a concrete code location
/// contribute; narration-only and diagram targets return `None`s.
fn target_locator(
    target: &WalkthroughTarget,
) -> (Option<String>, Option<String>, Option<[u32; 2]>) {
    match target {
        WalkthroughTarget::Class { fqn, highlight } => {
            let lines = highlight.first().map(|r| [r.from, r.to.max(r.from)]);
            (Some(fqn.clone()), None, lines)
        }
        WalkthroughTarget::File {
            path, highlight, ..
        } => {
            let lines = highlight.first().map(|r| [r.from, r.to.max(r.from)]);
            (None, Some(path.to_string_lossy().into_owned()), lines)
        }
        WalkthroughTarget::Risk { fqn, .. } => (Some(fqn.clone()), None, None),
        WalkthroughTarget::Diff { .. }
        | WalkthroughTarget::Artifact { .. }
        | WalkthroughTarget::Pattern { .. }
        | WalkthroughTarget::Atlas { .. }
        | WalkthroughTarget::Note => (None, None, None),
    }
}

/// Cosine similarity of two equal-length vectors. Returns `0.0` when the
/// lengths differ or either vector has zero magnitude — both are "no
/// signal" rather than an error, keeping the ranking total.
#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Clamp a similarity into the reported `0.0..=1.0` band. Cosine of two
/// non-negative bag-of-words vectors is already in range, but a real
/// model can emit slightly-negative dot products; clamping keeps the
/// public `confidence` / `score` fields well-behaved.
fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walkthrough::{LineRange, WalkthroughStep};

    /// Deterministic bag-of-words embedder for tests. Each distinct
    /// lowercase word gets a stable dimension (hashed into `DIM` buckets);
    /// a text's vector counts its words. Cosine similarity then measures
    /// word overlap — enough for "login" to score the auth tour highest
    /// without any model or network.
    struct FakeEmbedder {
        dim: usize,
    }

    impl FakeEmbedder {
        fn new() -> Self {
            Self { dim: 64 }
        }
    }

    impl Embedder for FakeEmbedder {
        fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0f32; self.dim];
                    for word in t
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty())
                    {
                        let w = word.to_ascii_lowercase();
                        let mut h: u64 = 14_695_981_039_346_656_037;
                        for b in w.bytes() {
                            h ^= u64::from(b);
                            h = h.wrapping_mul(1_099_511_628_211);
                        }
                        let idx = (h as usize) % self.dim;
                        v[idx] += 1.0;
                    }
                    v
                })
                .collect())
        }

        fn dim(&self) -> usize {
            self.dim
        }
    }

    fn class_step(title: &str, narration: &str, fqn: &str, from: u32, to: u32) -> WalkthroughStep {
        WalkthroughStep {
            title: title.into(),
            narration: narration.into(),
            target: WalkthroughTarget::Class {
                fqn: fqn.into(),
                highlight: vec![LineRange { from, to }],
            },
        }
    }

    fn note_step(title: &str, narration: &str) -> WalkthroughStep {
        WalkthroughStep {
            title: title.into(),
            narration: narration.into(),
            target: WalkthroughTarget::Note,
        }
    }

    fn tour(id: &str, title: &str, steps: Vec<WalkthroughStep>) -> Walkthrough {
        Walkthrough {
            schema_version: crate::walkthrough::CURRENT_SCHEMA_VERSION,
            id: id.into(),
            title: title.into(),
            summary: String::new(),
            steps,
            quiz: vec![],
            updated_at: 0,
        }
    }

    fn auth_tour() -> Walkthrough {
        tour(
            "auth-flow",
            "Authentication flow",
            vec![
                class_step(
                    "Login entry point",
                    "The user submits credentials to the login controller which validates the password and issues a session token.",
                    "com.example.auth.LoginController",
                    10,
                    40,
                ),
                class_step(
                    "Password verification",
                    "Hashed password comparison happens in the authentication service.",
                    "com.example.auth.AuthenticationService",
                    5,
                    60,
                ),
            ],
        )
    }

    fn billing_tour() -> Walkthrough {
        tour(
            "billing",
            "Billing and invoices",
            vec![
                class_step(
                    "Invoice generation",
                    "Monthly invoices are rendered as PDF from the billing aggregate.",
                    "com.example.billing.InvoiceRenderer",
                    1,
                    20,
                ),
                class_step(
                    "Payment capture",
                    "Credit card payment capture calls the external gateway.",
                    "com.example.billing.PaymentGateway",
                    30,
                    90,
                ),
            ],
        )
    }

    #[test]
    fn step_text_joins_present_parts_only() {
        assert_eq!(step_text("T", "N", Some("a.b.C")), "T\nN\na.b.C");
        assert_eq!(step_text("T", "", None), "T");
        assert_eq!(step_text("", "N", Some("")), "N");
        assert_eq!(step_text("  ", "", None), "");
    }

    #[test]
    fn cosine_of_identical_is_one_orthogonal_is_zero() {
        let a = vec![1.0, 0.0, 2.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // Mismatched length / empty → no signal, never a panic.
        assert!(cosine(&[1.0], &[1.0, 2.0]).abs() < 1e-6);
        assert!(cosine(&[], &[]).abs() < 1e-6);
    }

    #[test]
    fn build_skips_textless_steps_and_carries_locators() {
        let embedder = FakeEmbedder::new();
        // A note step with no title and no narration contributes nothing.
        let t = tour(
            "t",
            "T",
            vec![
                class_step("Has text", "narration", "a.b.C", 3, 9),
                note_step("", ""),
            ],
        );
        let index = TourIndex::build(&[t], &embedder).unwrap();
        assert_eq!(index.steps.len(), 1, "textless step is skipped");
        let s = &index.steps[0];
        assert_eq!(s.fqn.as_deref(), Some("a.b.C"));
        assert_eq!(s.lines, Some([3, 9]));
        assert_eq!(s.vector.len(), embedder.dim());
        assert_eq!(index.dim, embedder.dim());
        assert_eq!(index.format_version, INDEX_FORMAT_VERSION);
    }

    #[test]
    fn query_login_finds_the_auth_tour() {
        // The #161 integration criterion: a "login" question must select
        // the auth tour over the billing tour. Tour *selection* is the
        // contract — the absolute confidence depends on the embedder's
        // cosine calibration (a bag-of-words toy dilutes short queries),
        // so the strong-match / threshold behaviour is asserted separately
        // in `strong_match_clears_threshold`.
        let embedder = FakeEmbedder::new();
        let index = TourIndex::build(&[billing_tour(), auth_tour()], &embedder).unwrap();
        let result = index
            .query("how does user login and password work", &[], 5, &embedder)
            .unwrap();
        assert_eq!(result.tour_id.as_deref(), Some("auth-flow"));
        // Steps come back in tour order (step 0 before step 1), carrying
        // the fqn + line locators the answer exposes.
        assert_eq!(result.steps.len(), 2);
        assert_eq!(result.steps[0].title, "Login entry point");
        assert_eq!(
            result.steps[0].fqn.as_deref(),
            Some("com.example.auth.LoginController")
        );
        assert_eq!(result.steps[0].lines, Some([10, 40]));
    }

    #[test]
    fn strong_match_clears_threshold_no_fallback() {
        // A question that closely echoes a step's text scores above the
        // 0.45 threshold, so no grep fallback is emitted. This exercises
        // the `fallback == None` path deterministically.
        let embedder = FakeEmbedder::new();
        let index = TourIndex::build(&[auth_tour()], &embedder).unwrap();
        let result = index
            .query(
                "The user submits credentials to the login controller which validates the password and issues a session token.",
                &[],
                5,
                &embedder,
            )
            .unwrap();
        assert_eq!(result.tour_id.as_deref(), Some("auth-flow"));
        assert!(result.fallback.is_none(), "strong match → no grep fallback");
        assert!(result.confidence >= FALLBACK_THRESHOLD);
    }

    #[test]
    fn query_unrelated_question_falls_back_to_grep() {
        let embedder = FakeEmbedder::new();
        let index = TourIndex::build(&[auth_tour()], &embedder).unwrap();
        // Words that share nothing with the tour → cosine 0 → grep.
        let result = index
            .query("kubernetes helm chart deployment yaml", &[], 5, &embedder)
            .unwrap();
        assert_eq!(result.fallback.as_deref(), Some("grep"));
        assert!(result.confidence < FALLBACK_THRESHOLD);
    }

    #[test]
    fn empty_index_always_falls_back() {
        let embedder = FakeEmbedder::new();
        let index = TourIndex::build(&[], &embedder).unwrap();
        let result = index.query("anything", &[], 5, &embedder).unwrap();
        assert_eq!(result, QueryResult::fallback());
    }

    #[test]
    fn prefer_tours_biases_a_close_call() {
        // Two tours with overlapping vocabulary; the preference bonus
        // should pull the named tour ahead on an otherwise-close query.
        let embedder = FakeEmbedder::new();
        let a = tour(
            "alpha",
            "Alpha",
            vec![class_step(
                "Session token",
                "token session",
                "a.Token",
                1,
                2,
            )],
        );
        let b = tour(
            "beta",
            "Beta",
            vec![class_step(
                "Session token",
                "token session",
                "b.Token",
                1,
                2,
            )],
        );
        let index = TourIndex::build(&[a, b], &embedder).unwrap();
        let biased = index
            .query("session token", &["beta".to_string()], 5, &embedder)
            .unwrap();
        assert_eq!(biased.tour_id.as_deref(), Some("beta"));
    }

    #[test]
    fn top_k_caps_returned_steps() {
        let embedder = FakeEmbedder::new();
        let big = tour(
            "big",
            "Big",
            (0..10)
                .map(|i| {
                    class_step(
                        &format!("Step {i} login"),
                        "login",
                        &format!("a.C{i}"),
                        1,
                        2,
                    )
                })
                .collect(),
        );
        let index = TourIndex::build(&[big], &embedder).unwrap();
        let result = index.query("login", &[], 3, &embedder).unwrap();
        assert_eq!(result.steps.len(), 3, "top_k caps the step count");
    }

    #[test]
    fn save_load_round_trips_and_rejects_dim_mismatch() {
        let embedder = FakeEmbedder::new();
        let dir = std::env::temp_dir().join(format!(
            "projectmind-touridx-{}-{}",
            std::process::id(),
            "rt"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let index = TourIndex::build(&[auth_tour()], &embedder).unwrap();
        index.save(&dir).unwrap();
        assert!(TourIndex::index_path(&dir).exists());

        // Same dim → loads.
        let loaded = TourIndex::load(&dir, embedder.dim()).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().steps.len(), index.steps.len());

        // Different dim → treated as stale, caller rebuilds.
        let stale = TourIndex::load(&dir, embedder.dim() + 1).unwrap();
        assert!(stale.is_none(), "dim mismatch → rebuild");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_is_none() {
        let dir = std::env::temp_dir().join(format!(
            "projectmind-touridx-{}-missing",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(TourIndex::load(&dir, 64).unwrap().is_none());
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Embedder selection for the tour index (Cockpit 2.5, #161).
//!
//! The whole ranking core in [`crate::tour_index`] talks only to the
//! [`Embedder`](crate::tour_index::Embedder) trait. This module picks the
//! concrete embedder:
//!
//! - **Default build (no `embed` feature).** There is no local model, so
//!   [`default_embedder`] returns `None`. `walkthrough_query` then always
//!   answers `fallback: "grep"` — graceful, never a crash, and the MCP
//!   server binary stays slim.
//! - **`embed` feature.** [`default_embedder`] returns a
//!   [`FastEmbedEmbedder`] wrapping `fastembed` with `all-MiniLM-L6-v2`
//!   (a 384-dimensional, ~30MB model). The model is downloaded once on
//!   first use and cached by `fastembed`; if that download fails (offline
//!   sandbox, no network) construction returns an error and the caller
//!   falls back to grep rather than panicking.

use crate::tour_index::Embedder;

/// Embedding dimensionality of `all-MiniLM-L6-v2`, the model used behind
/// the `embed` feature. Exposed so the index-load path can ask "what dim
/// should the persisted index have?" without constructing a model.
pub const MINILM_DIM: usize = 384;

/// The best embedder this build can offer, if any.
///
/// - Without the `embed` feature: always `Ok(None)` — no local model, so
///   callers use the `grep` fallback.
/// - With the `embed` feature: `Ok(Some(..))` on success, or `Err(..)`
///   when the model can't be loaded (e.g. no network to download it).
///   Callers treat an `Err` the same as `None`: fall back to grep.
#[cfg(not(feature = "embed"))]
pub fn default_embedder() -> anyhow::Result<Option<Box<dyn Embedder>>> {
    Ok(None)
}

/// See the no-feature variant above.
#[cfg(feature = "embed")]
pub fn default_embedder() -> anyhow::Result<Option<Box<dyn Embedder>>> {
    Ok(Some(Box::new(FastEmbedEmbedder::new()?)))
}

/// Real local embedder backed by `fastembed` / `all-MiniLM-L6-v2`.
///
/// Only compiled with the `embed` feature. Kept deliberately small: it
/// owns a `fastembed::TextEmbedding` and forwards batches to it. The
/// model is fetched (once) and cached by `fastembed` on construction.
#[cfg(feature = "embed")]
pub struct FastEmbedEmbedder {
    inner: std::sync::Mutex<fastembed::TextEmbedding>,
}

#[cfg(feature = "embed")]
impl std::fmt::Debug for FastEmbedEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FastEmbedEmbedder")
            .field("model", &"all-MiniLM-L6-v2")
            .finish()
    }
}

#[cfg(feature = "embed")]
impl FastEmbedEmbedder {
    /// Load `all-MiniLM-L6-v2`. Downloads + caches the model on first use
    /// via `fastembed`; returns an error (not a panic) when that fails so
    /// the query path can fall back to grep.
    pub fn new() -> anyhow::Result<Self> {
        use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )
        .map_err(|e| anyhow::anyhow!("failed to load embedding model: {e}"))?;
        Ok(Self {
            inner: std::sync::Mutex::new(model),
        })
    }
}

#[cfg(feature = "embed")]
impl Embedder for FastEmbedEmbedder {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("embedder mutex poisoned"))?;
        let batch: Vec<&str> = texts.iter().map(String::as_str).collect();
        guard
            .embed(batch, None)
            .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))
    }

    fn dim(&self) -> usize {
        MINILM_DIM
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minilm_dim_matches_model() {
        // Runs in both build configurations; also keeps the `use super::*`
        // import live under the `embed` feature where the no-embedder test
        // below is compiled out.
        assert_eq!(MINILM_DIM, 384);
    }

    #[cfg(not(feature = "embed"))]
    #[test]
    fn default_build_has_no_embedder() {
        // Without the feature there is no local model, so the query path
        // must see `None` and answer with the grep fallback.
        assert!(default_embedder().unwrap().is_none());
    }
}

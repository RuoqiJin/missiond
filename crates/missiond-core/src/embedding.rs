//! Embedding service for semantic search.
//!
//! Architecture:
//! - `EmbeddingProvider` trait: pluggable interface for any embedding model
//! - `FastEmbedProvider`: default local backend (BAAI/bge-small-zh-v1.5, 512-dim)
//! - Future: OllamaProvider (Qwen3-Embedding-0.6B), VoyageProvider, etc.
//!
//! Feature-gated: `embeddings` enables FastEmbed backend,
//! otherwise provides a no-op implementation that falls back to FTS5 only.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single cached embedding: (id, vector)
pub type EmbeddingCache = Arc<RwLock<Vec<(String, Vec<f32>)>>>;

/// Create a new empty embedding cache
pub fn new_cache() -> EmbeddingCache {
    Arc::new(RwLock::new(Vec::new()))
}

// ── Pluggable Embedding Interface ────────────────────────────────

/// Pluggable embedding provider trait.
/// Implementations must be Send + Sync for use in async contexts.
pub trait EmbeddingProvider: Send + Sync {
    /// Unique identifier for this provider + model combo.
    /// Used in DB `embedding_provider` column for version isolation.
    /// e.g. "fastembed-bge-small-zh-v1.5", "ollama-qwen3-0.6b"
    fn provider_id(&self) -> &str;

    /// Output vector dimension.
    fn dimension(&self) -> usize;

    /// Generate embedding for a single text. Returns None on failure (graceful degradation).
    fn embed(&self, text: &str) -> Option<Vec<f32>>;

    /// Batch embed. Default implementation calls embed() in a loop.
    fn embed_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// Cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// RRF (Reciprocal Rank Fusion) score combining two ranked positions.
/// k=60 is the standard constant.
pub fn rrf_score(fts_rank: Option<usize>, vec_rank: Option<usize>, k: usize) -> f64 {
    let fts = fts_rank
        .map(|r| 1.0 / (k + r + 1) as f64)
        .unwrap_or(0.0);
    let vec = vec_rank
        .map(|r| 1.0 / (k + r + 1) as f64)
        .unwrap_or(0.0);
    fts + vec
}

/// Result of hybrid search combining FTS5 and vector search
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub id: String,
    pub fts_rank: Option<usize>,
    pub vec_rank: Option<usize>,
    pub cosine_sim: Option<f32>,
    pub rrf_score: f64,
}

/// Perform hybrid search: merge FTS5 ranked results with cosine similarity on cached vectors.
/// Returns results sorted by RRF score descending.
pub fn hybrid_search(
    query_embedding: &[f32],
    cache: &[(String, Vec<f32>)],
    fts_ids: &[(String, usize)], // (id, rank)
    top_k: usize,
    rrf_k: usize,
) -> Vec<HybridSearchResult> {
    // Vector search: cosine similarity for all cached entries, take top_k
    let mut vec_scores: Vec<(usize, f32)> = cache
        .iter()
        .enumerate()
        .map(|(i, (_, vec))| (i, cosine_similarity(query_embedding, vec)))
        .collect();
    vec_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Build merged map: id -> HybridSearchResult
    let mut merged: HashMap<String, HybridSearchResult> = HashMap::new();

    // Add FTS results
    for (id, rank) in fts_ids.iter().take(top_k) {
        merged
            .entry(id.clone())
            .or_insert_with(|| HybridSearchResult {
                id: id.clone(),
                fts_rank: None,
                vec_rank: None,
                cosine_sim: None,
                rrf_score: 0.0,
            })
            .fts_rank = Some(*rank);
    }

    // Add vector results (top_k only)
    for (rank, (idx, sim)) in vec_scores.iter().take(top_k).enumerate() {
        let id = &cache[*idx].0;
        let entry = merged
            .entry(id.clone())
            .or_insert_with(|| HybridSearchResult {
                id: id.clone(),
                fts_rank: None,
                vec_rank: None,
                cosine_sim: None,
                rrf_score: 0.0,
            });
        entry.vec_rank = Some(rank);
        entry.cosine_sim = Some(*sim);
    }

    // Calculate RRF scores
    for result in merged.values_mut() {
        result.rrf_score = rrf_score(result.fts_rank, result.vec_rank, rrf_k);
    }

    let mut results: Vec<HybridSearchResult> = merged.into_values().collect();
    results.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

/// Convert raw bytes (little-endian) to Vec<f32>
pub fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Convert Vec<f32> to raw bytes (little-endian)
pub fn f32_vec_to_bytes(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

// ── Feature ON: FastEmbed backend ─────────────────────────────────

// ── Feature ON: FastEmbed backend ─────────────────────────────────

#[cfg(feature = "embeddings")]
mod backend {
    use std::sync::Mutex;

    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    use tracing::{info, warn};

    use super::EmbeddingProvider;

    pub const FASTEMBED_PROVIDER_ID: &str = "fastembed-bge-small-zh-v1.5";

    pub struct EmbeddingService {
        model: Mutex<TextEmbedding>,
    }

    impl EmbeddingService {
        /// Initialize with BAAI/bge-small-zh-v1.5 (512-dim, Chinese+English).
        /// Model is auto-downloaded from HuggingFace on first use (~130MB).
        /// Returns None if initialization fails (graceful degradation).
        pub fn new() -> Option<Self> {
            let options = InitOptions::new(EmbeddingModel::BGESmallZHV15)
                .with_show_download_progress(true);
            match TextEmbedding::try_new(options) {
                Ok(model) => {
                    info!("EmbeddingService initialized (bge-small-zh-v1.5, 512-dim)");
                    Some(Self {
                        model: Mutex::new(model),
                    })
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Failed to initialize EmbeddingService, semantic search disabled"
                    );
                    None
                }
            }
        }
    }

    impl EmbeddingProvider for EmbeddingService {
        fn provider_id(&self) -> &str {
            FASTEMBED_PROVIDER_ID
        }

        fn dimension(&self) -> usize {
            512
        }

        fn embed(&self, text: &str) -> Option<Vec<f32>> {
            let mut model = self.model.lock().ok()?;
            match model.embed(vec![text], None) {
                Ok(mut embeddings) if !embeddings.is_empty() => Some(embeddings.remove(0)),
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!(error = %e, "Embedding generation failed");
                    None
                }
            }
        }

        fn embed_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
            if texts.is_empty() {
                return Vec::new();
            }
            let mut model = match self.model.lock() {
                Ok(m) => m,
                Err(_) => return texts.iter().map(|_| None).collect(),
            };
            match model.embed(texts.to_vec(), None) {
                Ok(embeddings) => embeddings.into_iter().map(Some).collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "Batch embedding failed");
                    texts.iter().map(|_| None).collect()
                }
            }
        }
    }
}

// ── Feature OFF: Noop backend ─────────────────────────────────────

#[cfg(not(feature = "embeddings"))]
mod backend {
    use super::EmbeddingProvider;

    pub const FASTEMBED_PROVIDER_ID: &str = "fastembed-bge-small-zh-v1.5";

    pub struct EmbeddingService;

    impl EmbeddingService {
        pub fn new() -> Option<Self> {
            tracing::info!("EmbeddingService: disabled (no embeddings feature)");
            None
        }
    }

    impl EmbeddingProvider for EmbeddingService {
        fn provider_id(&self) -> &str {
            FASTEMBED_PROVIDER_ID
        }

        fn dimension(&self) -> usize {
            512
        }

        fn embed(&self, _text: &str) -> Option<Vec<f32>> {
            None
        }
    }
}

pub use backend::EmbeddingService;
pub use backend::FASTEMBED_PROVIDER_ID;

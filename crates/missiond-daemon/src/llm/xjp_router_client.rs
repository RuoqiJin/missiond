//! Typed HTTP client for the xjp-router service (embedding lane).
//!
//! Aligned with `.missiond/v2/intent-worker.lisp :: section xjp-router-gateway` (v0.3):
//! - embedding 经此 client 调 QWEN3
//! - 禁止 fallback (sonnet/gemini/local)
//! - 失败直接上抛
//!
//! Public surface:
//! - [`XjpRouterConfig`] resolves endpoint / auth / model from env or `llm.yaml`.
//! - [`XjpRouterClient`] is the typed HTTP client (`POST /embed`).
//! - [`XjpRouterProvider`] adapts the client to [`missiond_core::embedding::EmbeddingProvider`].
//!
//! Runtime contract for the external xjp-router `/embed` API:
//! - Request body : `{ "model": <model>, "texts": [<string>, ...] }`
//! - Response body: `{ "embeddings": [[f32, ...], ...] }` — vector count must equal `texts.len()`.

use std::env;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::context::v3_blueprint_runtime::RouterRuntimeConfig;
use crate::helpers::default_mission_home;
use missiond_core::embedding::EmbeddingProvider;

const ENDPOINT_ENV: &str = "MISSION_XJP_ROUTER_ENDPOINT";
const TOKEN_ENV: &str = "MISSION_XJP_ROUTER_AUTH_TOKEN";
const MODEL_ENV: &str = "MISSION_XJP_ROUTER_EMBED_MODEL";
const DIM_ENV: &str = "MISSION_XJP_ROUTER_EMBED_DIM";
const DEFAULT_MODEL: &str = "qwen3";

#[derive(Debug, Error)]
pub(crate) enum XjpRouterError {
    #[error(
        "xjp-router endpoint not configured (set {ENDPOINT_ENV} or llm.yaml `xjp_router.endpoint`)"
    )]
    MissingEndpoint,
    #[error("xjp-router auth token not configured (set {TOKEN_ENV} or llm.yaml `xjp_router.auth_token`)")]
    MissingToken,
    #[error("xjp-router HTTP transport error: {0}")]
    Http(String),
    #[error("xjp-router HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("xjp-router response parse error: {0}")]
    Parse(String),
    #[error("xjp-router returned {got} vectors but {want} were requested")]
    CountMismatch { got: usize, want: usize },
    #[error("xjp-router returned vector with dimension {got}, expected {want}")]
    DimensionMismatch { got: usize, want: usize },
    #[error("V3_BLUEPRINT_CONFIG_ERROR: {0}")]
    V3BlueprintConfig(String),
}

#[derive(Debug, Default, Clone, Deserialize)]
struct LlmYamlXjpRouter {
    endpoint: Option<String>,
    auth_token: Option<String>,
    /// Indirect: read the bearer from the named env var.
    auth_token_env: Option<String>,
    model: Option<String>,
    embedding_dim: Option<usize>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct LlmYamlMinimal {
    #[serde(default)]
    xjp_router: Option<LlmYamlXjpRouter>,
}

fn read_llm_yaml_xjp_router() -> Option<LlmYamlXjpRouter> {
    let path = default_mission_home().join("llm.yaml");
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_yaml::from_str::<LlmYamlMinimal>(&content) {
            Ok(parsed) => parsed.xjp_router,
            Err(e) => {
                warn!(error = %e, path = %path.display(), "Failed to parse llm.yaml::xjp_router");
                None
            }
        },
        Err(_) => None,
    }
}

/// Resolved xjp-router configuration.
#[derive(Debug, Clone)]
pub(crate) struct XjpRouterConfig {
    pub endpoint: String,
    pub auth_token: String,
    pub model: String,
    pub timeout: Duration,
    /// Optional configured embedding dimension. When set, responses are validated.
    pub expected_dim: Option<usize>,
}

impl XjpRouterConfig {
    /// Construct config from already-resolved sources. Pure — used by tests.
    /// No production hard-coded defaults for endpoint / auth_token.
    #[cfg(test)]
    fn resolve_from(
        endpoint: Option<String>,
        auth_token: Option<String>,
        model: Option<String>,
        timeout_secs: Option<u64>,
        expected_dim: Option<usize>,
    ) -> Result<Self, XjpRouterError> {
        Self::resolve_from_with_default_timeout(
            endpoint,
            auth_token,
            model,
            timeout_secs,
            expected_dim,
            RouterRuntimeConfig::default().direct_http_timeout(),
        )
    }

    fn resolve_from_with_default_timeout(
        endpoint: Option<String>,
        auth_token: Option<String>,
        model: Option<String>,
        timeout_secs: Option<u64>,
        expected_dim: Option<usize>,
        default_timeout: Duration,
    ) -> Result<Self, XjpRouterError> {
        let endpoint = endpoint
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string();
        if endpoint.is_empty() {
            return Err(XjpRouterError::MissingEndpoint);
        }
        let auth_token = auth_token.unwrap_or_default();
        if auth_token.trim().is_empty() {
            return Err(XjpRouterError::MissingToken);
        }
        Ok(Self {
            endpoint,
            auth_token,
            model: model
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            timeout: timeout_secs
                .map(Duration::from_secs)
                .unwrap_or(default_timeout),
            expected_dim,
        })
    }

    /// Resolve from env vars first, `llm.yaml` second. No production default endpoint/token.
    pub fn resolve() -> Result<Self, XjpRouterError> {
        let router_config = RouterRuntimeConfig::load_for_current_dir()
            .map_err(|err| XjpRouterError::V3BlueprintConfig(err.to_string()))?;
        let yaml = read_llm_yaml_xjp_router();
        let endpoint = env::var(ENDPOINT_ENV)
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.endpoint.clone()));
        let token = env::var(TOKEN_ENV).ok().or_else(|| {
            yaml.as_ref().and_then(|y| {
                if let Some(t) = &y.auth_token {
                    return Some(t.clone());
                }
                if let Some(env_name) = &y.auth_token_env {
                    return env::var(env_name).ok();
                }
                None
            })
        });
        let model = env::var(MODEL_ENV)
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.model.clone()));
        let timeout_secs = yaml.as_ref().and_then(|y| y.timeout_secs);
        let expected_dim = env::var(DIM_ENV)
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| yaml.as_ref().and_then(|y| y.embedding_dim));
        Self::resolve_from_with_default_timeout(
            endpoint,
            token,
            model,
            timeout_secs,
            expected_dim,
            router_config.direct_http_timeout(),
        )
    }

    /// Returns true iff some xjp-router config is present (env or llm.yaml).
    /// Used by daemon bootstrap to decide whether xjp-router is the chosen embedding path.
    pub fn is_configured() -> bool {
        if env::var(ENDPOINT_ENV).is_ok() {
            return true;
        }
        read_llm_yaml_xjp_router()
            .and_then(|y| y.endpoint)
            .is_some()
    }
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    texts: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Typed HTTP client for the xjp-router service.
#[derive(Clone)]
pub(crate) struct XjpRouterClient {
    http: reqwest::Client,
    config: XjpRouterConfig,
    provider_id_str: String,
}

impl XjpRouterClient {
    pub fn new(config: XjpRouterConfig) -> Result<Self, XjpRouterError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| XjpRouterError::Http(e.to_string()))?;
        let provider_id_str = format!("xjp-router-{}", config.model);
        Ok(Self {
            http,
            config,
            provider_id_str,
        })
    }

    pub fn from_env_or_yaml() -> Result<Self, XjpRouterError> {
        let config = XjpRouterConfig::resolve()?;
        Self::new(config)
    }

    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }
    pub fn model(&self) -> &str {
        &self.config.model
    }
    pub fn provider_id(&self) -> &str {
        &self.provider_id_str
    }

    /// Pure helper — used by tests to pin the request shape.
    pub fn build_request_body(model: &str, texts: &[String]) -> serde_json::Value {
        serde_json::to_value(EmbedRequest { model, texts })
            .unwrap_or_else(|_| serde_json::json!({}))
    }

    /// Pure helper — used by tests to pin response parsing.
    pub fn parse_response_body(body: &str) -> Result<Vec<Vec<f32>>, XjpRouterError> {
        let parsed: EmbedResponse =
            serde_json::from_str(body).map_err(|e| XjpRouterError::Parse(e.to_string()))?;
        Ok(parsed.embeddings)
    }

    /// `POST /embed` batch. Empty input short-circuits without an HTTP call.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, XjpRouterError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/embed", self.config.endpoint);
        let body = EmbedRequest {
            model: &self.config.model,
            texts,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.auth_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| XjpRouterError::Http(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(XjpRouterError::Status {
                status: status.as_u16(),
                body: missiond_core::util::safe_byte_truncate(&body_text, 200).to_string(),
            });
        }

        let body_text = resp
            .text()
            .await
            .map_err(|e| XjpRouterError::Http(e.to_string()))?;
        let embeddings = Self::parse_response_body(&body_text)?;

        if embeddings.len() != texts.len() {
            return Err(XjpRouterError::CountMismatch {
                got: embeddings.len(),
                want: texts.len(),
            });
        }
        if let Some(want_dim) = self.config.expected_dim {
            if let Some(first) = embeddings.first() {
                if first.len() != want_dim {
                    return Err(XjpRouterError::DimensionMismatch {
                        got: first.len(),
                        want: want_dim,
                    });
                }
            }
        }
        Ok(embeddings)
    }
}

/// `EmbeddingProvider` adapter — slots the xjp-router client into the existing pipeline.
pub(crate) struct XjpRouterProvider {
    client: XjpRouterClient,
    dimension: usize,
}

impl XjpRouterProvider {
    /// Construct from env / llm.yaml and probe `/embed` once to learn the vector dimension.
    /// Fails fast with a structured error if config is missing or the probe fails.
    pub async fn try_from_env_or_yaml() -> Result<Self, XjpRouterError> {
        let client = XjpRouterClient::from_env_or_yaml()?;
        info!(
            endpoint = %client.endpoint(),
            model = %client.model(),
            "xjp-router embedding client configured"
        );
        let probe = client.embed(&["dimension probe".to_string()]).await?;
        let first = probe.first().ok_or_else(|| {
            XjpRouterError::Parse("xjp-router returned no embedding for probe".into())
        })?;
        let dimension = first.len();
        if let Some(want) = client.config.expected_dim {
            if dimension != want {
                return Err(XjpRouterError::DimensionMismatch {
                    got: dimension,
                    want,
                });
            }
        }
        info!(
            dimension,
            provider_id = %client.provider_id(),
            "xjp-router embedding provider ready"
        );
        Ok(Self { client, dimension })
    }
}

impl EmbeddingProvider for XjpRouterProvider {
    fn provider_id(&self) -> &str {
        self.client.provider_id()
    }
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, text: &str) -> Option<Vec<f32>> {
        if text.is_empty() {
            return None;
        }
        let client = self.client.clone();
        let texts = vec![text.to_string()];
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(client.embed(&texts))
        });
        match result {
            Ok(mut vs) if !vs.is_empty() => Some(vs.remove(0)),
            Ok(_) => None,
            Err(e) => {
                // Fail fast: log and surface None — no Sonnet/Gemini fallback here.
                warn!(error = %e, "xjp-router embed failed (single)");
                None
            }
        }
    }

    fn embed_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        if texts.is_empty() {
            return Vec::new();
        }
        let client = self.client.clone();
        let texts_owned = texts.to_vec();
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(client.embed(&texts_owned))
        });
        match result {
            Ok(vs) => vs.into_iter().map(Some).collect(),
            Err(e) => {
                warn!(error = %e, count = texts.len(), "xjp-router embed_batch failed");
                texts.iter().map(|_| None).collect()
            }
        }
    }
}

/// Hand-off used by `init_embedding_provider`: returns a typed Arc on success.
pub(crate) async fn try_init_provider() -> Result<Arc<dyn EmbeddingProvider>, XjpRouterError> {
    let provider = XjpRouterProvider::try_from_env_or_yaml().await?;
    Ok(Arc::new(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_serializes_model_and_texts() {
        let body = XjpRouterClient::build_request_body(
            "qwen3",
            &["hello".to_string(), "world".to_string()],
        );
        assert_eq!(body["model"], "qwen3");
        assert_eq!(body["texts"][0], "hello");
        assert_eq!(body["texts"][1], "world");
        assert_eq!(body["texts"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn response_body_parses_into_vectors() {
        let body = r#"{"embeddings":[[0.1,0.2,0.3],[0.4,0.5,0.6]]}"#;
        let vs = XjpRouterClient::parse_response_body(body).unwrap();
        assert_eq!(vs.len(), 2);
        assert_eq!(vs[0], vec![0.1_f32, 0.2, 0.3]);
        assert_eq!(vs[1], vec![0.4_f32, 0.5, 0.6]);
    }

    #[test]
    fn response_parse_rejects_bad_json() {
        let err = XjpRouterClient::parse_response_body("not-json").unwrap_err();
        assert!(matches!(err, XjpRouterError::Parse(_)));
    }

    #[test]
    fn missing_endpoint_returns_structured_error() {
        let err = XjpRouterConfig::resolve_from(None, Some("token".into()), None, None, None)
            .unwrap_err();
        assert!(matches!(err, XjpRouterError::MissingEndpoint));
        assert!(format!("{err}").contains("endpoint"));
    }

    #[test]
    fn missing_token_returns_structured_error() {
        let err = XjpRouterConfig::resolve_from(
            Some("https://router.example".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, XjpRouterError::MissingToken));
        assert!(format!("{err}").contains("token"));
    }

    #[test]
    fn whitespace_endpoint_or_token_treated_as_missing() {
        let err =
            XjpRouterConfig::resolve_from(Some("   ".into()), Some("tok".into()), None, None, None)
                .unwrap_err();
        assert!(matches!(err, XjpRouterError::MissingEndpoint));

        let err = XjpRouterConfig::resolve_from(
            Some("https://router.example".into()),
            Some("   ".into()),
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, XjpRouterError::MissingToken));
    }

    #[test]
    fn resolve_from_applies_default_model_and_strips_trailing_slash() {
        let cfg = XjpRouterConfig::resolve_from(
            Some("https://router.example/".into()),
            Some("tok".into()),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(cfg.endpoint, "https://router.example");
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(
            cfg.timeout,
            RouterRuntimeConfig::default().direct_http_timeout()
        );
        assert!(cfg.expected_dim.is_none());
    }

    #[test]
    fn resolve_from_uses_v3_default_timeout_when_unspecified() {
        let cfg = XjpRouterConfig::resolve_from_with_default_timeout(
            Some("https://router.example".into()),
            Some("tok".into()),
            None,
            None,
            None,
            Duration::from_secs(17),
        )
        .unwrap();
        assert_eq!(cfg.timeout, Duration::from_secs(17));
    }

    #[test]
    fn resolve_from_honors_explicit_model_and_timeout() {
        let cfg = XjpRouterConfig::resolve_from(
            Some("https://router.example".into()),
            Some("tok".into()),
            Some("qwen3-large".into()),
            Some(45),
            Some(1024),
        )
        .unwrap();
        assert_eq!(cfg.model, "qwen3-large");
        assert_eq!(cfg.timeout, Duration::from_secs(45));
        assert_eq!(cfg.expected_dim, Some(1024));
    }

    #[test]
    fn count_mismatch_surfaces_clear_error() {
        let err = XjpRouterError::CountMismatch { got: 1, want: 2 };
        let msg = format!("{err}");
        assert!(msg.contains("1"));
        assert!(msg.contains("2"));
    }

    #[test]
    fn embedding_path_does_not_import_sonnet() {
        // Architectural invariant: the xjp-router embedding module must not pull in
        // a sonnet gateway (no chat fallback). Scan `use ...` lines for the forbidden
        // module name — this test fires before review if someone wires it back in.
        let src = include_str!("xjp_router_client.rs");
        let forbidden = ['s', 'o', 'n', 'n', 'e', 't'].iter().collect::<String>();
        let has_use_of_forbidden = src.lines().any(|line| {
            let t = line.trim_start();
            (t.starts_with("use ") || t.starts_with("pub use ")) && t.contains(&forbidden)
        });
        assert!(
            !has_use_of_forbidden,
            "xjp_router_client must not import the chat-side gateway (fail-fast, no fallback)"
        );
    }
}

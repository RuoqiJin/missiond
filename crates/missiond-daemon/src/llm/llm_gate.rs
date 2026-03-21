//! LLM Gate — unified kill switch for all model providers.
//!
//! Architecture (per Gemini audit):
//! - **Hot path**: `AtomicBool` read — sub-nanosecond, zero syscall, no async blocking.
//! - **Cold path**: File persistence on set — survives daemon restart.
//! - **Strong-typed error**: `LlmGateError::GateClosed` — callers MUST NOT retry.
//!
//! Flag files: `$MISSIOND_HOME/{provider}_disabled` (e.g. `gemini_disabled`).

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use tracing::{info, warn};

// ── Provider Enum (P3: eliminate stringly-typed) ──────────────────────

/// Strongly-typed LLM provider identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LlmProvider {
    Gemini,
    Sonnet,
    Codex,
}

impl LlmProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Sonnet => "sonnet",
            Self::Codex => "codex",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "gemini" => Some(Self::Gemini),
            "sonnet" => Some(Self::Sonnet),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    const ALL: [Self; 3] = [Self::Gemini, Self::Sonnet, Self::Codex];
}

impl fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Error Type (P0: prevent retry storms) ─────────────────────────────

/// Dedicated error for gate-closed rejections.
/// Callers that catch this MUST abort immediately — never retry.
#[derive(Debug, thiserror::Error)]
#[error("{0} gate is closed — all API calls rejected")]
pub(crate) struct LlmGateError(pub LlmProvider);

// ── Atomic Gate State (P2: eliminate hot-path I/O) ────────────────────

struct LlmGates {
    gemini: AtomicBool,
    sonnet: AtomicBool,
    codex: AtomicBool,
}

impl LlmGates {
    fn atom(&self, provider: LlmProvider) -> &AtomicBool {
        match provider {
            LlmProvider::Gemini => &self.gemini,
            LlmProvider::Sonnet => &self.sonnet,
            LlmProvider::Codex => &self.codex,
        }
    }
}

/// Global gate state — initialized once from flag files at first access.
static GATES: LazyLock<LlmGates> = LazyLock::new(|| {
    let g = LlmGates {
        gemini: AtomicBool::new(flag_file_exists(LlmProvider::Gemini)),
        sonnet: AtomicBool::new(flag_file_exists(LlmProvider::Sonnet)),
        codex: AtomicBool::new(flag_file_exists(LlmProvider::Codex)),
    };
    info!(
        gemini = g.gemini.load(Ordering::Relaxed),
        sonnet = g.sonnet.load(Ordering::Relaxed),
        codex = g.codex.load(Ordering::Relaxed),
        "LLM gates initialized from flag files"
    );
    g
});

// ── Public API ────────────────────────────────────────────────────────

/// Check if a provider's gate is closed. Cost: single AtomicBool load (~1ns).
pub(crate) fn is_disabled(provider: LlmProvider) -> bool {
    GATES.atom(provider).load(Ordering::Relaxed)
}

/// Convenience: check gate and return typed error if closed.
/// Use at choke points: `llm_gate::check(LlmProvider::Gemini)?;`
pub(crate) fn check(provider: LlmProvider) -> Result<(), LlmGateError> {
    if is_disabled(provider) {
        Err(LlmGateError(provider))
    } else {
        Ok(())
    }
}

/// Open or close a provider's gate. Updates AtomicBool + persists to flag file.
pub(crate) fn set_disabled(provider: LlmProvider, disabled: bool) {
    GATES.atom(provider).store(disabled, Ordering::Relaxed);
    // Cold path: persist to flag file for restart survival
    let path = flag_path(provider);
    if disabled {
        let _ = std::fs::write(&path, chrono::Utc::now().to_rfc3339());
        warn!(%provider, "LLM gate closed — flag file created at {:?}", path);
    } else {
        let _ = std::fs::remove_file(&path);
        info!(%provider, "LLM gate opened — flag file removed");
    }
}

/// Return status of all gates.
pub(crate) fn all_status() -> Vec<(LlmProvider, bool)> {
    LlmProvider::ALL
        .iter()
        .map(|&p| (p, is_disabled(p)))
        .collect()
}

// ── File helpers (cold path only) ─────────────────────────────────────

fn missiond_home() -> PathBuf {
    let home = std::env::var("MISSIOND_HOME")
        .or_else(|_| std::env::var("XJP_MISSION_HOME"))
        .unwrap_or_else(|_| {
            let h = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            if h.join(".missiond").exists() {
                h.join(".missiond").to_string_lossy().to_string()
            } else {
                h.join(".xjp-mission").to_string_lossy().to_string()
            }
        });
    PathBuf::from(home)
}

fn flag_path(provider: LlmProvider) -> PathBuf {
    missiond_home().join(format!("{}_disabled", provider.as_str()))
}

fn flag_file_exists(provider: LlmProvider) -> bool {
    flag_path(provider).exists()
}

//! Unified Control Tree — centralized pause/resume for all daemon components.
//!
//! Architecture (per Gemini audit):
//! - Single `watch<ControlTree>` broadcast replaces scattered AtomicBools/files/KVs.
//! - Workers declare dependencies (`Vec<Dependency>`); `is_effectively_paused()`
//!   performs cascade evaluation (global → provider → domain → worker).
//! - Mutations flow through `ControlManager`, which atomically updates the tree
//!   and persists to `control_tree.json` for crash recovery.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

// ── Control Dimensions ──────────────────────────────────────────────

/// LLM provider identifier (superset of llm_gate::LlmProvider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CtlProvider {
    Gemini,
    Sonnet,
    Codex,
    Opus,
}

impl CtlProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Sonnet => "sonnet",
            Self::Codex => "codex",
            Self::Opus => "opus",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "gemini" => Some(Self::Gemini),
            "sonnet" => Some(Self::Sonnet),
            "codex" => Some(Self::Codex),
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }

    pub const ALL: [Self; 4] = [Self::Gemini, Self::Sonnet, Self::Codex, Self::Opus];
}

/// Functional domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CtlDomain {
    Memory,
    Flow,
    Board,
    Strategy,
}

impl CtlDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Flow => "flow",
            Self::Board => "board",
            Self::Strategy => "strategy",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "memory" => Some(Self::Memory),
            "flow" => Some(Self::Flow),
            "board" => Some(Self::Board),
            "strategy" => Some(Self::Strategy),
            _ => None,
        }
    }

    pub const ALL: [Self; 4] = [Self::Memory, Self::Flow, Self::Board, Self::Strategy];
}

// ── Dependency declaration ──────────────────────────────────────────

/// A worker's dependency — used for cascade evaluation.
#[derive(Debug, Clone)]
pub enum Dependency {
    Provider(CtlProvider),
    Domain(CtlDomain),
}

// ── Control Tree (the single source of truth snapshot) ──────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlTree {
    pub global_paused: bool,
    #[serde(default)]
    pub providers: HashMap<CtlProvider, bool>,
    #[serde(default)]
    pub domains: HashMap<CtlDomain, bool>,
    #[serde(default)]
    pub workers: HashMap<String, bool>,
    #[serde(default)]
    pub slot_roles: HashMap<String, bool>,
    /// Timestamps when domains were paused (informational only).
    #[serde(default)]
    pub domain_paused_at: HashMap<CtlDomain, i64>,
}

impl Default for ControlTree {
    fn default() -> Self {
        Self {
            global_paused: false,
            providers: HashMap::new(),
            domains: HashMap::new(),
            workers: HashMap::new(),
            slot_roles: HashMap::new(),
            domain_paused_at: HashMap::new(),
        }
    }
}

impl ControlTree {
    /// Check if a provider gate is closed.
    pub fn is_provider_paused(&self, p: CtlProvider) -> bool {
        self.global_paused || self.providers.get(&p).copied().unwrap_or(false)
    }

    /// Check if a domain is paused.
    pub fn is_domain_paused(&self, d: CtlDomain) -> bool {
        self.global_paused || self.domains.get(&d).copied().unwrap_or(false)
    }

    /// Check if a specific worker is paused (direct, not cascade).
    pub fn is_worker_paused(&self, name: &str) -> bool {
        self.global_paused || self.workers.get(name).copied().unwrap_or(false)
    }

    /// Check if a slot role is paused.
    pub fn is_slot_role_paused(&self, role: &str) -> bool {
        self.global_paused || self.slot_roles.get(role).copied().unwrap_or(false)
    }

    /// Cascade evaluation: is this worker effectively paused given its dependencies?
    pub fn is_effectively_paused(&self, worker_name: &str, deps: &[Dependency]) -> bool {
        if self.global_paused {
            return true;
        }
        if self.workers.get(worker_name).copied().unwrap_or(false) {
            return true;
        }
        for dep in deps {
            match dep {
                Dependency::Provider(p) => {
                    if self.providers.get(p).copied().unwrap_or(false) {
                        return true;
                    }
                }
                Dependency::Domain(d) => {
                    if self.domains.get(d).copied().unwrap_or(false) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Domain pause duration (informational, no auto-expire).
    pub fn domain_paused_duration_secs(&self, d: CtlDomain) -> Option<i64> {
        self.domain_paused_at
            .get(&d)
            .filter(|&&t| t > 0)
            .map(|&t| chrono::Utc::now().timestamp() - t)
    }

    /// Build a human-readable status summary.
    pub fn status_summary(&self) -> serde_json::Value {
        serde_json::json!({
            "global_paused": self.global_paused,
            "providers": self.providers.iter()
                .filter(|(_, &v)| v)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            "domains": self.domains.iter()
                .filter(|(_, &v)| v)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            "workers": self.workers.iter()
                .filter(|(_, &v)| v)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            "slot_roles": self.slot_roles.iter()
                .filter(|(_, &v)| v)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
        })
    }
}

// ── Control Manager (mutation + persistence + broadcast) ────────────

/// Centralized controller — holds the watch::Sender, provides mutation API.
pub struct ControlManager {
    tx: watch::Sender<ControlTree>,
    persist_path: PathBuf,
}

impl ControlManager {
    /// Create a new ControlManager, restoring state from disk if available.
    pub fn new(home: &std::path::Path) -> (Self, watch::Receiver<ControlTree>) {
        let persist_path = home.join("control_tree.json");
        let tree = Self::load_from_disk(&persist_path);

        info!(
            global = tree.global_paused,
            providers_paused = tree.providers.values().filter(|&&v| v).count(),
            domains_paused = tree.domains.values().filter(|&&v| v).count(),
            "ControlTree initialized"
        );

        let (tx, rx) = watch::channel(tree);
        (Self { tx, persist_path }, rx)
    }

    /// Get a new Receiver (for workers spawned after init).
    pub fn subscribe(&self) -> watch::Receiver<ControlTree> {
        self.tx.subscribe()
    }

    /// Read the current tree snapshot.
    pub fn current(&self) -> ControlTree {
        self.tx.borrow().clone()
    }

    // ── Mutations ──

    pub fn set_global_paused(&self, paused: bool) {
        self.mutate(|tree| {
            tree.global_paused = paused;
        });
        info!(paused, "ControlTree: global_paused changed");
    }

    pub fn set_provider(&self, provider: CtlProvider, paused: bool) {
        self.mutate(|tree| {
            tree.providers.insert(provider, paused);
        });
        info!(%paused, provider = provider.as_str(), "ControlTree: provider changed");
    }

    pub fn set_domain(&self, domain: CtlDomain, paused: bool) {
        self.mutate(|tree| {
            tree.domains.insert(domain, paused);
            if paused {
                tree.domain_paused_at
                    .insert(domain, chrono::Utc::now().timestamp());
            } else {
                tree.domain_paused_at.remove(&domain);
            }
        });
        info!(%paused, domain = domain.as_str(), "ControlTree: domain changed");
    }

    pub fn set_worker(&self, name: &str, paused: bool) {
        self.mutate(|tree| {
            tree.workers.insert(name.to_string(), paused);
        });
        info!(%paused, worker = name, "ControlTree: worker changed");
    }

    pub fn set_slot_role(&self, role: &str, paused: bool) {
        self.mutate(|tree| {
            tree.slot_roles.insert(role.to_string(), paused);
        });
        info!(%paused, role, "ControlTree: slot_role changed");
    }

    // Domain pause is permanent until user explicitly resumes via mission_control.

    // ── Internal helpers ──

    fn mutate<F: FnOnce(&mut ControlTree)>(&self, f: F) {
        self.tx.send_modify(f);
        // P0 fix (Gemini audit): persist via spawn_blocking to avoid
        // blocking the Tokio worker thread with synchronous I/O.
        let tree = self.tx.borrow().clone();
        let path = self.persist_path.clone();
        tokio::task::spawn_blocking(move || match serde_json::to_string_pretty(&tree) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, &json) {
                    warn!(error = %e, "ControlTree: failed to persist");
                }
            }
            Err(e) => warn!(error = %e, "ControlTree: failed to serialize"),
        });
    }

    fn load_from_disk(path: &std::path::Path) -> ControlTree {
        if !path.exists() {
            return ControlTree::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!(error = %e, "ControlTree: failed to parse, using default");
                ControlTree::default()
            }),
            Err(e) => {
                warn!(error = %e, "ControlTree: failed to read, using default");
                ControlTree::default()
            }
        }
    }
}

// ── Bridge: ControlTree → legacy llm_gate ───────────────────────────

impl CtlProvider {
    /// Convert to legacy LlmProvider for backward compatibility.
    pub fn to_llm_provider(self) -> Option<crate::llm_gate::LlmProvider> {
        match self {
            Self::Gemini => Some(crate::llm_gate::LlmProvider::Gemini),
            Self::Sonnet => Some(crate::llm_gate::LlmProvider::Sonnet),
            Self::Codex => Some(crate::llm_gate::LlmProvider::Codex),
            Self::Opus => None, // Opus has no legacy gate
        }
    }
}

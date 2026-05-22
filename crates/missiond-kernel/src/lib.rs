use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const DEFAULT_MAX_CAUSATION_DEPTH: u8 = 10;
pub const DEFAULT_MAX_EVENTS_PER_CORRELATION: u32 = 128;
pub const DEFAULT_MAX_CELL_RUNTIME_MS: u64 = 300_000;
pub const DEFAULT_IDEMPOTENCY_CACHE_SIZE: usize = 4_096;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("duplicate atom registered: {0}")]
    DuplicateAtom(String),
    #[error("missing atom: {0}")]
    MissingAtom(String),
    #[error("atom evaluation failed: {0}")]
    AtomEval(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEnvelope {
    pub id: String,
    pub kind: String,
    pub source: Option<String>,
    pub domain: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    #[serde(default)]
    pub causation_depth: u8,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
}

fn default_schema_version() -> u32 {
    1
}

impl EventEnvelope {
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            source: None,
            domain: None,
            payload,
            correlation_id: None,
            causation_id: None,
            causation_depth: 0,
            schema_version: default_schema_version(),
            meta: BTreeMap::new(),
        }
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl Default for RiskLevel {
    fn default() -> Self {
        Self::Low
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEnvelope {
    pub command_type: String,
    #[serde(default)]
    pub payload: Value,
    pub idempotency_key: Option<String>,
    pub required_capability: Option<String>,
    #[serde(default)]
    pub risk_level: RiskLevel,
    pub timeout_ms: u64,
}

impl CommandEnvelope {
    pub fn new(command_type: impl Into<String>, payload: Value) -> Self {
        Self {
            command_type: command_type.into(),
            payload,
            idempotency_key: None,
            required_capability: None,
            risk_level: RiskLevel::Low,
            timeout_ms: DEFAULT_MAX_CELL_RUNTIME_MS,
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.required_capability = Some(capability.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledEffect {
    pub schedule_id: String,
    pub event: EventEnvelope,
    pub after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistArtifactEffect {
    pub path: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEffect {
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricEffect {
    pub name: String,
    pub value: f64,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Effect {
    EmitEvent(EventEnvelope),
    Command(CommandEnvelope),
    Schedule(ScheduledEffect),
    PersistArtifact(PersistArtifactEffect),
    Log(LogEffect),
    Metric(MetricEffect),
    Noop,
}

impl Effect {
    pub fn idempotency_key(&self) -> Option<&str> {
        match self {
            Self::Command(command) => command.idempotency_key.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AtomKind {
    PurePredicate,
    CommandProducing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AtomMetadata {
    pub name: String,
    pub kind: AtomKind,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AtomCtx {
    pub genome_id: String,
    pub trace: Option<String>,
}

#[async_trait]
pub trait Atom: Send + Sync {
    fn metadata(&self) -> AtomMetadata;

    async fn eval(&self, ctx: &AtomCtx, input: &Value) -> Result<Value, KernelError>;
}

#[derive(Default, Clone)]
pub struct AtomRegistry {
    atoms: BTreeMap<String, Arc<dyn Atom>>,
}

impl AtomRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A>(&mut self, atom: A) -> Result<(), KernelError>
    where
        A: Atom + 'static,
    {
        let name = atom.metadata().name;
        if self.atoms.contains_key(&name) {
            return Err(KernelError::DuplicateAtom(name));
        }
        self.atoms.insert(name, Arc::new(atom));
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Atom>> {
        self.atoms.get(name).cloned()
    }

    pub fn require_all<'a, I>(&self, names: I) -> Result<(), KernelError>
    where
        I: IntoIterator<Item = &'a str>,
    {
        for name in names {
            if !self.atoms.contains_key(name) {
                return Err(KernelError::MissingAtom(name.to_string()));
            }
        }
        Ok(())
    }

    pub fn names(&self) -> Vec<String> {
        self.atoms.keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleNode {
    pub id: String,
    pub atom: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleGraph {
    pub id: String,
    #[serde(default)]
    pub nodes: Vec<RuleNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Molecule {
    pub id: String,
    pub on: String,
    pub when: Option<String>,
    #[serde(default)]
    pub atoms: Vec<String>,
    #[serde(default)]
    pub effects: Vec<String>,
    pub rule_graph: Option<RuleGraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeBudgets {
    pub max_causation_depth: u8,
    pub max_events_per_correlation: u32,
    pub max_cell_runtime_ms: u64,
    pub idempotency_cache_size: usize,
}

impl Default for RuntimeBudgets {
    fn default() -> Self {
        Self {
            max_causation_depth: DEFAULT_MAX_CAUSATION_DEPTH,
            max_events_per_correlation: DEFAULT_MAX_EVENTS_PER_CORRELATION,
            max_cell_runtime_ms: DEFAULT_MAX_CELL_RUNTIME_MS,
            idempotency_cache_size: DEFAULT_IDEMPOTENCY_CACHE_SIZE,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TissueProfile {
    pub id: String,
    #[serde(default)]
    pub receptors: Vec<String>,
    #[serde(default)]
    pub allow_atoms: Vec<String>,
    #[serde(default)]
    pub allow_effects: Vec<String>,
    #[serde(default)]
    pub molecules: Vec<Molecule>,
    #[serde(default)]
    pub budgets: RuntimeBudgets,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrganProfile {
    pub id: String,
    #[serde(default)]
    pub tissues: Vec<TissueProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Genome {
    pub id: String,
    pub schema: String,
    pub activation: ActivationMode,
    #[serde(default)]
    pub organs: Vec<OrganProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivationMode {
    Shadow,
    Active,
    Rollback,
}

impl Default for ActivationMode {
    fn default() -> Self {
        Self::Shadow
    }
}

impl ActivationMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "shadow" | "Shadow" => Some(Self::Shadow),
            "active" | "Active" => Some(Self::Active),
            "rollback" | "Rollback" => Some(Self::Rollback),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::Active => "active",
            Self::Rollback => "rollback",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CellCtx {
    pub genome_id: String,
    pub activation: ActivationMode,
}

#[async_trait]
pub trait Cell: Send + Sync {
    fn id(&self) -> &'static str;

    fn tissue(&self) -> &'static str;

    fn receptors(&self) -> &'static [&'static str];

    async fn on_event(&self, ctx: &CellCtx, event: &EventEnvelope) -> Vec<Effect>;
}

#[derive(Debug, Clone)]
pub struct IdempotencyWindow {
    capacity: usize,
    values: VecDeque<String>,
}

impl IdempotencyWindow {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            values: VecDeque::new(),
        }
    }

    pub fn insert_new(&mut self, key: impl Into<String>) -> bool {
        let key = key.into();
        if self.values.iter().any(|existing| existing == &key) {
            return false;
        }
        self.values.push_back(key);
        while self.values.len() > self.capacity {
            self.values.pop_front();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestAtom(&'static str);

    #[async_trait]
    impl Atom for TestAtom {
        fn metadata(&self) -> AtomMetadata {
            AtomMetadata {
                name: self.0.to_string(),
                kind: AtomKind::PurePredicate,
                input_schema: None,
                output_schema: None,
                permissions: Vec::new(),
            }
        }

        async fn eval(&self, _ctx: &AtomCtx, input: &Value) -> Result<Value, KernelError> {
            Ok(input.clone())
        }
    }

    #[tokio::test]
    async fn atom_registry_rejects_duplicates() {
        let mut registry = AtomRegistry::new();
        registry.register(TestAtom("a")).unwrap();
        let err = registry.register(TestAtom("a")).unwrap_err();
        assert!(matches!(err, KernelError::DuplicateAtom(name) if name == "a"));
    }

    #[test]
    fn atom_registry_validates_required_atoms() {
        let mut registry = AtomRegistry::new();
        registry.register(TestAtom("a")).unwrap();
        registry.require_all(["a"].into_iter()).unwrap();
        let err = registry.require_all(["b"].into_iter()).unwrap_err();
        assert!(matches!(err, KernelError::MissingAtom(name) if name == "b"));
    }

    #[test]
    fn idempotency_window_rejects_duplicates() {
        let mut window = IdempotencyWindow::new(2);
        assert!(window.insert_new("a"));
        assert!(!window.insert_new("a"));
        assert!(window.insert_new("b"));
        assert!(window.insert_new("c"));
        assert!(window.insert_new("a"));
    }
}

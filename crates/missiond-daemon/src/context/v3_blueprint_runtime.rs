use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_MODEL_PROFILE: &str = "coding-default-opus-4-7";
pub(crate) const DEFAULT_TIMEOUT_SECS: i64 = 1800;
pub(crate) const MIN_TIMEOUT_SECS: i64 = 60;
pub(crate) const MAX_TIMEOUT_SECS: i64 = 7200;
pub(crate) const DEFAULT_CC_SWARM_TIMEOUT_SECS: i64 = 600;
pub(crate) const MIN_CC_SWARM_TIMEOUT_SECS: i64 = 60;
pub(crate) const MAX_CC_SWARM_TIMEOUT_SECS: i64 = 7200;
pub(crate) const WATCHDOG_GRACE_SECS: i64 = 120;
pub(crate) const MISSING_SESSION_PROBE_SECS: i64 = 120;
pub(crate) const DEFAULT_SLOT_TTL_SECS: i64 = 14400;
pub(crate) const MIN_SLOT_TTL_SECS: i64 = 300;
pub(crate) const MAX_SLOT_TTL_SECS: i64 = 28800;
pub(crate) const DEFAULT_SLOT_EXTEND_SECS: i64 = 3600;
pub(crate) const MAX_SLOT_EXTEND_SECS: i64 = 3600;
pub(crate) const DEFAULT_FLOW_LLM_MAX_TOKENS: u32 = 65536;
pub(crate) const DEFAULT_FLOW_SLOT_MODEL: &str = "opus";
pub(crate) const DEFAULT_FLOW_SLOT_TIMEOUT_SECS: u64 = 3600;
pub(crate) const DEFAULT_FLOW_PARALLELISM: usize = 3;
pub(crate) const DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS: u64 = 1800;
pub(crate) const DEFAULT_CASCADE_MANIFEST_PATH: &str =
    "/Users/jinchen/Projects/universe.intent.lisp";
pub(crate) const DEFAULT_CASCADE_ALLOWED_ROOT: &str = "/Users/jinchen/Projects";
pub(crate) const DEFAULT_CASCADE_TRIGGER_ENABLED: bool = true;
pub(crate) const DEFAULT_CASCADE_MAX_CYCLES: usize = 3;
pub(crate) const MAX_CASCADE_MAX_CYCLES: usize = 12;
pub(crate) const DEFAULT_PROJECT_UNIVERSE_MANIFEST: &str =
    "/Users/jinchen/Projects/universe.intent.lisp";
pub(crate) const DEFAULT_PROJECT_INTENT_PATH_CANDIDATES: [&str; 3] = [
    ".missiond/intent.lisp",
    ".jarvis/intent.lisp",
    "intent.lisp",
];
pub(crate) const DEFAULT_CAPABILITY_REVIEW_SIDECAR: &str =
    ".missiond/v2/capability-usage-review.json";
pub(crate) const DEFAULT_PROTECTED_TOOL_PATTERNS: [&str; 12] = [
    "mission_execution",
    "mission_intent",
    "mission_forge_",
    "mission_sys_",
    "mission_daemon_update",
    "mission_health",
    "mission_power_control",
    "mission_kb_ops",
    "mission_audit",
    "mission_pty_signal",
    "mission_pty_confirm",
    "mission_incident",
];
pub(crate) const DEFAULT_PROTECTED_FLOW_PATTERNS: [&str; 4] = [
    "engineering",
    "F-execution-log-governance",
    "F-incident-reaction",
    "F-capability-usage-monitoring",
];
pub(crate) const DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT: usize = 60;
pub(crate) const DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS: usize = 1000;
pub(crate) const DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS: usize = 500;
pub(crate) const DEFAULT_CONVERSATION_GET_TAIL: i64 = 50;
pub(crate) const DEFAULT_CONVERSATION_SEARCH_LIMIT: i64 = 10;
pub(crate) const DEFAULT_MESSAGE_SEARCH_LIMIT: i64 = 20;
pub(crate) const DEFAULT_CONTEXT_BEFORE: i64 = 3;
pub(crate) const DEFAULT_CONTEXT_AFTER: i64 = 5;
pub(crate) const DEFAULT_CONVERSATION_EVENTS_LIMIT: i64 = 100;
pub(crate) const DEFAULT_AGENT_TRAJECTORY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_TIMELINE_QUERY_LIMIT: i64 = 50;
pub(crate) const MAX_TIMELINE_QUERY_LIMIT: i64 = 200;
pub(crate) const DEFAULT_TIMELINE_SEARCH_LIMIT: i64 = 20;
pub(crate) const MAX_TIMELINE_SEARCH_LIMIT: i64 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkstationRuntimeConfig {
    slot_default_profiles: HashMap<String, String>,
    pub timeout_policy: TimeoutPolicy,
    pub cc_swarm_timeout_policy: SimpleTimeoutPolicy,
    pub slot_ttl_policy: SlotTtlPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlowRuntimeConfig {
    pub llm_call_default_max_tokens: u32,
    pub slot_task_default_model: String,
    pub slot_task_default_timeout_secs: u64,
    pub parallel_slot_default_parallelism: usize,
    pub parallel_slot_default_timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimeoutPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
    pub watchdog_grace_secs: i64,
    pub missing_session_probe_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SimpleTimeoutPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotTtlPolicy {
    pub default_secs: i64,
    pub min_secs: i64,
    pub max_secs: i64,
    pub default_extend_secs: i64,
    pub max_extend_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CascadeRuntimeConfig {
    pub default_manifest_path: PathBuf,
    pub allowed_root: PathBuf,
    pub trigger_enabled: bool,
    pub default_max_cycles: usize,
    pub max_cycles_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectRegistryRuntimeConfig {
    pub intent_path_candidates: Vec<String>,
    pub default_universe_manifest: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapabilityGovernanceRuntimeConfig {
    pub review_sidecar_path: PathBuf,
    pub protected_tool_patterns: Vec<String>,
    pub protected_flow_patterns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryKbRuntimeConfig {
    pub pending_message_limit: usize,
    pub tool_result_preview_chars: usize,
    pub assistant_preview_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ConversationIngestionRuntimeConfig {
    pub conversation_get_tail_default: i64,
    pub conversation_search_default_limit: i64,
    pub message_search_default_limit: i64,
    pub context_before_default: i64,
    pub context_after_default: i64,
    pub conversation_events_default_limit: i64,
    pub agent_trajectory_default_limit: i64,
    pub timeline_query_default_limit: i64,
    pub timeline_query_max_limit: i64,
    pub timeline_search_default_limit: i64,
    pub timeline_search_max_limit: i64,
}

#[derive(Debug)]
pub(crate) enum BlueprintConfigError {
    MissingBlueprint(PathBuf),
    Read { path: PathBuf, message: String },
    Parse(String),
}

impl fmt::Display for BlueprintConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBlueprint(path) => {
                write!(f, "V3 blueprint missing at {}", path.display())
            }
            Self::Read { path, message } => {
                write!(
                    f,
                    "failed to read V3 blueprint {}: {}",
                    path.display(),
                    message
                )
            }
            Self::Parse(message) => write!(
                f,
                "failed to parse V3 blueprint runtime config: {}",
                message
            ),
        }
    }
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_TIMEOUT_SECS,
            min_secs: MIN_TIMEOUT_SECS,
            max_secs: MAX_TIMEOUT_SECS,
            watchdog_grace_secs: WATCHDOG_GRACE_SECS,
            missing_session_probe_secs: MISSING_SESSION_PROBE_SECS,
        }
    }
}

impl Default for SimpleTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_CC_SWARM_TIMEOUT_SECS,
            min_secs: MIN_CC_SWARM_TIMEOUT_SECS,
            max_secs: MAX_CC_SWARM_TIMEOUT_SECS,
        }
    }
}

impl Default for SlotTtlPolicy {
    fn default() -> Self {
        Self {
            default_secs: DEFAULT_SLOT_TTL_SECS,
            min_secs: MIN_SLOT_TTL_SECS,
            max_secs: MAX_SLOT_TTL_SECS,
            default_extend_secs: DEFAULT_SLOT_EXTEND_SECS,
            max_extend_secs: MAX_SLOT_EXTEND_SECS,
        }
    }
}

impl Default for WorkstationRuntimeConfig {
    fn default() -> Self {
        let mut slot_default_profiles = HashMap::new();
        slot_default_profiles.insert("coder".to_string(), DEFAULT_MODEL_PROFILE.to_string());
        slot_default_profiles.insert("researcher".to_string(), DEFAULT_MODEL_PROFILE.to_string());
        slot_default_profiles.insert("ops".to_string(), "daily-sonnet".to_string());
        Self {
            slot_default_profiles,
            timeout_policy: TimeoutPolicy::default(),
            cc_swarm_timeout_policy: SimpleTimeoutPolicy::default(),
            slot_ttl_policy: SlotTtlPolicy::default(),
        }
    }
}

impl Default for FlowRuntimeConfig {
    fn default() -> Self {
        Self {
            llm_call_default_max_tokens: DEFAULT_FLOW_LLM_MAX_TOKENS,
            slot_task_default_model: DEFAULT_FLOW_SLOT_MODEL.to_string(),
            slot_task_default_timeout_secs: DEFAULT_FLOW_SLOT_TIMEOUT_SECS,
            parallel_slot_default_parallelism: DEFAULT_FLOW_PARALLELISM,
            parallel_slot_default_timeout_secs: DEFAULT_FLOW_PARALLEL_TIMEOUT_SECS,
        }
    }
}

impl Default for CascadeRuntimeConfig {
    fn default() -> Self {
        Self {
            default_manifest_path: PathBuf::from(DEFAULT_CASCADE_MANIFEST_PATH),
            allowed_root: PathBuf::from(DEFAULT_CASCADE_ALLOWED_ROOT),
            trigger_enabled: DEFAULT_CASCADE_TRIGGER_ENABLED,
            default_max_cycles: DEFAULT_CASCADE_MAX_CYCLES,
            max_cycles_limit: MAX_CASCADE_MAX_CYCLES,
        }
    }
}

impl Default for ProjectRegistryRuntimeConfig {
    fn default() -> Self {
        Self {
            intent_path_candidates: DEFAULT_PROJECT_INTENT_PATH_CANDIDATES
                .iter()
                .map(|value| value.to_string())
                .collect(),
            default_universe_manifest: PathBuf::from(DEFAULT_PROJECT_UNIVERSE_MANIFEST),
        }
    }
}

impl Default for CapabilityGovernanceRuntimeConfig {
    fn default() -> Self {
        Self {
            review_sidecar_path: PathBuf::from(DEFAULT_CAPABILITY_REVIEW_SIDECAR),
            protected_tool_patterns: DEFAULT_PROTECTED_TOOL_PATTERNS
                .iter()
                .map(|value| value.to_string())
                .collect(),
            protected_flow_patterns: DEFAULT_PROTECTED_FLOW_PATTERNS
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }
}

impl Default for MemoryKbRuntimeConfig {
    fn default() -> Self {
        Self {
            pending_message_limit: DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT,
            tool_result_preview_chars: DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS,
            assistant_preview_chars: DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS,
        }
    }
}

impl Default for ConversationIngestionRuntimeConfig {
    fn default() -> Self {
        Self {
            conversation_get_tail_default: DEFAULT_CONVERSATION_GET_TAIL,
            conversation_search_default_limit: DEFAULT_CONVERSATION_SEARCH_LIMIT,
            message_search_default_limit: DEFAULT_MESSAGE_SEARCH_LIMIT,
            context_before_default: DEFAULT_CONTEXT_BEFORE,
            context_after_default: DEFAULT_CONTEXT_AFTER,
            conversation_events_default_limit: DEFAULT_CONVERSATION_EVENTS_LIMIT,
            agent_trajectory_default_limit: DEFAULT_AGENT_TRAJECTORY_LIMIT,
            timeline_query_default_limit: DEFAULT_TIMELINE_QUERY_LIMIT,
            timeline_query_max_limit: MAX_TIMELINE_QUERY_LIMIT,
            timeline_search_default_limit: DEFAULT_TIMELINE_SEARCH_LIMIT,
            timeline_search_max_limit: MAX_TIMELINE_SEARCH_LIMIT,
        }
    }
}

impl WorkstationRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_workstation_config(&source)
    }

    pub(crate) fn default_model_profile_for_template(&self, template: &str) -> Option<&str> {
        self.slot_default_profiles.get(template).map(String::as_str)
    }

    pub(crate) fn clamp_timeout_secs(&self, timeout_secs: Option<i64>) -> i64 {
        let raw = match timeout_secs {
            Some(value) if value > 0 => value,
            _ => self.timeout_policy.default_secs,
        };
        raw.clamp(self.timeout_policy.min_secs, self.timeout_policy.max_secs)
    }

    pub(crate) fn clamp_cc_swarm_timeout_ms(&self, timeout_ms: Option<u64>) -> u64 {
        let min_ms = (self.cc_swarm_timeout_policy.min_secs.max(1) as u64).saturating_mul(1000);
        let max_ms = (self.cc_swarm_timeout_policy.max_secs.max(1) as u64).saturating_mul(1000);
        let default_ms =
            (self.cc_swarm_timeout_policy.default_secs.max(1) as u64).saturating_mul(1000);
        let raw = timeout_ms.filter(|value| *value > 0).unwrap_or(default_ms);
        raw.clamp(min_ms, max_ms)
    }

    pub(crate) fn clamp_slot_ttl_secs(&self, ttl_secs: Option<i64>) -> i64 {
        let raw = match ttl_secs {
            Some(value) if value > 0 => value,
            _ => self.slot_ttl_policy.default_secs,
        };
        raw.clamp(self.slot_ttl_policy.min_secs, self.slot_ttl_policy.max_secs)
    }

    pub(crate) fn default_slot_extend_secs(&self) -> i64 {
        self.slot_ttl_policy
            .default_extend_secs
            .clamp(self.slot_ttl_policy.min_secs, self.max_slot_extend_secs())
    }

    pub(crate) fn max_slot_extend_secs(&self) -> i64 {
        self.slot_ttl_policy
            .max_extend_secs
            .clamp(self.slot_ttl_policy.min_secs, self.slot_ttl_policy.max_secs)
    }
}

impl FlowRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_flow_runtime_policy(&source)
    }
}

impl CascadeRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_cascade_policy(&source)
    }

    pub(crate) fn env_or_default_manifest_path(&self) -> PathBuf {
        std::env::var("UNIVERSE_MANIFEST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.default_manifest_path.clone())
    }

    pub(crate) fn env_or_allowed_root(&self) -> PathBuf {
        std::env::var("UNIVERSE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.allowed_root.clone())
    }

    pub(crate) fn env_or_trigger_enabled(&self) -> bool {
        std::env::var("CASCADE_TRIGGER_ENABLED")
            .ok()
            .and_then(|value| parse_bool_token(&value))
            .unwrap_or(self.trigger_enabled)
    }

    pub(crate) fn clamp_max_cycles(&self, max_cycles: Option<usize>) -> usize {
        let raw = max_cycles
            .filter(|value| *value > 0)
            .unwrap_or(self.default_max_cycles);
        raw.clamp(1, self.max_cycles_limit.max(1))
    }
}

impl ProjectRegistryRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_project_registry_policy(&source)
    }

    pub(crate) fn env_or_default_universe_manifest(&self) -> PathBuf {
        std::env::var("UNIVERSE_MANIFEST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.default_universe_manifest.clone())
    }
}

impl CapabilityGovernanceRuntimeConfig {
    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_capability_governance_policy(&source)
    }

    pub(crate) fn is_protected_tool(&self, name: &str) -> bool {
        self.protected_tool_patterns.iter().any(|pattern| {
            if pattern.ends_with('_') {
                name.starts_with(pattern)
            } else {
                name == pattern
            }
        })
    }

    pub(crate) fn is_protected_flow(&self, name: &str) -> bool {
        self.protected_flow_patterns
            .iter()
            .any(|pattern| name == pattern || name.starts_with(pattern))
    }
}

impl MemoryKbRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_memory_kb_policy(&source)
    }
}

impl ConversationIngestionRuntimeConfig {
    pub(crate) fn load_for_current_dir() -> Result<Self, BlueprintConfigError> {
        let cwd = std::env::current_dir().map_err(|err| BlueprintConfigError::Read {
            path: PathBuf::from("."),
            message: err.to_string(),
        })?;
        let root = nearest_missiond_root(&cwd);
        Self::load_for_project_root(Some(root.to_string_lossy().as_ref()))
    }

    pub(crate) fn load_for_project_root(
        project_root: Option<&str>,
    ) -> Result<Self, BlueprintConfigError> {
        let Some(root) = project_root.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(Self::default());
        };
        let root = Path::new(root);
        let missiond_dir = root.join(".missiond");
        let blueprint_path = missiond_dir.join("v3").join("missiond-blueprint.lisp");
        if !blueprint_path.exists() {
            if missiond_dir.exists() {
                return Err(BlueprintConfigError::MissingBlueprint(blueprint_path));
            }
            return Ok(Self::default());
        }
        let source =
            fs::read_to_string(&blueprint_path).map_err(|err| BlueprintConfigError::Read {
                path: blueprint_path.clone(),
                message: err.to_string(),
            })?;
        parse_conversation_ingestion_policy(&source)
    }

    pub(crate) fn timeline_query_limit(&self, requested: Option<i64>) -> i64 {
        requested
            .unwrap_or(self.timeline_query_default_limit)
            .min(self.timeline_query_max_limit)
    }

    pub(crate) fn timeline_search_limit(&self, requested: Option<i64>) -> i64 {
        requested
            .unwrap_or(self.timeline_search_default_limit)
            .min(self.timeline_search_max_limit)
    }
}

pub(crate) fn parse_workstation_config(
    source: &str,
) -> Result<WorkstationRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "workstation-config")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (workstation-config ...)".into()))?;
    let mut config = WorkstationRuntimeConfig::default();
    for form in find_forms(&block, "slot-template") {
        let tokens = tokenize_lisp(&form);
        if tokens.len() < 3 {
            continue;
        }
        let template = tokens[2].clone();
        if let Some(profile) = keyword_value(&tokens, ":default-model-profile") {
            config.slot_default_profiles.insert(template, profile);
        }
    }
    let timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens
                .get(2)
                .is_some_and(|name| name == "boardtask-dispatch")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy boardtask-dispatch ...) in workstation-config".into(),
            )
        })?;
    let timeout_tokens = tokenize_lisp(&timeout_form);
    config.timeout_policy = TimeoutPolicy {
        default_secs: int_keyword(&timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&timeout_tokens, ":max_secs")?,
        watchdog_grace_secs: int_keyword(&timeout_tokens, ":watchdog_grace_secs")?,
        missing_session_probe_secs: int_keyword(&timeout_tokens, ":missing_session_probe_secs")?,
    };
    let cc_swarm_timeout_form = find_forms(&block, "timeout-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "claudecode-swarm")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (timeout-policy claudecode-swarm ...) in workstation-config".into(),
            )
        })?;
    let cc_swarm_timeout_tokens = tokenize_lisp(&cc_swarm_timeout_form);
    config.cc_swarm_timeout_policy = SimpleTimeoutPolicy {
        default_secs: int_keyword(&cc_swarm_timeout_tokens, ":default_secs")?,
        min_secs: int_keyword(&cc_swarm_timeout_tokens, ":min_secs")?,
        max_secs: int_keyword(&cc_swarm_timeout_tokens, ":max_secs")?,
    };
    let ttl_form = find_forms(&block, "ttl-policy")
        .into_iter()
        .find(|form| {
            let tokens = tokenize_lisp(form);
            tokens.get(2).is_some_and(|name| name == "dynamic-slot")
        })
        .ok_or_else(|| {
            BlueprintConfigError::Parse(
                "missing (ttl-policy dynamic-slot ...) in workstation-config".into(),
            )
        })?;
    let ttl_tokens = tokenize_lisp(&ttl_form);
    config.slot_ttl_policy = SlotTtlPolicy {
        default_secs: int_keyword(&ttl_tokens, ":default_secs")?,
        min_secs: int_keyword(&ttl_tokens, ":min_secs")?,
        max_secs: int_keyword(&ttl_tokens, ":max_secs")?,
        default_extend_secs: int_keyword(&ttl_tokens, ":default_extend_secs")?,
        max_extend_secs: int_keyword(&ttl_tokens, ":max_extend_secs")?,
    };
    if config.timeout_policy.min_secs > config.timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "workstation timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.cc_swarm_timeout_policy.min_secs > config.cc_swarm_timeout_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "claudecode-swarm timeout :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.cc_swarm_timeout_policy.default_secs < config.cc_swarm_timeout_policy.min_secs
        || config.cc_swarm_timeout_policy.default_secs > config.cc_swarm_timeout_policy.max_secs
    {
        return Err(BlueprintConfigError::Parse(
            "claudecode-swarm timeout :default_secs must be within :min_secs..:max_secs".into(),
        ));
    }
    if config.slot_ttl_policy.min_secs > config.slot_ttl_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot ttl :min_secs must be <= :max_secs".into(),
        ));
    }
    if config.slot_ttl_policy.default_extend_secs > config.slot_ttl_policy.max_extend_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :default_extend_secs must be <= :max_extend_secs".into(),
        ));
    }
    if config.slot_ttl_policy.max_extend_secs < config.slot_ttl_policy.min_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :max_extend_secs must be >= :min_secs".into(),
        ));
    }
    if config.slot_ttl_policy.max_extend_secs > config.slot_ttl_policy.max_secs {
        return Err(BlueprintConfigError::Parse(
            "dynamic-slot :max_extend_secs must be <= :max_secs".into(),
        ));
    }
    Ok(config)
}

pub(crate) fn parse_flow_runtime_policy(
    source: &str,
) -> Result<FlowRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "flow-runtime-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (flow-runtime-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let slot_task_default_model = keyword_value(&tokens, ":slot-task-default-model")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :slot-task-default-model".into()))?;
    let cfg = FlowRuntimeConfig {
        llm_call_default_max_tokens: u32_keyword(&tokens, ":llm-call-default-max-tokens")?,
        slot_task_default_model,
        slot_task_default_timeout_secs: u64_keyword(&tokens, ":slot-task-default-timeout-secs")?,
        parallel_slot_default_parallelism: usize_keyword(
            &tokens,
            ":parallel-slot-default-parallelism",
        )?,
        parallel_slot_default_timeout_secs: u64_keyword(
            &tokens,
            ":parallel-slot-default-timeout-secs",
        )?,
    };
    if cfg.slot_task_default_model.trim().is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":slot-task-default-model must not be empty".into(),
        ));
    }
    if cfg.parallel_slot_default_parallelism == 0 {
        return Err(BlueprintConfigError::Parse(
            ":parallel-slot-default-parallelism must be positive".into(),
        ));
    }
    Ok(cfg)
}

pub(crate) fn parse_project_registry_policy(
    source: &str,
) -> Result<ProjectRegistryRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "project-registry-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (project-registry-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let intent_path_candidates = string_list_keyword(&tokens, ":intent-path-candidates")?;
    if intent_path_candidates.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":intent-path-candidates must not be empty".into(),
        ));
    }
    let default_universe_manifest = keyword_value(&tokens, ":default-universe-manifest")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :default-universe-manifest".into()))?;
    Ok(ProjectRegistryRuntimeConfig {
        intent_path_candidates,
        default_universe_manifest: PathBuf::from(default_universe_manifest),
    })
}

pub(crate) fn parse_cascade_policy(
    source: &str,
) -> Result<CascadeRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "cascade-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (cascade-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let default_manifest_path = keyword_value(&tokens, ":default-manifest")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :default-manifest".into()))?;
    let allowed_root = keyword_value(&tokens, ":allowed-root")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :allowed-root".into()))?;
    let trigger_enabled = keyword_value(&tokens, ":trigger-enabled")
        .and_then(|value| parse_bool_token(&value))
        .ok_or_else(|| {
            BlueprintConfigError::Parse(":trigger-enabled must be true or false".into())
        })?;
    let default_max_cycles = usize_keyword(&tokens, ":default-max-cycles")?;
    let max_cycles_limit = usize_keyword(&tokens, ":max-cycles-limit")?;
    if default_max_cycles == 0 {
        return Err(BlueprintConfigError::Parse(
            ":default-max-cycles must be positive".into(),
        ));
    }
    if max_cycles_limit < default_max_cycles {
        return Err(BlueprintConfigError::Parse(
            ":max-cycles-limit must be >= :default-max-cycles".into(),
        ));
    }
    Ok(CascadeRuntimeConfig {
        default_manifest_path: PathBuf::from(default_manifest_path),
        allowed_root: PathBuf::from(allowed_root),
        trigger_enabled,
        default_max_cycles,
        max_cycles_limit,
    })
}

pub(crate) fn parse_capability_governance_policy(
    source: &str,
) -> Result<CapabilityGovernanceRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "capability-governance-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (capability-governance-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let review_sidecar_path = keyword_value(&tokens, ":review-sidecar")
        .ok_or_else(|| BlueprintConfigError::Parse("missing :review-sidecar".into()))?;
    let protected_tool_patterns = string_list_keyword(&tokens, ":protected-tool-patterns")?;
    if protected_tool_patterns.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":protected-tool-patterns must not be empty".into(),
        ));
    }
    let protected_flow_patterns = string_list_keyword(&tokens, ":protected-flow-patterns")?;
    if protected_flow_patterns.is_empty() {
        return Err(BlueprintConfigError::Parse(
            ":protected-flow-patterns must not be empty".into(),
        ));
    }
    Ok(CapabilityGovernanceRuntimeConfig {
        review_sidecar_path: PathBuf::from(review_sidecar_path),
        protected_tool_patterns,
        protected_flow_patterns,
    })
}

pub(crate) fn parse_memory_kb_policy(
    source: &str,
) -> Result<MemoryKbRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "memory-kb-policy")
        .ok_or_else(|| BlueprintConfigError::Parse("missing (memory-kb-policy ...)".into()))?;
    let tokens = tokenize_lisp(&block);
    let pending_message_limit = usize_keyword(&tokens, ":pending-message-limit")?;
    let tool_result_preview_chars = usize_keyword(&tokens, ":tool-result-preview-chars")?;
    let assistant_preview_chars = usize_keyword(&tokens, ":assistant-preview-chars")?;
    if pending_message_limit == 0 {
        return Err(BlueprintConfigError::Parse(
            ":pending-message-limit must be positive".into(),
        ));
    }
    if tool_result_preview_chars == 0 || assistant_preview_chars == 0 {
        return Err(BlueprintConfigError::Parse(
            "memory preview char limits must be positive".into(),
        ));
    }
    Ok(MemoryKbRuntimeConfig {
        pending_message_limit,
        tool_result_preview_chars,
        assistant_preview_chars,
    })
}

pub(crate) fn parse_conversation_ingestion_policy(
    source: &str,
) -> Result<ConversationIngestionRuntimeConfig, BlueprintConfigError> {
    let block = find_form(source, "conversation-ingestion-policy").ok_or_else(|| {
        BlueprintConfigError::Parse("missing (conversation-ingestion-policy ...)".into())
    })?;
    let tokens = tokenize_lisp(&block);
    let cfg = ConversationIngestionRuntimeConfig {
        conversation_get_tail_default: int_keyword(&tokens, ":conversation-get-tail-default")?,
        conversation_search_default_limit: int_keyword(
            &tokens,
            ":conversation-search-default-limit",
        )?,
        message_search_default_limit: int_keyword(&tokens, ":message-search-default-limit")?,
        context_before_default: int_keyword(&tokens, ":context-before-default")?,
        context_after_default: int_keyword(&tokens, ":context-after-default")?,
        conversation_events_default_limit: int_keyword(
            &tokens,
            ":conversation-events-default-limit",
        )?,
        agent_trajectory_default_limit: int_keyword(&tokens, ":agent-trajectory-default-limit")?,
        timeline_query_default_limit: int_keyword(&tokens, ":timeline-query-default-limit")?,
        timeline_query_max_limit: int_keyword(&tokens, ":timeline-query-max-limit")?,
        timeline_search_default_limit: int_keyword(&tokens, ":timeline-search-default-limit")?,
        timeline_search_max_limit: int_keyword(&tokens, ":timeline-search-max-limit")?,
    };
    if [
        cfg.conversation_get_tail_default,
        cfg.conversation_search_default_limit,
        cfg.message_search_default_limit,
        cfg.context_before_default,
        cfg.context_after_default,
        cfg.conversation_events_default_limit,
        cfg.agent_trajectory_default_limit,
        cfg.timeline_query_default_limit,
        cfg.timeline_query_max_limit,
        cfg.timeline_search_default_limit,
        cfg.timeline_search_max_limit,
    ]
    .iter()
    .any(|value| *value <= 0)
    {
        return Err(BlueprintConfigError::Parse(
            "conversation-ingestion numeric limits must be positive".into(),
        ));
    }
    if cfg.timeline_query_max_limit < cfg.timeline_query_default_limit {
        return Err(BlueprintConfigError::Parse(
            ":timeline-query-max-limit must be >= :timeline-query-default-limit".into(),
        ));
    }
    if cfg.timeline_search_max_limit < cfg.timeline_search_default_limit {
        return Err(BlueprintConfigError::Parse(
            ":timeline-search-max-limit must be >= :timeline-search-default-limit".into(),
        ));
    }
    Ok(cfg)
}

fn string_list_keyword(tokens: &[String], key: &str) -> Result<Vec<String>, BlueprintConfigError> {
    let Some(pos) = tokens.iter().position(|token| token == key) else {
        return Err(BlueprintConfigError::Parse(format!("missing {}", key)));
    };
    let Some(next) = tokens.get(pos + 1) else {
        return Err(BlueprintConfigError::Parse(format!(
            "missing value for {}",
            key
        )));
    };
    if next != "[" {
        return Ok(vec![next.clone()]);
    }
    let mut out = Vec::new();
    for token in tokens.iter().skip(pos + 2) {
        if token == "]" {
            return Ok(out);
        }
        out.push(token.clone());
    }
    Err(BlueprintConfigError::Parse(format!(
        "{} list must close with ]",
        key
    )))
}

fn int_keyword(tokens: &[String], key: &str) -> Result<i64, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<i64>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be an integer", key)))
}

fn u32_keyword(tokens: &[String], key: &str) -> Result<u32, BlueprintConfigError> {
    let value = int_keyword(tokens, key)?;
    if value <= 0 || value > u32::MAX as i64 {
        return Err(BlueprintConfigError::Parse(format!(
            "{} must be a positive u32",
            key
        )));
    }
    Ok(value as u32)
}

fn u64_keyword(tokens: &[String], key: &str) -> Result<u64, BlueprintConfigError> {
    let value = int_keyword(tokens, key)?;
    if value <= 0 {
        return Err(BlueprintConfigError::Parse(format!(
            "{} must be a positive u64",
            key
        )));
    }
    Ok(value as u64)
}

fn usize_keyword(tokens: &[String], key: &str) -> Result<usize, BlueprintConfigError> {
    let value = keyword_value(tokens, key)
        .ok_or_else(|| BlueprintConfigError::Parse(format!("missing {}", key)))?;
    value
        .parse::<usize>()
        .map_err(|_| BlueprintConfigError::Parse(format!("{} must be a positive integer", key)))
}

fn keyword_value(tokens: &[String], key: &str) -> Option<String> {
    tokens
        .windows(2)
        .find(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
}

fn parse_bool_token(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "on" => Some(true),
        "false" | "nil" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn nearest_missiond_root(start: &Path) -> PathBuf {
    start
        .ancestors()
        .find(|candidate| candidate.join(".missiond").exists())
        .unwrap_or(start)
        .to_path_buf()
}

fn find_forms(source: &str, head: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let Some((start, end)) = find_form_span(&source[offset..], head) else {
            break;
        };
        let absolute_start = offset + start;
        let absolute_end = offset + end;
        out.push(source[absolute_start..absolute_end].to_string());
        offset = absolute_end;
    }
    out
}

fn find_form(source: &str, head: &str) -> Option<String> {
    let (start, end) = find_form_span(source, head)?;
    Some(source[start..end].to_string())
}

fn find_form_span(source: &str, head: &str) -> Option<(usize, usize)> {
    let needle = format!("({}", head);
    let mut offset = 0;
    while offset < source.len() {
        let rel = source[offset..].find(&needle)?;
        let start = offset + rel;
        let after = source[start + needle.len()..].chars().next();
        if after.is_none_or(|c| c.is_whitespace() || c == ')' || c == '(') {
            let end = scan_form_end(source, start)?;
            return Some((start, end));
        }
        offset = start + needle.len();
    }
    None
}

fn scan_form_end(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    for (idx, ch) in source[start..].char_indices() {
        let abs = start + idx;
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            ';' => in_comment = true,
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(abs + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn tokenize_lisp(source: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escape = false;
    let mut in_comment = false;
    while let Some(ch) = chars.next() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                current.push(ch);
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                tokens.push(std::mem::take(&mut current));
                in_string = false;
            } else {
                current.push(ch);
            }
            continue;
        }
        match ch {
            ';' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_comment = true;
            }
            '"' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                in_string = true;
            }
            '(' | ')' | '[' | ']' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUEPRINT: &str = r#"
(missiond-blueprint
  (workstation-config
    (model-profile coding-default-opus-4-7 :spawn-model-arg nil)
    (slot-template coder :role coder :default-model-profile coding-default-opus-4-7)
    (slot-template researcher :role coder :default-model-profile coding-default-opus-4-7)
    (slot-template ops :role operator :default-model-profile daily-sonnet)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600))
  (flow-runtime-policy
    :llm-call-default-max-tokens 65536
    :slot-task-default-model "opus"
    :slot-task-default-timeout-secs 3600
    :parallel-slot-default-parallelism 3
    :parallel-slot-default-timeout-secs 1800)
  (cascade-policy
    :default-manifest "/Users/jinchen/Projects/universe.intent.lisp"
    :allowed-root "/Users/jinchen/Projects"
    :trigger-enabled true
    :default-max-cycles 3
    :max-cycles-limit 12)
  (project-registry-policy
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp")
  (capability-governance-policy
    :review-sidecar ".missiond/v2/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution" "mission_intent" "mission_forge_" "mission_sys_" "mission_daemon_update" "mission_health" "mission_power_control" "mission_kb_ops" "mission_audit" "mission_pty_signal" "mission_pty_confirm" "mission_incident"]
    :protected-flow-patterns ["engineering" "F-execution-log-governance" "F-incident-reaction" "F-capability-usage-monitoring"])
  (memory-kb-policy
    :pending-message-limit 60
    :tool-result-preview-chars 1000
    :assistant-preview-chars 500)
  (conversation-ingestion-policy
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100))
"#;

    #[test]
    fn parses_workstation_config_defaults() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(
            cfg.default_model_profile_for_template("coder"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("researcher"),
            Some(DEFAULT_MODEL_PROFILE)
        );
        assert_eq!(
            cfg.default_model_profile_for_template("ops"),
            Some("daily-sonnet")
        );
        assert_eq!(cfg.timeout_policy.default_secs, 1800);
        assert_eq!(cfg.timeout_policy.min_secs, 60);
        assert_eq!(cfg.timeout_policy.max_secs, 7200);
        assert_eq!(cfg.timeout_policy.watchdog_grace_secs, 120);
        assert_eq!(cfg.cc_swarm_timeout_policy.default_secs, 600);
        assert_eq!(cfg.cc_swarm_timeout_policy.min_secs, 60);
        assert_eq!(cfg.cc_swarm_timeout_policy.max_secs, 7200);
        assert_eq!(cfg.slot_ttl_policy.default_secs, 14400);
        assert_eq!(cfg.slot_ttl_policy.min_secs, 300);
        assert_eq!(cfg.slot_ttl_policy.max_secs, 28800);
        assert_eq!(cfg.slot_ttl_policy.default_extend_secs, 3600);
        assert_eq!(cfg.slot_ttl_policy.max_extend_secs, 3600);
    }

    #[test]
    fn timeout_policy_clamps_values() {
        let cfg = parse_workstation_config(BLUEPRINT).expect("parse");
        assert_eq!(cfg.clamp_timeout_secs(None), 1800);
        assert_eq!(cfg.clamp_timeout_secs(Some(5)), 60);
        assert_eq!(cfg.clamp_timeout_secs(Some(99999)), 7200);
        assert_eq!(cfg.clamp_timeout_secs(Some(3300)), 3300);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(None), 600_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(1000)), 60_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(99_999_999)), 7_200_000);
        assert_eq!(cfg.clamp_cc_swarm_timeout_ms(Some(900_000)), 900_000);
        assert_eq!(cfg.clamp_slot_ttl_secs(None), 14400);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(5)), 300);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(99_999)), 28800);
        assert_eq!(cfg.clamp_slot_ttl_secs(Some(3600)), 3600);
        assert_eq!(cfg.default_slot_extend_secs(), 3600);
        assert_eq!(cfg.max_slot_extend_secs(), 3600);
    }

    #[test]
    fn parses_flow_runtime_policy() {
        let cfg = parse_flow_runtime_policy(BLUEPRINT).expect("parse");
        assert_eq!(cfg.llm_call_default_max_tokens, 65536);
        assert_eq!(cfg.slot_task_default_model, "opus");
        assert_eq!(cfg.slot_task_default_timeout_secs, 3600);
        assert_eq!(cfg.parallel_slot_default_parallelism, 3);
        assert_eq!(cfg.parallel_slot_default_timeout_secs, 1800);
    }

    #[test]
    fn missing_flow_runtime_policy_is_rejected() {
        let source = BLUEPRINT.replace("(flow-runtime-policy", "(flow-runtime-policy-disabled");
        let err = parse_flow_runtime_policy(&source).expect_err("missing flow policy");
        assert!(err.to_string().contains("flow-runtime-policy"));
    }

    #[test]
    fn missing_timeout_policy_is_rejected() {
        let err = parse_workstation_config("(missiond-blueprint (workstation-config))")
            .expect_err("missing policy");
        assert!(err
            .to_string()
            .contains("timeout-policy boardtask-dispatch"));
    }

    #[test]
    fn missing_ttl_policy_is_rejected() {
        let source = r#"
(missiond-blueprint
  (workstation-config
    (slot-template coder :role coder :default-model-profile coding-default-opus-4-7)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)))
"#;
        let err = parse_workstation_config(source).expect_err("missing ttl policy");
        assert!(err.to_string().contains("ttl-policy dynamic-slot"));
    }

    #[test]
    fn parses_cascade_policy_defaults() {
        let cfg = parse_cascade_policy(BLUEPRINT).expect("parse cascade policy");
        assert_eq!(
            cfg.default_manifest_path,
            PathBuf::from(DEFAULT_CASCADE_MANIFEST_PATH)
        );
        assert_eq!(
            cfg.allowed_root,
            PathBuf::from(DEFAULT_CASCADE_ALLOWED_ROOT)
        );
        assert!(cfg.trigger_enabled);
        assert_eq!(cfg.default_max_cycles, 3);
        assert_eq!(cfg.max_cycles_limit, 12);
        assert_eq!(cfg.clamp_max_cycles(None), 3);
        assert_eq!(cfg.clamp_max_cycles(Some(0)), 3);
        assert_eq!(cfg.clamp_max_cycles(Some(8)), 8);
        assert_eq!(cfg.clamp_max_cycles(Some(99)), 12);
    }

    #[test]
    fn missing_cascade_policy_is_rejected() {
        let err = parse_cascade_policy("(missiond-blueprint)")
            .expect_err("missing cascade policy should fail");
        assert!(err.to_string().contains("cascade-policy"));
    }

    #[test]
    fn parses_project_registry_policy_defaults() {
        let cfg = parse_project_registry_policy(BLUEPRINT).expect("parse project policy");
        assert_eq!(
            cfg.intent_path_candidates,
            vec![
                ".missiond/intent.lisp".to_string(),
                ".jarvis/intent.lisp".to_string(),
                "intent.lisp".to_string()
            ]
        );
        assert_eq!(
            cfg.default_universe_manifest,
            PathBuf::from(DEFAULT_PROJECT_UNIVERSE_MANIFEST)
        );
    }

    #[test]
    fn missing_project_registry_policy_is_rejected() {
        let err = parse_project_registry_policy("(missiond-blueprint)")
            .expect_err("missing project registry policy should fail");
        assert!(err.to_string().contains("project-registry-policy"));
    }

    #[test]
    fn parses_capability_governance_policy_defaults() {
        let cfg = parse_capability_governance_policy(BLUEPRINT)
            .expect("parse capability governance policy");
        assert_eq!(
            cfg.review_sidecar_path,
            PathBuf::from(DEFAULT_CAPABILITY_REVIEW_SIDECAR)
        );
        assert!(cfg.is_protected_tool("mission_intent"));
        assert!(cfg.is_protected_tool("mission_forge_build"));
        assert!(cfg.is_protected_tool("mission_audit"));
        assert!(!cfg.is_protected_tool("mission_board_query"));
        assert!(cfg.is_protected_flow("engineering"));
        assert!(cfg.is_protected_flow("F-execution-log-governance"));
        assert!(!cfg.is_protected_flow("hello-parallel"));
    }

    #[test]
    fn missing_capability_governance_policy_is_rejected() {
        let err = parse_capability_governance_policy("(missiond-blueprint)")
            .expect_err("missing capability governance policy should fail");
        assert!(err.to_string().contains("capability-governance-policy"));
    }

    #[test]
    fn parses_memory_kb_policy_defaults() {
        let cfg = parse_memory_kb_policy(BLUEPRINT).expect("parse memory kb policy");
        assert_eq!(
            cfg.pending_message_limit,
            DEFAULT_MEMORY_PENDING_MESSAGE_LIMIT
        );
        assert_eq!(
            cfg.tool_result_preview_chars,
            DEFAULT_MEMORY_TOOL_RESULT_PREVIEW_CHARS
        );
        assert_eq!(
            cfg.assistant_preview_chars,
            DEFAULT_MEMORY_ASSISTANT_PREVIEW_CHARS
        );
    }

    #[test]
    fn missing_memory_kb_policy_is_rejected() {
        let err = parse_memory_kb_policy("(missiond-blueprint)")
            .expect_err("missing memory kb policy should fail");
        assert!(err.to_string().contains("memory-kb-policy"));
    }

    #[test]
    fn parses_conversation_ingestion_policy_defaults() {
        let cfg = parse_conversation_ingestion_policy(BLUEPRINT)
            .expect("parse conversation ingestion policy");
        assert_eq!(
            cfg.conversation_get_tail_default,
            DEFAULT_CONVERSATION_GET_TAIL
        );
        assert_eq!(
            cfg.conversation_search_default_limit,
            DEFAULT_CONVERSATION_SEARCH_LIMIT
        );
        assert_eq!(
            cfg.message_search_default_limit,
            DEFAULT_MESSAGE_SEARCH_LIMIT
        );
        assert_eq!(cfg.context_before_default, DEFAULT_CONTEXT_BEFORE);
        assert_eq!(cfg.context_after_default, DEFAULT_CONTEXT_AFTER);
        assert_eq!(
            cfg.conversation_events_default_limit,
            DEFAULT_CONVERSATION_EVENTS_LIMIT
        );
        assert_eq!(
            cfg.agent_trajectory_default_limit,
            DEFAULT_AGENT_TRAJECTORY_LIMIT
        );
        assert_eq!(cfg.timeline_query_limit(None), DEFAULT_TIMELINE_QUERY_LIMIT);
        assert_eq!(
            cfg.timeline_query_limit(Some(999)),
            MAX_TIMELINE_QUERY_LIMIT
        );
        assert_eq!(
            cfg.timeline_search_limit(None),
            DEFAULT_TIMELINE_SEARCH_LIMIT
        );
        assert_eq!(
            cfg.timeline_search_limit(Some(999)),
            MAX_TIMELINE_SEARCH_LIMIT
        );
    }

    #[test]
    fn missing_conversation_ingestion_policy_is_rejected() {
        let err = parse_conversation_ingestion_policy("(missiond-blueprint)")
            .expect_err("missing conversation ingestion policy should fail");
        assert!(err.to_string().contains("conversation-ingestion-policy"));
    }
}

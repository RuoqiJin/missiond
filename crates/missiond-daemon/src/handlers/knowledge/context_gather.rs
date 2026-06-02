use anyhow::Result;
use missiond_core::types::{ContextGatherRunInput, EvidenceItemInput, EvidenceSearchInput};
use missiond_mcp::tools::{ToolContent, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use crate::context::v3_blueprint_runtime::{
    load_compiled_project_universe, CompiledServiceRuntimeEntry, CompiledServiceSupportCatalog,
    EvidenceLaneRuntimeConfig, EvidenceLaneRuntimeEntry,
};
use crate::feature_gates;
use crate::handlers::comm::conversation;
use crate::handlers::sysinfra::infra;
use crate::state::AppState;

use super::{board, intent, kb, project, skill};

#[derive(Debug, Deserialize)]
struct ContextGatherArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default, alias = "sourceProfile")]
    source_profile: Option<String>,
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "infraTarget")]
    infra_target: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    unknowns: Vec<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_kb: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_ssot: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_project: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_skill: Option<bool>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_infra: Option<bool>,
    #[serde(
        default,
        alias = "includeBoard",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_board: Option<bool>,
    #[serde(
        default,
        alias = "includeConversations",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_conversations: Option<bool>,
    #[serde(
        default,
        alias = "includeCredentials",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_credentials: Option<bool>,
    #[serde(
        default,
        alias = "includeRawSources",
        deserialize_with = "crate::lenient::option_bool"
    )]
    include_raw_sources: Option<bool>,
    #[serde(default, alias = "conversationTimeRange")]
    conversation_time_range: Option<String>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default, alias = "sourceId")]
    source_id: Option<String>,
    #[serde(default, alias = "conversationId", alias = "conversation_id")]
    conversation_id: Option<String>,
    #[serde(default, alias = "userId", alias = "user_id")]
    user_id: Option<String>,
    #[serde(default, alias = "tenantId", alias = "tenant_id")]
    tenant_id: Option<String>,
    #[serde(default, alias = "applicationId", alias = "application_id")]
    application_id: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default, alias = "topicId", alias = "topic_id")]
    topic_id: Option<String>,
    #[serde(default, alias = "topicLabel", alias = "topic_label")]
    topic_label: Option<String>,
    #[serde(default, alias = "permissionContext", alias = "permission_context")]
    permission_context: Option<Value>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    persist: Option<bool>,
    #[serde(
        default,
        alias = "persistReadModel",
        deserialize_with = "crate::lenient::option_bool"
    )]
    persist_read_model: Option<bool>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Default, Deserialize)]
struct ContextBootArgs {
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default, alias = "taskId")]
    task_id: Option<String>,
    #[serde(default, deserialize_with = "crate::lenient::option_bool")]
    include_capsule: Option<bool>,
}

const CODEX_BOOT_CONTEXT_REL: &str = ".missiond/v3/evidence/codex-boot-context.lisp";
const CODEX_BOOT_CONTEXT_FALLBACK: &str =
    include_str!("../../../../../.missiond/v3/evidence/codex-boot-context.lisp");
const CONTEXT_GATHER_RUNTIME_REL: &str = ".missiond/v3/runtime/context-gather";
const CONTEXT_GATHER_WORKER_VISIBLE_REL: &str = ".missiond/v3/runtime/context-gather-worker";

fn default_limit() -> usize {
    8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceProfile {
    IntentDefault,
    DeployOps,
    ConversationAudit,
    FullDebug,
}

impl SourceProfile {
    fn from_arg(value: Option<&str>) -> Self {
        match value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("intent_default")
        {
            "deploy_ops" | "deploy-ops" => Self::DeployOps,
            "conversation_audit" | "conversation-audit" => Self::ConversationAudit,
            "full_debug" | "full-debug" => Self::FullDebug,
            _ => Self::IntentDefault,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::IntentDefault => "intent_default",
            Self::DeployOps => "deploy_ops",
            Self::ConversationAudit => "conversation_audit",
            Self::FullDebug => "full_debug",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SourceSelection {
    include_project: bool,
    include_ssot: bool,
    include_kb: bool,
    include_skill: bool,
    include_infra: bool,
    include_board: bool,
    include_conversations: bool,
    include_credentials: bool,
    include_raw_sources: bool,
}

fn flag(default: bool, override_value: Option<bool>) -> bool {
    override_value.unwrap_or(default)
}

fn source_selection(args: &ContextGatherArgs, profile: SourceProfile) -> SourceSelection {
    let policy = EvidenceLaneRuntimeConfig::default();
    let allowed_lanes = allowed_lanes_for_profile(&policy, profile);
    source_selection_with_allowed_lanes(args, profile, &allowed_lanes)
}

fn context_gather_persist_artifact(args: &ContextGatherArgs) -> bool {
    args.persist.unwrap_or(false)
}

fn context_gather_persist_read_model(args: &ContextGatherArgs) -> bool {
    context_gather_persist_artifact(args) || args.persist_read_model.unwrap_or(true)
}

fn source_selection_with_allowed_lanes(
    args: &ContextGatherArgs,
    profile: SourceProfile,
    allowed_lanes: &[String],
) -> SourceSelection {
    let explicit_skill = normalized_scope_value(args.skill.as_deref()).is_some();
    let explicit_infra = normalized_scope_value(args.infra_target.as_deref()).is_some();
    let deploy_ops = profile == SourceProfile::DeployOps;
    let conversation_audit = profile == SourceProfile::ConversationAudit;
    let full_debug = profile == SourceProfile::FullDebug;
    let lane_enabled = |lane_id: &str| allowed_lanes.iter().any(|lane| lane == lane_id);

    SourceSelection {
        include_project: flag(
            lane_enabled("project_ssot") || lane_enabled("support_refs"),
            args.include_project,
        ),
        include_ssot: flag(lane_enabled("project_ssot"), args.include_ssot),
        include_kb: flag(lane_enabled("reviewed_kb"), args.include_kb),
        include_skill: flag(
            lane_enabled("skill_evidence") && (full_debug || deploy_ops || explicit_skill),
            args.include_skill,
        ),
        include_infra: flag(
            lane_enabled("skill_evidence")
                && (full_debug || deploy_ops || explicit_infra || explicit_skill),
            args.include_infra,
        ),
        include_board: flag(lane_enabled("active_board"), args.include_board),
        include_conversations: flag(
            lane_enabled("conversation_audit") && (full_debug || conversation_audit),
            args.include_conversations,
        ),
        include_credentials: flag(
            lane_enabled("support_refs") && (full_debug || deploy_ops),
            args.include_credentials,
        ),
        include_raw_sources: flag(
            lane_enabled("cold_archive") && full_debug,
            args.include_raw_sources,
        ),
    }
}

fn infra_os_feature_enabled() -> bool {
    feature_gates::optional_feature_enabled(feature_gates::INFRA_OS_ENV)
}

fn optional_infra_os_disabled_source(kind: &str, tool: &str) -> Value {
    json!({
        "schema": "missiond.optional-source-status.v1",
        "kind": kind,
        "status": "feature_disabled",
        "feature": "infra-os",
        "layer": "full-os",
        "tool": tool,
        "enable_env": feature_gates::INFRA_OS_ENV,
        "enable_all_env": feature_gates::FULL_OS_ENV,
        "reason": "infra, daemon update, power, and external OS operations are optional operations layers",
        "item_count": 0,
        "items": [],
        "credentialRefs": [],
        "authority": "infra-os optional layer is disabled; support_catalog and project SSOT remain available",
        "redaction": "credential values are never emitted"
    })
}

fn optional_infra_os_disabled_diagnostic(source: &str) -> Value {
    json!({
        "source": source,
        "status": "feature_disabled",
        "feature": "infra-os",
        "enable_env": feature_gates::INFRA_OS_ENV,
        "enable_all_env": feature_gates::FULL_OS_ENV,
        "message": "infra-os optional layer is disabled in kernel-core mode; enable the feature for live infra skill evidence or credential refs"
    })
}

fn diagnostics_have_hard_failures(diagnostics: &[Value]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.get("error").is_some()
            || diagnostic
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status != "feature_disabled")
    })
}

fn load_evidence_lane_policy() -> (EvidenceLaneRuntimeConfig, String, Option<Value>) {
    match EvidenceLaneRuntimeConfig::load_for_current_dir() {
        Ok(policy) => (policy, "compiled-v3".to_string(), None),
        Err(err) => (
            EvidenceLaneRuntimeConfig::default(),
            "embedded-defaults-fallback".to_string(),
            Some(json!({
                "source": "evidence_lane_policy",
                "error": err.to_string(),
                "fallback": "embedded-defaults"
            })),
        ),
    }
}

fn allowed_lanes_for_profile(
    policy: &EvidenceLaneRuntimeConfig,
    profile: SourceProfile,
) -> Vec<String> {
    policy
        .profiles
        .iter()
        .find(|entry| entry.profile == profile.as_str())
        .map(|entry| entry.allowed_lanes.clone())
        .filter(|lanes| !lanes.is_empty())
        .unwrap_or_else(|| default_allowed_lanes_for_profile(profile))
}

fn default_allowed_lanes_for_profile(profile: SourceProfile) -> Vec<String> {
    let lanes = match profile {
        SourceProfile::IntentDefault => &[
            "runtime_truth",
            "project_ssot",
            "reviewed_kb",
            "active_board",
            "support_refs",
        ][..],
        SourceProfile::DeployOps => &[
            "runtime_truth",
            "project_ssot",
            "reviewed_kb",
            "active_board",
            "support_refs",
            "skill_evidence",
        ],
        SourceProfile::ConversationAudit => &[
            "runtime_truth",
            "project_ssot",
            "reviewed_kb",
            "active_board",
            "support_refs",
            "conversation_audit",
        ],
        SourceProfile::FullDebug => &[
            "runtime_truth",
            "project_ssot",
            "reviewed_kb",
            "active_board",
            "support_refs",
            "skill_evidence",
            "conversation_audit",
            "cold_archive",
        ],
    };
    lanes.iter().map(|lane| (*lane).to_string()).collect()
}

async fn search_evidence_item_read_model(
    state: &AppState,
    query: &str,
    allowed_lanes: &[String],
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
    limit: usize,
) -> (Vec<EvidenceItemInput>, Value) {
    if !evidence_item_read_model_scope_allows_search(profile, project_id) {
        return (
            Vec::new(),
            json!({
                "schema": "missiond.evidence-item-search.v1",
                "ok": true,
                "source": "postgres.evidence_items",
                "query": query,
                "allowed_lanes": allowed_lanes,
                "project_id": project_id,
                "task_id": task_id,
                "source_profile": profile.as_str(),
                "include_global": false,
                "scope_skipped": true,
                "scope_skip_reason": "unresolved_unscoped_context_requires_project_or_full_debug",
                "limit": limit,
                "read_limit": 0,
                "raw_hit_count": 0,
                "freshness_filtered_count": 0,
                "compiled_policy_filtered_count": 0,
                "runtime_environment_filtered_count": 0,
                "incomplete_filtered_count": 0,
                "deduplicated_count": 0,
                "truncated_count": 0,
                "hit_count": 0,
                "lane_counts": {},
                "filter_before_vector": true,
            }),
        );
    }

    let input = EvidenceSearchInput {
        query: query.to_string(),
        allowed_lanes: allowed_lanes.to_vec(),
        project_id: project_id.map(ToOwned::to_owned),
        task_id: task_id.map(ToOwned::to_owned),
        include_global: true,
        limit: (limit.saturating_mul(2)).clamp(1, 50) as i64,
    };

    match state.store.search_evidence_items(&input).await {
        Ok(items) => {
            let raw_hit_count = items.len();
            let (items, compiled_policy_filtered_count) =
                filter_stale_compiled_policy_evidence_items(items);
            let (items, runtime_environment_filtered_count) =
                filter_stale_runtime_environment_evidence_items(items);
            let freshness_filtered_count =
                compiled_policy_filtered_count + runtime_environment_filtered_count;
            let (items, incomplete_filtered_count) =
                filter_incomplete_deployment_closure_evidence_items(items);
            let (items, deduplicated_count, truncated_count) =
                dedupe_evidence_search_items(items, limit);
            let lane_counts = lane_counts_for_evidence_items(&items);
            (
                items,
                json!({
                    "schema": "missiond.evidence-item-search.v1",
                    "ok": true,
                    "source": "postgres.evidence_items",
                    "query": query,
                    "allowed_lanes": allowed_lanes,
                    "project_id": project_id,
                    "task_id": task_id,
                    "source_profile": profile.as_str(),
                    "include_global": true,
                    "scope_skipped": false,
                    "limit": limit,
                    "read_limit": input.limit,
                    "raw_hit_count": raw_hit_count,
                    "freshness_filtered_count": freshness_filtered_count,
                    "compiled_policy_filtered_count": compiled_policy_filtered_count,
                    "runtime_environment_filtered_count": runtime_environment_filtered_count,
                    "incomplete_filtered_count": incomplete_filtered_count,
                    "deduplicated_count": deduplicated_count,
                    "truncated_count": truncated_count,
                    "hit_count": lane_counts.values().filter_map(Value::as_u64).sum::<u64>(),
                    "lane_counts": lane_counts,
                    "filter_before_vector": true,
                }),
            )
        }
        Err(err) => (
            Vec::new(),
            json!({
                "schema": "missiond.evidence-item-search.v1",
                "ok": false,
                "source": "postgres.evidence_items",
                "query": query,
                "allowed_lanes": allowed_lanes,
                "project_id": project_id,
                "task_id": task_id,
                "source_profile": profile.as_str(),
                "include_global": true,
                "scope_skipped": false,
                "limit": input.limit,
                "raw_hit_count": 0,
                "freshness_filtered_count": 0,
                "compiled_policy_filtered_count": 0,
                "runtime_environment_filtered_count": 0,
                "incomplete_filtered_count": 0,
                "deduplicated_count": 0,
                "truncated_count": 0,
                "hit_count": 0,
                "lane_counts": {},
                "filter_before_vector": true,
                "error": err.to_string(),
            }),
        ),
    }
}

fn evidence_item_read_model_scope_allows_search(
    profile: SourceProfile,
    project_id: Option<&str>,
) -> bool {
    profile == SourceProfile::FullDebug || normalized_scope_value(project_id).is_some()
}

#[derive(Debug, Clone)]
struct CompiledDeploymentPolicyFingerprint {
    compiled_runtime_dir: PathBuf,
    source_hash: Option<String>,
}

fn filter_stale_compiled_policy_evidence_items(
    items: Vec<EvidenceItemInput>,
) -> (Vec<EvidenceItemInput>, usize) {
    let Some(fingerprint) = active_compiled_deployment_policy_fingerprint() else {
        return (items, 0);
    };
    filter_stale_compiled_policy_evidence_items_with_fingerprint(items, &fingerprint)
}

fn filter_stale_runtime_environment_evidence_items(
    items: Vec<EvidenceItemInput>,
) -> (Vec<EvidenceItemInput>, usize) {
    let Some(compiled_runtime_dir) = active_compiled_runtime_dir() else {
        return (items, 0);
    };
    filter_stale_runtime_environment_evidence_items_with_dir(items, &compiled_runtime_dir)
}

fn filter_incomplete_deployment_closure_evidence_items(
    items: Vec<EvidenceItemInput>,
) -> (Vec<EvidenceItemInput>, usize) {
    let mut filtered_count = 0usize;
    let filtered = items
        .into_iter()
        .filter(|item| {
            if evidence_item_has_incomplete_deployment_closure_placeholder(item) {
                filtered_count += 1;
                false
            } else {
                true
            }
        })
        .collect();
    (filtered, filtered_count)
}

fn evidence_item_has_incomplete_deployment_closure_placeholder(item: &EvidenceItemInput) -> bool {
    let text = format!("{} {}", item.title, item.summary).to_ascii_lowercase();
    if !text.contains("deployment closure") {
        return false;
    }
    [
        "service deployment closure support",
        "deploy center slug deploy-center",
        "runtime target runtime-target",
        "manifest refs []",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn filter_stale_runtime_environment_evidence_items_with_dir(
    items: Vec<EvidenceItemInput>,
    compiled_runtime_dir: &Path,
) -> (Vec<EvidenceItemInput>, usize) {
    let mut filtered_count = 0usize;
    let filtered = items
        .into_iter()
        .filter(|item| {
            if evidence_item_has_stale_runtime_environment_ref(item, compiled_runtime_dir) {
                filtered_count += 1;
                false
            } else {
                true
            }
        })
        .collect();
    (filtered, filtered_count)
}

fn filter_stale_compiled_policy_evidence_items_with_fingerprint(
    items: Vec<EvidenceItemInput>,
    fingerprint: &CompiledDeploymentPolicyFingerprint,
) -> (Vec<EvidenceItemInput>, usize) {
    let mut filtered_count = 0usize;
    let filtered = items
        .into_iter()
        .filter(|item| {
            if evidence_item_has_stale_compiled_policy_ref(item, fingerprint) {
                filtered_count += 1;
                false
            } else {
                true
            }
        })
        .collect();
    (filtered, filtered_count)
}

fn active_compiled_deployment_policy_fingerprint() -> Option<CompiledDeploymentPolicyFingerprint> {
    let dir = active_compiled_runtime_dir()?;
    let path = dir.join("compiled-deployment-policy.json");
    let source_hash = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value
                .get("source_hash")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
    Some(CompiledDeploymentPolicyFingerprint {
        compiled_runtime_dir: dir,
        source_hash,
    })
}

fn active_compiled_runtime_dir() -> Option<PathBuf> {
    env::var("MISSIOND_COMPILED_RUNTIME_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn evidence_item_has_stale_compiled_policy_ref(
    item: &EvidenceItemInput,
    fingerprint: &CompiledDeploymentPolicyFingerprint,
) -> bool {
    if !evidence_item_references_compiled_policy(item) {
        return false;
    }
    let policy_path = evidence_item_compiled_policy_path(item);
    let policy_hash = evidence_item_compiled_policy_hash(item);
    let stale_path = policy_path
        .as_deref()
        .map(|path| !Path::new(path).starts_with(&fingerprint.compiled_runtime_dir))
        .unwrap_or(false);
    let stale_hash = policy_hash
        .as_deref()
        .zip(fingerprint.source_hash.as_deref())
        .map(|(item_hash, active_hash)| item_hash != active_hash)
        .unwrap_or(false);
    stale_path || stale_hash
}

fn evidence_item_has_stale_runtime_environment_ref(
    item: &EvidenceItemInput,
    active_compiled_runtime_dir: &Path,
) -> bool {
    if !evidence_item_references_runtime_environment(item) {
        return false;
    }
    evidence_item_runtime_environment_compiled_dir(item)
        .as_deref()
        .map(|compiled_dir| !Path::new(compiled_dir).starts_with(active_compiled_runtime_dir))
        .unwrap_or(false)
}

fn evidence_item_references_compiled_policy(item: &EvidenceItemInput) -> bool {
    item.source_type == "deployment_closure_policy"
        || evidence_ref_text(&item.evidence_refs, &["source"]) == Some("compiled-deployment-policy")
        || evidence_ref_text(&item.evidence_refs, &["policy", "source"])
            == Some("compiled-deployment-policy")
        || evidence_item_compiled_policy_path(item).is_some()
        || evidence_item_compiled_policy_hash(item).is_some()
}

fn evidence_item_references_runtime_environment(item: &EvidenceItemInput) -> bool {
    item.source_type == "runtime_environment"
        || evidence_ref_text(&item.evidence_refs, &["source"]) == Some("runtime_environment")
        || evidence_item_runtime_environment_compiled_dir(item).is_some()
}

fn evidence_item_compiled_policy_path(item: &EvidenceItemInput) -> Option<String> {
    evidence_ref_text(&item.evidence_refs, &["policy", "path"])
        .or_else(|| evidence_ref_text(&item.evidence_refs, &["path"]))
        .or_else(|| evidence_ref_text(&item.evidence_refs, &["policy_path"]))
        .map(ToOwned::to_owned)
}

fn evidence_item_compiled_policy_hash(item: &EvidenceItemInput) -> Option<String> {
    evidence_ref_text(&item.evidence_refs, &["policy", "source_hash"])
        .or_else(|| evidence_ref_text(&item.evidence_refs, &["source_hash"]))
        .or_else(|| evidence_ref_text(&item.evidence_refs, &["policy_hash"]))
        .map(ToOwned::to_owned)
}

fn evidence_item_runtime_environment_compiled_dir(item: &EvidenceItemInput) -> Option<String> {
    evidence_ref_text(
        &item.evidence_refs,
        &["runtime_environment", "compiled_runtime_dir"],
    )
    .or_else(|| evidence_ref_text(&item.evidence_refs, &["compiled_runtime_dir"]))
    .or_else(|| evidence_ref_text(&item.evidence_refs, &["compiledRuntimeDir"]))
    .map(ToOwned::to_owned)
    .or_else(|| {
        serde_json::from_str::<Value>(&item.summary)
            .ok()
            .and_then(|value| {
                text_field(&value, "compiled_runtime_dir")
                    .or_else(|| text_field(&value, "compiledRuntimeDir"))
            })
    })
}

fn evidence_ref_text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

fn dedupe_evidence_search_items(
    items: Vec<EvidenceItemInput>,
    limit: usize,
) -> (Vec<EvidenceItemInput>, usize, usize) {
    let mut seen = HashSet::new();
    let mut deduplicated_count = 0usize;
    let mut unique_count = 0usize;
    let mut out = Vec::new();
    let return_limit = limit.max(1);
    for item in items {
        let key = evidence_search_dedupe_key(&item);
        if !seen.insert(key) {
            deduplicated_count += 1;
            continue;
        }
        unique_count += 1;
        if out.len() < return_limit {
            out.push(item);
        }
    }
    let truncated_count = unique_count.saturating_sub(out.len());
    (out, deduplicated_count, truncated_count)
}

fn evidence_search_dedupe_key(item: &EvidenceItemInput) -> String {
    format!(
        "{}|{}|{}|{}",
        item.lane_id,
        item.source_type,
        item.project_id
            .as_deref()
            .or(item.source_id.as_deref())
            .unwrap_or(""),
        item.task_id.as_deref().unwrap_or("")
    )
}

fn lane_counts_for_evidence_items(items: &[EvidenceItemInput]) -> serde_json::Map<String, Value> {
    let mut counts = serde_json::Map::new();
    for item in items {
        let count = counts
            .get(item.lane_id.as_str())
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        counts.insert(item.lane_id.clone(), json!(count));
    }
    counts
}

fn normalized_scope_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !matches!(*value, "unknown" | "null" | "undefined"))
        .map(ToOwned::to_owned)
}

fn compact_topic_label(text: &str) -> Option<String> {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if collapsed.is_empty() {
        return None;
    }
    let mut label = collapsed.chars().take(96).collect::<String>();
    if collapsed.chars().count() > 96 {
        label.push_str("...");
    }
    Some(label)
}

fn stable_topic_id(
    user_id: Option<&str>,
    tenant_id: Option<&str>,
    application_id: Option<&str>,
    channel: &str,
    topic_label: Option<&str>,
) -> Option<String> {
    let topic_label = topic_label
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let input = format!(
        "{}|{}|{}|{}|{}",
        tenant_id.unwrap_or(""),
        user_id.unwrap_or(""),
        application_id.unwrap_or("missiond"),
        channel,
        topic_label.to_ascii_lowercase()
    );
    let digest = Sha256::digest(input.as_bytes());
    let short = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Some(format!("topic-{short}"))
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    if name == "mission_context_boot" {
        return handle_context_boot(args);
    }

    let args: ContextGatherArgs = serde_json::from_value(args)?;
    let profile = SourceProfile::from_arg(args.source_profile.as_deref());
    let (evidence_lane_policy, evidence_lane_policy_source, evidence_lane_policy_diagnostic) =
        load_evidence_lane_policy();
    let allowed_lanes = allowed_lanes_for_profile(&evidence_lane_policy, profile);
    let selection = source_selection_with_allowed_lanes(&args, profile, &allowed_lanes);
    let persist_artifact = context_gather_persist_artifact(&args);
    let persist_read_model = context_gather_persist_read_model(&args);
    let query = normalized_query(&args);
    if query.is_empty()
        && args.project_id.is_none()
        && args.skill.is_none()
        && args.infra_target.is_none()
    {
        return Ok(ToolResult::error(
            "mission_context_gather requires query, project/project_id, skill, infra_target, or unknowns",
        ));
    }

    let limit = args.limit.clamp(1, 25);
    let mut sources = serde_json::Map::new();
    let mut diagnostics = Vec::new();
    if let Some(diagnostic) = evidence_lane_policy_diagnostic {
        diagnostics.push(diagnostic);
    }
    let mut project_resolution_payload: Option<Value> = None;
    sources.insert(
        "runtime_environment".to_string(),
        runtime_environment_payload(),
    );

    let mut effective_project_id = args.project_id.clone();
    if effective_project_id.is_none() && !query.is_empty() && selection.include_project {
        match project::handle(
            state,
            "mission_project",
            json!({
                "action": "resolve",
                "query": query,
                "limit": 8,
                "include_unregistered_candidates": true
            }),
        )
        .await
        {
            Ok(result) => {
                let is_error = result.is_error.unwrap_or(false);
                let value = tool_result_to_value(result);
                if is_error {
                    diagnostics.push(json!({"source": "project_resolution", "error": value}));
                } else {
                    if value.get("status").and_then(Value::as_str) == Some("resolved") {
                        effective_project_id = value
                            .get("matched_project_id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                    project_resolution_payload = Some(value.clone());
                    sources.insert("project_resolution".to_string(), value);
                }
            }
            Err(err) => diagnostics.push(json!({
                "source": "project_resolution",
                "error": err.to_string()
            })),
        }
    }

    if selection.include_project {
        if let Some(project_payload) = project_resolution_payload
            .as_ref()
            .and_then(|value| value.get("matched_project"))
            .filter(|value| value.is_object())
            .cloned()
        {
            sources.insert("project_registry".to_string(), project_payload);
        } else {
            let payload = if let Some(project_id) = effective_project_id.as_deref() {
                json!({"action": "get", "id": project_id})
            } else {
                json!({"action": "list"})
            };
            insert_subcall(
                &mut sources,
                &mut diagnostics,
                "project_registry",
                project::handle(state, "mission_project", payload).await,
            );
        }
    }

    if profile == SourceProfile::DeployOps {
        let support_catalog_hint = build_support_catalog(&sources);
        let deployment_events = deployment_events_source(
            state,
            effective_project_id.as_deref(),
            &support_catalog_hint,
            &query,
            limit,
        )
        .await;
        sources.insert("deployment_events".to_string(), deployment_events);
    }

    let (mut evidence_item_inputs, evidence_item_search) = search_evidence_item_read_model(
        state,
        &query,
        &allowed_lanes,
        profile,
        effective_project_id.as_deref(),
        args.task_id.as_deref(),
        limit,
    )
    .await;

    if selection.include_ssot {
        let payload = if let Some(project_id) = effective_project_id.as_deref() {
            json!({"action": "summary", "project": project_id})
        } else {
            json!({"action": "list"})
        };
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "ssot",
            intent::handle(state, "mission_intent", payload).await,
        );
    }

    if selection.include_kb {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "kb",
            kb::handle(
                state,
                "mission_kb_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": effective_project_id.clone(),
                    "limit": limit,
                    "include_archived": false
                }),
            )
            .await,
        );
    }

    if selection.include_skill {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "skill_context",
            skill::handle(
                state,
                "mission_skill_context",
                json!({
                    "action": "resolve",
                    "query": query,
                    "project_id": effective_project_id.clone(),
                    "skill": args.skill.clone(),
                    "include_kb": false,
                    "include_board": false
                }),
            )
            .await,
        );
    }

    if selection.include_infra {
        if !infra_os_feature_enabled() {
            diagnostics.push(optional_infra_os_disabled_diagnostic("infra"));
            sources.insert(
                "infra".to_string(),
                optional_infra_os_disabled_source("infra", "mission_infra_query"),
            );
        } else {
            let infra_payload = if let Some(target_id) = args.infra_target.as_deref() {
                json!({"action": "get", "id": target_id})
            } else {
                json!({
                    "action": "skill_evidence",
                    "skill": args.skill.clone(),
                    "target_id": args.infra_target.clone(),
                    "query": query,
                    "project_id": effective_project_id.clone(),
                    "limit": limit
                })
            };
            insert_subcall(
                &mut sources,
                &mut diagnostics,
                "infra",
                infra::handle(state, "mission_infra_query", infra_payload).await,
            );
        }
    }

    if selection.include_credentials {
        if !infra_os_feature_enabled() {
            diagnostics.push(optional_infra_os_disabled_diagnostic("credential_refs"));
            sources.insert(
                "credential_refs".to_string(),
                optional_infra_os_disabled_source("credential_refs", "mission_infra_query"),
            );
        } else {
            insert_subcall(
                &mut sources,
                &mut diagnostics,
                "credential_refs",
                infra::handle(
                    state,
                    "mission_infra_query",
                    json!({
                        "action": "credential_refs",
                        "target_id": args.infra_target.clone(),
                        "skill": args.skill.clone(),
                        "query": query,
                        "project_id": effective_project_id.clone(),
                        "limit": limit
                    }),
                )
                .await,
            );
        }
    }

    if selection.include_board {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "board_tasks",
            board::handle(
                state,
                "mission_board_query",
                json!({
                    "action": "search",
                    "query": query,
                    "project": effective_project_id.clone(),
                    "scope": "active",
                    "limit": limit
                }),
            )
            .await,
        );
    }

    if selection.include_conversations {
        insert_subcall(
            &mut sources,
            &mut diagnostics,
            "conversation_logs",
            conversation::handle(
                state,
                "mission_conversation_query",
                json!({
                    "action": "search",
                    "query": query,
                        "project": effective_project_id.clone(),
                        "limit": limit,
                        "user_id": args.user_id.clone(),
                        "tenant_id": args.tenant_id.clone(),
                        "application_id": args.application_id.clone(),
                        "channel": args.channel.clone(),
                        "time_range": args
                            .conversation_time_range
                        .as_deref()
                        .unwrap_or("last_30d"),
                    "query_mode": "hybrid"
                }),
            )
            .await,
        );
    }

    attach_infra_os_disabled_support_fallback(&mut sources);
    let evidence_lanes = build_evidence_lanes_from_policy(&sources, &evidence_lane_policy);
    let authority_order = authority_order();
    let noise_diagnostics = noise_diagnostics(profile, selection, &sources);
    let context_noise_metrics = context_noise_metrics(
        profile,
        selection,
        &sources,
        &evidence_lanes,
        &allowed_lanes,
        &evidence_item_search,
    );
    let source_summaries = build_source_summaries(&sources);
    let support_catalog = build_support_catalog(&sources);
    evidence_item_inputs.extend(build_evidence_items(
        &sources,
        &source_summaries,
        &support_catalog,
        profile,
        effective_project_id.as_deref(),
        args.task_id.as_deref(),
    ));
    dedupe_evidence_items(&mut evidence_item_inputs);
    let evidence_items = serde_json::to_value(&evidence_item_inputs).unwrap_or_else(|_| json!([]));
    let response_sources =
        response_sources(&sources, &source_summaries, selection.include_raw_sources);
    let evidence_refs = if selection.include_raw_sources {
        collect_evidence_refs(&sources)
    } else {
        collect_evidence_refs_from_value(&source_summaries)
    };
    let unresolved = args
        .unknowns
        .iter()
        .filter(|unknown| !unknown.trim().is_empty())
        .map(|unknown| {
            json!({
                "unknown": unknown,
                "status": "needs_synthesis",
                "hint": "Review sources and promote to resolved/evidence_gap in the context-pack."
            })
        })
        .collect::<Vec<_>>();

    let sources_used = sources.keys().cloned().collect::<Vec<_>>();
    let runtime_environment = sources
        .get("runtime_environment")
        .cloned()
        .unwrap_or(Value::Null);
    let user_id = normalized_scope_value(args.user_id.as_deref());
    let tenant_id = normalized_scope_value(args.tenant_id.as_deref());
    let application_id = normalized_scope_value(args.application_id.as_deref())
        .or_else(|| normalized_scope_value(effective_project_id.as_deref()))
        .or_else(|| Some("missiond".to_string()));
    let channel =
        normalized_scope_value(args.channel.as_deref()).unwrap_or_else(|| "cli".to_string());
    let topic_label =
        normalized_scope_value(args.topic_label.as_deref()).or_else(|| compact_topic_label(&query));
    let topic_id = normalized_scope_value(args.topic_id.as_deref()).or_else(|| {
        stable_topic_id(
            user_id.as_deref(),
            tenant_id.as_deref(),
            application_id.as_deref(),
            &channel,
            topic_label.as_deref(),
        )
    });
    let mut payload = json!({
            "ok": !diagnostics_have_hard_failures(&diagnostics),
            "schema": "missiond.context-gather.v1",
            "query": query,
            "project_id": effective_project_id.clone(),
            "requested_project_id": args.project_id.clone(),
            "skill": args.skill.clone(),
            "infra_target": args.infra_target.clone(),
            "task_id": args.task_id.clone(),
            "source_id": args.source_id.clone(),
            "conversation_id": args.conversation_id.clone(),
            "isolation_scope": {
                "user_id": user_id.clone(),
                "tenant_id": tenant_id.clone(),
                "application_id": application_id.clone(),
                "channel": channel.clone(),
            },
            "topic_id": topic_id.clone(),
            "topic_label": topic_label.clone(),
            "permission_context": args.permission_context.clone(),
            "unknowns": args.unknowns.clone(),
        "sources_used": sources_used,
        "runtime_environment": runtime_environment,
        "sources": response_sources,
        "source_summaries": source_summaries,
        "raw_sources_omitted": !selection.include_raw_sources,
        "raw_sources_policy": raw_sources_policy(selection.include_raw_sources),
        "source_profile": profile.as_str(),
        "persist_artifact": persist_artifact,
        "persist_read_model": persist_read_model,
        "evidence_lane_policy": {
            "schema": "missiond.evidence-lane-policy-runtime.v1",
            "source": evidence_lane_policy_source,
            "allowed_lanes": allowed_lanes,
        },
        "evidence_item_search": evidence_item_search,
        "evidence_lanes": evidence_lanes,
        "evidence_items": evidence_items,
        "support_catalog": support_catalog,
        "authority_order": authority_order,
        "noise_diagnostics": noise_diagnostics,
        "context_noise_metrics": context_noise_metrics,
        "evidence_refs": evidence_refs,
        "unresolved": unresolved,
        "diagnostics": diagnostics,
        "next_action": "Synthesize grounded intent. If intent is confirmed, assign a plan-authoring worker to compile plan.lisp from the confirmed intent plus tool/resource inventory."
    });

    let mut artifact_hash_for_run: Option<String> = None;

    if persist_artifact {
        let artifact_payload =
            context_pack_artifact_payload(&payload, selection.include_raw_sources);
        let metadata = json!({
            "schema": "missiond.context-gather-artifact.v1",
            "query": payload.get("query").cloned().unwrap_or(Value::Null),
            "project_id": payload.get("project_id").cloned().unwrap_or(Value::Null),
            "task_id": payload.get("task_id").cloned().unwrap_or(Value::Null),
            "source_id": payload.get("source_id").cloned().unwrap_or(Value::Null),
            "source_profile": payload.get("source_profile").cloned().unwrap_or(Value::Null),
            "raw_sources_in_artifact": selection.include_raw_sources,
            "context_noise_metrics": payload
                .get("context_noise_metrics")
                .cloned()
                .unwrap_or(Value::Null),
            "unknown_count": payload
                .get("unknowns")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0),
            "sources_used": payload.get("sources_used").cloned().unwrap_or(Value::Null),
        });
        let artifact = state
            .shared_memory
            .put_json_artifact(
                "context-gather",
                payload.get("project_id").and_then(Value::as_str),
                payload.get("task_id").and_then(Value::as_str),
                &artifact_payload,
                metadata,
            )
            .await?;
        let context_pack_file = if let Some(hash) = artifact.get("hash").and_then(Value::as_str) {
            Some((
                hash.to_string(),
                materialize_context_pack_file(hash, &artifact_payload)?,
            ))
        } else {
            None
        };
        let isolation = super::context_capsule::CapsuleIsolation {
            user_id: user_id.as_deref(),
            tenant_id: tenant_id.as_deref(),
            application_id: application_id.as_deref(),
            channel: &channel,
        };
        let (capsule_lisp, capsule_hash) = super::context_capsule::generate_lisp_capsule(
            &isolation,
            &artifact_payload,
            topic_id.as_deref(),
            topic_label.as_deref(),
            args.task_id.as_deref(),
            None,
            None,
        );
        let capsule_path = materialize_context_capsule(&capsule_hash, &capsule_lisp);
        if let Some(conversation_id) = args.conversation_id.as_deref() {
            if let Err(err) = state
                .store
                .bind_context_capsule(
                    conversation_id,
                    &capsule_hash,
                    topic_id.as_deref(),
                    topic_label.as_deref(),
                )
                .await
            {
                tracing::warn!(conversation_id, error = %err, "failed to bind context capsule to conversation");
            }
            if let (Some(embedding_service), Some(topic)) =
                (state.embedding_service.as_ref(), topic_label.as_deref())
            {
                let topic_text = format!(
                    "{}\n{}",
                    topic,
                    args.unknowns
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                if let Some(vector) = embedding_service.embed(&topic_text) {
                    if let Err(err) = state
                        .store
                        .set_conversation_topic_vectors(
                            conversation_id,
                            &[(topic.to_string(), vector)],
                            embedding_service.provider_id(),
                        )
                        .await
                    {
                        tracing::warn!(conversation_id, error = %err, "failed to write conversation topic vector");
                    }
                }
            }
        }
        if let Some(task_id) = args.task_id.as_deref() {
            if let Ok(Some(task)) = state.store.get_board_task(task_id).await {
                let mut metadata = task.runtime_metadata.clone();
                if !metadata.is_object() {
                    metadata = json!({});
                }
                if let Some(object) = metadata.as_object_mut() {
                    object.insert(
                        "context_capsule_hash".to_string(),
                        Value::String(capsule_hash.clone()),
                    );
                    object.insert("topic_id".to_string(), json!(topic_id));
                    object.insert("topic_label".to_string(), json!(topic_label));
                    if let Some((hash, _)) = context_pack_file.as_ref() {
                        object.insert(
                            "grounding_context_id".to_string(),
                            Value::String(format!("context-gather:{hash}")),
                        );
                    }
                }
                let update = missiond_core::types::UpdateBoardTaskInput {
                    runtime_metadata: Some(metadata),
                    ..Default::default()
                };
                if let Err(err) = state
                    .store
                    .update_board_task(task.id.as_str(), &update)
                    .await
                {
                    tracing::warn!(task_id = task.id.as_str(), error = %err, "failed to bind context capsule to BoardTask runtime_metadata");
                }
            }
        }

        let artifact_hash = context_pack_file
            .as_ref()
            .map(|(hash, _)| hash.as_str())
            .or_else(|| artifact.get("hash").and_then(Value::as_str));
        artifact_hash_for_run = artifact_hash.map(ToOwned::to_owned);

        if let Some(object) = payload.as_object_mut() {
            if let Some((hash, context_pack_file)) = context_pack_file.as_ref() {
                object.insert(
                    "grounding_context_id".to_string(),
                    Value::String(format!("context-gather:{}", hash)),
                );
                object.insert(
                    "context_pack_path".to_string(),
                    Value::String(format!("shared-artifact://{}", hash)),
                );
                object.insert(
                    "context_pack_file".to_string(),
                    Value::String(context_pack_file.worker_path.display().to_string()),
                );
                object.insert(
                    "canonical_context_pack_file".to_string(),
                    Value::String(context_pack_file.canonical_path.display().to_string()),
                );
                object.insert("artifact_hash".to_string(), Value::String(hash.clone()));
            }
            object.insert("artifact".to_string(), artifact);
            object.insert(
                "context_capsule_hash".to_string(),
                Value::String(capsule_hash),
            );
            if let Ok(path) = capsule_path {
                object.insert(
                    "context_capsule_file".to_string(),
                    Value::String(path.display().to_string()),
                );
            }
        }
    }

    let evidence_lane_persistence = if persist_read_model {
        let context_gather_run = build_context_gather_run_input(
            &payload,
            profile,
            selection,
            artifact_hash_for_run.as_deref(),
        );
        persist_evidence_lane_projection(state, &context_gather_run, &evidence_item_inputs).await
    } else {
        json!({
            "schema": "missiond.evidence-lane-persistence.v1",
            "ok": true,
            "status": "disabled",
            "reason": "persist_read_model=false",
            "evidence_item_count": evidence_item_inputs.len(),
            "evidence_items_written": 0,
            "errors": [],
        })
    };

    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "evidence_lane_persistence".to_string(),
            evidence_lane_persistence,
        );
    }

    Ok(ToolResult::json_pretty(&payload))
}

fn authority_order() -> Value {
    json!([
        "runtime_truth",
        "project_ssot",
        "reviewed_kb",
        "active_board",
        "support_refs",
        "skill_evidence",
        "conversation_audit",
        "cold_archive"
    ])
}

fn build_evidence_lanes(sources: &serde_json::Map<String, Value>) -> Value {
    build_evidence_lanes_from_policy(sources, &EvidenceLaneRuntimeConfig::default())
}

fn build_evidence_lanes_from_policy(
    sources: &serde_json::Map<String, Value>,
    policy: &EvidenceLaneRuntimeConfig,
) -> Value {
    let lanes = policy
        .lanes
        .iter()
        .map(|lane| {
            (
                lane.lane_id.clone(),
                evidence_lane_from_policy(sources, lane),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "schema": "missiond.context-gather-evidence-lanes.v1",
        "lanes": lanes
    })
}

fn evidence_lane_from_policy(
    sources: &serde_json::Map<String, Value>,
    lane: &EvidenceLaneRuntimeEntry,
) -> Value {
    let keys = source_keys_for_lane(lane.lane_id.as_str());
    let role = lane.source_types.join(", ");
    let validity = lane.validity.join(", ");
    evidence_lane(
        sources,
        lane.lane_id.as_str(),
        lane.authority_class.as_str(),
        role.as_str(),
        &keys,
        &lane.default_profiles,
        lane.raw_policy.as_str(),
        lane.privacy_class.as_str(),
        validity.as_str(),
        lane.freshness.as_str(),
        lane.injectable_by_default,
    )
}

fn source_keys_for_lane(lane_id: &str) -> Vec<&'static str> {
    match lane_id {
        "runtime_truth" => vec!["runtime_environment", "deployment_events"],
        "project_ssot" => vec!["project_resolution", "project_registry", "ssot"],
        "reviewed_kb" => vec!["kb"],
        "active_board" => vec!["board_tasks"],
        "skill_evidence" => vec!["skill_context", "infra"],
        "conversation_audit" => vec!["conversation_logs"],
        "support_refs" => vec!["project_resolution", "project_registry", "credential_refs"],
        "cold_archive" => Vec::new(),
        _ => Vec::new(),
    }
}

fn evidence_lane(
    sources: &serde_json::Map<String, Value>,
    lane_id: &str,
    authority_class: &str,
    role: &str,
    keys: &[&str],
    default_profiles: &[String],
    raw_policy: &str,
    privacy_class: &str,
    validity: &str,
    freshness: &str,
    injectable_by_default: bool,
) -> Value {
    let mut source_keys = Vec::new();
    let mut item_count = 0usize;
    for key in keys {
        let Some(value) = sources.get(*key) else {
            continue;
        };
        let count = evidence_source_item_count(key, value);
        if count > 0 {
            source_keys.push((*key).to_string());
            item_count += count;
        }
    }
    json!({
        "lane_id": lane_id,
        "authority_class": authority_class,
        "role": role,
        "default_profiles": default_profiles,
        "raw_policy": raw_policy,
        "privacy_class": privacy_class,
        "validity": validity,
        "freshness": freshness,
        "injectable_by_default": injectable_by_default,
        "source_count": source_keys.len(),
        "item_count": item_count,
        "source_keys": source_keys,
    })
}

fn evidence_source_item_count(key: &str, value: &Value) -> usize {
    match key {
        "kb" => array_len(value.get("items"))
            .max(array_len(value.get("results")))
            .max(array_len(value.get("data")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        "board_tasks" => array_len(value.get("data"))
            .max(array_len(value.get("items")))
            .max(array_len(value.get("results"))),
        "conversation_logs" => array_len(value.get("results"))
            .max(array_len(value.get("items")))
            .max(array_len(value.get("data")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        "skill_context" => {
            array_len(value.get("skills"))
                + array_len(value.get("project_skill_links"))
                + array_len(value.get("operational_facts"))
        }
        "infra" => array_len(value.get("items")).max(value.as_array().map(Vec::len).unwrap_or(0)),
        "credential_refs" => array_len(value.get("credentialRefs"))
            .max(array_len(value.get("credential_refs")))
            .max(value.as_array().map(Vec::len).unwrap_or(0)),
        "deployment_events" => array_len(value.get("events")),
        _ if value.is_null() => 0,
        _ if value.as_object().is_some_and(|object| object.is_empty()) => 0,
        _ => 1,
    }
}

fn build_support_catalog(sources: &serde_json::Map<String, Value>) -> Value {
    let project = first_project_payload(sources);
    let project_id_hint = text_from_sources(&[project], &["id", "project_id", "projectId"]);
    let compiled_service = compiled_service_runtime_payload_for_project(project_id_hint.as_deref());
    let source_service = first_service_runtime_payload(sources);
    let service = compiled_service.as_ref().or(source_service);
    let service_catalog = service.and_then(|value| {
        value
            .get("supportCatalog")
            .or_else(|| value.get("support_catalog"))
    });
    let credential_refs = redacted_credential_refs(sources);
    let credential_ref_count = credential_refs.len();
    let project_id = project_id_hint
        .or_else(|| text_from_sources(&[service_catalog, service], &["project_id", "projectId"]))
        .or_else(|| text_from_sources(&[service], &["project"]));
    let service_id = text_from_sources(
        &[service, service_catalog],
        &["id", "service_id", "serviceId"],
    );
    let deploy_center_slug = text_from_sources(
        &[service_catalog, service],
        &[
            "deploy_center_slug",
            "deployCenterSlug",
            "deployCenter",
            "deploy_center",
        ],
    );
    let runtime_target = text_from_sources(
        &[service_catalog, service],
        &["runtime_target", "runtimeTarget", "surface"],
    );
    let service_manifest_refs = string_list_from_sources(
        &[service_catalog, service],
        &[
            "service_manifest_refs",
            "serviceManifestRefs",
            "source_evidence",
            "sourceEvidence",
        ],
    );
    let health_endpoints = string_list_from_sources(
        &[service_catalog, service],
        &["health", "health_endpoints", "healthEndpoints"],
    );
    let smoke_endpoints = string_list_from_sources(
        &[service_catalog, service],
        &["smoke", "smoke_endpoints", "smokeEndpoints"],
    );
    let deployment_policy =
        compiled_deployment_policy_for_service(project_id.as_deref(), service_id.as_deref());
    let deployment_closure = build_deployment_closure_support(
        project_id.as_deref(),
        service_id.as_deref(),
        deploy_center_slug.as_deref(),
        runtime_target.as_deref(),
        service_manifest_refs.as_slice(),
        health_endpoints.as_slice(),
        smoke_endpoints.as_slice(),
        service_catalog,
        service,
        deployment_policy.as_ref(),
    );

    json!({
        "schema": "missiond.support-catalog.v1",
        "authority": "compiled-project-service-runtime-plus-redacted-support-refs",
        "project_id": project_id,
        "service_id": service_id,
        "resolver_source": text_from_sources(&[project], &["source"])
            .or_else(|| text_from_sources(&[service], &["source", "sourceKind", "source_kind"]))
            .or_else(|| text_from_sources(&[service_catalog], &["resolver_source", "resolverSource"])),
        "deploy_center_slug": deploy_center_slug,
        "runtime_target": {
            "environment": text_from_sources(&[service_catalog, service], &["environment"]),
            "target": runtime_target,
            "ops_capability": text_from_sources(&[service_catalog, service], &["ops_capability", "opsCapability"]),
            "executor": text_from_sources(&[service_catalog, service], &["executor"]),
            "container": text_from_sources(&[service_catalog, service], &["container"]),
        },
        "urls": {
            "public_base_url": text_from_sources(&[service_catalog, service], &["public_base_url", "publicBaseUrl"]),
            "frontend_url": text_from_sources(&[service_catalog, service], &["frontend_url", "frontendUrl"]),
            "api_base_url": text_from_sources(&[service_catalog, service], &["api_base_url", "apiBaseUrl"]),
        },
        "domains": string_list_from_sources(&[service_catalog, service], &["domains"]),
        "manifest_refs": {
            "root": text_from_sources(&[service_catalog, service], &["root"]),
            "intent": text_from_sources(&[service_catalog, service], &["intent"]),
            "backend": text_from_sources(&[service_catalog, service], &["backend"]),
            "frontend": text_from_sources(&[service_catalog, service], &["frontend"]),
            "operations": text_from_sources(&[service_catalog, service], &["operations"]),
            "service_manifest_refs": service_manifest_refs,
        },
        "endpoints": {
            "health": health_endpoints,
            "smoke": smoke_endpoints,
        },
        "dependencies": string_list_from_sources(&[service_catalog, service], &["dependencies"]),
        "database": {
            "migration_namespace": text_from_sources(
                &[service_catalog, service],
                &["db_migration_namespace", "dbMigrationNamespace", "migration_namespace", "migrationNamespace"],
            ),
            "database_namespace": text_from_sources(
                &[service_catalog, service],
                &["db_namespace", "dbNamespace", "database_namespace", "databaseNamespace"],
            ),
        },
        "agent_refs": string_list_from_sources(&[service_catalog, service], &["agent_refs", "agentRefs", "vm_refs", "vmRefs"]),
        "deployment_closure": deployment_closure,
        "credential_refs": credential_refs,
        "credential_ref_count": credential_ref_count,
        "secret_policy": "Only secret namespace/key references and availability state are exposed. Secret values are not indexed or injected."
    })
}

#[allow(clippy::too_many_arguments)]
fn build_deployment_closure_support(
    project_id: Option<&str>,
    service_id: Option<&str>,
    deploy_center_slug: Option<&str>,
    runtime_target: Option<&str>,
    service_manifest_refs: &[String],
    health_endpoints: &[String],
    smoke_endpoints: &[String],
    service_catalog: Option<&Value>,
    service: Option<&Value>,
    deployment_policy: Option<&Value>,
) -> Value {
    if service_catalog.is_none()
        && service.is_none()
        && deployment_policy.is_none()
        && deploy_center_slug.is_none()
        && runtime_target.is_none()
        && service_manifest_refs.is_empty()
    {
        return Value::Null;
    }
    let policy = deployment_policy.and_then(|value| value.get("policy"));
    json!({
    "schema": "missiond.deployment-closure-support.v1",
    "authority": "compiled-service-runtime-plus-compiled-deployment-policy",
    "project_id": project_id,
    "service_id": service_id,
    "deploy_center_slug": deploy_center_slug,
    "runtime_target": runtime_target,
    "executor": text_from_sources(&[service_catalog, service], &["executor"]),
    "container": text_from_sources(&[service_catalog, service], &["container"]),
    "manifest_refs": service_manifest_refs,
    "health_endpoints": health_endpoints,
    "smoke_endpoints": smoke_endpoints,
    "policy_source": deployment_policy
        .and_then(|value| value.get("source"))
        .cloned()
        .unwrap_or(Value::Null),
    "policy_path": deployment_policy
        .and_then(|value| value.get("path"))
        .cloned()
        .unwrap_or(Value::Null),
    "policy_hash": deployment_policy
        .and_then(|value| value.get("source_hash"))
        .cloned()
        .unwrap_or(Value::Null),
    "runtime_fact_authority": text_from_sources(&[policy], &["runtime_fact_authority", "runtimeFactAuthority"])
        .or_else(|| Some("deploy-center".to_string())),
    "closure_authority": text_from_sources(&[policy], &["closure_authority", "closureAuthority"])
        .or_else(|| Some("ReleaseEvidence+ClosureVerdict".to_string())),
    "required_gates": {
        "manifest_required": bool_from_sources(&[policy], &["manifest_required", "manifestRequired"]),
        "immutable_image_required": bool_from_sources(&[policy], &["immutable_image_required", "immutableImageRequired"]),
        "runtime_digest_required": bool_from_sources(&[policy], &["runtime_digest_required", "runtimeDigestRequired"]),
        "smoke_required": bool_from_sources(&[policy], &["smoke_required", "smokeRequired"]),
        "db_adoption_required": bool_from_sources(&[policy], &["db_adoption_required", "dbAdoptionRequired"]),
        "release_lease_required": bool_from_sources(&[policy], &["release_lease_required", "releaseLeaseRequired"]),
    },
    "artifact_lane": text_from_sources(&[policy], &["artifact_lane", "artifactLane"]),
    "target_side_build_allowed": bool_from_sources(&[policy], &["target_side_build_allowed", "targetSideBuildAllowed"]),
    "approval_policy": text_from_sources(&[policy], &["approval_policy", "approvalPolicy"]),
    "closure_required_fields": string_list_from_sources(&[policy], &["closure_required_fields", "closureRequiredFields"]),
    "fail_closed_blockers": string_list_from_sources(&[policy], &["fail_closed_blockers", "failClosedBlockers"]),
    "diagnostic_profiles": string_list_from_sources(&[policy], &["diagnostic_profiles", "diagnosticProfiles"]),
    "closure_state_machine": deployment_policy
        .and_then(|value| value.get("closure_state_machine"))
        .cloned()
        .unwrap_or(Value::Null),
    "closure_verdicts": deployment_policy
        .and_then(|value| value.get("closure_verdicts"))
        .cloned()
        .unwrap_or(Value::Null),
    "typed_diagnostics": deployment_policy
        .and_then(|value| value.get("typed_diagnostics"))
        .cloned()
        .unwrap_or(Value::Null),
    "diagnostic_terms": [
        "service.manifest.toml",
        "manifest gate",
        "Deploy Center canary",
        "smoke",
        "runtime digest",
        "running image digest",
        "binary marker",
        "image marker",
        "entrypoint",
        "old binary",
        "compose",
        "volume override",
        "_sqlx_migrations",
        "db adoption",
        "ReleaseLease",
        "RuntimeObservation",
        "ReleaseEvidence",
        "ClosureVerdict",
    ],
    "rule": "GitHub workflow success, curl probes, and local git state are diagnostic only; Deploy Center ReleaseEvidence plus ClosureVerdict is the closure authority."
    })
}

fn attach_infra_os_disabled_support_fallback(sources: &mut serde_json::Map<String, Value>) {
    let support_catalog = build_support_catalog(sources);
    if !support_catalog_has_content(&support_catalog) {
        return;
    }
    let Some(infra_source) = sources.get_mut("infra") else {
        return;
    };
    if infra_source.get("status").and_then(Value::as_str) != Some("feature_disabled") {
        return;
    }
    let fallback_items = infra_os_disabled_support_fallback_items(&support_catalog);
    if fallback_items.is_empty() {
        return;
    }
    let Some(object) = infra_source.as_object_mut() else {
        return;
    };
    object.insert(
        "fallback_status".to_string(),
        json!("support_catalog_available"),
    );
    object.insert(
        "fallback_source".to_string(),
        json!("support_catalog.deployment_closure"),
    );
    object.insert(
        "authority".to_string(),
        json!("infra-os optional layer is disabled; using scoped support_catalog/deployment_closure fallback facts as evidence-only deploy facts"),
    );
    object.insert("item_count".to_string(), json!(fallback_items.len()));
    object.insert("items".to_string(), Value::Array(fallback_items.clone()));
    object.insert("fallback_items".to_string(), Value::Array(fallback_items));
}

fn infra_os_disabled_support_fallback_items(support_catalog: &Value) -> Vec<Value> {
    let project_id = text_field(support_catalog, "project_id");
    let service_id = text_field(support_catalog, "service_id");
    let deploy_center_slug = text_field(support_catalog, "deploy_center_slug");
    let runtime_target = support_catalog
        .get("runtime_target")
        .and_then(|value| text_field(value, "target"));
    let manifest_refs = support_catalog
        .get("manifest_refs")
        .map(|value| string_list_field(value, "service_manifest_refs"))
        .unwrap_or_default();
    let health_endpoints = support_catalog
        .get("endpoints")
        .map(|value| string_list_field(value, "health"))
        .unwrap_or_default();

    let mut items = Vec::new();
    if deploy_center_slug.is_some() || runtime_target.is_some() || !manifest_refs.is_empty() {
        items.push(json!({
            "sourceType": "support_catalog",
            "source": "support_catalog",
            "authority": "support_refs fallback while infra-os is feature_disabled",
            "title": "Scoped deploy support catalog",
            "project_id": project_id,
            "service_id": service_id,
            "deploy_center_slug": deploy_center_slug,
            "runtime_target": runtime_target,
            "manifest_refs": manifest_refs,
            "health_endpoints": health_endpoints,
            "excerpt": "Compiled support catalog supplies scoped deploy facts while infra-os live skill evidence is disabled."
        }));
    }

    if let Some(closure) = support_catalog
        .get("deployment_closure")
        .filter(|value| !json_value_is_empty(value))
    {
        items.push(json!({
            "sourceType": "deployment_closure_policy",
            "source": "support_catalog.deployment_closure",
            "authority": "compiled deployment closure fallback while infra-os is feature_disabled",
            "title": "Scoped deployment closure policy",
            "project_id": text_field(closure, "project_id"),
            "service_id": text_field(closure, "service_id"),
            "deploy_center_slug": text_field(closure, "deploy_center_slug"),
            "runtime_target": text_field(closure, "runtime_target"),
            "manifest_refs": string_list_field(closure, "manifest_refs"),
            "diagnostic_terms": string_list_field(closure, "diagnostic_terms"),
            "excerpt": deployment_closure_summary(closure),
        }));
    }
    items
}

fn compiled_deployment_policy_for_service(
    project_id: Option<&str>,
    service_id: Option<&str>,
) -> Option<Value> {
    let project_key = normalized_lookup_key(project_id);
    let service_key = normalized_lookup_key(service_id);
    for path in compiled_deployment_policy_candidates() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let payload = value.get("payload").unwrap_or(&value);
        let Some(policy) = payload
            .get("policies")
            .and_then(Value::as_array)
            .and_then(|policies| {
                policies.iter().find(|policy| {
                    policy_matches_lookup(policy, "project_id", project_key.as_deref())
                        || policy_matches_lookup(policy, "projectId", project_key.as_deref())
                        || policy_matches_lookup(policy, "service_id", service_key.as_deref())
                        || policy_matches_lookup(policy, "serviceId", service_key.as_deref())
                })
            })
        else {
            continue;
        };
        return Some(json!({
            "source": "compiled-deployment-policy",
            "path": path.display().to_string(),
            "source_hash": value.get("source_hash").cloned().unwrap_or(Value::Null),
            "policy": policy,
            "closure_state_machine": payload.get("closure_state_machine").cloned().unwrap_or(Value::Null),
            "closure_verdicts": payload.get("closure_verdicts").cloned().unwrap_or(Value::Null),
            "typed_diagnostics": payload.get("typed_diagnostics").cloned().unwrap_or(Value::Null),
        }));
    }
    None
}

fn compiled_deployment_policy_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(dir) = env::var("MISSIOND_COMPILED_RUNTIME_DIR") {
        candidates.push(PathBuf::from(dir).join("compiled-deployment-policy.json"));
    }
    if let Ok(dir) = env::var("MISSIOND_RUNTIME_DIR") {
        candidates.push(PathBuf::from(dir).join("compiled/compiled-deployment-policy.json"));
    }
    if let Ok(root) = env::var("MISSIOND_PROJECT_ROOT") {
        candidates.push(
            PathBuf::from(root)
                .join(".missiond/v3/runtime/compiled/compiled-deployment-policy.json"),
        );
    }
    if let Ok(root) = env::current_dir() {
        candidates.push(root.join(".missiond/v3/runtime/compiled/compiled-deployment-policy.json"));
    }
    candidates
}

fn normalized_lookup_key(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('_', "-").to_ascii_lowercase())
}

fn policy_matches_lookup(policy: &Value, key: &str, lookup: Option<&str>) -> bool {
    let Some(lookup) = lookup else {
        return false;
    };
    policy
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| normalized_lookup_key(Some(value)))
        .as_deref()
        == Some(lookup)
}

fn first_project_payload<'a>(sources: &'a serde_json::Map<String, Value>) -> Option<&'a Value> {
    sources
        .get("project_resolution")
        .and_then(|value| {
            value
                .get("matched_project")
                .or_else(|| value.get("matchedProject"))
        })
        .or_else(|| sources.get("project_registry"))
}

fn first_service_runtime_payload<'a>(
    sources: &'a serde_json::Map<String, Value>,
) -> Option<&'a Value> {
    first_project_payload(sources)
        .and_then(|value| {
            value
                .get("serviceRuntime")
                .or_else(|| value.get("service_runtime"))
        })
        .or_else(|| {
            sources.get("project_resolution").and_then(|value| {
                value
                    .get("matched_project")
                    .or_else(|| value.get("matchedProject"))
                    .and_then(|project| {
                        project
                            .get("serviceRuntime")
                            .or_else(|| project.get("service_runtime"))
                    })
            })
        })
}

fn compiled_service_runtime_payload_for_project(project_id: Option<&str>) -> Option<Value> {
    let lookup = normalized_lookup_key(project_id)?;
    let project_root = crate::helpers::missiond_project_root();
    let universe = load_compiled_project_universe(&project_root, None);
    let payload = universe.payload?;
    payload
        .services
        .iter()
        .find(|service| compiled_service_matches_lookup(service, &lookup))
        .map(compiled_service_runtime_entry_to_value)
}

fn compiled_service_matches_lookup(service: &CompiledServiceRuntimeEntry, lookup: &str) -> bool {
    [service.id.as_deref(), service.project.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(|value| normalized_lookup_key(Some(value)))
        .any(|value| value == lookup)
}

fn compiled_service_runtime_entry_to_value(service: &CompiledServiceRuntimeEntry) -> Value {
    json!({
        "source": "compiled-project-universe",
        "sourceKind": "compiled-runtime",
        "id": service.id,
        "project": service.project,
        "root": service.root,
        "intent": service.intent,
        "backend": service.backend,
        "frontend": service.frontend,
        "operations": service.operations,
        "environment": service.environment,
        "publicBaseUrl": service.public_base_url,
        "frontendUrl": service.frontend_url,
        "apiBaseUrl": service.api_base_url,
        "domains": service.domains,
        "health": service.health,
        "dependencies": service.dependencies,
        "opsCapability": service.ops_capability,
        "surface": service.surface,
        "supportCatalog": service
            .support_catalog
            .as_ref()
            .map(compiled_support_catalog_to_value)
            .unwrap_or(Value::Null),
    })
}

fn compiled_support_catalog_to_value(catalog: &CompiledServiceSupportCatalog) -> Value {
    json!({
        "service_id": catalog.service_id,
        "project_id": catalog.project_id,
        "domains": catalog.domains,
        "public_base_url": catalog.public_base_url,
        "frontend_url": catalog.frontend_url,
        "api_base_url": catalog.api_base_url,
        "health": catalog.health,
        "dependencies": catalog.dependencies,
        "deploy_center_slug": catalog.deploy_center_slug,
        "runtime_target": catalog.runtime_target,
        "executor": catalog.executor,
        "container": catalog.container,
        "service_manifest_refs": catalog.service_manifest_refs,
        "source_evidence": catalog.source_evidence,
        "db_migration_namespace": catalog.db_migration_namespace,
        "database_namespace": catalog.database_namespace,
    })
}

fn text_from_sources(sources: &[Option<&Value>], keys: &[&str]) -> Option<String> {
    sources.iter().find_map(|source| {
        let source = source.as_ref()?;
        keys.iter().find_map(|key| text_field(source, key))
    })
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_from_sources(sources: &[Option<&Value>], keys: &[&str]) -> Option<bool> {
    sources.iter().find_map(|source| {
        let source = source.as_ref()?;
        keys.iter()
            .find_map(|key| source.get(*key).and_then(Value::as_bool))
    })
}

fn string_list_from_sources(sources: &[Option<&Value>], keys: &[&str]) -> Vec<String> {
    for source in sources.iter().flatten() {
        for key in keys {
            let values = string_list_field(source, key);
            if !values.is_empty() {
                return values;
            }
        }
    }
    Vec::new()
}

fn string_list_field(value: &Value, key: &str) -> Vec<String> {
    match value.get(key) {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => vec![text.trim().to_string()],
        _ => Vec::new(),
    }
}

fn redacted_credential_refs(sources: &serde_json::Map<String, Value>) -> Vec<Value> {
    let Some(source) = sources.get("credential_refs") else {
        return Vec::new();
    };
    credential_ref_items(source)
        .into_iter()
        .take(20)
        .map(redacted_credential_ref)
        .collect()
}

fn credential_ref_items(value: &Value) -> Vec<&Value> {
    value
        .get("credentialRefs")
        .or_else(|| value.get("credential_refs"))
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .or_else(|| value.as_array().map(|items| items.iter().collect()))
        .unwrap_or_default()
}

fn redacted_credential_ref(item: &Value) -> Value {
    json!({
        "namespace": text_field(item, "namespace"),
        "key_name": text_field(item, "keyName").or_else(|| text_field(item, "key_name")),
        "target_id": text_field(item, "targetId").or_else(|| text_field(item, "target_id")),
        "required_capability": text_field(item, "requiredCapability")
            .or_else(|| text_field(item, "required_capability")),
        "availability": text_field(item, "availability"),
        "purpose": text_field(item, "purpose").map(|value| compact_text(&value, 220)),
        "provenance": text_field(item, "provenance")
            .or_else(|| text_field(item, "source"))
            .or_else(|| text_field(item, "sourcePath"))
            .or_else(|| text_field(item, "source_path")),
    })
}

fn build_evidence_items(
    sources: &serde_json::Map<String, Value>,
    source_summaries: &Value,
    support_catalog: &Value,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
) -> Vec<EvidenceItemInput> {
    let mut items = Vec::new();

    add_source_summary_item(
        &mut items,
        source_summaries,
        "runtime_environment",
        "runtime_truth",
        "runtime_environment",
        "Runtime truth",
        "Current runtime environment, compiled runtime locations, and monitor endpoints.",
        profile,
        project_id,
        task_id,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "deployment_events",
        "events",
        "runtime_truth",
        "deploy_center_event",
        profile,
        project_id,
        task_id,
        8,
    );
    for source_key in ["project_resolution", "project_registry", "ssot"] {
        add_source_summary_item(
            &mut items,
            source_summaries,
            source_key,
            "project_ssot",
            source_key,
            "Project SSOT",
            "Project resolver, registry, and Lisp/compiled project universe facts.",
            profile,
            project_id,
            task_id,
        );
    }
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "kb",
        "items",
        "reviewed_kb",
        "knowledge",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "board_tasks",
        "items",
        "active_board",
        "board_task",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "skill_context",
        "skills",
        "skill_evidence",
        "skill_metadata",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "skill_context",
        "project_skill_links",
        "skill_evidence",
        "skill_project_link",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "infra",
        "items",
        "skill_evidence",
        "skill_operational_fact",
        profile,
        project_id,
        task_id,
        10,
    );
    add_summary_collection_items(
        &mut items,
        source_summaries,
        "conversation_logs",
        "items",
        "conversation_audit",
        "conversation_fact_extract",
        profile,
        project_id,
        task_id,
        10,
    );

    if support_catalog_has_content(support_catalog) {
        let source_id = text_field(support_catalog, "service_id")
            .or_else(|| text_field(support_catalog, "project_id"));
        push_evidence_item(
            &mut items,
            "support_refs",
            "support_catalog",
            source_id.as_deref(),
            None,
            project_id,
            task_id,
            "Support catalog",
            "Domain, service, deploy, endpoint, DB/migration, agent, and redacted secret-reference support catalog.",
            support_catalog,
            profile,
            None,
        );
        if let Some(deployment_closure) = support_catalog
            .get("deployment_closure")
            .filter(|value| !json_value_is_empty(value))
        {
            push_evidence_item(
                &mut items,
                "support_refs",
                "deployment_closure_policy",
                source_id.as_deref(),
                None,
                project_id,
                task_id,
                "Deployment closure policy",
                &deployment_closure_summary(deployment_closure),
                deployment_closure,
                profile,
                None,
            );
        }
    }

    if let Some(source) = sources.get("credential_refs") {
        for credential in credential_ref_items(source).into_iter().take(20) {
            let redacted = redacted_credential_ref(credential);
            let source_id = credential_ref_source_id(&redacted);
            push_evidence_item(
                &mut items,
                "support_refs",
                "skill_credential_ref",
                source_id.as_deref(),
                source_id.as_deref(),
                project_id,
                task_id,
                "Credential reference",
                "Redacted credential reference. Secret value is intentionally unavailable to retrieval and worker context.",
                &redacted,
                profile,
                None,
            );
        }
    }

    items
}

fn deployment_closure_summary(value: &Value) -> String {
    let service = text_field(value, "service_id").unwrap_or_else(|| "service".to_string());
    let deploy_center =
        text_field(value, "deploy_center_slug").unwrap_or_else(|| "deploy-center".to_string());
    let runtime =
        text_field(value, "runtime_target").unwrap_or_else(|| "runtime-target".to_string());
    let manifest_refs = string_list_field(value, "manifest_refs").join(", ");
    let closure_fields = string_list_field(value, "closure_required_fields").join(", ");
    let blockers = string_list_field(value, "fail_closed_blockers").join(", ");
    let diagnostic_terms = string_list_field(value, "diagnostic_terms").join(", ");
    compact_text(
        &format!(
            "{service} deployment closure support: deploy center slug {deploy_center}; runtime target {runtime}; manifest refs [{manifest_refs}]; required closure records [{closure_fields}]; fail-closed blockers [{blockers}]. Search anchors: {diagnostic_terms}. GitHub workflow success and curl probes are diagnostics only; Deploy Center ReleaseEvidence plus ClosureVerdict is the authority for canary/smoke/runtime digest/binary marker/db adoption closure.",
        ),
        1200,
    )
}

async fn deployment_events_source(
    state: &AppState,
    project_id: Option<&str>,
    support_catalog: &Value,
    query: &str,
    limit: usize,
) -> Value {
    let service_id =
        text_field(support_catalog, "service_id").or_else(|| project_id.map(str::to_string));
    let deploy_center_slug = text_field(support_catalog, "deploy_center_slug");
    let filters = json!({
        "project_id": project_id,
        "service_id": service_id,
        "deploy_center_slug": deploy_center_slug,
        "query": query,
    });
    let read_limit = (limit.saturating_mul(8)).clamp(8, 80) as i64;
    match state
        .store
        .query_timeline_filtered(
            Some("system::external_service_event"),
            None,
            Some("30d"),
            None,
            read_limit,
            0,
        )
        .await
    {
        Ok(rows) => {
            let candidate_count = rows.len();
            let mut events = Vec::new();
            let mut matching_count = 0usize;
            let mut drop_reason_counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut sample_dropped_events = Vec::new();
            for row in &rows {
                match deployment_event_filter_timeline_row(
                    row,
                    project_id,
                    service_id.as_deref(),
                    deploy_center_slug.as_deref(),
                    query,
                ) {
                    DeploymentEventFilterResult::Keep(event) => {
                        matching_count += 1;
                        if events.len() < limit {
                            events.push(event);
                        }
                    }
                    DeploymentEventFilterResult::Drop { reason, sample } => {
                        *drop_reason_counts.entry(reason.to_string()).or_default() += 1;
                        if sample_dropped_events.len() < 5 {
                            sample_dropped_events.push(sample);
                        }
                    }
                }
            }
            let status = if events.is_empty() {
                "no_matching_deploy_center_events"
            } else {
                "ok"
            };
            let diagnostic = if events.is_empty() {
                Some("No scoped Deploy Center ExternalServiceEvent was found in the local event_log window; deploy_ops context is using support_catalog/deployment_closure policy until Deploy Center emits durable release/canary evidence into MissionD EventBridge.")
            } else {
                None
            };
            json!({
                "schema": "missiond.deployment-events-context.v1",
                "status": status,
                "source": "event_log",
                "authority": "deploy-center ExternalServiceEvent via MissionD EventBridge",
                "read_model": "event_log",
                "event_type": "system::external_service_event",
                "since": "30d",
                "filters": filters,
                "filter_before_injection": true,
                "candidate_count": candidate_count,
                "filtered_count": matching_count,
                "returned_count": events.len(),
                "dropped_count": candidate_count.saturating_sub(matching_count),
                "drop_reason_counts": drop_reason_counts,
                "sample_dropped_events": sample_dropped_events,
                "events": events,
                "diagnostic": diagnostic,
            })
        }
        Err(err) => json!({
            "schema": "missiond.deployment-events-context.v1",
            "status": "error",
            "source": "event_log",
            "authority": "deploy-center ExternalServiceEvent via MissionD EventBridge",
            "read_model": "event_log",
            "event_type": "system::external_service_event",
            "since": "30d",
            "filters": filters,
            "filter_before_injection": true,
            "candidate_count": 0,
            "filtered_count": 0,
            "returned_count": 0,
            "dropped_count": 0,
            "drop_reason_counts": {},
            "sample_dropped_events": [],
            "events": [],
            "diagnostic": format!("deployment event_log query failed: {err}"),
        }),
    }
}

enum DeploymentEventFilterResult {
    Keep(Value),
    Drop { reason: &'static str, sample: Value },
}

fn deployment_event_item_from_timeline_row(
    row: &missiond_core::db::TimelineRow,
    project_id: Option<&str>,
    service_id: Option<&str>,
    deploy_center_slug: Option<&str>,
    query: &str,
) -> Option<Value> {
    match deployment_event_filter_timeline_row(
        row,
        project_id,
        service_id,
        deploy_center_slug,
        query,
    ) {
        DeploymentEventFilterResult::Keep(event) => Some(event),
        DeploymentEventFilterResult::Drop { .. } => None,
    }
}

fn deployment_event_filter_timeline_row(
    row: &missiond_core::db::TimelineRow,
    project_id: Option<&str>,
    service_id: Option<&str>,
    deploy_center_slug: Option<&str>,
    query: &str,
) -> DeploymentEventFilterResult {
    if row.event_type != "external_service_event" {
        return deployment_event_drop_result(
            row,
            "event_type_mismatch",
            None,
            None,
            None,
            false,
            project_id,
            service_id,
            deploy_center_slug,
        );
    }
    let payload = match serde_json::from_str::<Value>(&row.payload) {
        Ok(payload) => payload,
        Err(_) => {
            return deployment_event_drop_result(
                row,
                "payload_parse_failed",
                None,
                None,
                None,
                false,
                project_id,
                service_id,
                deploy_center_slug,
            )
        }
    };
    let Some(producer_service_id) = text_field(&payload, "service_id") else {
        return deployment_event_drop_result(
            row,
            "missing_producer_service_id",
            Some(&payload),
            None,
            None,
            false,
            project_id,
            service_id,
            deploy_center_slug,
        );
    };
    let Some(event_kind) = text_field(&payload, "event_kind") else {
        return deployment_event_drop_result(
            row,
            "missing_event_kind",
            Some(&payload),
            None,
            None,
            false,
            project_id,
            service_id,
            deploy_center_slug,
        );
    };
    if !deployment_event_kind_is_relevant(&event_kind) {
        return deployment_event_drop_result(
            row,
            "irrelevant_event_kind",
            Some(&payload),
            None,
            None,
            false,
            project_id,
            service_id,
            deploy_center_slug,
        );
    }
    let Some(event_id) = text_field(&payload, "event_id") else {
        return deployment_event_drop_result(
            row,
            "missing_event_id",
            Some(&payload),
            None,
            None,
            false,
            project_id,
            service_id,
            deploy_center_slug,
        );
    };
    let (payload_json, payload_json_parse_failed) = match text_field(&payload, "payload_json") {
        Some(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => (value, false),
            Err(_) => (Value::Null, true),
        },
        None => (Value::Null, false),
    };
    let envelope = payload_json.get("_envelope");
    if !deployment_event_has_deploy_center_authority(&producer_service_id, &event_id, envelope) {
        return deployment_event_drop_result(
            row,
            "non_deploy_center_authority",
            Some(&payload),
            Some(&payload_json),
            envelope,
            payload_json_parse_failed,
            project_id,
            service_id,
            deploy_center_slug,
        );
    }
    if !deployment_event_matches_scope(
        &payload,
        &payload_json,
        envelope,
        project_id,
        service_id,
        deploy_center_slug,
        query,
    ) {
        return deployment_event_drop_result(
            row,
            "scope_mismatch",
            Some(&payload),
            Some(&payload_json),
            envelope,
            payload_json_parse_failed,
            project_id,
            service_id,
            deploy_center_slug,
        );
    }

    let target_project_id = text_from_sources(
        &[envelope, Some(&payload_json), Some(&payload)],
        &["project_id", "projectId", "project"],
    )
    .or_else(|| project_id.map(str::to_string));
    let target_service_id = text_from_sources(
        &[Some(&payload_json), envelope, Some(&payload)],
        &[
            "target_service_id",
            "targetServiceId",
            "deploy_service_id",
            "deployServiceId",
            "project",
            "project_id",
            "projectId",
        ],
    )
    .or_else(|| service_id.map(str::to_string));
    let subject = text_from_sources(&[envelope, Some(&payload_json)], &["subject"]);
    let correlation_id = text_from_sources(
        &[envelope, Some(&payload_json)],
        &["correlation_id", "correlationId"],
    );
    let authority = text_from_sources(&[envelope, Some(&payload_json)], &["authority"])
        .unwrap_or_else(|| "deploy-center".to_string());
    let source = text_from_sources(&[envelope, Some(&payload_json)], &["source"])
        .unwrap_or_else(|| "deploy-center".to_string());
    let summary = text_field(&payload, "summary").unwrap_or_else(|| {
        format!(
            "Deploy Center event {event_kind} for {}",
            target_project_id
                .as_deref()
                .or(target_service_id.as_deref())
                .unwrap_or("deployment")
        )
    });

    DeploymentEventFilterResult::Keep(json!({
        "sourceType": "deploy_center_event",
        "source": "event_log.external_service_event",
        "authority": "deploy-center durable event via MissionD EventBridge",
        "event_ref": format!("event_log:{}", row.seq),
        "seq": row.seq,
        "created_at": row.created_at,
        "event_id": event_id,
        "event_kind": event_kind,
        "producer_service_id": producer_service_id,
        "project_id": target_project_id,
        "target_service_id": target_service_id,
        "deploy_center_slug": deploy_center_slug,
        "subject": subject,
        "correlation_id": correlation_id,
        "trace_id": text_field(&payload, "trace_id").or_else(|| row.trace_id.clone()),
        "event_source": source,
        "event_authority": authority,
        "summary": compact_text(&summary, 420),
    }))
}

#[allow(clippy::too_many_arguments)]
fn deployment_event_drop_result(
    row: &missiond_core::db::TimelineRow,
    reason: &'static str,
    payload: Option<&Value>,
    payload_json: Option<&Value>,
    envelope: Option<&Value>,
    payload_json_parse_failed: bool,
    requested_project_id: Option<&str>,
    requested_service_id: Option<&str>,
    requested_deploy_center_slug: Option<&str>,
) -> DeploymentEventFilterResult {
    DeploymentEventFilterResult::Drop {
        reason,
        sample: deployment_event_drop_sample(
            row,
            reason,
            payload,
            payload_json,
            envelope,
            payload_json_parse_failed,
            requested_project_id,
            requested_service_id,
            requested_deploy_center_slug,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn deployment_event_drop_sample(
    row: &missiond_core::db::TimelineRow,
    reason: &str,
    payload: Option<&Value>,
    payload_json: Option<&Value>,
    envelope: Option<&Value>,
    payload_json_parse_failed: bool,
    requested_project_id: Option<&str>,
    requested_service_id: Option<&str>,
    requested_deploy_center_slug: Option<&str>,
) -> Value {
    let producer_service_id = text_from_sources(&[payload], &["service_id", "serviceId"]);
    let event_kind = text_from_sources(&[payload], &["event_kind", "eventKind"]);
    let event_id = text_from_sources(&[payload], &["event_id", "eventId"])
        .map(|value| compact_text(&value, 160));
    let target_project_id = text_from_sources(
        &[envelope, payload_json, payload],
        &["project_id", "projectId", "project"],
    );
    let target_service_id = text_from_sources(
        &[payload_json, envelope, payload],
        &[
            "target_service_id",
            "targetServiceId",
            "deploy_service_id",
            "deployServiceId",
            "service_id",
            "serviceId",
            "project",
            "project_id",
            "projectId",
        ],
    );
    let target_deploy_center_slug = text_from_sources(
        &[payload_json, envelope, payload],
        &[
            "deploy_center_slug",
            "deployCenterSlug",
            "deploy_center",
            "deployCenter",
        ],
    );
    let subject = text_from_sources(&[envelope, payload_json, payload], &["subject", "summary"])
        .map(|value| compact_text(&value, 180));
    let source = text_from_sources(&[envelope, payload_json], &["source"]);
    let authority = text_from_sources(&[envelope, payload_json], &["authority"]);
    json!({
        "reason": reason,
        "seq": row.seq,
        "created_at": row.created_at,
        "event_type": row.event_type,
        "event_kind": event_kind,
        "producer_service_id": producer_service_id,
        "event_id": event_id,
        "target_project_id": target_project_id,
        "target_service_id": target_service_id,
        "target_deploy_center_slug": target_deploy_center_slug,
        "subject": subject,
        "event_source": source,
        "event_authority": authority,
        "payload_json_parse_failed": payload_json_parse_failed,
        "requested_scope": {
            "project_id": requested_project_id,
            "service_id": requested_service_id,
            "deploy_center_slug": requested_deploy_center_slug,
        }
    })
}

fn deployment_event_kind_is_relevant(event_kind: &str) -> bool {
    matches!(
        event_kind,
        "deploy_created"
            | "build_started"
            | "build_succeeded"
            | "build_failed"
            | "deploy_started"
            | "deploy_succeeded"
            | "deploy_failed"
            | "smoke_succeeded"
            | "smoke_failed"
            | "rollback_started"
            | "rollback_succeeded"
            | "rollback_failed"
            | "agent_heartbeat"
            | "agent_offline"
            | "agent_update_started"
            | "agent_update_succeeded"
            | "agent_update_failed"
            | "provenance_changed"
            | "closure_verdict"
    )
}

fn deployment_event_has_deploy_center_authority(
    producer_service_id: &str,
    event_id: &str,
    envelope: Option<&Value>,
) -> bool {
    normalized_lookup_key(Some(producer_service_id)).as_deref() == Some("deploy-center")
        || normalized_lookup_key(Some(event_id))
            .is_some_and(|value| value.contains("deploy-center"))
        || text_from_sources(&[envelope], &["source", "authority"]).is_some_and(|value| {
            normalized_lookup_key(Some(&value)).is_some_and(|key| key.contains("deploy-center"))
        })
}

fn deployment_event_matches_scope(
    payload: &Value,
    payload_json: &Value,
    envelope: Option<&Value>,
    project_id: Option<&str>,
    service_id: Option<&str>,
    deploy_center_slug: Option<&str>,
    query: &str,
) -> bool {
    let filters = [project_id, service_id, deploy_center_slug]
        .into_iter()
        .flatten()
        .filter_map(|value| normalized_lookup_key(Some(value)))
        .collect::<Vec<_>>();
    if filters.is_empty() {
        return query
            .split_whitespace()
            .filter_map(|token| normalized_lookup_key(Some(token)))
            .any(|token| {
                deployment_event_scope_text(payload, payload_json, envelope).contains(&token)
            });
    }
    let scope_text = deployment_event_scope_text(payload, payload_json, envelope);
    filters.iter().any(|filter| scope_text.contains(filter))
}

fn deployment_event_scope_text(
    payload: &Value,
    payload_json: &Value,
    envelope: Option<&Value>,
) -> String {
    let mut values = Vec::new();
    for source in [Some(payload), Some(payload_json), envelope] {
        let Some(source) = source else {
            continue;
        };
        for key in [
            "event_id",
            "event_kind",
            "project_id",
            "projectId",
            "project",
            "target_service_id",
            "targetServiceId",
            "deploy_service_id",
            "deployServiceId",
            "deploy_center_slug",
            "deployCenterSlug",
            "subject",
            "summary",
            "correlation_id",
            "correlationId",
        ] {
            if let Some(text) = text_field(source, key) {
                values.push(text);
            }
        }
    }
    normalized_lookup_key(Some(&values.join(" "))).unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn add_source_summary_item(
    items: &mut Vec<EvidenceItemInput>,
    source_summaries: &Value,
    source_key: &str,
    lane_id: &str,
    source_type: &str,
    title: &str,
    fallback_summary: &str,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
) {
    let Some(summary) = source_summaries.get(source_key) else {
        return;
    };
    if summary_is_empty(summary) {
        return;
    }
    let summary_text =
        compact_json_text(summary, 900).unwrap_or_else(|| fallback_summary.to_string());
    let source_id = text_from_sources(
        &[Some(summary)],
        &["id", "matched_project_id", "matchedProjectId"],
    );
    let source_ref = source_ref_from_value(summary);
    push_evidence_item(
        items,
        lane_id,
        source_type,
        source_id.as_deref(),
        source_ref.as_deref(),
        project_id,
        task_id,
        title,
        &summary_text,
        summary,
        profile,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn add_summary_collection_items(
    items: &mut Vec<EvidenceItemInput>,
    source_summaries: &Value,
    source_key: &str,
    collection_key: &str,
    lane_id: &str,
    source_type: &str,
    profile: SourceProfile,
    project_id: Option<&str>,
    task_id: Option<&str>,
    limit: usize,
) {
    let Some(collection) = source_summaries
        .get(source_key)
        .and_then(|summary| summary.get(collection_key))
        .and_then(Value::as_array)
    else {
        return;
    };
    for item in collection.iter().take(limit) {
        let title = evidence_title(item, source_type);
        let summary = evidence_summary(item);
        let source_id = source_id_from_value(item);
        let source_ref = source_ref_from_value(item);
        let score = numeric_field(item, &["score", "confidence"]);
        push_evidence_item(
            items,
            lane_id,
            source_type,
            source_id.as_deref(),
            source_ref.as_deref(),
            project_id,
            task_id,
            &title,
            &summary,
            item,
            profile,
            score,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_evidence_item(
    items: &mut Vec<EvidenceItemInput>,
    lane_id: &str,
    source_type: &str,
    source_id: Option<&str>,
    source_ref: Option<&str>,
    project_id: Option<&str>,
    task_id: Option<&str>,
    title: &str,
    summary: &str,
    evidence_value: &Value,
    profile: SourceProfile,
    score: Option<f64>,
) {
    let title = compact_text(title, 180);
    let summary = compact_text(summary, 1200);
    let id = evidence_item_id(
        lane_id,
        source_type,
        source_id,
        source_ref,
        project_id,
        task_id,
        profile,
        &title,
        &summary,
    );
    let mut refs = collect_evidence_refs_from_value(evidence_value);
    refs.truncate(12);
    let (authority_class, validity, privacy_class, freshness, raw_policy) =
        evidence_item_policy(lane_id);
    items.push(EvidenceItemInput {
        id,
        lane_id: lane_id.to_string(),
        source_type: source_type.to_string(),
        source_id: source_id.map(ToOwned::to_owned),
        source_ref: source_ref.map(ToOwned::to_owned),
        project_id: project_id.map(ToOwned::to_owned),
        task_id: task_id.map(ToOwned::to_owned),
        title,
        summary,
        authority_class: authority_class.to_string(),
        validity: validity.to_string(),
        privacy_class: privacy_class.to_string(),
        freshness: freshness.to_string(),
        score,
        raw_policy: raw_policy.to_string(),
        evidence_refs: Value::Array(refs),
        metadata: json!({
            "source_profile": profile.as_str(),
            "projection": "mission_context_gather.compact_evidence",
            "derived_from_raw_source": lane_id == "conversation_audit" || lane_id == "skill_evidence",
        }),
    });
}

fn evidence_item_policy(
    lane_id: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    match lane_id {
        "runtime_truth" => (
            "runtime-env-and-monitor",
            "current_rule",
            "operational",
            "hot_runtime",
            "compact_only",
        ),
        "project_ssot" => (
            "file-first-lisp-and-compiled-project-universe",
            "current_rule",
            "internal",
            "compiled_runtime_bound",
            "compact_only",
        ),
        "reviewed_kb" => (
            "knowledge_review_state",
            "active_fact",
            "internal",
            "ttl_or_review_bound",
            "compact_only",
        ),
        "active_board" => (
            "board_projection",
            "current_state",
            "internal",
            "active_task_bound",
            "compact_only",
        ),
        "skill_evidence" => (
            "evidence-only",
            "evidence_only",
            "internal",
            "version_bound_or_historical",
            "compact_only",
        ),
        "conversation_audit" => (
            "provider_durable_conversation_read_model",
            "derived_from_conversation",
            "audit",
            "time_range_bound",
            "raw_opt_in_only",
        ),
        "support_refs" => (
            "redacted-support-catalog",
            "current_reference",
            "reference",
            "runtime_or_catalog_bound",
            "secret_refs_only",
        ),
        _ => (
            "forensics-only-cold-archive",
            "historical_evidence",
            "audit",
            "cold_archive",
            "explicit_path_or_full_debug_only",
        ),
    }
}

fn evidence_item_id(
    lane_id: &str,
    source_type: &str,
    source_id: Option<&str>,
    source_ref: Option<&str>,
    project_id: Option<&str>,
    task_id: Option<&str>,
    profile: SourceProfile,
    title: &str,
    summary: &str,
) -> String {
    let input = if evidence_item_uses_stable_projection_id(source_type) {
        format!(
            "stable|{}|{lane_id}|{source_type}|project:{}|task:{}|source_id:{}|source_ref:{}|title:{title}",
            profile.as_str(),
            project_id.unwrap_or(""),
            task_id.unwrap_or(""),
            source_id.unwrap_or(""),
            source_ref.unwrap_or("")
        )
    } else {
        format!(
            "content|{}|{lane_id}|{source_type}|project:{}|task:{}|source_id:{}|source_ref:{}|title:{title}|summary:{summary}",
            profile.as_str(),
            project_id.unwrap_or(""),
            task_id.unwrap_or(""),
            source_id.unwrap_or(""),
            source_ref.unwrap_or("")
        )
    };
    format!("evi-{}", short_sha256(&input, 16))
}

fn evidence_item_uses_stable_projection_id(source_type: &str) -> bool {
    matches!(
        source_type,
        "runtime_environment" | "support_catalog" | "deployment_closure_policy"
    )
}

fn dedupe_evidence_items(items: &mut Vec<EvidenceItemInput>) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(item.id.clone()));
}

fn short_sha256(input: &str, hex_chars: usize) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(hex_chars)
        .collect()
}

fn support_catalog_has_content(value: &Value) -> bool {
    any_field_has_content(
        value,
        &[
            "project_id",
            "projectId",
            "service_id",
            "serviceId",
            "deploy_center_slug",
            "deployCenterSlug",
            "domains",
            "dependencies",
            "agent_refs",
            "agentRefs",
        ],
    ) || nested_field_has_content(
        value,
        "runtime_target",
        &[
            "environment",
            "target",
            "ops_capability",
            "opsCapability",
            "executor",
            "container",
        ],
    ) || nested_field_has_content(
        value,
        "urls",
        &[
            "public_base_url",
            "publicBaseUrl",
            "frontend_url",
            "frontendUrl",
            "api_base_url",
            "apiBaseUrl",
        ],
    ) || nested_field_has_content(
        value,
        "manifest_refs",
        &[
            "root",
            "intent",
            "backend",
            "frontend",
            "operations",
            "service_manifest_refs",
            "serviceManifestRefs",
        ],
    ) || nested_field_has_content(value, "endpoints", &["health", "smoke"])
        || nested_field_has_content(
            value,
            "database",
            &[
                "migration_namespace",
                "migrationNamespace",
                "database_namespace",
                "databaseNamespace",
            ],
        )
        || value
            .get("deployment_closure")
            .is_some_and(deployment_closure_has_identity_content)
}

fn deployment_closure_has_identity_content(value: &Value) -> bool {
    any_field_has_content(
        value,
        &[
            "project_id",
            "projectId",
            "service_id",
            "serviceId",
            "deploy_center_slug",
            "deployCenterSlug",
            "runtime_target",
            "runtimeTarget",
            "executor",
            "container",
            "manifest_refs",
            "manifestRefs",
            "health_endpoints",
            "healthEndpoints",
            "smoke_endpoints",
            "smokeEndpoints",
        ],
    )
}

fn nested_field_has_content(value: &Value, object_key: &str, keys: &[&str]) -> bool {
    value
        .get(object_key)
        .is_some_and(|object| any_field_has_content(object, keys))
}

fn any_field_has_content(value: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        value
            .get(*key)
            .is_some_and(|field| !json_value_is_empty(field))
    })
}

fn summary_is_empty(value: &Value) -> bool {
    json_value_is_empty(value)
        || value
            .as_object()
            .is_some_and(|object| object.keys().all(|key| key == "schema" || key == "kind"))
}

fn json_value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(object) => object.is_empty() || object.values().all(json_value_is_empty),
        _ => false,
    }
}

fn compact_json_text(value: &Value, max_chars: usize) -> Option<String> {
    serde_json::to_string(value)
        .ok()
        .map(|text| compact_text(&text, max_chars))
        .filter(|text| !text.is_empty())
}

fn evidence_title(value: &Value, fallback: &str) -> String {
    text_from_sources(
        &[Some(value)],
        &[
            "title",
            "key",
            "name",
            "id",
            "conversation_id",
            "conversationId",
            "session_id",
            "sessionId",
            "sourceSkill",
        ],
    )
    .unwrap_or_else(|| fallback.to_string())
}

fn evidence_summary(value: &Value) -> String {
    text_from_sources(
        &[Some(value)],
        &[
            "summary",
            "description",
            "snippet",
            "content",
            "text",
            "excerpt",
            "purpose",
        ],
    )
    .map(|text| compact_text(&text, 900))
    .or_else(|| compact_json_text(value, 900))
    .unwrap_or_else(|| "Compact evidence summary".to_string())
}

fn source_id_from_value(value: &Value) -> Option<String> {
    text_from_sources(
        &[Some(value)],
        &[
            "id",
            "key",
            "conversation_id",
            "conversationId",
            "session_id",
            "sessionId",
            "name",
            "sourceSkill",
            "targetId",
            "target_id",
        ],
    )
}

fn source_ref_from_value(value: &Value) -> Option<String> {
    text_from_sources(
        &[Some(value)],
        &[
            "source_ref",
            "sourceRef",
            "source_path",
            "sourcePath",
            "path",
            "intent_path",
            "intentPath",
            "file_path",
            "source_file",
        ],
    )
}

fn numeric_field(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
}

fn credential_ref_source_id(value: &Value) -> Option<String> {
    let namespace = text_field(value, "namespace")?;
    let key_name = text_field(value, "key_name").or_else(|| text_field(value, "keyName"))?;
    Some(format!("{namespace}:{key_name}"))
}

fn noise_diagnostics(
    profile: SourceProfile,
    selection: SourceSelection,
    sources: &serde_json::Map<String, Value>,
) -> Value {
    let mut excluded = Vec::new();
    for (name, included) in [
        ("project_registry", selection.include_project),
        ("ssot", selection.include_ssot),
        ("kb", selection.include_kb),
        ("skill_context", selection.include_skill),
        ("infra", selection.include_infra),
        ("credential_refs", selection.include_credentials),
        ("board_tasks", selection.include_board),
        ("conversation_logs", selection.include_conversations),
    ] {
        if !included {
            excluded.push(name);
        }
    }
    json!({
        "schema": "missiond.context-noise-diagnostics.v1",
        "source_profile": profile.as_str(),
        "included_sources": sources.keys().cloned().collect::<Vec<_>>(),
        "excluded_by_profile": excluded,
        "credential_lane_opt_in": selection.include_credentials,
        "conversation_lane_opt_in": selection.include_conversations,
        "raw_sources_in_artifact": selection.include_raw_sources,
        "raw_sources_in_response": selection.include_raw_sources,
        "raw_sources_omitted": !selection.include_raw_sources,
        "rule": "Default grounding is authority-aware and does not preload conversations, infra skill evidence, or credential refs unless the source profile or explicit include flag opts in."
    })
}

fn context_noise_metrics(
    profile: SourceProfile,
    selection: SourceSelection,
    sources: &serde_json::Map<String, Value>,
    evidence_lanes: &Value,
    allowed_lanes: &[String],
    evidence_item_search: &Value,
) -> Value {
    let lane_counts = evidence_lanes
        .get("lanes")
        .and_then(Value::as_object)
        .map(|lanes| {
            lanes
                .iter()
                .map(|(lane, value)| {
                    (
                        lane.clone(),
                        json!({
                            "source_count": value
                                .get("source_count")
                                .cloned()
                                .unwrap_or_else(|| json!(0)),
                            "item_count": value
                                .get("item_count")
                                .cloned()
                                .unwrap_or_else(|| json!(0)),
                        }),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    json!({
        "schema": "missiond.context-noise-metrics.v1",
        "source_profile": profile.as_str(),
        "allowed_lanes": allowed_lanes,
        "raw_source_count": sources.len(),
        "lane_counts": lane_counts,
        "evidence_item_read_model": {
            "ok": evidence_item_search
                .get("ok")
                .cloned()
                .unwrap_or(Value::Bool(false)),
                "hit_count": evidence_item_search
                    .get("hit_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "raw_hit_count": evidence_item_search
                    .get("raw_hit_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "freshness_filtered_count": evidence_item_search
                    .get("freshness_filtered_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "compiled_policy_filtered_count": evidence_item_search
                    .get("compiled_policy_filtered_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "runtime_environment_filtered_count": evidence_item_search
                    .get("runtime_environment_filtered_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "incomplete_filtered_count": evidence_item_search
                    .get("incomplete_filtered_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "deduplicated_count": evidence_item_search
                    .get("deduplicated_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "truncated_count": evidence_item_search
                    .get("truncated_count")
                    .cloned()
                    .unwrap_or_else(|| json!(0)),
                "lane_counts": evidence_item_search
                .get("lane_counts")
                .cloned()
                .unwrap_or_else(|| json!({})),
            "filter_before_vector": evidence_item_search
                .get("filter_before_vector")
                .cloned()
                .unwrap_or(Value::Bool(true)),
        },
        "conversation_lane_enabled": selection.include_conversations,
        "credential_lane_enabled": selection.include_credentials,
        "raw_sources_in_artifact": selection.include_raw_sources,
        "raw_sources_in_response": selection.include_raw_sources,
        "raw_sources_omitted": !selection.include_raw_sources,
        "filtered_semantic_conversation_hits": filtered_semantic_conversation_hits(sources),
        "conversation_cross_project_drops": conversation_cross_project_drops(sources),
        "skill_low_confidence_drops": skill_low_confidence_drops(sources),
        "conversation_filtering": "conversation search owns project/time/type filter metrics; context-gather records whether the lane was enabled."
    })
}

fn filtered_semantic_conversation_hits(sources: &serde_json::Map<String, Value>) -> Value {
    sources
        .get("conversation_logs")
        .and_then(|value| {
            value
                .get("filteredSemanticHits")
                .or_else(|| value.get("filtered_semantic_hits"))
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn conversation_cross_project_drops(sources: &serde_json::Map<String, Value>) -> Value {
    sources
        .get("conversation_logs")
        .and_then(|value| {
            value
                .get("crossProjectDrops")
                .or_else(|| value.get("cross_project_drops"))
                .or_else(|| {
                    value
                        .get("filterMetrics")
                        .or_else(|| value.get("filter_metrics"))
                        .and_then(|metrics| {
                            metrics
                                .get("crossProjectDrops")
                                .or_else(|| metrics.get("cross_project_drops"))
                        })
                })
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn skill_low_confidence_drops(sources: &serde_json::Map<String, Value>) -> Value {
    sources
        .get("skill_context")
        .and_then(|value| {
            value
                .get("lowConfidenceDrops")
                .or_else(|| value.get("low_confidence_drops"))
                .or_else(|| {
                    value
                        .get("filterMetrics")
                        .or_else(|| value.get("filter_metrics"))
                        .and_then(|metrics| {
                            metrics
                                .get("lowConfidenceDrops")
                                .or_else(|| metrics.get("low_confidence_drops"))
                        })
                })
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn response_sources(
    sources: &serde_json::Map<String, Value>,
    source_summaries: &Value,
    include_raw_sources: bool,
) -> Value {
    if include_raw_sources {
        Value::Object(sources.clone())
    } else {
        source_summaries.clone()
    }
}

fn raw_sources_policy(include_raw_sources: bool) -> &'static str {
    if include_raw_sources {
        "Raw legacy sources are included because include_raw_sources=true or source_profile=full_debug."
    } else {
        "Raw legacy sources are omitted from the tool response and worker context pack; use source_summaries/evidence_lanes or rerun with include_raw_sources=true/full_debug for diagnostics."
    }
}

fn build_source_summaries(sources: &serde_json::Map<String, Value>) -> Value {
    let mut summaries = serde_json::Map::new();
    for (key, value) in sources {
        summaries.insert(key.clone(), summarize_source(key, value));
    }
    Value::Object(summaries)
}

fn summarize_source(key: &str, value: &Value) -> Value {
    match key {
        "runtime_environment" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "authority");
            insert_field(&mut map, value, "runtime_dir");
            insert_field(&mut map, value, "compiled_runtime_dir");
            insert_field(&mut map, value, "repo_runtime_authority");
            insert_field(&mut map, value, "monitor_endpoints");
            Value::Object(map)
        }
        "project_resolution" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "status");
            insert_field(&mut map, value, "matched_project_id");
            insert_field(&mut map, value, "matchedProjectId");
            insert_field(&mut map, value, "candidate_count");
            if let Some(project) = value.get("matched_project") {
                map.insert("matched_project".to_string(), summarize_project(project));
            }
            Value::Object(map)
        }
        "project_registry" => {
            let mut map = summary_base(key);
            for (name, summarized) in summarize_project(value).as_object().into_iter().flatten() {
                map.insert(name.clone(), summarized.clone());
            }
            Value::Object(map)
        }
        "ssot" => {
            let mut map = summary_base(key);
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                map.insert(
                    "text_preview".to_string(),
                    Value::String(compact_text(text, 720)),
                );
                map.insert("text_chars".to_string(), json!(text.chars().count()));
            }
            insert_field(&mut map, value, "path");
            insert_field(&mut map, value, "source");
            Value::Object(map)
        }
        "kb" => summarize_array_source(key, value, 5),
        "deployment_events" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "status");
            insert_field(&mut map, value, "source");
            insert_field(&mut map, value, "authority");
            insert_field(&mut map, value, "read_model");
            insert_field(&mut map, value, "event_type");
            insert_field(&mut map, value, "since");
            insert_field(&mut map, value, "filter_before_injection");
            insert_field(&mut map, value, "candidate_count");
            insert_field(&mut map, value, "filtered_count");
            insert_field(&mut map, value, "dropped_count");
            insert_field(&mut map, value, "filters");
            insert_compact_field(&mut map, value, "diagnostic", 260);
            map.insert(
                "event_count".to_string(),
                json!(array_len(value.get("events"))),
            );
            map.insert(
                "events".to_string(),
                summarize_items(value.get("events"), 8, summarize_deployment_event_item),
            );
            Value::Object(map)
        }
        "board_tasks" => {
            let mut map = summary_base(key);
            if let Some(meta) = value.get("meta") {
                map.insert("meta".to_string(), meta.clone());
            }
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get("data"))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get("data"), 5, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "id");
                    insert_field(&mut item_map, item, "title");
                    insert_field(&mut item_map, item, "status");
                    Value::Object(item_map)
                }),
            );
            Value::Object(map)
        }
        "skill_context" => {
            let mut map = summary_base(key);
            map.insert(
                "skills".to_string(),
                summarize_items(value.get("skills"), 10, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "name");
                    insert_field(&mut item_map, item, "matched_by");
                    insert_field(&mut item_map, item, "path");
                    insert_compact_field(&mut item_map, item, "description", 180);
                    Value::Object(item_map)
                }),
            );
            map.insert(
                "project_skill_links".to_string(),
                summarize_items(value.get("project_skill_links"), 6, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "skill");
                    insert_field(&mut item_map, item, "confidence");
                    insert_field(&mut item_map, item, "matchedBy");
                    insert_field(&mut item_map, item, "path");
                    Value::Object(item_map)
                }),
            );
            map.insert(
                "operational_fact_count".to_string(),
                json!(array_len(value.get("operational_facts"))),
            );
            map.insert("kb_count".to_string(), json!(array_len(value.get("kb"))));
            map.insert(
                "board_count".to_string(),
                json!(array_len(value.get("board"))),
            );
            Value::Object(map)
        }
        "infra" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "status");
            insert_field(&mut map, value, "feature");
            insert_field(&mut map, value, "layer");
            insert_field(&mut map, value, "enable_env");
            insert_field(&mut map, value, "enable_all_env");
            insert_field(&mut map, value, "fallback_status");
            insert_field(&mut map, value, "fallback_source");
            insert_compact_field(&mut map, value, "reason", 220);
            insert_field(&mut map, value, "authority");
            insert_field(&mut map, value, "redaction");
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get("items"))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get("items"), 5, summarize_infra_item),
            );
            map.insert(
                "fallback_items".to_string(),
                summarize_items(value.get("fallback_items"), 5, summarize_infra_item),
            );
            Value::Object(map)
        }
        "credential_refs" => {
            let mut map = summary_base(key);
            insert_field(&mut map, value, "status");
            insert_field(&mut map, value, "feature");
            insert_field(&mut map, value, "layer");
            insert_field(&mut map, value, "enable_env");
            insert_field(&mut map, value, "enable_all_env");
            insert_compact_field(&mut map, value, "reason", 220);
            map.insert(
                "credential_ref_count".to_string(),
                json!(array_len(value.get("credentialRefs"))),
            );
            map.insert(
                "credentialRefs".to_string(),
                summarize_items(value.get("credentialRefs"), 8, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "namespace");
                    insert_field(&mut item_map, item, "keyName");
                    insert_field(&mut item_map, item, "targetId");
                    insert_field(&mut item_map, item, "requiredCapability");
                    insert_field(&mut item_map, item, "availability");
                    insert_compact_field(&mut item_map, item, "purpose", 220);
                    Value::Object(item_map)
                }),
            );
            Value::Object(map)
        }
        "conversation_logs" => summarize_conversation_source(key, value),
        _ => summarize_array_source(key, value, 5),
    }
}

fn summary_base(kind: &str) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "schema".to_string(),
        Value::String("missiond.source-summary.v1".to_string()),
    );
    map.insert("kind".to_string(), Value::String(kind.to_string()));
    map
}

fn summarize_project(value: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for key in [
        "id",
        "path",
        "intent_path",
        "intentPath",
        "kind",
        "active",
        "source",
        "db_status",
        "dbStatus",
        "github_url",
        "parent_id",
    ] {
        insert_field(&mut map, value, key);
    }
    Value::Object(map)
}

fn summarize_array_source(key: &str, value: &Value, limit: usize) -> Value {
    let mut map = summary_base(key);
    if let Some(items) = value.as_array() {
        map.insert("item_count".to_string(), json!(items.len()));
        map.insert(
            "items".to_string(),
            Value::Array(
                items
                    .iter()
                    .take(limit)
                    .map(summarize_generic_item)
                    .collect(),
            ),
        );
    } else {
        map.insert(
            "shape".to_string(),
            Value::String(value_shape(value).to_string()),
        );
    }
    Value::Object(map)
}

fn summarize_conversation_source(key: &str, value: &Value) -> Value {
    let mut map = summary_base(key);
    for item_key in ["results", "items", "data"] {
        if value.get(item_key).and_then(Value::as_array).is_some() {
            map.insert(
                "item_count".to_string(),
                json!(array_len(value.get(item_key))),
            );
            map.insert(
                "items".to_string(),
                summarize_items(value.get(item_key), 5, |item| {
                    let mut item_map = serde_json::Map::new();
                    insert_field(&mut item_map, item, "conversation_id");
                    insert_field(&mut item_map, item, "conversationId");
                    insert_field(&mut item_map, item, "session_id");
                    insert_field(&mut item_map, item, "sessionId");
                    insert_field(&mut item_map, item, "project");
                    insert_field(&mut item_map, item, "conversation_type");
                    insert_field(&mut item_map, item, "timestamp");
                    insert_compact_field(&mut item_map, item, "snippet", 260);
                    insert_compact_field(&mut item_map, item, "content", 260);
                    Value::Object(item_map)
                }),
            );
            return Value::Object(map);
        }
    }
    summarize_array_source(key, value, 5)
}

fn summarize_generic_item(item: &Value) -> Value {
    if let Some(object) = item.as_object() {
        let mut map = serde_json::Map::new();
        for key in [
            "id",
            "key",
            "title",
            "category",
            "source_path",
            "sourcePath",
            "path",
            "status",
        ] {
            insert_field(&mut map, item, key);
        }
        for key in ["summary", "description", "text", "content", "snippet"] {
            insert_compact_field(&mut map, item, key, 260);
        }
        if map.is_empty() {
            map.insert("field_count".to_string(), json!(object.len()));
        }
        Value::Object(map)
    } else if let Some(text) = item.as_str() {
        Value::String(compact_text(text, 260))
    } else {
        item.clone()
    }
}

fn summarize_items<F>(value: Option<&Value>, limit: usize, mapper: F) -> Value
where
    F: Fn(&Value) -> Value,
{
    let Some(items) = value.and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    Value::Array(items.iter().take(limit).map(mapper).collect())
}

fn summarize_infra_item(item: &Value) -> Value {
    let mut item_map = serde_json::Map::new();
    insert_field(&mut item_map, item, "sourceType");
    insert_field(&mut item_map, item, "source");
    insert_field(&mut item_map, item, "title");
    insert_field(&mut item_map, item, "authority");
    insert_field(&mut item_map, item, "project_id");
    insert_field(&mut item_map, item, "service_id");
    insert_field(&mut item_map, item, "deploy_center_slug");
    insert_field(&mut item_map, item, "runtime_target");
    insert_field(&mut item_map, item, "manifest_refs");
    insert_field(&mut item_map, item, "sourceSkill");
    insert_field(&mut item_map, item, "sourcePath");
    insert_field(&mut item_map, item, "sourceLine");
    insert_field(&mut item_map, item, "confidence");
    insert_field(&mut item_map, item, "promoteTo");
    insert_field(&mut item_map, item, "credentialInlineRisk");
    insert_compact_field(&mut item_map, item, "excerpt", 360);
    Value::Object(item_map)
}

fn summarize_deployment_event_item(item: &Value) -> Value {
    let mut item_map = serde_json::Map::new();
    for key in [
        "sourceType",
        "source",
        "authority",
        "event_ref",
        "seq",
        "created_at",
        "event_id",
        "event_kind",
        "producer_service_id",
        "project_id",
        "target_service_id",
        "deploy_center_slug",
        "subject",
        "correlation_id",
        "trace_id",
        "event_source",
        "event_authority",
    ] {
        insert_field(&mut item_map, item, key);
    }
    insert_compact_field(&mut item_map, item, "summary", 420);
    Value::Object(item_map)
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map(Vec::len).unwrap_or(0)
}

fn insert_field(map: &mut serde_json::Map<String, Value>, value: &Value, key: &str) {
    if let Some(field) = value.get(key) {
        map.insert(key.to_string(), field.clone());
    }
}

fn insert_compact_field(
    map: &mut serde_json::Map<String, Value>,
    value: &Value,
    key: &str,
    max_chars: usize,
) {
    if let Some(text) = value.get(key).and_then(Value::as_str) {
        map.insert(
            key.to_string(),
            Value::String(compact_text(text, max_chars)),
        );
    }
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut truncated = collapsed.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn value_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn context_pack_artifact_payload(payload: &Value, include_raw_sources: bool) -> Value {
    if include_raw_sources {
        return payload.clone();
    }
    let mut compact = payload.clone();
    if let Some(object) = compact.as_object_mut() {
        object.remove("sources");
        object.insert("raw_sources_omitted".to_string(), Value::Bool(true));
        object.insert(
            "raw_sources_policy".to_string(),
            Value::String(
                "Raw legacy sources are omitted from the worker context pack; use evidence_lanes or rerun with include_raw_sources=true/full_debug for diagnostics."
                    .to_string(),
            ),
        );
    }
    compact
}

fn build_context_gather_run_input(
    payload: &Value,
    profile: SourceProfile,
    selection: SourceSelection,
    artifact_hash: Option<&str>,
) -> ContextGatherRunInput {
    let query = payload
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let project_id = payload
        .get("project_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let task_id = payload
        .get("task_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let lane_counts = payload
        .get("context_noise_metrics")
        .and_then(|value| value.get("lane_counts"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut metrics = payload
        .get("context_noise_metrics")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(object) = metrics.as_object_mut() {
        object.insert(
            "raw_source_injected".to_string(),
            Value::Bool(selection.include_raw_sources),
        );
        object.insert(
            "credential_opt_in".to_string(),
            Value::Bool(selection.include_credentials),
        );
        object.insert(
            "conversation_opt_in".to_string(),
            Value::Bool(selection.include_conversations),
        );
        object.insert(
            "support_ref_count".to_string(),
            payload
                .get("support_catalog")
                .and_then(|value| value.get("credential_ref_count"))
                .cloned()
                .unwrap_or_else(|| json!(0)),
        );
        object.insert(
            "resolver_source".to_string(),
            resolver_source_from_payload(payload)
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "runtime_root_consistent".to_string(),
            runtime_root_consistent_from_payload(payload)
                .map(Value::Bool)
                .unwrap_or(Value::Null),
        );
    }
    ContextGatherRunInput {
        id: stable_context_gather_run_id(
            &query,
            project_id.as_deref(),
            task_id.as_deref(),
            profile,
            artifact_hash,
        ),
        query,
        project_id,
        task_id,
        source_profile: profile.as_str().to_string(),
        lane_counts,
        metrics,
        raw_sources_included: selection.include_raw_sources,
        credential_opt_in: selection.include_credentials,
        conversation_opt_in: selection.include_conversations,
        resolver_source: resolver_source_from_payload(payload),
        runtime_root_consistent: runtime_root_consistent_from_payload(payload),
        artifact_hash: artifact_hash.map(ToOwned::to_owned),
        diagnostics: payload
            .get("diagnostics")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    }
}

fn stable_context_gather_run_id(
    query: &str,
    project_id: Option<&str>,
    task_id: Option<&str>,
    profile: SourceProfile,
    artifact_hash: Option<&str>,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}",
        query,
        project_id.unwrap_or(""),
        task_id.unwrap_or(""),
        profile.as_str(),
        artifact_hash.unwrap_or("")
    );
    format!("context-gather-{}", short_sha256(&input, 16))
}

fn resolver_source_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("source_summaries")
        .and_then(|summaries| summaries.get("project_resolution"))
        .and_then(|summary| summary.get("matched_project"))
        .and_then(|project| project.get("source"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("source_summaries")
                .and_then(|summaries| summaries.get("project_registry"))
                .and_then(|summary| summary.get("source"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("support_catalog")
                .and_then(|catalog| catalog.get("resolver_source"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn runtime_root_consistent_from_payload(payload: &Value) -> Option<bool> {
    let runtime = payload.get("runtime_environment")?;
    let project_root = runtime.get("project_root").and_then(Value::as_str)?;
    let runtime_dir = runtime.get("runtime_dir").and_then(Value::as_str)?;
    let compiled_runtime_dir = runtime
        .get("compiled_runtime_dir")
        .and_then(Value::as_str)?;
    let env_runtime = runtime
        .get("env_presence")
        .and_then(|env| env.get("MISSIOND_RUNTIME_DIR"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let env_compiled = runtime
        .get("env_presence")
        .and_then(|env| env.get("MISSIOND_COMPILED_RUNTIME_DIR"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if env_runtime || env_compiled {
        return Some(!runtime_dir.trim().is_empty() && !compiled_runtime_dir.trim().is_empty());
    }
    Some(runtime_dir.starts_with(project_root) && compiled_runtime_dir.starts_with(runtime_dir))
}

async fn persist_evidence_lane_projection(
    state: &AppState,
    run: &ContextGatherRunInput,
    items: &[EvidenceItemInput],
) -> Value {
    let mut errors = Vec::new();
    let mut evidence_items_written = 0usize;
    if let Err(err) = state.store.record_context_gather_run(run).await {
        tracing::warn!(run_id = run.id.as_str(), error = %err, "failed to persist context_gather_runs row");
        errors.push(json!({"target": "context_gather_runs", "error": err.to_string()}));
    }
    if !items.is_empty() {
        match state.store.upsert_evidence_items(items).await {
            Ok(count) => evidence_items_written = count,
            Err(err) => {
                tracing::warn!(run_id = run.id.as_str(), error = %err, "failed to persist evidence_items projection");
                errors.push(json!({"target": "evidence_items", "error": err.to_string()}));
            }
        }
    }
    json!({
        "schema": "missiond.evidence-lane-persistence.v1",
        "ok": errors.is_empty(),
        "context_gather_run_id": run.id.as_str(),
        "evidence_item_count": items.len(),
        "evidence_items_written": evidence_items_written,
        "errors": errors,
    })
}

fn handle_context_boot(args: Value) -> Result<ToolResult> {
    let args: ContextBootArgs = serde_json::from_value(args).unwrap_or_default();
    let (capsule, source_path) = read_codex_boot_context();
    let include_capsule = args.include_capsule.unwrap_or(true);
    let capsule_len = capsule.chars().count();
    Ok(ToolResult::json_pretty(&json!({
        "ok": true,
        "schema": "missiond.codex-boot-context.v1",
        "project_id": args.project_id,
        "task_id": args.task_id,
        "source_path": source_path,
        "capsule_chars": capsule_len,
        "layers": ["L0-always-on", "L1-current-task", "L2-grounded-facts", "L3-cold-evidence"],
        "capsule": if include_capsule { Value::String(capsule) } else { Value::Null },
        "next_action": "Use this boot capsule as the collaboration protocol. For missing task/project facts, call mission_context_gather with explicit unknowns instead of preloading broad KB or logs."
    })))
}

fn read_codex_boot_context() -> (String, String) {
    for candidate in codex_boot_context_candidates() {
        if let Ok(text) = fs::read_to_string(&candidate) {
            return (text, candidate.display().to_string());
        }
    }
    (
        CODEX_BOOT_CONTEXT_FALLBACK.to_string(),
        "compiled-fallback:.missiond/v3/evidence/codex-boot-context.lisp".to_string(),
    )
}

fn codex_boot_context_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = env::var("MISSIOND_CODEX_BOOT_CONTEXT") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(root) = env::var("MISSIOND_PROJECT_ROOT") {
        candidates.push(PathBuf::from(root).join(CODEX_BOOT_CONTEXT_REL));
    }
    if let Ok(root) = env::var("MISSIOND_ORCHESTRATOR_ROOT") {
        candidates.push(PathBuf::from(root).join(CODEX_BOOT_CONTEXT_REL));
    }
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join(CODEX_BOOT_CONTEXT_REL));
    }
    candidates
}

const CONTEXT_CAPSULE_RUNTIME_REL: &str = ".missiond/v3/runtime/context-capsules";

#[derive(Debug)]
struct MaterializedContextPack {
    canonical_path: PathBuf,
    worker_path: PathBuf,
}

fn materialize_context_capsule(hash: &str, lisp_content: &str) -> Result<PathBuf> {
    let dir = context_capsule_runtime_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{hash}.lisp"));
    fs::write(&path, lisp_content)?;
    Ok(path)
}

fn materialize_context_pack_file(hash: &str, payload: &Value) -> Result<MaterializedContextPack> {
    let bytes = serde_json::to_vec_pretty(payload)?;
    let canonical_dir = context_gather_runtime_dir();
    fs::create_dir_all(&canonical_dir)?;
    let canonical_path = canonical_dir.join(format!("{hash}.json"));
    fs::write(&canonical_path, &bytes)?;

    let worker_path = if context_gather_uses_external_runtime() {
        let worker_dir = context_gather_worker_visible_dir();
        fs::create_dir_all(&worker_dir)?;
        let worker_path = worker_dir.join(format!("{hash}.json"));
        fs::write(&worker_path, &bytes)?;
        worker_path
    } else {
        canonical_path.clone()
    };

    Ok(MaterializedContextPack {
        canonical_path,
        worker_path,
    })
}

fn runtime_environment_payload() -> Value {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    let compiled_runtime_dir = missiond_compiled_runtime_dir(&runtime_dir);
    let repo_runtime_dir = project_root.join(".missiond/v3/runtime");
    let compiled_runtime_config = compiled_runtime_dir.join("compiled-runtime-config.json");

    json!({
        "schema": "missiond.runtime-environment-context.v1",
        "authority": "runtime-env-and-monitor",
        "rule": "For deployed MissionD, runtime artifacts are authoritative under MISSIOND_RUNTIME_DIR and MISSIOND_COMPILED_RUNTIME_DIR. Repo .missiond/v3/runtime/** is dev/cold evidence only and must not be used to declare deployed compiled projections missing. A bounded worker-readable mirror under MISSIOND_RUNTIME_DIR/context-gather-worker/** may be written for provider CLIs that cannot read outside their workspace; repo .missiond/v3/runtime/context-gather-worker/** is dev fallback only.",
        "project_root": project_root.display().to_string(),
        "orchestrator_root": env::var("MISSIOND_ORCHESTRATOR_ROOT").ok(),
        "runtime_dir": runtime_dir.display().to_string(),
        "compiled_runtime_dir": compiled_runtime_dir.display().to_string(),
        "repo_runtime_dir": repo_runtime_dir.display().to_string(),
        "repo_runtime_authority": "dev-cold-evidence-only",
        "context_pack_worker_visible_dir": context_gather_worker_visible_dir().display().to_string(),
        "monitor_endpoints": {
            "canonical_local_http": "http://127.0.0.1:9120/api/monitor/jarvis",
            "canonical_public_https": "https://jarvis.xiaojinpro.top/api/monitor/jarvis",
            "public_path": "/api/monitor/jarvis",
            "daemon_path": "/api/monitor/jarvis",
            "rule": "Use these endpoints for Jarvis chain readiness. Do not guess ports or probe unix-socket paths unless a dedicated diagnostic asks for it."
        },
        "compiled_runtime_config": {
            "path": compiled_runtime_config.display().to_string(),
            "exists": compiled_runtime_config.exists()
        },
        "env_presence": {
            "MISSIOND_PROJECT_ROOT": env::var("MISSIOND_PROJECT_ROOT").is_ok(),
            "MISSIOND_ORCHESTRATOR_ROOT": env::var("MISSIOND_ORCHESTRATOR_ROOT").is_ok(),
            "MISSIOND_RUNTIME_DIR": env::var("MISSIOND_RUNTIME_DIR").is_ok(),
            "MISSIOND_COMPILED_RUNTIME_DIR": env::var("MISSIOND_COMPILED_RUNTIME_DIR").is_ok()
        },
        "diagnostic": "If runtime files appear missing in the repository, check this runtime_environment source and monitor_endpoints.canonical_local_http or monitor_endpoints.canonical_public_https before reporting a deployed runtime failure."
    })
}

fn context_gather_runtime_dir() -> PathBuf {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    if env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return runtime_dir.join("context-gather");
    }
    project_root.join(CONTEXT_GATHER_RUNTIME_REL)
}

fn context_gather_worker_visible_dir() -> PathBuf {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    context_gather_worker_visible_dir_for(
        &project_root,
        &runtime_dir,
        context_gather_uses_external_runtime(),
    )
}

fn context_gather_worker_visible_dir_for(
    project_root: &Path,
    runtime_dir: &Path,
    uses_external_runtime: bool,
) -> PathBuf {
    if uses_external_runtime {
        return runtime_dir.join("context-gather-worker");
    }
    project_root.join(CONTEXT_GATHER_WORKER_VISIBLE_REL)
}

fn context_gather_uses_external_runtime() -> bool {
    env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn context_capsule_runtime_dir() -> PathBuf {
    let project_root = missiond_project_root();
    let runtime_dir = missiond_runtime_dir(&project_root);
    if env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return runtime_dir.join("context-capsules");
    }
    project_root.join(CONTEXT_CAPSULE_RUNTIME_REL)
}

fn missiond_runtime_dir(project_root: &Path) -> PathBuf {
    if let Ok(value) = env::var("MISSIOND_RUNTIME_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    project_root.join(".missiond/v3/runtime")
}

fn missiond_compiled_runtime_dir(runtime_dir: &Path) -> PathBuf {
    if let Ok(value) = env::var("MISSIOND_COMPILED_RUNTIME_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    runtime_dir.join("compiled")
}

fn missiond_project_root() -> PathBuf {
    for key in [
        "MISSIOND_PROJECT_ROOT",
        "MISSIOND_REPO_ROOT",
        "MISSIOND_WORKSPACE_ROOT",
        "MISSIOND_ORCHESTRATOR_ROOT",
    ] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn normalized_query(args: &ContextGatherArgs) -> String {
    args.query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let joined = args
                .unknowns
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        })
        .or_else(|| args.project_id.clone())
        .or_else(|| args.skill.clone())
        .or_else(|| args.infra_target.clone())
        .unwrap_or_default()
}

fn insert_subcall(
    sources: &mut serde_json::Map<String, Value>,
    diagnostics: &mut Vec<Value>,
    key: &str,
    result: Result<ToolResult>,
) {
    match result {
        Ok(result) => {
            let is_error = result.is_error.unwrap_or(false);
            let value = tool_result_to_value(result);
            if is_error {
                diagnostics.push(json!({"source": key, "error": value}));
            } else {
                sources.insert(key.to_string(), value);
            }
        }
        Err(err) => diagnostics.push(json!({"source": key, "error": err.to_string()})),
    }
}

fn tool_result_to_value(result: ToolResult) -> Value {
    let Some(ToolContent::Text { text }) = result.content.into_iter().next() else {
        return Value::Null;
    };
    serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "text": text }))
}

fn collect_evidence_refs(sources: &serde_json::Map<String, Value>) -> Vec<Value> {
    collect_evidence_refs_from_value(&Value::Object(sources.clone()))
}

fn collect_evidence_refs_from_value(value: &Value) -> Vec<Value> {
    let mut refs = Vec::new();
    collect_evidence_refs_inner(value, "$", &mut refs);
    refs.truncate(60);
    refs
}

fn collect_evidence_refs_inner(value: &Value, path: &str, refs: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            for key in [
                "path",
                "intentPath",
                "intent_path",
                "file_path",
                "source_path",
                "sourcePath",
                "source_file",
                "id",
                "key",
            ] {
                if let Some(v) = map.get(key).and_then(Value::as_str) {
                    if !v.is_empty() {
                        refs.push(json!({"source_path": path, "field": key, "value": v}));
                    }
                }
            }
            for (k, v) in map {
                collect_evidence_refs_inner(v, &format!("{path}.{k}"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                collect_evidence_refs_inner(item, &format!("{path}[{idx}]"), refs);
                if refs.len() >= 60 {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{
        attach_infra_os_disabled_support_fallback, build_evidence_items, build_evidence_lanes,
        build_source_summaries, build_support_catalog, collect_evidence_refs_from_value,
        context_gather_persist_artifact, context_gather_persist_read_model,
        context_gather_worker_visible_dir_for, context_noise_metrics,
        context_pack_artifact_payload, dedupe_evidence_search_items,
        deployment_event_filter_timeline_row, deployment_event_item_from_timeline_row,
        diagnostics_have_hard_failures, evidence_item_id,
        evidence_item_read_model_scope_allows_search, evidence_item_uses_stable_projection_id,
        filter_incomplete_deployment_closure_evidence_items,
        filter_stale_compiled_policy_evidence_items_with_fingerprint,
        filter_stale_runtime_environment_evidence_items_with_dir,
        optional_infra_os_disabled_diagnostic, optional_infra_os_disabled_source, response_sources,
        source_selection, support_catalog_has_content, CompiledDeploymentPolicyFingerprint,
        ContextGatherArgs, DeploymentEventFilterResult, SourceProfile,
    };

    fn args(value: serde_json::Value) -> ContextGatherArgs {
        serde_json::from_value(value).expect("context gather args")
    }

    #[test]
    fn intent_default_profile_excludes_noisy_sources() {
        let args = args(json!({"query": "MissionD noise"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert_eq!(profile.as_str(), "intent_default");
        assert!(selection.include_project);
        assert!(selection.include_ssot);
        assert!(selection.include_kb);
        assert!(selection.include_board);
        assert!(!selection.include_skill);
        assert!(!selection.include_infra);
        assert!(!selection.include_conversations);
        assert!(!selection.include_credentials);
        assert!(!selection.include_raw_sources);
    }

    #[test]
    fn default_context_gather_persists_read_model_without_artifact() {
        let args = args(json!({"query": "MissionD noise"}));

        assert!(!context_gather_persist_artifact(&args));
        assert!(context_gather_persist_read_model(&args));
    }

    #[test]
    fn artifact_persistence_forces_read_model_projection() {
        let args = args(json!({
            "query": "MissionD noise",
            "persist": true,
            "persist_read_model": false
        }));

        assert!(context_gather_persist_artifact(&args));
        assert!(context_gather_persist_read_model(&args));
    }

    #[test]
    fn explicit_read_model_disable_only_applies_without_artifact() {
        let args = args(json!({
            "query": "MissionD noise",
            "persist_read_model": false
        }));

        assert!(!context_gather_persist_artifact(&args));
        assert!(!context_gather_persist_read_model(&args));
    }

    #[test]
    fn deploy_ops_profile_enables_skill_infra_and_credentials() {
        let args = args(json!({"query": "deploy payments", "source_profile": "deploy_ops"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert!(selection.include_skill);
        assert!(selection.include_infra);
        assert!(selection.include_credentials);
        assert!(!selection.include_conversations);
    }

    #[test]
    fn evidence_read_model_skips_unscoped_non_debug_profiles() {
        assert!(!evidence_item_read_model_scope_allows_search(
            SourceProfile::IntentDefault,
            None
        ));
        assert!(!evidence_item_read_model_scope_allows_search(
            SourceProfile::ConversationAudit,
            Some(" ")
        ));
        assert!(evidence_item_read_model_scope_allows_search(
            SourceProfile::ConversationAudit,
            Some("payments")
        ));
        assert!(evidence_item_read_model_scope_allows_search(
            SourceProfile::FullDebug,
            None
        ));
    }

    #[test]
    fn explicit_conversation_opt_in_overrides_default_profile() {
        let args = args(json!({"query": "audit prior answer", "include_conversations": true}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        assert!(selection.include_conversations);
        assert!(!selection.include_credentials);
    }

    #[test]
    fn worker_visible_context_pack_uses_runtime_dir_when_deployed() {
        let project_root = Path::new("/release/source");
        let runtime_dir = Path::new("/var/missiond/runtime");

        assert_eq!(
            context_gather_worker_visible_dir_for(project_root, runtime_dir, true),
            Path::new("/var/missiond/runtime/context-gather-worker")
        );
        assert_eq!(
            context_gather_worker_visible_dir_for(project_root, runtime_dir, false),
            Path::new("/release/source/.missiond/v3/runtime/context-gather-worker")
        );
    }

    #[test]
    fn infra_os_disabled_sources_are_visible_but_not_evidence_items() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "infra".to_string(),
            optional_infra_os_disabled_source("infra", "mission_infra_query"),
        );
        sources.insert(
            "credential_refs".to_string(),
            optional_infra_os_disabled_source("credential_refs", "mission_infra_query"),
        );

        let summaries = build_source_summaries(&sources);
        assert_eq!(
            summaries
                .get("infra")
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str),
            Some("feature_disabled")
        );
        assert_eq!(
            summaries
                .get("credential_refs")
                .and_then(|value| value.get("feature"))
                .and_then(Value::as_str),
            Some("infra-os")
        );

        let lanes = build_evidence_lanes(&sources);
        assert_eq!(
            lanes
                .get("lanes")
                .and_then(|value| value.get("skill_evidence"))
                .and_then(|value| value.get("item_count"))
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            lanes
                .get("lanes")
                .and_then(|value| value.get("support_refs"))
                .and_then(|value| value.get("item_count"))
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            optional_infra_os_disabled_diagnostic("infra")
                .get("status")
                .and_then(Value::as_str),
            Some("feature_disabled")
        );
    }

    #[test]
    fn infra_os_disabled_uses_support_catalog_fallback_for_deploy_ops() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "infra".to_string(),
            optional_infra_os_disabled_source("infra", "mission_infra_query"),
        );
        sources.insert(
            "project_registry".to_string(),
            json!({
                "id": "payments",
                "source": "compiled-service-runtime",
                "serviceRuntime": {
                    "id": "payments",
                    "project": "payments",
                    "health": ["/payments/health/ready"],
                    "supportCatalog": {
                        "service_id": "payments",
                        "project_id": "payments",
                        "deploy_center_slug": "xjp-payments",
                        "runtime_target": "gcp-runtime",
                        "executor": "gcp-agent",
                        "container": "xjp-payments",
                        "service_manifest_refs": ["services/payments/service.manifest.toml"],
                        "db_migration_namespace": "payments"
                    }
                }
            }),
        );

        attach_infra_os_disabled_support_fallback(&mut sources);
        let summaries = build_source_summaries(&sources);
        let infra = summaries.get("infra").expect("infra summary");
        assert_eq!(
            infra.get("status").and_then(Value::as_str),
            Some("feature_disabled")
        );
        assert_eq!(
            infra.get("fallback_status").and_then(Value::as_str),
            Some("support_catalog_available")
        );
        assert_eq!(infra.get("item_count").and_then(Value::as_u64), Some(2));
        assert_eq!(
            infra
                .get("fallback_items")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        let rendered = serde_json::to_string(infra).expect("infra summary json");
        assert!(rendered.contains("service.manifest.toml"));
        assert!(rendered.contains("deployment_closure_policy"));

        let lanes = build_evidence_lanes(&sources);
        assert_eq!(
            lanes
                .get("lanes")
                .and_then(|value| value.get("skill_evidence"))
                .and_then(|value| value.get("item_count"))
                .and_then(Value::as_u64),
            Some(2)
        );
    }

    #[test]
    fn context_gather_evidence_search_dedupes_repeated_projections() {
        let first = missiond_core::types::EvidenceItemInput {
            id: "evi-a".to_string(),
            lane_id: "support_refs".to_string(),
            source_type: "deployment_closure_policy".to_string(),
            source_id: Some("payments".to_string()),
            source_ref: None,
            project_id: Some("payments".to_string()),
            task_id: None,
            title: "Deployment closure policy".to_string(),
            summary: "context gather projection".to_string(),
            authority_class: "redacted-support-catalog".to_string(),
            validity: "current_reference".to_string(),
            privacy_class: "reference".to_string(),
            freshness: "runtime_or_catalog_bound".to_string(),
            score: Some(1.0),
            raw_policy: "secret_refs_only".to_string(),
            evidence_refs: json!([]),
            metadata: json!({}),
        };
        let mut duplicate = first.clone();
        duplicate.id = "evi-b".to_string();
        duplicate.source_ref = Some("/srv/payments".to_string());
        duplicate.summary = "compiled backfill projection".to_string();

        let (items, deduplicated_count, truncated_count) =
            dedupe_evidence_search_items(vec![first.clone(), duplicate.clone()], 10);
        assert_eq!(items.len(), 1);
        assert_eq!(deduplicated_count, 1);
        assert_eq!(truncated_count, 0);
        assert_eq!(items[0].id, "evi-a");

        let mut second_unique = first.clone();
        second_unique.id = "evi-c".to_string();
        second_unique.source_type = "support_catalog".to_string();
        let mut third_unique = first;
        third_unique.id = "evi-d".to_string();
        third_unique.source_type = "service_runtime".to_string();

        let (items, deduplicated_count, truncated_count) = dedupe_evidence_search_items(
            vec![items[0].clone(), duplicate, second_unique, third_unique],
            2,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(deduplicated_count, 1);
        assert_eq!(truncated_count, 1);
    }

    #[test]
    fn volatile_projection_evidence_ids_ignore_release_specific_summary_text() {
        assert!(evidence_item_uses_stable_projection_id(
            "runtime_environment"
        ));
        assert!(evidence_item_uses_stable_projection_id(
            "deployment_closure_policy"
        ));
        assert!(!evidence_item_uses_stable_projection_id(
            "conversation_fact_extract"
        ));

        let first = evidence_item_id(
            "runtime_truth",
            "runtime_environment",
            None,
            None,
            Some("missiond"),
            None,
            SourceProfile::IntentDefault,
            "Runtime truth",
            r#"{"compiled_runtime_dir":"/release/old/compiled-runtime"}"#,
        );
        let second = evidence_item_id(
            "runtime_truth",
            "runtime_environment",
            None,
            None,
            Some("missiond"),
            None,
            SourceProfile::IntentDefault,
            "Runtime truth",
            r#"{"compiled_runtime_dir":"/release/new/compiled-runtime"}"#,
        );
        let deploy_ops = evidence_item_id(
            "runtime_truth",
            "runtime_environment",
            None,
            None,
            Some("missiond"),
            None,
            SourceProfile::DeployOps,
            "Runtime truth",
            r#"{"compiled_runtime_dir":"/release/new/compiled-runtime"}"#,
        );
        let conversation_first = evidence_item_id(
            "conversation_audit",
            "conversation_fact_extract",
            Some("c1"),
            None,
            Some("missiond"),
            None,
            SourceProfile::ConversationAudit,
            "Conversation fact",
            "old content",
        );
        let conversation_second = evidence_item_id(
            "conversation_audit",
            "conversation_fact_extract",
            Some("c1"),
            None,
            Some("missiond"),
            None,
            SourceProfile::ConversationAudit,
            "Conversation fact",
            "new content",
        );

        assert_eq!(first, second);
        assert_ne!(first, deploy_ops);
        assert_ne!(conversation_first, conversation_second);
    }

    #[test]
    fn evidence_search_filters_stale_compiled_policy_refs() {
        let stale = missiond_core::types::EvidenceItemInput {
            id: "evi-stale".to_string(),
            lane_id: "support_refs".to_string(),
            source_type: "deployment_closure_policy".to_string(),
            source_id: Some("payments".to_string()),
            source_ref: None,
            project_id: Some("payments".to_string()),
            task_id: None,
            title: "Deployment closure policy".to_string(),
            summary: "stale compiled policy projection".to_string(),
            authority_class: "redacted-support-catalog".to_string(),
            validity: "current_reference".to_string(),
            privacy_class: "reference".to_string(),
            freshness: "runtime_or_catalog_bound".to_string(),
            score: Some(1.0),
            raw_policy: "secret_refs_only".to_string(),
            evidence_refs: json!({
                "source": "compiled-deployment-policy",
                "policy": {
                    "path": "/Users/jinchen/.missiond/runtime/missiond/compiled/compiled-deployment-policy.json",
                    "source_hash": "old-hash"
                }
            }),
            metadata: json!({}),
        };
        let mut current = stale.clone();
        current.id = "evi-current".to_string();
        current.evidence_refs = json!({
            "source": "compiled-deployment-policy",
            "policy": {
                "path": "/Users/jinchen/.xjp-mission/releases/current/compiled-runtime/compiled-deployment-policy.json",
                "source_hash": "current-hash"
            }
        });
        let fingerprint = CompiledDeploymentPolicyFingerprint {
            compiled_runtime_dir: Path::new(
                "/Users/jinchen/.xjp-mission/releases/current/compiled-runtime",
            )
            .to_path_buf(),
            source_hash: Some("current-hash".to_string()),
        };

        let (items, filtered_count) = filter_stale_compiled_policy_evidence_items_with_fingerprint(
            vec![stale, current],
            &fingerprint,
        );

        assert_eq!(filtered_count, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "evi-current");
    }

    #[test]
    fn evidence_search_filters_stale_runtime_environment_refs() {
        let active_dir = Path::new("/Users/jinchen/.xjp-mission/releases/current/compiled-runtime");
        let stale = missiond_core::types::EvidenceItemInput {
            id: "evi-runtime-stale".to_string(),
            lane_id: "runtime_truth".to_string(),
            source_type: "runtime_environment".to_string(),
            source_id: None,
            source_ref: None,
            project_id: Some("missiond".to_string()),
            task_id: None,
            title: "Runtime truth".to_string(),
            summary: json!({
                "compiled_runtime_dir": "/Users/jinchen/.xjp-mission/releases/old/compiled-runtime",
                "runtime_dir": "/Users/jinchen/.missiond/runtime/missiond"
            })
            .to_string(),
            authority_class: "runtime-env-and-monitor".to_string(),
            validity: "current_rule".to_string(),
            privacy_class: "operational".to_string(),
            freshness: "hot_runtime".to_string(),
            score: Some(3.0),
            raw_policy: "compact_only".to_string(),
            evidence_refs: json!([]),
            metadata: json!({}),
        };
        let mut current = stale.clone();
        current.id = "evi-runtime-current".to_string();
        current.summary = json!({
            "compiled_runtime_dir": "/Users/jinchen/.xjp-mission/releases/current/compiled-runtime",
            "runtime_dir": "/Users/jinchen/.missiond/runtime/missiond"
        })
        .to_string();

        let mut deploy_event = stale.clone();
        deploy_event.id = "evi-deploy-event".to_string();
        deploy_event.source_type = "deploy_center_event".to_string();
        deploy_event.summary = "Deploy Center smoke failed".to_string();

        let (items, filtered_count) = filter_stale_runtime_environment_evidence_items_with_dir(
            vec![stale, current, deploy_event],
            active_dir,
        );

        assert_eq!(filtered_count, 1);
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.id == "evi-runtime-current"));
        assert!(items.iter().any(|item| item.id == "evi-deploy-event"));
    }

    #[test]
    fn evidence_search_filters_incomplete_deployment_closure_placeholders() {
        let placeholder = missiond_core::types::EvidenceItemInput {
            id: "evi-placeholder".to_string(),
            lane_id: "skill_evidence".to_string(),
            source_type: "skill_operational_fact".to_string(),
            source_id: None,
            source_ref: None,
            project_id: Some("payments".to_string()),
            task_id: None,
            title: "Scoped deployment closure policy".to_string(),
            summary: "service deployment closure support: deploy center slug deploy-center; runtime target runtime-target; manifest refs []; required closure records [ReleaseLease, RuntimeObservation, ReleaseEvidence, ClosureVerdict].".to_string(),
            authority_class: "evidence-only".to_string(),
            validity: "evidence_only".to_string(),
            privacy_class: "internal".to_string(),
            freshness: "version_bound_or_historical".to_string(),
            score: Some(1.0),
            raw_policy: "compact_only".to_string(),
            evidence_refs: json!([]),
            metadata: json!({"projection": "mission_context_gather.compact_evidence"}),
        };
        let valid_payments = missiond_core::types::EvidenceItemInput {
            id: "evi-payments".to_string(),
            lane_id: "skill_evidence".to_string(),
            source_type: "skill_operational_fact".to_string(),
            source_id: None,
            source_ref: None,
            project_id: Some("payments".to_string()),
            task_id: None,
            title: "Scoped deployment closure policy".to_string(),
            summary: "payments deployment closure support: deploy center slug xjp-payments; runtime target gcp-runtime; manifest refs [services/payments/service.manifest.toml]; required closure records [ReleaseLease, RuntimeObservation, ReleaseEvidence, ClosureVerdict].".to_string(),
            authority_class: "evidence-only".to_string(),
            validity: "evidence_only".to_string(),
            privacy_class: "internal".to_string(),
            freshness: "version_bound_or_historical".to_string(),
            score: Some(1.0),
            raw_policy: "compact_only".to_string(),
            evidence_refs: json!([]),
            metadata: json!({"projection": "mission_context_gather.compact_evidence"}),
        };

        let (items, filtered_count) =
            filter_incomplete_deployment_closure_evidence_items(vec![placeholder, valid_payments]);

        assert_eq!(filtered_count, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "evi-payments");
    }

    #[test]
    fn deployment_event_filter_keeps_scoped_deploy_center_events() {
        let payload_json = json!({
            "_envelope": {
                "project_id": "payments",
                "subject": "xjp-payments canary smoke",
                "correlation_id": "run-42",
                "source": "deploy-center",
                "authority": "deploy-center.deploy_events"
            },
            "deploy_event_id": 42,
            "target_service_id": "payments"
        })
        .to_string();
        let row = missiond_core::db::TimelineRow {
            seq: 42,
            trace_id: Some("trace-1".to_string()),
            span_id: None,
            parent_span_id: None,
            event_type: "external_service_event".to_string(),
            summary: Some("Payments canary failed".to_string()),
            payload: json!({
                "service_id": "deploy-center",
                "event_id": "deploy-center:deploy_events:42",
                "event_kind": "smoke_failed",
                "summary": "Payments canary failed at /payments/health/ready",
                "trace_id": "trace-1",
                "payload_json": payload_json,
            })
            .to_string(),
            created_at: "2026-06-02 20:08:16".to_string(),
        };

        let event = deployment_event_item_from_timeline_row(
            &row,
            Some("payments"),
            Some("payments"),
            Some("xjp-payments"),
            "Payments canary",
        )
        .expect("scoped deploy-center event");

        assert_eq!(
            event.get("event_kind").and_then(Value::as_str),
            Some("smoke_failed")
        );
        assert_eq!(
            event.get("project_id").and_then(Value::as_str),
            Some("payments")
        );
        assert_eq!(
            event.get("correlation_id").and_then(Value::as_str),
            Some("run-42")
        );
    }

    #[test]
    fn deployment_event_filter_rejects_unscoped_or_non_deploy_events() {
        let unscoped = missiond_core::db::TimelineRow {
            seq: 7,
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            event_type: "external_service_event".to_string(),
            summary: Some("Router deploy failed".to_string()),
            payload: json!({
                "service_id": "deploy-center",
                "event_id": "deploy-center:deploy_events:7",
                "event_kind": "deploy_failed",
                "summary": "router deploy failed",
                "payload_json": json!({
                    "_envelope": {
                        "project_id": "router",
                        "subject": "xjp-router deploy",
                        "source": "deploy-center"
                    }
                }).to_string(),
            })
            .to_string(),
            created_at: "2026-06-02 20:08:16".to_string(),
        };
        assert!(deployment_event_item_from_timeline_row(
            &unscoped,
            Some("payments"),
            Some("payments"),
            Some("xjp-payments"),
            "Payments canary",
        )
        .is_none());
        match deployment_event_filter_timeline_row(
            &unscoped,
            Some("payments"),
            Some("payments"),
            Some("xjp-payments"),
            "Payments canary",
        ) {
            DeploymentEventFilterResult::Drop { reason, sample } => {
                assert_eq!(reason, "scope_mismatch");
                assert_eq!(
                    sample.get("target_project_id").and_then(Value::as_str),
                    Some("router")
                );
                assert_eq!(
                    sample
                        .get("requested_scope")
                        .and_then(|value| value.get("project_id"))
                        .and_then(Value::as_str),
                    Some("payments")
                );
            }
            DeploymentEventFilterResult::Keep(_) => panic!("unscoped router event was kept"),
        }

        let mut non_deploy = unscoped;
        non_deploy.payload = json!({
            "service_id": "deploy-center",
            "event_id": "deploy-center:deploy_events:8",
            "event_kind": "usage_burst",
            "summary": "not a deployment event",
            "payload_json": "{}",
        })
        .to_string();
        assert!(deployment_event_item_from_timeline_row(
            &non_deploy,
            Some("deploy-center"),
            Some("deploy-center"),
            Some("xjp-deploy-center"),
            "deploy-center",
        )
        .is_none());
        match deployment_event_filter_timeline_row(
            &non_deploy,
            Some("deploy-center"),
            Some("deploy-center"),
            Some("xjp-deploy-center"),
            "deploy-center",
        ) {
            DeploymentEventFilterResult::Drop { reason, sample } => {
                assert_eq!(reason, "irrelevant_event_kind");
                assert_eq!(
                    sample.get("event_kind").and_then(Value::as_str),
                    Some("usage_burst")
                );
            }
            DeploymentEventFilterResult::Keep(_) => panic!("irrelevant deploy event was kept"),
        }
    }

    #[test]
    fn infra_os_disabled_diagnostics_are_not_hard_context_failures() {
        assert!(!diagnostics_have_hard_failures(&[
            optional_infra_os_disabled_diagnostic("infra"),
            optional_infra_os_disabled_diagnostic("credential_refs"),
        ]));
        assert!(diagnostics_have_hard_failures(&[json!({
            "source": "project_registry",
            "error": "not found"
        })]));
        assert!(diagnostics_have_hard_failures(&[json!({
            "source": "infra",
            "status": "unavailable"
        })]));
    }

    #[test]
    fn compact_artifact_payload_omits_raw_sources() {
        let payload = json!({
            "schema": "missiond.context-gather.v1",
            "sources": {"conversation_logs": [{"sessionId": "abc"}]},
            "evidence_lanes": {"lanes": {}}
        });
        let compact = context_pack_artifact_payload(&payload, false);
        assert!(compact.get("sources").is_none());
        assert_eq!(
            compact.get("raw_sources_omitted").and_then(|v| v.as_bool()),
            Some(true)
        );
        let raw = context_pack_artifact_payload(&payload, true);
        assert!(raw.get("sources").is_some());
    }

    #[test]
    fn compact_response_sources_omit_raw_skill_operational_facts() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "skill_context".to_string(),
            json!({
                "skills": [{"name": "deploy-ops", "path": "/skills/deploy-ops/SKILL.md", "matched_by": "query"}],
                "operational_facts": [{
                    "skill": "deploy-ops",
                    "source_path": "/skills/deploy-ops/SKILL.md",
                    "source_line": 174,
                    "key": "xjp-router docker-compose.yml volumes",
                    "value": "full raw operational fact"
                }]
            }),
        );
        let summaries = build_source_summaries(&sources);
        let compact = response_sources(&sources, &summaries, false);
        let skill_summary = compact
            .get("skill_context")
            .expect("skill context summary in compact response");
        assert!(skill_summary.get("operational_facts").is_none());
        assert_eq!(
            skill_summary
                .get("operational_fact_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        let refs = collect_evidence_refs_from_value(&compact);
        assert!(refs.iter().any(|item| {
            item.get("value").and_then(|value| value.as_str())
                == Some("/skills/deploy-ops/SKILL.md")
        }));
        assert!(!refs.iter().any(|item| {
            item.get("value").and_then(|value| value.as_str())
                == Some("xjp-router docker-compose.yml volumes")
        }));

        let raw = response_sources(&sources, &summaries, true);
        assert!(raw
            .get("skill_context")
            .and_then(|value| value.get("operational_facts"))
            .and_then(|value| value.as_array())
            .is_some_and(|items| items.len() == 1));
    }

    #[test]
    fn evidence_lanes_count_only_non_empty_sources() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [], "filteredSemanticHits": 3}),
        );
        sources.insert(
            "runtime_environment".to_string(),
            json!({
                "schema": "missiond.runtime-environment-context.v1",
                "authority": "runtime-env-and-monitor",
                "runtime_dir": "/runtime/missiond"
            }),
        );
        let lanes = build_evidence_lanes(&sources);
        let conversation_lane = lanes
            .get("lanes")
            .and_then(|value| value.get("conversation_audit"))
            .expect("conversation lane");
        assert_eq!(
            conversation_lane
                .get("source_count")
                .and_then(|value| value.as_u64()),
            Some(0)
        );
        assert_eq!(
            conversation_lane
                .get("item_count")
                .and_then(|value| value.as_u64()),
            Some(0)
        );

        let runtime_lane = lanes
            .get("lanes")
            .and_then(|value| value.get("runtime_truth"))
            .expect("runtime lane");
        assert_eq!(
            runtime_lane
                .get("source_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn context_noise_metrics_reports_filtered_conversation_hits() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [], "filteredSemanticHits": 4}),
        );
        let lanes = build_evidence_lanes(&sources);
        let args = args(json!({"query": "audit", "source_profile": "conversation_audit"}));
        let profile = SourceProfile::from_arg(args.source_profile.as_deref());
        let selection = source_selection(&args, profile);
        let allowed_lanes = super::default_allowed_lanes_for_profile(profile);
        let metrics = context_noise_metrics(
            profile,
            selection,
            &sources,
            &lanes,
            &allowed_lanes,
            &json!({
                "ok": true,
                "hit_count": 0,
                "raw_hit_count": 2,
                "freshness_filtered_count": 2,
                "compiled_policy_filtered_count": 1,
                "runtime_environment_filtered_count": 1,
                "incomplete_filtered_count": 1,
                "deduplicated_count": 0,
                "truncated_count": 0,
                "lane_counts": {}
            }),
        );
        assert_eq!(
            metrics
                .get("filtered_semantic_conversation_hits")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
        assert_eq!(
            metrics
                .get("evidence_item_read_model")
                .and_then(|value| value.get("freshness_filtered_count"))
                .and_then(Value::as_u64),
            Some(2)
        );
        assert_eq!(
            metrics
                .get("evidence_item_read_model")
                .and_then(|value| value.get("compiled_policy_filtered_count"))
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            metrics
                .get("evidence_item_read_model")
                .and_then(|value| value.get("runtime_environment_filtered_count"))
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            metrics
                .get("evidence_item_read_model")
                .and_then(|value| value.get("incomplete_filtered_count"))
                .and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn support_catalog_redacts_credential_refs() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "project_registry".to_string(),
            json!({
                "id": "payments",
                "source": "compiled-service-runtime",
                "serviceRuntime": {
                    "id": "payments-api",
                    "project": "payments",
                    "domains": ["pay.example.com"],
                    "health": ["/health"],
                    "public_base_url": "https://pay.example.com"
                }
            }),
        );
        sources.insert(
            "credential_refs".to_string(),
            json!({
                "credentialRefs": [{
                    "namespace": "secret-store",
                    "keyName": "PAYMENTS_DB_URL",
                    "value": "postgres://should-not-appear",
                    "availability": "available"
                }]
            }),
        );

        let catalog = build_support_catalog(&sources);
        assert_eq!(
            catalog.get("service_id").and_then(Value::as_str),
            Some("payments-api")
        );
        let rendered = serde_json::to_string(&catalog).expect("support catalog json");
        assert!(rendered.contains("PAYMENTS_DB_URL"));
        assert!(!rendered.contains("postgres://should-not-appear"));
    }

    #[test]
    fn support_catalog_projects_deployment_closure_evidence() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "project_registry".to_string(),
            json!({
                "id": "payments",
                "source": "compiled-service-runtime",
                "serviceRuntime": {
                    "id": "payments",
                    "project": "payments",
                    "health": ["/payments/health/ready"],
                    "supportCatalog": {
                        "service_id": "payments",
                        "project_id": "payments",
                        "deploy_center_slug": "xjp-payments",
                        "runtime_target": "gcp-runtime",
                        "executor": "gcp-agent",
                        "container": "xjp-payments",
                        "service_manifest_refs": ["services/payments/service.manifest.toml"],
                        "db_migration_namespace": "payments"
                    }
                }
            }),
        );

        let summaries = build_source_summaries(&sources);
        let catalog = build_support_catalog(&sources);
        let closure = catalog
            .get("deployment_closure")
            .expect("deployment closure support");
        let rendered = serde_json::to_string(closure).expect("deployment closure json");
        assert!(rendered.contains("service.manifest.toml"));
        assert!(rendered.contains("ReleaseEvidence"));
        assert!(rendered.contains("ClosureVerdict"));

        let items = build_evidence_items(
            &sources,
            &summaries,
            &catalog,
            SourceProfile::DeployOps,
            Some("payments"),
            None,
        );
        let deployment_item = items
            .iter()
            .find(|item| item.source_type == "deployment_closure_policy")
            .expect("deployment closure evidence item");
        assert_eq!(deployment_item.lane_id, "support_refs");
        assert!(deployment_item.summary.contains("canary"));
        assert!(deployment_item.summary.contains("binary marker"));
        assert!(deployment_item.summary.contains("db adoption"));
    }

    #[test]
    fn empty_support_catalog_does_not_project_support_refs() {
        let sources = serde_json::Map::new();
        let summaries = build_source_summaries(&sources);
        let catalog = build_support_catalog(&sources);

        assert!(!support_catalog_has_content(&catalog));
        assert_eq!(
            catalog.get("schema").and_then(Value::as_str),
            Some("missiond.support-catalog.v1")
        );

        let items = build_evidence_items(
            &sources,
            &summaries,
            &catalog,
            SourceProfile::ConversationAudit,
            None,
            None,
        );
        assert!(!items
            .iter()
            .any(|item| item.source_type == "support_catalog"
                || item.source_type == "deployment_closure_policy"));
    }

    #[test]
    fn evidence_items_use_typed_lanes_and_compact_sources() {
        let mut sources = serde_json::Map::new();
        sources.insert(
            "runtime_environment".to_string(),
            json!({
                "schema": "missiond.runtime-environment-context.v1",
                "authority": "runtime-env-and-monitor",
                "runtime_dir": "/runtime/missiond",
                "compiled_runtime_dir": "/runtime/missiond/compiled"
            }),
        );
        sources.insert(
            "conversation_logs".to_string(),
            json!({"results": [{"conversation_id": "c1", "content": "bounded audit summary"}]}),
        );
        let summaries = build_source_summaries(&sources);
        let catalog = build_support_catalog(&sources);
        let items = build_evidence_items(
            &sources,
            &summaries,
            &catalog,
            SourceProfile::ConversationAudit,
            Some("payments"),
            None,
        );
        assert!(items.iter().any(|item| item.lane_id == "runtime_truth"));
        assert!(items.iter().any(
            |item| item.lane_id == "conversation_audit" && item.raw_policy == "raw_opt_in_only"
        ));
    }
}

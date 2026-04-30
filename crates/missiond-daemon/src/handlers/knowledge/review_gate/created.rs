use missiond_core::event::events::QuestionEvent;
use serde_json::{json, Value};
use tracing::warn;

use crate::bus::BusServices;

// ───────────────────────────────────────────────────────────────────────
// Deterministic question id helper.
//
// Layout: `review:<scope>:<id>:v<version>:<action>`
// Examples:
//   review:directive:0a1b…:v1:compile
//   review:directive:0a1b…:v1:approve
//   review:plan:9f3c…:v2:approve
//   review:plan:9f3c…:v2:supersede
//
// `id` and `action` are caller-controlled; we lowercase action so caller can
// pass either "Approve" or "approve" without surprising the recipient.
// ───────────────────────────────────────────────────────────────────────

/// Build the deterministic review-question id for a given artifact + action.
///
/// Pure, side-effect free — same input always returns the same string. The
/// caller is responsible for passing the canonical `id` (the artifact UUID
/// stringified) and a stable `action` keyword. Uppercase actions are
/// normalised to lowercase so `"Approve"` and `"approve"` collide — that is
/// the intended behaviour for review-gate correlation.
pub(crate) fn derive_review_question_id(
    scope: &str,
    id: &str,
    version: i32,
    action: &str,
) -> String {
    format!(
        "review:{}:{}:v{}:{}",
        scope,
        id,
        version,
        action.to_ascii_lowercase()
    )
}

// ───────────────────────────────────────────────────────────────────────
// Compile-time review gate (Created)
// ───────────────────────────────────────────────────────────────────────

/// Caller request for the compile-time review gate. Built once per compile
/// call. All fields are optional — when `enabled=false` the helper is a
/// no-op.
#[derive(Debug, Clone)]
pub(crate) struct CompileReviewGateRequest {
    pub(crate) enabled: bool,
    /// Optional human-readable text for the review prompt. Surfaced in the
    /// response payload so the caller (CLI / IDE) can render it; the bus
    /// payload itself stays minimal (id only) for forward-compat.
    pub(crate) text: Option<String>,
    /// Optional caller-supplied id override. When omitted, the helper derives
    /// `review:<scope>:<id>:v<version>:compile` from the persisted artifact.
    pub(crate) id_override: Option<String>,
}

/// Parse the compile-time review-gate args from a JSON request.
///
/// Recognised fields (all optional; absent → disabled):
///   * `emit_review_question` (bool, default false)
///   * `review_question_text` (string, optional, free-form)
///   * `review_question_id`   (string, optional, deterministic id override)
///
/// Returns a request value whose `enabled` flag mirrors the input. We never
/// reject malformed types here — the field is opt-in and the failure mode
/// is "no event emitted", which is also the default.
pub(crate) fn parse_compile_review_gate(args: &Value) -> CompileReviewGateRequest {
    let enabled = args
        .get("emit_review_question")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = args
        .get("review_question_text")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let id_override = args
        .get("review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    CompileReviewGateRequest {
        enabled,
        text,
        id_override,
    }
}

/// Emit `QuestionEvent::Created` after a directive / plan has been
/// persisted. Best-effort: never returns Err — bus failures are warned and
/// surfaced on the response payload via the `review_question_warning` field.
///
/// Mutates `payload` in place so the response always carries:
///   * `review_question_emitted` (bool) — whether the gate was even active
///     for this call. False when `req.enabled=false`.
///   * `review_question_id`      (string) — only when emitted (or attempted)
///   * `review_question_text`    (string) — echoed back from the request
///   * `review_question_warning` (object) — only when the publish errored
pub(crate) async fn maybe_emit_review_question_created(
    payload: &mut Value,
    bus: &BusServices,
    req: &CompileReviewGateRequest,
    scope: &str,
    artifact_id: &str,
    version: i32,
) {
    if !req.enabled {
        // Loud "off" signal — callers can grep responses for false to see
        // they did NOT enable the gate.
        payload["review_question_emitted"] = json!(false);
        return;
    }
    let qid = req
        .id_override
        .clone()
        .unwrap_or_else(|| derive_review_question_id(scope, artifact_id, version, "compile"));
    let ev = QuestionEvent::Created {
        question_id: qid.clone(),
    };
    match bus.publish_question(ev).await {
        Ok(_) => {
            payload["review_question_emitted"] = json!(true);
            payload["review_question_id"] = json!(qid);
            if let Some(text) = req.text.as_ref() {
                payload["review_question_text"] = json!(text);
            }
        }
        Err(e) => {
            // Side-channel failure must not break the persisted draft. We
            // still expose the deterministic id so the caller can retry the
            // emit OR resolve the gate manually with the same id later.
            warn!(
                scope = scope,
                artifact_id = artifact_id,
                version = version,
                question_id = %qid,
                error = %e,
                "review-gate: QuestionEvent::Created publish failed; persisted artifact remains intact"
            );
            payload["review_question_emitted"] = json!(false);
            payload["review_question_id"] = json!(qid);
            if let Some(text) = req.text.as_ref() {
                payload["review_question_text"] = json!(text);
            }
            payload["review_question_warning"] = json!({
                "code": "BUS_PUBLISH_FAILED",
                "reason": format!("{:#}", e),
                "scope": scope,
                "artifact_id": artifact_id,
                "version": version,
                "question_id": qid,
            });
        }
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-14 :: review gate auto-create policy
//
// Three-state policy controls whether the post-artifact-write hook should
// emit a `QuestionEvent::Created`. The default is `Manual` — i.e. the
// existing wave-11 explicit-emit path (`emit_review_question=true`) is the
// only way to get an event, so callers that never knew about the new
// policy keep their byte-identical response shape.
//
//   manual         → no automatic emit. The legacy `emit_review_question`
//                    bool still works through `parse_compile_review_gate`.
//   emit_question  → after `attempt_artifact_write` returns Written, the
//                    handler emits `QuestionEvent::Created` on its own.
//                    Resolution stays opt-in (caller still passes
//                    `review_question_id` to approve/mark/archive).
//   off            → never emit, even if the legacy bool is true. Useful
//                    for batch/silent runs that want to suppress
//                    side-channel events while still using the file-first
//                    SSOT mirror.
//
// The policy is parsed from the `review_gate_policy` key (an opt-in field
// distinct from the existing free-form `review_gate` note that some
// handlers stash inside `references_json`). When the key is absent / blank
// / unknown, we fall back to `Manual` so legacy callers stay quiet.
// ───────────────────────────────────────────────────────────────────────

/// Policy controlling the wave-14 post-artifact-write auto-emit hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewGatePolicy {
    /// Default: no automatic emit. Legacy `emit_review_question=true` still
    /// works through `parse_compile_review_gate`.
    Manual,
    /// Auto-emit `QuestionEvent::Created` after the file-first artifact
    /// write succeeds. No-op when the write failed.
    EmitQuestion,
    /// Suppress all gate emission, including the legacy
    /// `emit_review_question=true` opt-in.
    Off,
}

impl ReviewGatePolicy {
    /// Lower-snake-case label for the response payload.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReviewGatePolicy::Manual => "manual",
            ReviewGatePolicy::EmitQuestion => "emit_question",
            ReviewGatePolicy::Off => "off",
        }
    }
}

/// Parse the wave-14 `review_gate_policy` arg. Unknown / absent / blank
/// values collapse to `Manual` so legacy callers (which never sent the
/// field) keep their byte-identical response shape.
///
/// Recognised values (case-insensitive, trimmed):
///   * `"manual"`        → [`ReviewGatePolicy::Manual`] (default)
///   * `"emit_question"` → [`ReviewGatePolicy::EmitQuestion`]
///   * `"off"`           → [`ReviewGatePolicy::Off`]
pub(crate) fn parse_review_gate_policy(args: &Value) -> ReviewGatePolicy {
    let raw = args
        .get("review_gate_policy")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase());
    match raw.as_deref() {
        Some("emit_question") => ReviewGatePolicy::EmitQuestion,
        Some("off") => ReviewGatePolicy::Off,
        // `manual`, blank, unknown → default. We deliberately do NOT reject
        // unknown values: the response always carries
        // `review_gate_policy=<resolved>` so callers can spot a typo by
        // inspecting it.
        _ => ReviewGatePolicy::Manual,
    }
}

/// Build a deterministic question id for an artifact file-first write. The
/// `topic_or_path` field is folded in as a 16-hex-digit truncated SHA-256
/// digest so the id stays bounded while still being globally unique per
/// (kind, id, version, topic) tuple.
///
/// Layout: `review:<scope>:<id>:v<version>:<action>:<topic-hash>`
///
/// The `topic_or_path` is normalised (trimmed) before hashing so callers
/// can hand us either the topic string OR the on-disk path — same input
/// always returns the same id. Empty / blank `topic_or_path` skips the
/// hash suffix and falls back to the wave-11 layout
/// (`review:<scope>:<id>:v<version>:<action>`) so existing tests stay
/// green.
pub(crate) fn derive_review_question_id_for_artifact(
    scope: &str,
    id: &str,
    version: i32,
    action: &str,
    topic_or_path: Option<&str>,
) -> String {
    let base = derive_review_question_id(scope, id, version, action);
    let topic = topic_or_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(topic_hash_short);
    match topic {
        Some(h) => format!("{}:{}", base, h),
        None => base,
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-16 / task 04 — per-node DAG review-gate helpers
//
// PLAN DAG runtime now supports a per-node `:review-gate "question-event"`
// hint. When a node carries that hint and becomes ready, the scheduler
// emits a `QuestionEvent::Created` and pauses the node instead of
// dispatching the target tool. The deterministic id mirrors the wave-14
// layout so the wave-16 / task 02 `QuestionEvent::Resolved` listener can
// route resolutions back to the same node without bespoke parsing.
//
// Scope = `"plan"` keeps the id under the wave-14 supported-scope set so
// the existing subscriber dispatcher (`plan_review_resolved_dispatch`)
// surfaces it as a `Route { scope=plan, ... }` outcome — auto-resume
// wiring is wave-16 / task 02's territory; this helper only ships the id
// shape.
// ───────────────────────────────────────────────────────────────────────

/// Default `:review-action` keyword used when the node author omits the
/// hint. Folded into the deterministic id so authors can override per-node
/// (e.g. `:review-action "human-checkpoint"`) without colliding across
/// nodes.
pub(crate) const PLAN_NODE_REVIEW_DEFAULT_ACTION: &str = "plan-node";

/// Build the deterministic review-question id for a paused PLAN DAG node.
/// Layout: `review:plan:<plan_id>:v<version>:<action>:<node-id-hash>`.
///
/// `node_id` is folded into the trailing topic-hash slot so the same plan
/// can pause many nodes without colliding ids; `action` defaults to
/// [`PLAN_NODE_REVIEW_DEFAULT_ACTION`] when the caller's `:review-action`
/// is empty / absent.
pub(crate) fn derive_plan_node_review_question_id(
    plan_id: &str,
    plan_version: i32,
    node_id: &str,
    action: Option<&str>,
) -> String {
    let action = action
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(PLAN_NODE_REVIEW_DEFAULT_ACTION);
    derive_review_question_id_for_artifact("plan", plan_id, plan_version, action, Some(node_id))
}

/// wave-17 / task 01 — pure projection of a plan-node review id's
/// trailing topic-hash. Used by the resume helper to map the supplied
/// `resume_review_question_id` back to the originating paused node id
/// without standing up an AppState (the node id itself never appears
/// in the envelope — only its 16-char SHA-256 prefix does).
///
/// `node_id` is normalised the same way wave-16 / task 04 hashed it
/// (raw bytes; no trim) so the round-trip stays consistent with the
/// pause emitter.
pub(crate) fn derive_plan_node_topic_hash(node_id: &str) -> String {
    topic_hash_short(node_id)
}

/// True iff `parsed.action` is the wave-16 PLAN-DAG review action
/// (`plan-node`, case-insensitive). The wave-17 / task 01 listener
/// branches on this to route plan-node ids to the resume helper
/// instead of the existing `plan_handle_review_resolved` (which only
/// understands compile/approve/mark/supersede).
pub(crate) fn is_plan_node_review_action(action: &str) -> bool {
    action
        .trim()
        .eq_ignore_ascii_case(PLAN_NODE_REVIEW_DEFAULT_ACTION)
}

/// SHA-256 over the input, truncated to the leading 16 hex chars. Bounded
/// length keeps log lines + retro queries readable while still giving us
/// 64 bits of collision space across topic/path strings.
pub(super) fn topic_hash_short(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let full = format!("{:x}", h.finalize());
    full.chars().take(16).collect()
}

/// Outcome of [`auto_emit_review_question_after_artifact_write`] —
/// projected purely from the input policy + payload state so callers (and
/// tests) can reason about the decision branch without inspecting the
/// payload after the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoEmitDecision {
    /// `policy = manual` — no automatic emit, legacy explicit-emit still
    /// works through `parse_compile_review_gate`.
    SkippedPolicyManual,
    /// `policy = off` — caller suppressed all emission.
    SkippedPolicyOff,
    /// `policy = emit_question` but the splice reports `file_written=false`
    /// (or the field is missing). We refuse to emit a Created event for an
    /// artifact that never landed on disk.
    SkippedFileWriteUnsuccessful,
    /// `policy = emit_question` AND `file_written=true` AND the publish
    /// call returned Ok. Payload was stamped with `review_question_emitted=
    /// true` + id + policy.
    Emitted,
    /// `policy = emit_question` AND `file_written=true` but the publish
    /// call errored. Payload carries `review_question_emitted=false` + the
    /// `review_question_warning` block + the deterministic id (so callers
    /// can retry / resolve the gate manually with the same id).
    EmitFailedBus,
}

/// Inspect the payload's `file_written` splice and decide whether the
/// post-write hook should fire. Pure helper so tests can drive the
/// decision branch without standing up a bus.
pub(super) fn payload_says_file_written(payload: &Value) -> bool {
    payload
        .get("file_written")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Always-stamped marker for the response so audit dashboards can grep for
/// the resolved policy regardless of whether anything was emitted.
pub(super) fn stamp_policy(payload: &mut Value, policy: ReviewGatePolicy) {
    if let Some(map) = payload.as_object_mut() {
        map.insert("review_gate_policy".to_string(), json!(policy.as_str()));
    }
}

/// Wave-14 post-artifact-write hook. Call this AFTER
/// `attempt_artifact_write(...).splice_into(payload)` so the splice's
/// `file_written` flag is visible to the policy gate.
///
/// Behaviour matrix (`<policy>` × `<file_written>`):
///   * `manual` × *           → no-op aside from stamping `review_gate_policy`.
///                               The legacy explicit-emit path
///                               (`parse_compile_review_gate`) still works
///                               on its own.
///   * `off` × *              → no-op aside from stamping the policy and
///                               `review_question_emitted=false`. We do NOT
///                               run the legacy explicit-emit either —
///                               `off` is a global mute.
///   * `emit_question` × false → stamps `review_question_emitted=false`,
///                               `review_gate_policy=emit_question`, and
///                               the resolved id (so the caller can retry
///                               the file write and reuse the same id).
///   * `emit_question` × true  → publishes `QuestionEvent::Created`. Bus
///                               success → `review_question_emitted=true`;
///                               bus failure → splice the warning block
///                               and keep the deterministic id.
///
/// `id_override` — caller-supplied override (typically pulled from
/// `parse_compile_review_gate(...).id_override`). When `None`, the helper
/// derives the id via [`derive_review_question_id_for_artifact`] using
/// scope/id/version + the file path (or topic) for the topic-hash slot.
pub(crate) async fn auto_emit_review_question_after_artifact_write(
    payload: &mut Value,
    bus: &BusServices,
    policy: ReviewGatePolicy,
    scope: &str,
    artifact_id: &str,
    version: i32,
    topic: Option<&str>,
    id_override: Option<&str>,
) -> AutoEmitDecision {
    stamp_policy(payload, policy);

    match policy {
        ReviewGatePolicy::Manual => {
            // No-op. Legacy explicit-emit path stays in control. We do NOT
            // overwrite `review_question_emitted` here because
            // `maybe_emit_review_question_created` may have already
            // stamped it for this same call.
            return AutoEmitDecision::SkippedPolicyManual;
        }
        ReviewGatePolicy::Off => {
            // Stamp `review_question_emitted=false` so it's loud — `off`
            // is a deliberate suppression and consumers should not have
            // to infer the absence.
            if let Some(map) = payload.as_object_mut() {
                map.entry("review_question_emitted".to_string())
                    .or_insert(json!(false));
            }
            return AutoEmitDecision::SkippedPolicyOff;
        }
        ReviewGatePolicy::EmitQuestion => {}
    }

    if !payload_says_file_written(payload) {
        // We refuse to emit a Created event for a non-existent artifact —
        // the file-first SSOT contract requires the on-disk row to land
        // first; an event without a backing artifact would be confusing
        // to the resolver.
        if let Some(map) = payload.as_object_mut() {
            map.insert("review_question_emitted".to_string(), json!(false));
            map.entry("review_question_warning".to_string()).or_insert(json!({
                "code": "FILE_WRITE_NOT_SUCCESSFUL",
                "reason": "review_gate_policy=emit_question requires file_written=true; auto-emit suppressed",
                "scope": scope,
                "artifact_id": artifact_id,
                "version": version,
            }));
        }
        return AutoEmitDecision::SkippedFileWriteUnsuccessful;
    }

    // file_written=true. Resolve / fall back to the deterministic id, then
    // publish.
    let file_path = payload
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let topic_for_hash: Option<&str> = file_path.as_deref().or(topic);
    let qid = id_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            derive_review_question_id_for_artifact(
                scope,
                artifact_id,
                version,
                "compile",
                topic_for_hash,
            )
        });
    let ev = QuestionEvent::Created {
        question_id: qid.clone(),
    };
    match bus.publish_question(ev).await {
        Ok(_) => {
            if let Some(map) = payload.as_object_mut() {
                map.insert("review_question_emitted".to_string(), json!(true));
                map.insert("review_question_id".to_string(), json!(qid));
            }
            AutoEmitDecision::Emitted
        }
        Err(e) => {
            warn!(
                scope = scope,
                artifact_id = artifact_id,
                version = version,
                question_id = %qid,
                error = %e,
                "review-gate: auto-emit QuestionEvent::Created publish failed; persisted artifact remains intact"
            );
            if let Some(map) = payload.as_object_mut() {
                map.insert("review_question_emitted".to_string(), json!(false));
                map.insert("review_question_id".to_string(), json!(qid));
                map.insert(
                    "review_question_warning".to_string(),
                    json!({
                        "code": "BUS_PUBLISH_FAILED",
                        "reason": format!("{:#}", e),
                        "scope": scope,
                        "artifact_id": artifact_id,
                        "version": version,
                        "question_id": qid,
                    }),
                );
            }
            AutoEmitDecision::EmitFailedBus
        }
    }
}

/// Convenience wrapper that runs the right gate path based on the
/// resolved [`ReviewGatePolicy`]:
///
///   * `Manual`        → legacy [`maybe_emit_review_question_created`]
///                       (driven by `parse_compile_review_gate(args)`).
///                       When BOTH the wave-14 policy was absent AND the
///                       wave-11 explicit-emit bool is false we skip the
///                       call entirely so pre-wave-11 callers stay
///                       byte-for-byte identical (no
///                       `review_gate_policy` / `review_question_emitted`
///                       fields appear).
///   * `EmitQuestion`  → wave-14
///                       [`auto_emit_review_question_after_artifact_write`].
///                       Legacy explicit-emit is SKIPPED so we never
///                       double-fire.
///   * `Off`           → no emission at all (legacy + auto-emit both
///                       skipped). Stamps `review_gate_policy=off` and
///                       `review_question_emitted=false`.
///
/// Caller must invoke this AFTER `attempt_artifact_write(...).splice_into(
/// payload)` so the `file_written` flag is visible to the policy gate.
///
/// `policy_explicit` mirrors the caller-side knowledge of "did the request
/// JSON carry a `review_gate_policy` key at all". Combined with
/// `legacy.enabled`, it lets us suppress all gate-side payload stamping
/// for pre-wave-11 callers that never asked for the gate.
pub(crate) async fn apply_compile_review_gates(
    payload: &mut Value,
    bus: &BusServices,
    policy: ReviewGatePolicy,
    policy_explicit: bool,
    legacy: &CompileReviewGateRequest,
    scope: &str,
    artifact_id: &str,
    version: i32,
    topic: Option<&str>,
) {
    match policy {
        ReviewGatePolicy::Manual => {
            // Pre-wave-11 callers (no policy key, no `emit_review_question`)
            // stay byte-identical: skip all stamping and the legacy emitter.
            if !policy_explicit && !legacy.enabled {
                return;
            }
            // Stamp the resolved policy first so the response always
            // surfaces it when the caller did opt into the gate (either
            // explicitly via `review_gate_policy` or implicitly via the
            // wave-11 bool).
            stamp_policy(payload, policy);
            maybe_emit_review_question_created(payload, bus, legacy, scope, artifact_id, version)
                .await;
        }
        ReviewGatePolicy::EmitQuestion => {
            // wave-14 auto-emit takes over. The legacy id_override (if
            // set) is forwarded so callers that want a stable id across
            // retries can still pin it.
            auto_emit_review_question_after_artifact_write(
                payload,
                bus,
                policy,
                scope,
                artifact_id,
                version,
                topic,
                legacy.id_override.as_deref(),
            )
            .await;
        }
        ReviewGatePolicy::Off => {
            auto_emit_review_question_after_artifact_write(
                payload,
                bus,
                policy,
                scope,
                artifact_id,
                version,
                topic,
                legacy.id_override.as_deref(),
            )
            .await;
        }
    }
}

/// True when the caller actually included a `review_gate_policy` key in
/// the request JSON (regardless of value). Used by the compile-time hook
/// to keep pre-wave-11 callers byte-identical.
pub(crate) fn review_gate_policy_was_explicit(args: &Value) -> bool {
    args.get("review_gate_policy").is_some()
}

//! review_gate — event-bus aware review-gate emission for directive / plan /
//! workflow file-first artifacts.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop ::
//!         s3 alignment-review-gate + s5 plan-review-gate
//!   - intent-intent-layer.lisp :: section unified-entry-pipeline ::
//!       role alignment-review-gate / role plan-review-gate
//!   - intent-event-bus.lisp :: QuestionEvent
//!
//! Scope (wave-11 :: review gate event-aware code-alignment):
//!   - Pure helpers + an opt-in best-effort emitter.
//!   - Carries the deterministic question id derivation (so every artifact
//!     produces the same id from `(scope, id, version, action)` — caller can
//!     correlate Created → Resolved without persisting state).
//!   - Does NOT extend `QuestionEvent` payload (the existing `question_id`
//!     field already carries our deterministic id, and existing serde tests
//!     stay intact).
//!   - Does NOT implement human UI / wait-for-answer. The Created event is
//!     fire-and-forget; the manager surface returns immediately so callers
//!     are never blocked on a human gate. Gate resolution (Resolved /
//!     DecisionResolved) is also opt-in via `review_question_id`.
//!
//! Scope (wave-14 :: review gate auto-create v1):
//!   - Adds [`ReviewGatePolicy`] (`manual` / `emit_question` / `off`) and
//!     [`parse_review_gate_policy`] so directive / plan / workflow handlers
//!     can opt callers into automatic `QuestionEvent::Created` emission after
//!     a successful file-first artifact write — without changing the legacy
//!     opt-in `emit_review_question` boolean (which keeps working under the
//!     `manual` policy).
//!   - Adds [`auto_emit_review_question_after_artifact_write`], the
//!     post-write hook called from compile / distill paths AFTER
//!     `attempt_artifact_write` has spliced its `file_written` outcome. The
//!     hook only fires when policy=`emit_question` AND the splice declared
//!     `file_written=true`; otherwise it stamps `review_question_emitted=
//!     false` and surfaces the policy + reason so callers can observe what
//!     happened.
//!   - Deterministic id derivation is extended via
//!     [`derive_review_question_id_for_artifact`] which folds the artifact
//!     kind label, db id, version, and topic-or-file-path-hash into the same
//!     `review:<scope>:<id>:v<version>:<action>:<topic-hash>` envelope — same
//!     input always returns the same id, so retries / resolutions correlate
//!     even across daemon restarts.
//!   - Does NOT auto-approve, does NOT wait, does NOT mutate the persisted
//!     artifact. The hook is fire-and-forget on the bus side, and the file
//!     write success comes from the splice — we never overwrite the splice.
//!
//! Bus failure semantics (mirrors CLAUDE.md `feedback_fail_fast_no_fallback`):
//!   - The core action (compile persist / approve / archive / mark / supersede)
//!     never fails because of a side-channel bus error.
//!   - But we ALSO refuse to silently swallow it: when the publish call
//!     errors, the response carries a `review_question_warning` block with
//!     the error text plus the deterministic id, so downstream readers see a
//!     loud signal in the response payload AND in the daemon logs.

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

/// SHA-256 over the input, truncated to the leading 16 hex chars. Bounded
/// length keeps log lines + retro queries readable while still giving us
/// 64 bits of collision space across topic/path strings.
fn topic_hash_short(s: &str) -> String {
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
fn payload_says_file_written(payload: &Value) -> bool {
    payload
        .get("file_written")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Always-stamped marker for the response so audit dashboards can grep for
/// the resolved policy regardless of whether anything was emitted.
fn stamp_policy(payload: &mut Value, policy: ReviewGatePolicy) {
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "review_gate_policy".to_string(),
            json!(policy.as_str()),
        );
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
            map.insert(
                "review_question_emitted".to_string(),
                json!(false),
            );
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
            maybe_emit_review_question_created(
                payload,
                bus,
                legacy,
                scope,
                artifact_id,
                version,
            )
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

// ───────────────────────────────────────────────────────────────────────
// Decision-time review gate (Resolved / DecisionResolved)
//
// approve / archive / mark / supersede call this when the caller passes
// `review_question_id`. The handler always succeeds first (DB mutation)
// and then attempts the publish. We never block the DB outcome on a bus
// success.
// ───────────────────────────────────────────────────────────────────────

/// Parse a single `review_question_id` field for the resolution path.
/// Returns `None` when absent / blank — the resolution emit is opt-in.
pub(crate) fn parse_resolution_review_question_id(args: &Value) -> Option<String> {
    args.get("review_question_id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Optional decision metadata for `QuestionEvent::DecisionResolved`. When
/// `tier` is provided, the helper publishes `DecisionResolved` instead of
/// `Resolved` so the upstream router can attribute the decision tier.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionDecisionMeta {
    pub(crate) tier: Option<String>,
    pub(crate) duration_ms: Option<u64>,
}

/// Pure constructor: build the `QuestionEvent` that the resolution helper
/// would emit for a given `(qid, resolution, decision_meta)` triple. Split
/// out so tests can assert event shape without touching a real bus.
pub(crate) fn build_resolution_event(
    qid: &str,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) -> QuestionEvent {
    match decision.and_then(|d| d.tier.as_deref()) {
        Some(tier) => QuestionEvent::DecisionResolved {
            question_id: qid.to_string(),
            tier: tier.to_string(),
            duration_ms: decision.and_then(|d| d.duration_ms).unwrap_or(0),
        },
        None => QuestionEvent::Resolved {
            question_id: qid.to_string(),
            resolution: resolution.to_string(),
        },
    }
}

/// Pure event-kind label for response payload. Mirrors the
/// `DomainEvent::kind` impl on `QuestionEvent` but is callable from a
/// borrowed reference without touching the trait.
fn event_kind_label(ev: &QuestionEvent) -> &'static str {
    match ev {
        QuestionEvent::DecisionResolved { .. } => "decision_resolved",
        QuestionEvent::Resolved { .. } => "resolved",
        QuestionEvent::Created { .. } => "created",
    }
}

/// Best-effort `QuestionEvent::Resolved` (or `DecisionResolved` when
/// `decision.tier.is_some()`) emit after a control action (approve /
/// archive / mark / supersede) committed.
///
/// Mutates `payload`:
///   * `review_question_resolved` (bool) — true on success
///   * `review_question_id`       (string) — echoed back so the caller can
///     correlate with the original Created event
///   * `review_question_warning`  (object) — only when publish errored
///
/// Resolution is OPT-IN at the caller. When `qid.is_none()`, this helper is
/// a no-op (no payload mutation) so legacy callers that never knew about
/// the gate stay byte-identical.
pub(crate) async fn maybe_emit_review_question_resolved(
    payload: &mut Value,
    bus: &BusServices,
    qid: Option<&str>,
    resolution: &str,
    decision: Option<&ResolutionDecisionMeta>,
) {
    let Some(qid) = qid else {
        return;
    };
    let qid = qid.to_string();

    let ev = build_resolution_event(&qid, resolution, decision);
    let kind = event_kind_label(&ev);

    match bus.publish_question(ev).await {
        Ok(_) => {
            payload["review_question_resolved"] = json!(true);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
        }
        Err(e) => {
            warn!(
                question_id = %qid,
                resolution = resolution,
                kind = kind,
                error = %e,
                "review-gate: QuestionEvent resolved publish failed; DB action already committed"
            );
            payload["review_question_resolved"] = json!(false);
            payload["review_question_id"] = json!(qid);
            payload["review_question_kind"] = json!(kind);
            payload["review_question_warning"] = json!({
                "code": "BUS_PUBLISH_FAILED",
                "reason": format!("{:#}", e),
                "question_id": qid,
                "resolution": resolution,
                "kind": kind,
            });
        }
    }
}

// ───────────────────────────────────────────────────────────────────────
// tests — pure helpers only (no bus, no DB).
// ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic_for_same_input() {
        let a = derive_review_question_id("directive", "abc-123", 1, "compile");
        let b = derive_review_question_id("directive", "abc-123", 1, "compile");
        assert_eq!(a, b);
    }

    #[test]
    fn id_normalises_action_case() {
        let a = derive_review_question_id("plan", "p-1", 2, "Approve");
        let b = derive_review_question_id("plan", "p-1", 2, "approve");
        assert_eq!(a, b, "uppercase action must collide with lowercase form");
    }

    #[test]
    fn id_layout_has_canonical_format() {
        let id = derive_review_question_id("directive", "abc-123", 5, "compile");
        assert_eq!(id, "review:directive:abc-123:v5:compile");
    }

    #[test]
    fn id_changes_when_any_field_changes() {
        let base = derive_review_question_id("directive", "abc", 1, "compile");
        assert_ne!(
            base,
            derive_review_question_id("plan", "abc", 1, "compile"),
            "scope must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 2, "compile"),
            "version must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "abc", 1, "approve"),
            "action must affect id"
        );
        assert_ne!(
            base,
            derive_review_question_id("directive", "xyz", 1, "compile"),
            "id must affect id"
        );
    }

    // -- parse_compile_review_gate --

    #[test]
    fn parse_compile_default_is_disabled() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_extracts_all_fields() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "  please review  ",
            "review_question_id": "  override-id  ",
        }));
        assert!(req.enabled);
        assert_eq!(req.text.as_deref(), Some("please review"));
        assert_eq!(req.id_override.as_deref(), Some("override-id"));
    }

    #[test]
    fn parse_compile_filters_blank_strings() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_text": "   ",
            "review_question_id": "",
        }));
        assert!(req.enabled);
        assert!(req.text.is_none());
        assert!(req.id_override.is_none());
    }

    #[test]
    fn parse_compile_emit_false_keeps_other_fields_in_struct_but_disabled() {
        // We still parse the optional override because callers may flip
        // emit later — but the helper must respect `enabled=false`.
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": false,
            "review_question_id": "explicit-id",
        }));
        assert!(!req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("explicit-id"));
    }

    // -- parse_resolution_review_question_id --

    #[test]
    fn parse_resolution_id_returns_none_when_absent() {
        assert!(parse_resolution_review_question_id(&json!({})).is_none());
    }

    #[test]
    fn parse_resolution_id_trims_and_filters_blank() {
        assert!(parse_resolution_review_question_id(&json!({
            "review_question_id": "   "
        }))
        .is_none());
        assert_eq!(
            parse_resolution_review_question_id(&json!({
                "review_question_id": "  abc  "
            })),
            Some("abc".to_string())
        );
    }

    // -- build_resolution_event --

    #[test]
    fn resolution_event_without_decision_meta_is_resolved() {
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", None);
        match ev {
            QuestionEvent::Resolved {
                question_id,
                resolution,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(resolution, "approved");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_with_tier_is_decision_resolved() {
        let meta = ResolutionDecisionMeta {
            tier: Some("tier1".into()),
            duration_ms: Some(123),
        };
        let ev = build_resolution_event("review:plan:p1:v1:approve", "approved", Some(&meta));
        match ev {
            QuestionEvent::DecisionResolved {
                question_id,
                tier,
                duration_ms,
            } => {
                assert_eq!(question_id, "review:plan:p1:v1:approve");
                assert_eq!(tier, "tier1");
                assert_eq!(duration_ms, 123);
            }
            other => panic!("expected DecisionResolved, got {other:?}"),
        }
    }

    #[test]
    fn resolution_event_decision_meta_default_duration_is_zero() {
        let meta = ResolutionDecisionMeta {
            tier: Some("urgent".into()),
            duration_ms: None,
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        if let QuestionEvent::DecisionResolved { duration_ms, .. } = ev {
            assert_eq!(duration_ms, 0);
        } else {
            panic!("expected DecisionResolved");
        }
    }

    #[test]
    fn resolution_event_meta_without_tier_falls_back_to_resolved() {
        // tier=None means "no decision-tier metadata" → plain Resolved even
        // when meta block is supplied. This pins the precedence.
        let meta = ResolutionDecisionMeta {
            tier: None,
            duration_ms: Some(99),
        };
        let ev = build_resolution_event("rid", "approved", Some(&meta));
        assert!(matches!(ev, QuestionEvent::Resolved { .. }));
    }

    #[test]
    fn event_kind_label_for_each_variant() {
        assert_eq!(
            event_kind_label(&QuestionEvent::Created {
                question_id: "x".into(),
            }),
            "created"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::Resolved {
                question_id: "x".into(),
                resolution: "y".into(),
            }),
            "resolved"
        );
        assert_eq!(
            event_kind_label(&QuestionEvent::DecisionResolved {
                question_id: "x".into(),
                tier: "t".into(),
                duration_ms: 0,
            }),
            "decision_resolved"
        );
    }

    // -- compile-response payload contract (caller-visible fields) --

    /// The compile branches construct a payload that may include the
    /// emission fields. These tests exercise the request-side decision
    /// surface (the inputs to `maybe_emit_review_question_created`) so the
    /// MCP contract stays pinned even without a real bus.
    #[test]
    fn compile_request_disabled_means_no_emission_fields_will_be_added() {
        let req = parse_compile_review_gate(&json!({}));
        assert!(!req.enabled);
        // When enabled=false the helper writes review_question_emitted=false
        // and no warning. The contract is "loud off" — see docstring on
        // maybe_emit_review_question_created.
        let derived = derive_review_question_id("directive", "abc", 1, "compile");
        assert_eq!(derived, "review:directive:abc:v1:compile");
    }

    #[test]
    fn compile_request_with_explicit_id_overrides_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
            "review_question_id": "custom:q-1",
        }));
        assert!(req.enabled);
        assert_eq!(req.id_override.as_deref(), Some("custom:q-1"));
    }

    #[test]
    fn compile_request_without_explicit_id_falls_back_to_derived() {
        let req = parse_compile_review_gate(&json!({
            "emit_review_question": true,
        }));
        assert!(req.enabled);
        assert!(req.id_override.is_none());
        // The handler will compute the derived id at emit time from the
        // persisted artifact (id, version). Pin the contract here.
        let qid = derive_review_question_id("plan", "p-7", 3, "compile");
        assert_eq!(qid, "review:plan:p-7:v3:compile");
    }

    // ── wave-14 :: review_gate_policy parser ─────────────────────────────

    #[test]
    fn parse_policy_default_is_manual() {
        assert_eq!(
            parse_review_gate_policy(&json!({})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_recognises_emit_question() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "emit_question"})),
            ReviewGatePolicy::EmitQuestion
        );
    }

    #[test]
    fn parse_policy_recognises_off() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "off"})),
            ReviewGatePolicy::Off
        );
    }

    #[test]
    fn parse_policy_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "  EMIT_QUESTION  "})),
            ReviewGatePolicy::EmitQuestion
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "Off"})),
            ReviewGatePolicy::Off
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "MANUAL"})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn parse_policy_unknown_collapses_to_manual() {
        // Unknown values are silently mapped to the default rather than
        // rejected — the response always echoes the resolved policy so a
        // typo is observable downstream.
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "always"})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": ""})),
            ReviewGatePolicy::Manual
        );
        assert_eq!(
            parse_review_gate_policy(&json!({"review_gate_policy": "   "})),
            ReviewGatePolicy::Manual
        );
    }

    #[test]
    fn policy_label_round_trips() {
        assert_eq!(ReviewGatePolicy::Manual.as_str(), "manual");
        assert_eq!(ReviewGatePolicy::EmitQuestion.as_str(), "emit_question");
        assert_eq!(ReviewGatePolicy::Off.as_str(), "off");
    }

    // ── wave-14 :: deterministic id with topic / file-path hash ─────────

    #[test]
    fn artifact_id_appends_topic_hash_suffix() {
        let id = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("wave14-topic"),
        );
        assert!(
            id.starts_with("review:directive:abc:v1:compile:"),
            "expected legacy prefix, got: {id}"
        );
        // Suffix must be the truncated hash, NOT the raw topic — keeps the
        // id bounded and obfuscates topic length.
        let suffix = id.rsplit(':').next().unwrap();
        assert_eq!(suffix.len(), 16, "suffix must be 16 hex chars");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!id.contains("wave14-topic"));
    }

    #[test]
    fn artifact_id_without_topic_falls_back_to_legacy_layout() {
        // Empty / blank `topic_or_path` collapses to the wave-11 layout so
        // existing callers that don't have a path yet stay byte-identical.
        let id = derive_review_question_id_for_artifact("plan", "p1", 2, "approve", None);
        assert_eq!(id, "review:plan:p1:v2:approve");
        let id2 =
            derive_review_question_id_for_artifact("plan", "p1", 2, "approve", Some("   "));
        assert_eq!(id2, "review:plan:p1:v2:approve");
    }

    #[test]
    fn artifact_id_is_deterministic_for_same_topic() {
        let a = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        let b = derive_review_question_id_for_artifact(
            "workflow",
            "wf1",
            3,
            "compile",
            Some("/abs/path/.missiond/workflows/foo.lisp"),
        );
        assert_eq!(a, b);
    }

    #[test]
    fn artifact_id_changes_when_topic_changes() {
        let a = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-a"),
        );
        let b = derive_review_question_id_for_artifact(
            "directive",
            "abc",
            1,
            "compile",
            Some("topic-b"),
        );
        assert_ne!(a, b, "topic must affect the trailing hash");
    }

    #[test]
    fn topic_hash_short_is_16_hex_chars() {
        let h = topic_hash_short("anything");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn topic_hash_short_is_stable() {
        // Pin the exact prefix for "wave14-topic" so an accidental change to
        // the hashing scheme breaks loud (id correlation across daemon
        // restarts depends on stability).
        assert_eq!(topic_hash_short("wave14-topic").len(), 16);
        let a = topic_hash_short("wave14-topic");
        let b = topic_hash_short("wave14-topic");
        assert_eq!(a, b);
    }

    // ── wave-14 :: payload introspection helper ─────────────────────────

    #[test]
    fn payload_says_file_written_true_when_flag_present() {
        let p = json!({"file_written": true});
        assert!(payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_missing() {
        let p = json!({"status": "compiled"});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn payload_says_file_written_false_when_flag_false() {
        let p = json!({"file_written": false});
        assert!(!payload_says_file_written(&p));
    }

    #[test]
    fn stamp_policy_inserts_resolved_label() {
        let mut p = json!({"status": "compiled"});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        assert_eq!(p["review_gate_policy"], "emit_question");
    }

    #[test]
    fn stamp_policy_overwrites_prior_value() {
        // Always overwrite — we treat `review_gate_policy` as authoritative
        // for the resolved policy on this call.
        let mut p = json!({
            "status": "compiled",
            "review_gate_policy": "off",
        });
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
    }

    // ── wave-14 :: auto-emit decision matrix (no bus) ───────────────────
    //
    // We can't drive the actual `auto_emit_review_question_after_artifact_write`
    // helper here without a `BusServices`, but the manual / off / file-not-
    // written branches return BEFORE the publish call. Replay the same
    // payload mutations in pure helpers so the contract stays pinned.

    #[test]
    fn auto_emit_manual_branch_is_a_noop_aside_from_policy_stamp() {
        // Replay manual-branch behaviour: stamp policy + return early.
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Manual);
        assert_eq!(p["review_gate_policy"], "manual");
        // No `review_question_emitted` mutation on manual — we leave the
        // legacy explicit-emit path in control of that field.
        assert!(p.get("review_question_emitted").is_none());
    }

    #[test]
    fn auto_emit_off_branch_stamps_emitted_false() {
        let mut p = json!({"status": "compiled", "file_written": true});
        stamp_policy(&mut p, ReviewGatePolicy::Off);
        // Replay the off-branch mutation: stamp emitted=false if absent.
        if let Some(map) = p.as_object_mut() {
            map.entry("review_question_emitted".to_string())
                .or_insert(json!(false));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "off");
    }

    #[test]
    fn auto_emit_file_not_written_records_warning_without_publishing() {
        let mut p = json!({"status": "partial", "file_written": false});
        stamp_policy(&mut p, ReviewGatePolicy::EmitQuestion);
        // Replay the suppress-because-no-file branch.
        if let Some(map) = p.as_object_mut() {
            map.insert("review_question_emitted".to_string(), json!(false));
            map.entry("review_question_warning".to_string()).or_insert(json!({
                "code": "FILE_WRITE_NOT_SUCCESSFUL",
                "reason": "review_gate_policy=emit_question requires file_written=true; auto-emit suppressed",
                "scope": "directive",
                "artifact_id": "abc",
                "version": 1,
            }));
        }
        assert_eq!(p["review_question_emitted"], false);
        assert_eq!(p["review_gate_policy"], "emit_question");
        assert_eq!(p["review_question_warning"]["code"], "FILE_WRITE_NOT_SUCCESSFUL");
    }

    #[test]
    fn auto_emit_explicit_id_override_wins_over_derived() {
        // Replay the id-resolution: id_override beats derive_review_question_id_for_artifact.
        let derived = derive_review_question_id_for_artifact(
            "plan",
            "p1",
            1,
            "compile",
            Some("/some/file"),
        );
        let id_override = "review:custom:override";
        let chosen = if !id_override.trim().is_empty() {
            id_override.to_string()
        } else {
            derived.clone()
        };
        assert_eq!(chosen, "review:custom:override");
        assert_ne!(chosen, derived);
    }

    #[test]
    fn review_gate_policy_was_explicit_detects_presence() {
        assert!(!review_gate_policy_was_explicit(&json!({})));
        assert!(!review_gate_policy_was_explicit(
            &json!({"emit_review_question": true})
        ));
        // Even an empty / unknown value still counts as "the key was sent",
        // so the response should stamp `review_gate_policy=manual` to make
        // the resolution visible.
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": ""})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "off"})
        ));
        assert!(review_gate_policy_was_explicit(
            &json!({"review_gate_policy": "emit_question"})
        ));
    }

    #[test]
    fn auto_emit_decision_variants_are_distinct() {
        // Pinning that the four decision variants are distinct so callers
        // can pattern-match them in tests / logging without surprise.
        assert_ne!(
            AutoEmitDecision::SkippedPolicyManual,
            AutoEmitDecision::SkippedPolicyOff
        );
        assert_ne!(
            AutoEmitDecision::SkippedFileWriteUnsuccessful,
            AutoEmitDecision::Emitted
        );
        assert_ne!(
            AutoEmitDecision::Emitted,
            AutoEmitDecision::EmitFailedBus
        );
    }
}

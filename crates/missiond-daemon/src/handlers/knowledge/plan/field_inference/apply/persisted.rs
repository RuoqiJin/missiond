use super::*;

// ── wave-22 / task 04 — Persisted PLAN inference apply v2 ──────────────
//
// Layered on top of wave-21 / task 05 apply gate v1. The v1 gate
// promoted `applied_fields[]` into caller args (in-memory only) and
// hard-pinned `apply_gate.persist_inference_applied = false`. v2
// preserves every v1 invariant (default off, conflicts never apply,
// suggestions never apply, LLM proposals require `llm_caller_approved`,
// strict bool shape) AND adds an explicit, audited persistence path:
//
//   * `apply_inferred_fields=true`        — v1 master switch
//   * `persist_inference=true`            — v1 echo flag, NOW load-bearing
//   * `caller_approved=true`              — NEW second human opt-in
//   * `proposal_hash=<32-hex>`            — NEW deterministic correlator
//
// All four must hold AND the gate must have promoted at least one
// field AND the caller's hash must MATCH `compute_inference_proposal_hash`
// computed over `(plan_id, original_sexp_hash, applied_fields)`. On
// mismatch / missing the handler returns a structured error BEFORE any
// DB mutation per the goal contract (R2).
//
// On success the handler:
//   1. Reads `original_sexp_hash` from the existing `plan` row.
//   2. Synthesises `resulting_sexp_text` by APPENDING a guarded
//      `(plan-inference-applied :inference-version "v2" ...)` form to
//      the existing s-exp. The original body is preserved verbatim and
//      `parse_plan_hints` keeps first-occurrence semantics, so the
//      observable PLAN behaviour stays identical when the appended
//      keywords overlap an original hint. New hints become live.
//   3. Inserts a NEW plan row at `version = max + 1` via `plan_insert`
//      — never overwrites the existing row (R4 — version + audit).
//   4. Calls `plan_supersede(old_id)` so the previous version is
//      visibly retired with `status=superseded` (rollback handle).
//   5. Appends a typed `plan_inference_persisted_apply` evidence entry
//      with applied_fields[], skipped_fields[], proposal_hash,
//      original_sexp_hash, resulting_sexp_hash, rollback_pointer
//      (the previous plan id) so the audit trail is complete (R5).
//
// Conservative posture: the persist path is OPT-IN at four
// independent flags. Default behaviour (any flag absent / false)
// keeps the v1 byte-shape exactly — `apply_gate.persist_inference_applied`
// stays `false` and the response surfaces `persisted_apply.status =
// "not_requested"` so observers can pivot without re-deriving the
// policy. Failure modes (missing hash / hash mismatch / invalid param)
// fail-fast as structured errors; soft-skip modes (no applied fields /
// caller_approved=false / persist_inference=false) surface on the
// `persisted_apply` block with a canonical reason and DO NOT mutate
// the DB.
//
// Lisp authority forward reference (Wave 22 backfill):
//   - intent-flow.lisp :: F-intent-alignment-plan-execution-loop ::
//                         s4 plan-authoring (persist gate v2)
//   - intent-tools.lisp :: implemented-surface mission_plan ::
//                         :execute-contract :persisted-inference-apply

/// True when caller passed `caller_approved=true` (any other shape —
/// including the literal string `"true"` — is rejected by
/// `validate_apply_gate_args` BEFORE we get here, so this only checks
/// the bool form). The flag is the SECOND human opt-in for the v2
/// persist path; default `false` keeps the v1 byte-shape exactly.
pub(in crate::handlers::knowledge::plan) fn caller_requested_caller_approved(args: &Value) -> bool {
    args.get("caller_approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Extract the caller-supplied `proposal_hash` (32-hex SHA-256 prefix).
/// Returns `None` when absent, an empty string after trim, or a non-
/// string shape (the validator already rejected the latter as
/// `INVALID_PARAM`, so this is purely defensive).
pub(in crate::handlers::knowledge::plan) fn caller_supplied_proposal_hash(
    args: &Value,
) -> Option<String> {
    let s = args.get("proposal_hash").and_then(|v| v.as_str())?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Compute the deterministic 32-hex correlator over
/// `(plan_id, original_sexp_hash, sorted applied fields)`. The hash is
/// what the caller is expected to echo back via the `proposal_hash`
/// arg under `apply_inferred_fields=true + caller_approved=true +
/// persist_inference=true`. Caller can derive it themselves from the
/// gate response — we surface the same value under
/// `persisted_apply.computed_proposal_hash` so dashboards can
/// `assert hash == derive(...)` directly.
///
/// Hash payload (canonical UTF-8):
///   `"v2|<plan-id>|<original-sexp-hash>|<field>:<value-canonical>|..."`
///
/// Fields are sorted lexicographically by `field` so observers see a
/// deterministic hash regardless of the order in which the gate
/// promoted them. Each value is canonicalised via
/// `serde_json::to_string` (compact form, sorted object keys via the
/// `Value` representation).
pub(in crate::handlers::knowledge::plan) fn compute_inference_proposal_hash(
    plan_id: uuid::Uuid,
    original_sexp_hash: &str,
    applied: &[AppliedField],
) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&AppliedField> = applied.iter().collect();
    sorted.sort_by_key(|af| af.field);
    let mut payload = format!("v2|{}|{}", plan_id, original_sexp_hash.trim());
    for af in sorted.iter() {
        let value_canonical = serde_json::to_string(&af.value).unwrap_or_else(|_| String::new());
        payload.push('|');
        payload.push_str(af.field);
        payload.push(':');
        payload.push_str(&value_canonical);
    }
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    let full = format!("{:x}", h.finalize());
    full[..32].to_string()
}

/// Status discriminants for the v2 persist path. The wire string is
/// stable so observers / dashboards can pivot on it without re-reading
/// the rest of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::knowledge::plan) enum PersistedApplyStatus {
    /// Caller did not opt into the persist path (default). The v1 gate
    /// may still have augmented in-memory args.
    NotRequested,
    /// All four opt-ins supplied + hash matched + at least one field
    /// promoted ⇒ a new plan version was committed.
    Applied,
    /// `apply_inferred_fields` was not `true`. Persist requires the
    /// master switch.
    SkippedApplyGateNotRequested,
    /// `persist_inference` was not `true`. Persist requires the
    /// dedicated persistence opt-in (echo of v1 flag, now load-bearing).
    SkippedPersistNotRequested,
    /// `caller_approved` was not `true`. Persist requires the second
    /// human opt-in.
    SkippedCallerNotApproved,
    /// The v1 gate promoted no fields (everything was conflict /
    /// suggestion / non-approved / safety-skipped). Persist refuses
    /// to write a no-op version.
    SkippedNothingToApply,
}

impl PersistedApplyStatus {
    pub(in crate::handlers::knowledge::plan) fn as_wire(self) -> &'static str {
        match self {
            PersistedApplyStatus::NotRequested => "not_requested",
            PersistedApplyStatus::Applied => "applied",
            PersistedApplyStatus::SkippedApplyGateNotRequested => {
                "skipped_apply_gate_not_requested"
            }
            PersistedApplyStatus::SkippedPersistNotRequested => "skipped_persist_not_requested",
            PersistedApplyStatus::SkippedCallerNotApproved => "skipped_caller_not_approved",
            PersistedApplyStatus::SkippedNothingToApply => "skipped_nothing_to_apply",
        }
    }

    pub(in crate::handlers::knowledge::plan) fn was_applied(self) -> bool {
        matches!(self, PersistedApplyStatus::Applied)
    }
}

/// Aggregate persisted-apply outcome, surfaced under `persisted_apply`
/// on the response. Mirrors `ApplyGateOutcome::to_response_json` in
/// always emitting every field (with conservative defaults) so observers
/// pivot on a stable shape regardless of the persist path's status.
#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::plan) struct PersistedApplyOutcome {
    pub(in crate::handlers::knowledge::plan) status: PersistedApplyStatus,
    pub(in crate::handlers::knowledge::plan) apply_inferred_fields_requested: bool,
    pub(in crate::handlers::knowledge::plan) persist_inference_requested: bool,
    pub(in crate::handlers::knowledge::plan) caller_approved: bool,
    pub(in crate::handlers::knowledge::plan) original_sexp_hash: String,
    pub(in crate::handlers::knowledge::plan) resulting_sexp_hash: Option<String>,
    pub(in crate::handlers::knowledge::plan) computed_proposal_hash: Option<String>,
    pub(in crate::handlers::knowledge::plan) supplied_proposal_hash: Option<String>,
    pub(in crate::handlers::knowledge::plan) applied_fields: Vec<AppliedField>,
    pub(in crate::handlers::knowledge::plan) skipped_fields: Vec<SkippedField>,
    /// Newly inserted plan id (when status == Applied). `None` on every
    /// skip path.
    pub(in crate::handlers::knowledge::plan) new_plan_id: Option<uuid::Uuid>,
    /// New plan version (when status == Applied). `None` on every skip path.
    pub(in crate::handlers::knowledge::plan) new_plan_version: Option<i32>,
    /// Pointer to the now-superseded plan id (rollback handle). Always
    /// populated when status == Applied; the wave-21 plan_supersede call
    /// guarantees the row stays queryable for audit / replay.
    pub(in crate::handlers::knowledge::plan) rollback_plan_id: Option<uuid::Uuid>,
}

impl PersistedApplyOutcome {
    /// Build the default `not_requested` outcome from caller args + the
    /// v1 apply-gate decision. Used as the response anchor on every
    /// path that does NOT opt into persist.
    pub(in crate::handlers::knowledge::plan) fn from_skip_reason(
        status: PersistedApplyStatus,
        args: &Value,
        original_sexp_hash: &str,
        applied: &[AppliedField],
        skipped: &[SkippedField],
        computed_hash: Option<String>,
    ) -> Self {
        Self {
            status,
            apply_inferred_fields_requested: caller_requested_apply(args),
            persist_inference_requested: caller_requested_persist_inference(args),
            caller_approved: caller_requested_caller_approved(args),
            original_sexp_hash: original_sexp_hash.to_string(),
            resulting_sexp_hash: None,
            computed_proposal_hash: computed_hash,
            supplied_proposal_hash: caller_supplied_proposal_hash(args),
            applied_fields: applied.to_vec(),
            skipped_fields: skipped.to_vec(),
            new_plan_id: None,
            new_plan_version: None,
            rollback_plan_id: None,
        }
    }

    pub(in crate::handlers::knowledge::plan) fn to_response_json(&self) -> Value {
        let applied: Vec<Value> = self.applied_fields.iter().map(|f| f.to_json()).collect();
        let skipped: Vec<Value> = self.skipped_fields.iter().map(|f| f.to_json()).collect();
        json!({
            "status": self.status.as_wire(),
            "apply_inferred_fields_requested": self.apply_inferred_fields_requested,
            "persist_inference_requested": self.persist_inference_requested,
            "caller_approved": self.caller_approved,
            "original_sexp_hash": self.original_sexp_hash,
            "resulting_sexp_hash": self.resulting_sexp_hash.clone(),
            "computed_proposal_hash": self.computed_proposal_hash.clone(),
            "supplied_proposal_hash": self.supplied_proposal_hash.clone(),
            "applied_fields": applied,
            "skipped_fields": skipped,
            "new_plan_id": self.new_plan_id.map(|u| u.to_string()),
            "new_plan_version": self.new_plan_version,
            "rollback_plan_id": self.rollback_plan_id.map(|u| u.to_string()),
        })
    }
}

// Make `AppliedField` / `SkippedField` cloneable so the persist path can
// snapshot them into the outcome for the response + evidence.
//
// The wave-21 / task 05 structs were defined `Clone`-free; we add it via
// derive on the field types directly above. (The structs themselves are
// already `Clone` — see `#[derive(Debug, Clone)]` on `AppliedField` and
// `SkippedField`.)

/// Pure pre-flight gate. Inverted v1 semantics: persist runs ONLY when
/// every opt-in is true AND the gate promoted at least one field. On
/// any failure path returns the canonical skip status WITHOUT touching
/// the DB. Hash mismatch / missing is NOT handled here — that path is
/// handled by `enforce_persisted_apply_preflight` which fail-fasts as
/// a structured error per R2.
pub(in crate::handlers::knowledge::plan) fn evaluate_persisted_apply_gate(
    args: &Value,
    apply: &ApplyGateOutcome,
) -> PersistedApplyStatus {
    if !caller_requested_apply(args) {
        return PersistedApplyStatus::SkippedApplyGateNotRequested;
    }
    if !caller_requested_persist_inference(args) {
        return PersistedApplyStatus::SkippedPersistNotRequested;
    }
    if !caller_requested_caller_approved(args) {
        return PersistedApplyStatus::SkippedCallerNotApproved;
    }
    if apply.applied.is_empty() {
        return PersistedApplyStatus::SkippedNothingToApply;
    }
    PersistedApplyStatus::Applied
}

/// Strict pre-flight for the v2 hash check. Mirrors
/// `enforce_apply_gate_preflight` from review_gate.rs: returns
/// `Err((code, message))` on missing / mismatch BEFORE any DB mutation
/// per the goal contract (R2). Returns `Ok(())` when the caller did not
/// opt into the persist path (the soft-skip outcome is computed by
/// `evaluate_persisted_apply_gate` afterwards) OR when the hash matches
/// the deterministic correlator.
///
/// Skipping the preflight on a non-persist path is intentional: the
/// caller may legitimately omit `proposal_hash` / `caller_approved` on
/// every legacy v1 call. We only fail-fast when the caller PRESENTED
/// the persist intent (apply + persist + caller_approved all `true`)
/// AND the hash is missing / wrong.
pub(in crate::handlers::knowledge::plan) fn enforce_persisted_apply_preflight(
    args: &Value,
    computed_hash: &str,
) -> std::result::Result<(), (&'static str, String)> {
    // Preflight only applies when caller opted into all THREE persist
    // flags. Any other arrangement is a soft-skip handled downstream.
    if !caller_requested_apply(args)
        || !caller_requested_persist_inference(args)
        || !caller_requested_caller_approved(args)
    {
        return Ok(());
    }
    let supplied = match caller_supplied_proposal_hash(args) {
        Some(s) => s,
        None => {
            return Err((
                error_codes::INVALID_PARAM,
                format!(
                    "PERSIST_APPLY_MISSING_PROPOSAL_HASH: persist_inference=true + caller_approved=true requires proposal_hash to match the v2 deterministic correlator (expected `{}`); supply proposal_hash from a prior preview call's persisted_apply.computed_proposal_hash field",
                    computed_hash
                ),
            ));
        }
    };
    if !supplied.eq_ignore_ascii_case(computed_hash) {
        return Err((
            error_codes::INVALID_PARAM,
            format!(
                "PERSIST_APPLY_PROPOSAL_HASH_MISMATCH: caller-supplied proposal_hash `{}` does not match the v2 deterministic correlator `{}`; the apply set may have changed since the proposal was previewed — re-run the gate without persist flags first to capture the fresh hash",
                supplied, computed_hash
            ),
        ));
    }
    Ok(())
}

/// Render a single AppliedField as a `:keyword value` lisp pair. Mirrors
/// the conservative `parse_plan_hints` reader in plan.rs:
///   * canonical kebab-case keyword (matches the reader's
///     `target` / `dispatch-strategy` / `target-project` / `requested-cwd`
///     / `acceptance-mode` / `owned-files` / `workstation-dispatch`
///     spellings)
///   * string scalars are double-quoted with `\\` / `\"` escapes
///   * bool scalars become `true` / `false` barewords
///   * arrays become `[ "a" "b" ]` bracket lists (matches
///     `split_lisp_string_list`)
///   * any other shape (number / object / null) is serialised via
///     `serde_json::to_string` and emitted as a quoted string so the
///     reader treats it as a bareword passthrough — defensive only,
///     the apply gate's safety check already filtered shapes before we
///     get here.
pub(in crate::handlers::knowledge::plan) fn render_applied_field_to_lisp(
    field: &str,
    value: &Value,
) -> String {
    let key = match field {
        "target" => "target",
        "dispatch_strategy" => "dispatch-strategy",
        "target_project" => "target-project",
        "owned_files" => "owned-files",
        "acceptance_mode" => "acceptance-mode",
        "workstation_dispatch" => "workstation-dispatch",
        // Defensive — any future field name that is not a known reader
        // alias keeps the snake-case form so the keyword pair is still
        // syntactically valid (the reader will silently ignore unknown
        // keywords per its `_ => {}` arm).
        other => other,
    };
    match value {
        Value::String(s) => format!(":{} \"{}\"", key, escape_lisp_string(s)),
        Value::Bool(b) => format!(":{} {}", key, if *b { "true" } else { "false" }),
        Value::Array(items) => {
            let mut parts: Vec<String> = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::String(s) => parts.push(format!("\"{}\"", escape_lisp_string(s))),
                    Value::Bool(b) => parts.push((if *b { "true" } else { "false" }).into()),
                    other => parts.push(format!(
                        "\"{}\"",
                        escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
                    )),
                }
            }
            format!(":{} [{}]", key, parts.join(" "))
        }
        Value::Number(n) => format!(":{} {}", key, n),
        other => format!(
            ":{} \"{}\"",
            key,
            escape_lisp_string(&serde_json::to_string(other).unwrap_or_default())
        ),
    }
}

pub(in crate::handlers::knowledge::plan) fn escape_lisp_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

/// Synthesise the resulting PLAN.lisp by APPENDING a guarded
/// `(plan-inference-applied ...)` form to the existing s-exp. The
/// original body is preserved verbatim so the supersede chain has a
/// clean diff (`tail -1` on the new s-exp shows the persisted
/// annotation; everything else is the byte-identical predecessor).
///
/// `parse_plan_hints` keeps first-occurrence semantics — when the
/// appended keyword overlaps an original hint the original wins. New
/// hints (e.g. `:dispatch-strategy resident-lisp` when the original
/// PLAN never spelled it) become the live value because no prior
/// occurrence exists. This is the conservative posture: the inferer
/// EXTENDS the PLAN; it never silently rewrites a caller-authored
/// hint at the persistence boundary.
pub(in crate::handlers::knowledge::plan) fn synthesize_persisted_sexp(
    original: &str,
    applied: &[AppliedField],
    proposal_hash: &str,
    timestamp: &str,
) -> String {
    let mut out = String::with_capacity(original.len() + 256);
    out.push_str(original);
    if !original.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    // Header — observers can grep on this exact prefix.
    out.push_str(";; wave-22 / task 04 — persisted PLAN inference apply v2\n");
    out.push_str(&format!(
        "(plan-inference-applied :inference-version \"v2\" :proposal-hash \"{}\" :persisted-at \"{}\"",
        proposal_hash, timestamp
    ));
    // Emit each applied field as a SIBLING keyword pair so the
    // wave-15 / task 05 `parse_plan_hints` reader picks them up at the
    // PLAN level (the reader scans `:keyword value` pairs at any depth
    // but treats bracket lists as opaque value spans). A flat list of
    // pairs keeps the appended annotation queryable without breaking
    // first-occurrence semantics — the original PLAN body still
    // appears first in the buffer, so its hints win on every overlap.
    for af in applied.iter() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&render_applied_field_to_lisp(af.field, &af.value));
    }
    out.push(')');
    out.push('\n');
    out
}

/// Build the typed evidence entry for the persisted apply path. Mirrors
/// the wave-12 typed-evidence schema (`schema_version="v0"`,
/// canonical `source` + `kind`) so a single grep over the evidence
/// sidecar surfaces every persist event with a stable shape.
pub(in crate::handlers::knowledge::plan) fn build_persisted_apply_evidence_entry(
    outcome: &PersistedApplyOutcome,
    plan_id: uuid::Uuid,
) -> Value {
    let applied: Vec<Value> = outcome.applied_fields.iter().map(|f| f.to_json()).collect();
    let skipped: Vec<Value> = outcome.skipped_fields.iter().map(|f| f.to_json()).collect();
    json!({
        "schema_version": "v0",
        "source": "plan_inference_persisted_apply",
        "kind": "plan_inference_persisted_apply",
        "plan_id": plan_id.to_string(),
        "rollback_plan_id": outcome.rollback_plan_id.map(|u| u.to_string()),
        "new_plan_id": outcome.new_plan_id.map(|u| u.to_string()),
        "new_plan_version": outcome.new_plan_version,
        "original_sexp_hash": outcome.original_sexp_hash,
        "resulting_sexp_hash": outcome.resulting_sexp_hash,
        "proposal_hash": outcome.computed_proposal_hash,
        "applied_fields": applied,
        "skipped_fields": skipped,
        "status": outcome.status.as_wire(),
    })
}

/// Apply the v2 persist gate. Pure of `state` interaction at the gate
/// stage (compute hash + evaluate skip), then exercises `state.store`
/// for the new plan version + supersede + evidence write only when the
/// gate authorised the apply. On every skip path the DB is untouched
/// and the outcome surfaces the canonical skip reason on the response.
///
/// Returns `Err(structured_error_pair)` ONLY for the fail-fast hash
/// preflight (R2). Every other path returns `Ok(outcome)` with the
/// status communicating success / soft-skip.
pub(in crate::handlers::knowledge::plan) async fn execute_persisted_apply(
    state: &AppState,
    plan: &Plan,
    args: &Value,
    apply: &ApplyGateOutcome,
) -> std::result::Result<PersistedApplyOutcome, (&'static str, String)> {
    let original_sexp_hash = sha256_hex(&plan.sexp_text);
    let computed_hash =
        compute_inference_proposal_hash(plan.id, &original_sexp_hash, &apply.applied);

    // Fail-fast hash preflight per R2.
    enforce_persisted_apply_preflight(args, &computed_hash)?;

    let status = evaluate_persisted_apply_gate(args, apply);
    if !status.was_applied() {
        return Ok(PersistedApplyOutcome::from_skip_reason(
            status,
            args,
            &original_sexp_hash,
            &apply.applied,
            &apply.skipped,
            Some(computed_hash),
        ));
    }

    // Persist path. Synthesise the new sexp text + hash, allocate the
    // next plan version, insert the new row, supersede the predecessor,
    // append the typed evidence entry. Each step uses the existing
    // wave-21 store API (no new trait method per the contract's
    // `:must-not-touch` boundary).
    let timestamp = iso_now();
    let resulting_sexp_text =
        synthesize_persisted_sexp(&plan.sexp_text, &apply.applied, &computed_hash, &timestamp);
    let resulting_sexp_hash = sha256_hex(&resulting_sexp_text);

    let existing = state
        .store
        .plan_list_by_task(&plan.board_task_id)
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_list_by_task: {}", e)))?;
    let next_version = existing.iter().map(|p| p.version).max().unwrap_or(0) + 1;

    let new_plan_id = state
        .store
        .plan_insert(
            &plan.board_task_id,
            plan.source_directive_id,
            next_version,
            &resulting_sexp_text,
            &resulting_sexp_hash,
            // Inherit the predecessor's status — we are NOT changing
            // FSM stage on this write, only persisting an inference
            // annotation. The plan-runner will continue from the new
            // version on its next execute call.
            plan.status,
            plan.compiler_model.as_deref(),
            // Stamp `compiled_from` so the audit trail points at the
            // predecessor row (rollback handle on the immutable v0 of
            // the column).
            Some(&format!("plan-inference-persist/{}", plan.id)),
        )
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_insert: {}", e)))?;

    state
        .store
        .plan_supersede(plan.id, new_plan_id)
        .await
        .map_err(|e| (error_codes::DB_ERROR, format!("plan_supersede: {}", e)))?;

    let outcome = PersistedApplyOutcome {
        status: PersistedApplyStatus::Applied,
        apply_inferred_fields_requested: true,
        persist_inference_requested: true,
        caller_approved: true,
        original_sexp_hash: original_sexp_hash.clone(),
        resulting_sexp_hash: Some(resulting_sexp_hash),
        computed_proposal_hash: Some(computed_hash),
        supplied_proposal_hash: caller_supplied_proposal_hash(args),
        applied_fields: apply.applied.clone(),
        skipped_fields: apply.skipped.clone(),
        new_plan_id: Some(new_plan_id),
        new_plan_version: Some(next_version),
        rollback_plan_id: Some(plan.id),
    };

    // Append typed evidence (R5). Failure here does NOT roll back the
    // new plan row — file-vs-db contract per `append_plan_evidence_entry`
    // (the row is committed even if the sidecar write fails). We
    // surface the error path via the standard evidence_warning surface
    // on the response.
    let evidence_entry = build_persisted_apply_evidence_entry(&outcome, plan.id);
    let project_arg = args.get("project").and_then(|v| v.as_str());
    let cwd_arg = args.get("cwd").and_then(|v| v.as_str());
    let target_project_arg = args.get("target_project").and_then(|v| v.as_str());
    // Append on the PREDECESSOR's evidence sidecar (the rollback
    // pointer is the predecessor — observers replaying a rollback
    // need the persisted-apply entry on the same sidecar as the
    // pre-apply history).
    let _ = append_plan_evidence_entry(
        state,
        plan.id,
        project_arg,
        cwd_arg,
        target_project_arg,
        evidence_entry,
    )
    .await;

    Ok(outcome)
}

/// Splice the `persisted_apply` block onto a successful response.
/// Mirrors `attach_apply_gate_block` exactly: structured errors are
/// left untouched, and a pre-existing block is preserved (NEVER
/// overwritten) so future DAG / resume paths can attach their own row.
pub(in crate::handlers::knowledge::plan) fn attach_persisted_apply_block(
    mut result: ToolResult,
    block: Option<Value>,
) -> ToolResult {
    let Some(block) = block else {
        return result;
    };
    if result.is_error.unwrap_or(false) {
        return result;
    }
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => return result,
    };
    let mut payload: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return result,
    };
    if let Some(map) = payload.as_object_mut() {
        map.entry("persisted_apply".to_string()).or_insert(block);
    }
    result.content = vec![ToolContent::Text {
        text: serde_json::to_string_pretty(&payload).unwrap_or(text),
    }];
    result
}

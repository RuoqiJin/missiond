use super::*;

/// Inference rule input — what the deterministic engine actually reads.
/// Built once per `mission_plan(action=execute)` call so the rule
/// functions can stay pure.
#[derive(Debug, Default, Clone)]
pub(in crate::handlers::knowledge::plan) struct PlanInferenceInput<'a> {
    pub(in crate::handlers::knowledge::plan) plan_hints: ParsedPlanHints,
    /// Typed `plan.contract_json` projected by missiond-lispc. Per-field
    /// rules may read `payload.top_level` for facts that are not represented
    /// in [`ParsedPlanHints`].
    pub(in crate::handlers::knowledge::plan) plan_contract: Option<&'a Value>,
    pub(in crate::handlers::knowledge::plan) compiled_from: Option<&'a str>,
    pub(in crate::handlers::knowledge::plan) evidence_entries: Vec<Value>,
}

/// Pure inference engine over the input above. Produces the aggregate
/// result + the list of recommended arg augmentations (only filled when
/// `mode=ApplySafe`; preview mode also computes inference but the caller
/// short-circuits before using the augmentations).
///
/// Conflict semantics: when the caller supplied a value AND the inferer
/// derived a different one from a recognised source, the field becomes a
/// conflict. The conflict is REPORTED (never auto-resolved); apply_safe
/// will NEVER mutate over a caller-supplied value.
pub(in crate::handlers::knowledge::plan) fn compute_plan_field_inference(
    args: &Value,
    input: &PlanInferenceInput<'_>,
) -> PlanFieldInference {
    let mut result = PlanFieldInference::default();
    let mut sources: Vec<&'static str> = Vec::new();
    if !is_empty_hints(&input.plan_hints) || plan_contract_top_level_has_signal(input) {
        sources.push("plan_contract");
    }
    if input
        .compiled_from
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        sources.push("compiled_from");
    }
    if !input.evidence_entries.is_empty() {
        sources.push("evidence_sidecar");
    }
    result.evidence_sources = sources;

    infer_target(args, input, &mut result);
    infer_dispatch_strategy(args, input, &mut result);
    infer_target_project(args, input, &mut result);
    infer_owned_files(args, input, &mut result);
    infer_acceptance_mode(args, input, &mut result);
    infer_workstation_dispatch(args, input, &mut result);

    result
}

/// True when the parsed hints carry no usable signal at all. Used to
/// drive `evidence_sources` reporting; does NOT change the inferer's
/// per-field decisions.
pub(super) fn is_empty_hints(h: &ParsedPlanHints) -> bool {
    h.target.is_none()
        && h.flow_id.is_none()
        && h.dispatch_strategy.is_none()
        && h.parallelism.is_none()
        && h.target_project.is_none()
        && h.requested_cwd.is_none()
        && h.objective.is_none()
        && h.summary.is_none()
        && h.scope.is_none()
        && h.commit_policy.is_none()
        && h.owned_files_raw.is_none()
        && h.forbidden_files_raw.is_none()
        && h.acceptance_commands_raw.is_none()
        && h.workstation_dispatch_flag.is_none()
}

fn plan_contract_top_level_has_signal(input: &PlanInferenceInput<'_>) -> bool {
    input
        .plan_contract
        .and_then(|contract| contract.get("payload").and_then(|p| p.get("top_level")))
        .and_then(Value::as_object)
        .map(|top_level| !top_level.is_empty())
        .unwrap_or(false)
}

fn plan_contract_top_level_string<'a>(
    input: &'a PlanInferenceInput<'a>,
    key: &str,
) -> Option<&'a str> {
    input
        .plan_contract?
        .get("payload")?
        .get("top_level")?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Helper: the caller's explicit string value for a field, trimmed and
/// non-empty. `None` means "caller did not specify" so the inferer is
/// free to fill.
pub(in crate::handlers::knowledge::plan) fn caller_str<'a>(
    args: &'a Value,
    key: &str,
) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Helper: the caller's explicit bool value for a field. `None` means
/// "caller did not specify" so the inferer is free to fill.
pub(in crate::handlers::knowledge::plan) fn caller_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Helper: caller-supplied string list for `owned_files`-shaped args.
/// Honours both string and array forms (mirroring the caller-side schema).
pub(in crate::handlers::knowledge::plan) fn caller_string_list(
    args: &Value,
    key: &str,
) -> Vec<String> {
    collect_string_list(args.get(key))
}

/// Push an inferred field — high-confidence fields land in `inferred`,
/// medium / low always land in `suggested`.
pub(super) fn record_inferred(result: &mut PlanFieldInference, field: InferredField) {
    if field.confidence.meets_apply_threshold() {
        result.inferred.push(field);
    } else {
        result.suggested.push(field);
    }
}

/// Record a conflict (caller value differs from inferred value). NEVER
/// promotes the inferred value into `inferred` even when confidence is
/// `high` — apply_safe must not silently override caller intent.
pub(super) fn record_conflict(result: &mut PlanFieldInference, conflict: InferenceConflict) {
    result.conflicts.push(conflict);
}

// ── per-field rule fns ────────────────────────────────────────────────

/// Infer `target`. Confidence:
///   * `high`   — PLAN.lisp `:target` hint normalises to a canonical target.
///   * `high`   — ≥1 evidence entry agrees on the same target string.
///   * `medium` — `compiled_from` text contains an unambiguous keyword.
pub(super) fn infer_target(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "target");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    // 1. Typed plan contract hint.
    if let Some(raw) = input.plan_hints.target.as_deref() {
        if let Some(canonical) = normalize_target(raw, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_contract",
                Some(format!(":target hint resolved to `{}`", canonical)),
            ));
        }
    }

    // 2. Evidence sidecar — the most recent dispatch record carries
    //    `target_tool`. Multiple agreeing entries reinforce the signal.
    let evidence_target = scan_evidence_string_field(&input.evidence_entries, &["target_tool"])
        .and_then(|s| {
            normalize_target(&s, input.plan_hints.flow_id.is_some()).map(|canonical| (canonical, s))
        });
    if let Some((canonical, raw)) = evidence_target {
        hits.push((
            InferenceConfidence::High,
            canonical,
            "evidence_sidecar",
            Some(format!("prior dispatch target_tool=`{}`", raw)),
        ));
    }

    // 3. compiled_from keyword scan.
    if let Some(text) = input.compiled_from {
        if let Some(canonical) = normalize_target(text, input.plan_hints.flow_id.is_some()) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "compiled_from",
                Some(format!("compiled_from `{}` mentions `{}`", text, canonical)),
            ));
        }
    }

    finalize_string_field("target", caller, hits, result);
}

/// Infer `dispatch_strategy`. Confidence:
///   * `high`   — PLAN.lisp `:dispatch-strategy` (canonicalised).
///   * `high`   — evidence entry carries a known strategy.
///   * `medium` — PLAN.lisp `:parallelism` keyword maps to a strategy.
///   * `medium` — `compiled_from` carries a keyword like "agent-team".
pub(super) fn infer_dispatch_strategy(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "dispatch_strategy");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    if let Some(raw) = input.plan_hints.dispatch_strategy.as_deref() {
        if let Some(c) = canonicalize_strategy(raw) {
            hits.push((
                InferenceConfidence::High,
                c,
                "plan_contract",
                Some(format!(":dispatch-strategy hint `{}`", raw)),
            ));
        }
    }

    if let Some(s) = scan_evidence_string_field(&input.evidence_entries, &["dispatch_strategy"]) {
        if let Some(c) = canonicalize_strategy(&s) {
            hits.push((
                InferenceConfidence::High,
                c,
                "evidence_sidecar",
                Some(format!("prior dispatch dispatch_strategy=`{}`", s)),
            ));
        }
    }

    if let Some(p) = input.plan_hints.parallelism.as_deref() {
        if let Some(c) = canonicalize_strategy(p) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "plan_contract",
                Some(format!(":parallelism hint `{}` mapped to strategy", p)),
            ));
        }
    }

    if let Some(text) = input.compiled_from {
        if let Some(c) = canonicalize_strategy(text) {
            hits.push((
                InferenceConfidence::Medium,
                c,
                "compiled_from",
                Some(format!("compiled_from keyword maps to `{}`", c)),
            ));
        }
    }

    finalize_string_field("dispatch_strategy", caller, hits, result);
}

/// Infer `target_project`. Confidence:
///   * `high`   — PLAN.lisp `:target-project` non-empty.
///   * `high`   — evidence entry carries the same target_project >=2 times.
///   * `medium` — single evidence entry carries target_project.
pub(super) fn infer_target_project(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "target_project");
    let mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)> = Vec::new();

    if let Some(tp) = input.plan_hints.target_project.as_deref() {
        let v = tp.trim();
        if !v.is_empty() {
            hits.push((
                InferenceConfidence::High,
                v.to_string(),
                "plan_contract",
                Some(":target-project hint".to_string()),
            ));
        }
    }

    let evidence_hits = scan_evidence_string_counts(&input.evidence_entries, &["target_project"]);
    if let Some((value, count)) = evidence_hits.first().cloned() {
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        hits.push((
            conf,
            value.clone(),
            "evidence_sidecar",
            Some(format!(
                "prior dispatch target_project=`{}` (x{})",
                value, count
            )),
        ));
    }

    finalize_owned_string_field("target_project", caller, hits, result);
}

/// Infer `owned_files`. Confidence:
///   * `high`   — PLAN.lisp `:owned-files` parses to >=1 entry.
///   * `medium` — evidence sidecar carries `owned_files` (any non-empty list).
///                Files change across runs, so we never claim `high` from
///                evidence alone.
pub(super) fn infer_owned_files(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_string_list(args, "owned_files");
    let mut hits: Vec<(
        InferenceConfidence,
        Vec<String>,
        &'static str,
        Option<String>,
    )> = Vec::new();

    let plan_owned = split_lisp_string_list(input.plan_hints.owned_files_raw.as_deref());
    if !plan_owned.is_empty() {
        hits.push((
            InferenceConfidence::High,
            plan_owned.clone(),
            "plan_contract",
            Some(format!(
                ":owned-files declares {} entries",
                plan_owned.len()
            )),
        ));
    }

    if let Some(list) = scan_evidence_string_list(&input.evidence_entries, "owned_files") {
        if !list.is_empty() {
            hits.push((
                InferenceConfidence::Medium,
                list.clone(),
                "evidence_sidecar",
                Some(format!(
                    "prior dispatch owned_files carries {} entries",
                    list.len()
                )),
            ));
        }
    }

    finalize_string_list_field("owned_files", caller, hits, result);
}

/// Infer `acceptance_mode`. Confidence:
///   * `high`   — plan.contract_json payload.top_level.acceptance_mode
///                projects to a known AcceptanceMode.
///   * `medium` — evidence entry carries an `acceptance.mode` field.
pub(super) fn infer_acceptance_mode(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_str(args, "acceptance_mode");
    let mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )> = Vec::new();

    if let Some(raw) = plan_contract_top_level_string(input, "acceptance_mode") {
        if let Some(canonical) = canonicalize_acceptance_mode(&raw) {
            hits.push((
                InferenceConfidence::High,
                canonical,
                "plan_contract",
                Some(format!(":acceptance-mode hint `{}`", raw)),
            ));
        }
    }

    if let Some(mode) = scan_evidence_string_field(
        &input.evidence_entries,
        &["acceptance_mode", "acceptance.mode"],
    ) {
        if let Some(canonical) = canonicalize_acceptance_mode(&mode) {
            hits.push((
                InferenceConfidence::Medium,
                canonical,
                "evidence_sidecar",
                Some(format!("prior evidence acceptance_mode=`{}`", mode)),
            ));
        }
    }

    finalize_string_field("acceptance_mode", caller, hits, result);
}

/// Infer `workstation_dispatch`. Confidence:
///   * `high`   — PLAN.lisp `:workstation-dispatch true`.
///   * `high`   — every recent evidence entry that carries
///                `workstation_dispatch_source` lands on a non-disabled
///                source AND the inferable_strategy gate passed.
///   * `medium` — single evidence entry hint.
pub(super) fn infer_workstation_dispatch(
    args: &Value,
    input: &PlanInferenceInput<'_>,
    result: &mut PlanFieldInference,
) {
    let caller = caller_bool(args, "workstation_dispatch");
    let mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)> = Vec::new();

    if input.plan_hints.workstation_dispatch_opt_in() {
        hits.push((
            InferenceConfidence::High,
            true,
            "plan_contract",
            Some(":workstation-dispatch true".to_string()),
        ));
    } else if let Some(raw) = input.plan_hints.workstation_dispatch_flag.as_deref() {
        // Explicit false in PLAN — high confidence "do NOT enable".
        let lc = raw.trim().to_ascii_lowercase();
        if matches!(lc.as_str(), "false" | "no" | "off" | "0") {
            hits.push((
                InferenceConfidence::High,
                false,
                "plan_contract",
                Some(":workstation-dispatch false".to_string()),
            ));
        }
    }

    let ws_sources =
        scan_evidence_string_counts(&input.evidence_entries, &["workstation_dispatch_source"]);
    if let Some((value, count)) = ws_sources.first().cloned() {
        let lc = value.to_ascii_lowercase();
        let positive = matches!(lc.as_str(), "explicit_arg" | "plan_hint" | "inferred");
        let conf = if count >= 2 {
            InferenceConfidence::High
        } else {
            InferenceConfidence::Medium
        };
        if positive {
            hits.push((
                conf,
                true,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`{}` (x{})",
                    value, count
                )),
            ));
        } else if matches!(lc.as_str(), "disabled") {
            hits.push((
                conf,
                false,
                "evidence_sidecar",
                Some(format!(
                    "prior workstation_dispatch_source=`disabled` (x{})",
                    count
                )),
            ));
        }
    }

    finalize_bool_field("workstation_dispatch", caller, hits, result);
}

// ── finalize helpers (per value-shape) ────────────────────────────────

/// Resolve the highest-confidence string-shaped hint and emit either an
/// inferred / suggested entry, or a conflict against caller value.
pub(super) fn finalize_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(
        InferenceConfidence,
        &'static str,
        &'static str,
        Option<String>,
    )>,
    result: &mut PlanFieldInference,
) {
    // Prefer the highest-confidence hit; ties broken by source order.
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };

    if let Some(c) = caller {
        if c.eq_ignore_ascii_case(value) {
            // Caller already agrees with the inference — nothing to do.
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }

    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same as [`finalize_string_field`] but for owned-`String`-shaped hits
/// (where the value is computed dynamically per-call rather than carried
/// as a `&'static str`).
pub(super) fn finalize_owned_string_field(
    field: &'static str,
    caller: Option<&str>,
    mut hits: Vec<(InferenceConfidence, String, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

/// Same shape as [`finalize_string_field`] but for `Vec<String>`-shaped
/// hits. Caller equality compares as set-like (order-independent) so a
/// PLAN.lisp + caller permutation does not trigger a spurious conflict.
pub(super) fn finalize_string_list_field(
    field: &'static str,
    caller: Vec<String>,
    mut hits: Vec<(
        InferenceConfidence,
        Vec<String>,
        &'static str,
        Option<String>,
    )>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if !caller.is_empty() {
        let mut a = caller.clone();
        a.sort();
        let mut b = value.clone();
        b.sort();
        if a == b {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(caller),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

pub(super) fn finalize_bool_field(
    field: &'static str,
    caller: Option<bool>,
    mut hits: Vec<(InferenceConfidence, bool, &'static str, Option<String>)>,
    result: &mut PlanFieldInference,
) {
    hits.sort_by_key(|(c, _, _, _)| match c {
        InferenceConfidence::High => 0,
        InferenceConfidence::Medium => 1,
        InferenceConfidence::Low => 2,
    });
    let Some((conf, value, source, detail)) = hits.first().cloned() else {
        return;
    };
    if let Some(c) = caller {
        if c == value {
            return;
        }
        record_conflict(
            result,
            InferenceConflict {
                field,
                caller_value: json!(c),
                inferred_value: json!(value),
                confidence: conf,
                source,
            },
        );
        return;
    }
    record_inferred(
        result,
        InferredField {
            field,
            value: json!(value),
            confidence: conf,
            source,
            detail,
        },
    );
}

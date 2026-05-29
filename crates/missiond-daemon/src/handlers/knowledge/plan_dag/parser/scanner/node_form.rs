use super::super::super::acceptance::{AcceptanceMode, AcceptanceRequires};
use super::super::super::rollback::{RollbackCascadeMode, RollbackPolicy};
use super::super::types::{DagNode, FAILURE_POLICY_CONTINUE, FAILURE_POLICY_FAIL_FAST};
use super::keyword_pairs::scan_keyword_pairs;
use super::lists::parse_id_list;

pub(super) fn parse_node_form(form: &str) -> Option<DagNode> {
    let pairs = scan_keyword_pairs(form);
    let mut id: Option<String> = None;
    let mut target: Option<String> = None;
    let mut objective: Option<String> = None;
    let mut depends_on: Vec<String> = Vec::new();
    let mut condition: Option<String> = None;
    let mut failure_policy: Option<String> = None;
    let mut timeout_ms: Option<i64> = None;
    let mut dispatch_strategy: Option<String> = None;
    let mut target_project: Option<String> = None;
    let mut requested_cwd: Option<String> = None;
    let mut flow_id: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut commit_policy: Option<String> = None;
    let mut owned_files_raw: Option<String> = None;
    let mut forbidden_files_raw: Option<String> = None;
    let mut acceptance_commands_raw: Option<String> = None;
    let mut execution_order: Option<String> = None;
    let mut parallel_group: Option<String> = None;
    let mut atom_level: Option<u32> = None;
    let mut atom_task_id: Option<String> = None;
    let mut predicted_tool_sequence_raw: Option<String> = None;
    let mut context_sources_raw: Option<String> = None;
    // wave-17 / task 03 — declarative acceptance evaluator hints.
    let mut acceptance_mode_raw: Option<String> = None;
    let mut acceptance_evidence_keys_raw: Option<String> = None;
    // wave-18 / task 03 — cross-node acceptance fan-in hints.
    let mut acceptance_depends_on: Vec<String> = Vec::new();
    let mut acceptance_requires_raw: Option<String> = None;
    let mut acceptance_source_node: Option<String> = None;
    let mut workstation_dispatch_flag: Option<String> = None;
    let mut review_gate: Option<String> = None;
    let mut review_action: Option<String> = None;
    let mut review_text: Option<String> = None;
    // wave-16 / task 05 — bounded per-node retry hints. Both the count
    // and the delay are parsed strictly inside this loop; the first
    // hint failure is captured into `retry_parse_error` so the
    // validator can fail-fast at `build_validated_dag` time without
    // re-tokenising the form. We keep only the FIRST error (later
    // hints still flow through their normal handler / unsupported
    // path so the audit trail captures every signal).
    let mut retry_count: Option<u32> = None;
    let mut retry_delay_ms: Option<u64> = None;
    let mut retry_parse_error: Option<(String, String, String)> = None;
    // wave-17 / task 04 — conservative rollback descriptor hints.
    let mut rollback_policy: Option<String> = None;
    let mut rollback_objective: Option<String> = None;
    let mut rollback_owned_files_raw: Option<String> = None;
    let mut rollback_acceptance_commands_raw: Option<String> = None;
    // wave-18 / task 04 — cascade rollback hints.
    let mut compensates: Option<String> = None;
    let mut rollback_cascade: Option<String> = None;
    let mut rollback_after: Vec<String> = Vec::new();
    // wave-19 / task 10 — forward `:compensate-node` declaration on the
    // failing-node side (alias `:compensate-ref`). Validated against the
    // reverse `:compensates` direction in `build_validated_dag`.
    let mut compensate_node: Option<String> = None;
    let mut unsupported_fields: Vec<(String, String)> = Vec::new();

    for (raw_key, value) in pairs {
        let key = raw_key.to_ascii_lowercase();
        match key.as_str() {
            "id" => set_first(&mut id, &value),
            "target" | "target-tool" | "tool" => set_first(&mut target, &value),
            "objective" => set_first(&mut objective, &value),
            "depends-on" | "depends_on" | "deps" => {
                depends_on = parse_id_list(&value);
            }
            "condition" => set_first(&mut condition, &value),
            "failure-policy" | "failure_policy" => set_first(&mut failure_policy, &value),
            "timeout-ms" | "timeout_ms" => {
                if let Ok(n) = value.trim().parse::<i64>() {
                    if timeout_ms.is_none() {
                        timeout_ms = Some(n);
                    }
                }
            }
            "dispatch-strategy" | "dispatch_strategy" => set_first(&mut dispatch_strategy, &value),
            "target-project" | "target_project" | "project" => {
                set_first(&mut target_project, &value)
            }
            "requested-cwd" | "requested_cwd" | "cwd" => set_first(&mut requested_cwd, &value),
            "flow-id" | "flow_id" => set_first(&mut flow_id, &value),
            // wave-15 / task 05 — workstation-dispatch hint contract.
            // Captured here so the scheduler can route eligible nodes
            // without a second parse pass; only consumed when
            // `:workstation-dispatch true` is also set.
            "scope" => set_first(&mut scope, &value),
            "commit-policy" | "commit_policy" => set_first(&mut commit_policy, &value),
            "owned-files" | "owned_files" => set_first(&mut owned_files_raw, &value),
            "forbidden-files" | "forbidden_files" => set_first(&mut forbidden_files_raw, &value),
            "acceptance-commands" | "acceptance_commands" => {
                set_first(&mut acceptance_commands_raw, &value)
            }
            "execution-order" | "execution_order" => {
                let raw = value.trim();
                if !raw.is_empty() {
                    let lc = raw.to_ascii_lowercase();
                    if !matches!(
                        lc.as_str(),
                        "serial"
                            | "parallel"
                            | "serial-after-dependencies"
                            | "serial_after_dependencies"
                    ) {
                        unsupported_fields.push((raw_key.clone(), value.clone()));
                    }
                }
                set_first(&mut execution_order, &value);
            }
            "parallel-group" | "parallel_group" => set_first(&mut parallel_group, &value),
            "atom-level" | "atom_level" => {
                if atom_level.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => atom_level = Some(n.min(u32::MAX as i64) as u32),
                        _ => unsupported_fields.push((raw_key.clone(), value.clone())),
                    }
                }
            }
            "atom-task-id" | "atom_task_id" => set_first(&mut atom_task_id, &value),
            "predicted-tool-sequence" | "predicted_tool_sequence" => {
                set_first(&mut predicted_tool_sequence_raw, &value)
            }
            "context-sources" | "context_sources" => set_first(&mut context_sources_raw, &value),
            // wave-17 / task 03 — declarative acceptance evaluator hints.
            // `:acceptance-mode` is parsed strictly: unknown values land
            // BOTH on the typed slot AND in `unsupported_fields` so the
            // scheduler safely degrades to the manual-required default
            // while the typo surfaces through `node_hint_summary`.
            "acceptance-mode" | "acceptance_mode" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_mode_raw, &value);
            }
            "acceptance-evidence-keys" | "acceptance_evidence_keys" => {
                set_first(&mut acceptance_evidence_keys_raw, &value)
            }
            // wave-18 / task 03 — cross-node acceptance fan-in hints.
            // `:acceptance-depends-on` accepts the same shapes as
            // `:depends-on` (`["a" "b"]` / `(a b)` / bareword run);
            // `:acceptance-requires` is parsed strictly so a typo lands
            // BOTH on the typed slot AND in `unsupported_fields` while
            // the validator raises a structured error before the
            // scheduler dispatches the node. Single
            // `:acceptance-source-node` is captured verbatim; only
            // consumed under `evidence_keys` mode.
            "acceptance-depends-on" | "acceptance_depends_on" => {
                if acceptance_depends_on.is_empty() {
                    acceptance_depends_on = parse_id_list(&value);
                }
            }
            "acceptance-requires" | "acceptance_requires" => {
                let raw = value.trim();
                if !raw.is_empty() && AcceptanceRequires::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut acceptance_requires_raw, &value);
            }
            "acceptance-source-node" | "acceptance_source_node" => {
                set_first(&mut acceptance_source_node, &value)
            }
            "workstation-dispatch" | "workstation_dispatch" => {
                set_first(&mut workstation_dispatch_flag, &value)
            }
            // wave-16 / task 04 — review-gate hint contract. `:review-gate`
            // is the gate kind (recognised: "none", "question-event");
            // unrecognised raw values still land on the typed slot AND
            // get recorded into `unsupported_fields` so the typo surfaces
            // through `node_hint_summary` while the scheduler safely
            // dispatches as if no gate was set.
            "review-gate" | "review_gate" => {
                let raw = value.trim();
                if !raw.is_empty() {
                    let lc = raw.to_ascii_lowercase();
                    if !matches!(lc.as_str(), "none" | "question-event" | "question_event") {
                        unsupported_fields.push((raw_key.clone(), value.clone()));
                    }
                }
                set_first(&mut review_gate, &value);
            }
            "review-action" | "review_action" => set_first(&mut review_action, &value),
            "review-text" | "review_text" => set_first(&mut review_text, &value),
            // wave-16 / task 05 — bounded per-node retry policy. Two
            // spellings, distinct semantics:
            //   `:retry-count N`   = N **additional** attempts beyond
            //                        the first (so total = N+1).
            //   `:max-attempts N`  = N **total** attempts including
            //                        the first (so retry_count = N-1).
            // Both lower into `retry_count` (additional retries) so the
            // runtime has a single source of truth; the parser
            // converts on the way in. First hint wins; later ones are
            // ignored so a duplicate doesn't silently shadow the author's
            // earlier declaration.
            //
            // Strict parsing: any non-numeric / negative value lands
            // in `retry_parse_error` and the validator raises a
            // structured `DagBuildError::InvalidRetryHint` BEFORE the
            // scheduler ever sees the node — silent fall-through to
            // "no retry" would lose the author's policy.
            "retry-count" | "retry_count" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            // Preserve the raw upper bound so callers
                            // can see what they declared; the cap is
                            // applied by `effective_max_attempts`.
                            retry_count = Some(n.min(u32::MAX as i64) as u32);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "max-attempts" | "max_attempts" => {
                if retry_count.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        // `:max-attempts 0` is meaningless (zero
                        // attempts = never run) — we reject it as a
                        // structured parse error so the author sees
                        // the typo instead of a silently-skipped node.
                        Ok(n) if n >= 1 => {
                            // Convert total attempts → additional
                            // retries. Subtract one then clamp to u32.
                            let extra = (n - 1).min(u32::MAX as i64) as u32;
                            retry_count = Some(extra);
                        }
                        Ok(_zero_or_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a positive integer (>= 1)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            "retry-delay-ms" | "retry_delay_ms" => {
                if retry_delay_ms.is_none() {
                    let trimmed = value.trim();
                    match trimmed.parse::<i64>() {
                        Ok(n) if n >= 0 => {
                            retry_delay_ms = Some(n as u64);
                        }
                        Ok(_neg) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                "value must be a non-negative integer (ms)".to_string(),
                            ));
                        }
                        Err(e) if retry_parse_error.is_none() => {
                            retry_parse_error = Some((
                                raw_key.clone(),
                                value.clone(),
                                format!("not a valid integer: {}", e),
                            ));
                        }
                        _ => { /* second error: keep the first */ }
                    }
                }
            }
            // wave-17 / task 04 — conservative rollback descriptor
            // contract. Strict parsing: unrecognised raw values land
            // BOTH on the typed slot AND in `unsupported_fields` so
            // the scheduler safely degrades to "no rollback" while
            // the typo surfaces through `node_hint_summary`.
            "rollback-policy" | "rollback_policy" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackPolicy::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_policy, &value);
            }
            "rollback-objective" | "rollback_objective" => {
                set_first(&mut rollback_objective, &value)
            }
            "rollback-owned-files" | "rollback_owned_files" => {
                set_first(&mut rollback_owned_files_raw, &value)
            }
            "rollback-acceptance-commands" | "rollback_acceptance_commands" => {
                set_first(&mut rollback_acceptance_commands_raw, &value)
            }
            // wave-18 / task 04 — cascade rollback hint contract.
            //
            // `:compensates "<failed-node-id>"` declares THIS node as a
            // candidate compensation step for the named failed node. The
            // cascade evaluator (which runs AFTER the named node's final
            // failed attempt) consumes the field; outside that flow it
            // is pure metadata.
            //
            // `:rollback-cascade "none|plan|dispatch-safe"` opts the
            // failed (cascade-root) node into the cascade evaluator.
            // Strict parsing: unrecognised raw values land BOTH on the
            // typed slot AND in `unsupported_fields` so the scheduler
            // safely degrades to "no cascade" while the typo surfaces
            // through `node_hint_summary`.
            //
            // `:rollback-after ["node-a" "node-b"]` is an additional
            // ordering hint for cascade compensation order. Same shape
            // as `:depends-on` (paren / bracket / bareword run); never
            // promoted to a real `:depends-on` so forward dispatch order
            // is unaffected.
            "compensates" => set_first(&mut compensates, &value),
            // wave-19 / task 10 — forward compensate ref. Two spellings,
            // identical semantics: `:compensate-node "<comp-id>"` /
            // `:compensate-ref "<comp-id>"` declared on the failing node
            // points AT the compensation node id. First hint wins; later
            // duplicates are ignored so a typo cannot silently shadow
            // the author's first declaration. Plan-level validation
            // (declared-id resolution, self-ref rejection, agreement
            // with reverse `:compensates`) runs in `build_validated_dag`.
            "compensate-node" | "compensate_node" | "compensate-ref" | "compensate_ref" => {
                set_first(&mut compensate_node, &value)
            }
            "rollback-cascade" | "rollback_cascade" => {
                let raw = value.trim();
                if !raw.is_empty() && RollbackCascadeMode::parse(raw).is_none() {
                    unsupported_fields.push((raw_key.clone(), value.clone()));
                }
                set_first(&mut rollback_cascade, &value);
            }
            "rollback-after" | "rollback_after" => {
                if rollback_after.is_empty() {
                    rollback_after = parse_id_list(&value);
                }
            }
            _ => {
                unsupported_fields.push((raw_key, value));
            }
        }
    }

    let id = id?;
    let target = target?;
    let policy = failure_policy.unwrap_or_else(|| FAILURE_POLICY_FAIL_FAST.to_string());
    let policy = match policy.as_str() {
        FAILURE_POLICY_FAIL_FAST | FAILURE_POLICY_CONTINUE => policy,
        _ => {
            // Unknown policy → record into unsupported_fields and fall back
            // to fail-fast (the safe default).
            unsupported_fields.push(("failure-policy".to_string(), policy));
            FAILURE_POLICY_FAIL_FAST.to_string()
        }
    };
    Some(DagNode {
        id,
        target,
        objective,
        depends_on,
        condition,
        failure_policy: policy,
        timeout_ms,
        dispatch_strategy,
        target_project,
        requested_cwd,
        flow_id,
        scope,
        commit_policy,
        owned_files_raw,
        forbidden_files_raw,
        acceptance_commands_raw,
        execution_order,
        parallel_group,
        atom_level,
        atom_task_id,
        predicted_tool_sequence_raw,
        context_sources_raw,
        acceptance_mode_raw,
        acceptance_evidence_keys_raw,
        acceptance_depends_on,
        acceptance_requires_raw,
        acceptance_source_node,
        workstation_dispatch_flag,
        review_gate,
        review_action,
        review_text,
        retry_count,
        retry_delay_ms,
        retry_parse_error,
        rollback_policy,
        rollback_objective,
        rollback_owned_files_raw,
        rollback_acceptance_commands_raw,
        compensates,
        compensate_node,
        rollback_cascade,
        rollback_after,
        unsupported_fields,
    })
}

fn set_first(slot: &mut Option<String>, value: &str) {
    if slot.is_none() {
        let v = value.trim();
        if !v.is_empty() {
            *slot = Some(v.to_string());
        }
    }
}

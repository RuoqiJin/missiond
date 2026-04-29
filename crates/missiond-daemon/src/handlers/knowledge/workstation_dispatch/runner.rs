use std::path::{Path, PathBuf};

use missiond_core::types::Plan;
use serde_json::json;

use crate::handlers::compute::task_delegate;
use crate::slot_orchestrator::project_root::resolve_target_project_root;
use crate::state::AppState;

use super::super::evidence_collector::{self, AppendOutcome, EventRef, EvidenceEntry};
use super::super::plan::tool_result_payload;
use super::descriptor::resolve_contract_path;
use super::{
    build_task_brief_with_source_and_trace, load_task_contract, truncate_brief_preview,
    SafeDescriptorReason, WorkstationDispatchHints, WorkstationDispatchOutcome,
    COMMIT_POLICY_SCOPED,
};

/// Top-level entry point invoked from `plan::action_execute_internal` and
/// `plan_dag::dispatch_node` when the caller / plan opted in.
///
/// `target` MUST be the resolved target string (already normalised by the
/// outer handler). When `target != "mission_task_delegate"` we return a
/// safe descriptor instead of dispatching.
///
/// `dispatch_strategy` is the already-normalised strategy from the outer
/// handler (one of `VALID_DISPATCH_STRATEGIES` in plan.rs, including
/// `unknown`). It controls only the agent-team hint injection.
///
/// Wave-19 / task 07 — preserved as-is for the no-contract dispatch path.
/// Delegates to [`run_workstation_dispatch_with_contract`] with
/// `task_contract_path = None`, so the legacy objective / owned-files
/// brief is built byte-identically. Future call sites that have a
/// task-contract v1 file on disk should call
/// `run_workstation_dispatch_with_contract` directly.
pub(crate) async fn run_workstation_dispatch(
    state: &AppState,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: WorkstationDispatchHints,
    dry_run: bool,
) -> WorkstationDispatchOutcome {
    run_workstation_dispatch_with_contract(
        state,
        plan,
        target,
        dispatch_strategy,
        hints,
        dry_run,
        None,
    )
    .await
}

/// Wave-19 / task 07 — contract-aware variant of
/// [`run_workstation_dispatch`].
///
/// Behaviour matrix:
///   * `task_contract_path = None`  → identical to wave-15/16/17. The
///     hints feed `build_task_brief` directly; no contract IO happens.
///   * `task_contract_path = Some`  → load + parse the file, overlay
///     contract fields onto the hints (contract is the SSOT — non-empty
///     contract fields beat caller args/hints), and prefix the brief
///     with a `## Source contract` block naming the on-disk file. The
///     scoped-commit handoff section (wave-17 / task 07) is preserved
///     verbatim because it lives in `build_task_brief_with_source` after
///     the optional preamble.
///
/// Failure semantics (contract path supplied):
///   * IO error / lex error / schema mismatch / missing required field
///     → `SafeDescriptor { reason: MalformedTaskContract { ... } }`.
///     We refuse to fall back to the legacy natural-language brief —
///     downgrading silently would defeat the whole point of having a
///     machine SSOT.
///
/// Path resolution: a relative `task_contract_path` is joined against
/// the resolved project root (NOT the daemon's process cwd). An
/// absolute path is taken verbatim.
pub(crate) async fn run_workstation_dispatch_with_contract(
    state: &AppState,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: WorkstationDispatchHints,
    dry_run: bool,
    task_contract_path: Option<&Path>,
) -> WorkstationDispatchOutcome {
    run_workstation_dispatch_with_contract_and_trace(
        state,
        plan,
        target,
        dispatch_strategy,
        hints,
        dry_run,
        task_contract_path,
        None,
    )
    .await
}

/// wave-23 / task 05 — variant of `run_workstation_dispatch_with_contract`
/// that also forwards a session-trace ledger path. The path is rendered
/// into the brief (under a `## Session trace` block) AND threaded into
/// the inner `mission_task_delegate` args under `session_trace_path` so
/// the worker can echo it back when calling
/// `mission_execution(action=open|preflight_commit|complete)`. Evidence
/// sidecar carries the same string under `session_trace_path` for audit.
///
/// Why a sibling function rather than a struct field on
/// `WorkstationDispatchHints`: extending the hint struct or the outcome
/// variants would break struct-literal initializers in plan_dag.rs and
/// unified_entry.rs (out-of-scope under this wave's contract).
/// Threading the path as a function parameter keeps the existing call
/// surface stable for those callers while letting the in-scope plan.rs
/// surface forward the field cleanly.
pub(crate) async fn run_workstation_dispatch_with_contract_and_trace(
    state: &AppState,
    plan: &Plan,
    target: &str,
    dispatch_strategy: &str,
    hints: WorkstationDispatchHints,
    dry_run: bool,
    task_contract_path: Option<&Path>,
    session_trace_path: Option<&str>,
) -> WorkstationDispatchOutcome {
    // 1. Refuse non-task_delegate targets up front (architecture rule).
    if target != "mission_task_delegate" {
        return WorkstationDispatchOutcome::SafeDescriptor {
            reason: SafeDescriptorReason::UnsupportedTarget(target.to_string()),
            task_brief: None,
        };
    }

    // 2. Hint-only safety: a content-free brief is useless. Refuse rather
    //    than dispatch a placeholder objective.
    //
    //    wave-19 / task 07 — when a contract file is pinned, defer this
    //    check: the contract's `:goal` field will populate
    //    `hints.objective` during overlay (step 3.5). The post-overlay
    //    re-check enforces the same invariant for contract-driven
    //    dispatches and keeps the failure mode identical
    //    (`SafeDescriptorReason::MissingObjective`).
    if task_contract_path.is_none() {
        let has_meaningful_objective = hints
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !has_meaningful_objective {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::MissingObjective,
                task_brief: None,
            };
        }
    }

    // 3. Project-root resolution. `cwd` MUST be absolute when supplied —
    //    `resolve_target_project_root` enforces that contract; we surface
    //    the error string verbatim so the caller can fix and retry.
    //    Owned strings here so the later `hints` move into `cap_lists`
    //    doesn't conflict with the borrows feeding the resolver.
    let project_arg_owned: Option<String> = hints.target_project.clone();
    let cwd_arg_owned: Option<String> = hints.requested_cwd.clone();
    if let Some(cwd) = cwd_arg_owned.as_deref() {
        if !Path::new(cwd).is_absolute() {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::ProjectRootUnresolved(format!(
                    "requested_cwd `{}` is not absolute; \
                     workstation-dispatch never joins a relative cwd against the daemon process cwd",
                    cwd
                )),
                task_brief: None,
            };
        }
    }
    let resolution = resolve_target_project_root(
        project_arg_owned.as_deref(),
        cwd_arg_owned.as_deref().map(Path::new),
        project_arg_owned.as_deref(),
        &state.project_registry,
    )
    .await;
    let resolution = match resolution {
        Ok(r) => r,
        Err(e) => {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::ProjectRootUnresolved(e.to_string()),
                task_brief: None,
            };
        }
    };

    // 3.5 wave-19 / task 07 — when a task-contract v1 file is pinned,
    //     load + parse it and overlay onto the hints. The contract is
    //     the SSOT, so non-empty contract fields beat caller args. A
    //     parse failure refuses the dispatch with a typed safe descriptor
    //     rather than silently downgrading to the legacy brief — keeping
    //     a malformed contract from masquerading as a working brief is
    //     the whole point of this layer.
    let mut hints = hints;
    let mut contract_source_path: Option<PathBuf> = None;
    let mut contract_dispatch_strategy: Option<String> = None;
    let mut contract_session_trace_path: Option<String> = None;
    if let Some(raw_path) = task_contract_path {
        let resolved_path = resolve_contract_path(raw_path, &resolution.project_root);
        match load_task_contract(&resolved_path) {
            Ok(contract) => {
                contract_session_trace_path = contract
                    .session_trace_path
                    .as_deref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                hints.overlay_contract(&contract);
                contract_dispatch_strategy = contract.dispatch_strategy.clone();
                contract_source_path = Some(resolved_path);
            }
            Err(err) => {
                return WorkstationDispatchOutcome::SafeDescriptor {
                    reason: SafeDescriptorReason::MalformedTaskContract {
                        path: resolved_path.display().to_string(),
                        reason: err.reason(),
                    },
                    task_brief: None,
                };
            }
        }
        // Defence in depth: re-check the post-overlay objective. The
        // pure parser already rejects empty `:goal`, so this cannot
        // fire today — but if a future overlay rule loosens, the
        // dispatch refuses rather than silently shipping a content-free
        // brief.
        let still_has_objective = hints
            .objective
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !still_has_objective {
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::MissingObjective,
                task_brief: None,
            };
        }
    }

    // wave-23 / task 05 — resolve the session-trace path priority:
    //   caller param > contract `:session-trace-path` overlay
    // (the contract is the SSOT only when no explicit caller param was
    //  supplied; explicit caller wins because the caller may legitimately
    //  redirect a contract-default ledger to a wave-specific override).
    let resolved_trace_path: Option<String> = session_trace_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or(contract_session_trace_path);

    // 4. Build the brief. Hints have already been arg-merged + (when a
    //    contract was supplied) overlaid with the contract; cap lists
    //    here so the brief stays under the 16K objective limit.
    let _capped = hints.cap_lists();
    // Strategy precedence: when the contract pins `:dispatch-strategy`,
    // it overrides the caller-passed `dispatch_strategy` for brief
    // rendering ONLY (the response-facing `dispatch_strategy` keeps the
    // resolver's value so observers can see what was requested vs what
    // the contract enforced).
    let brief_dispatch_strategy: &str = contract_dispatch_strategy
        .as_deref()
        .unwrap_or(dispatch_strategy);
    let brief = build_task_brief_with_source_and_trace(
        plan,
        &hints,
        brief_dispatch_strategy,
        contract_source_path.as_deref(),
        resolved_trace_path.as_deref(),
    );

    // 5. dry_run: stop here, no dispatch, no evidence.
    if dry_run {
        return WorkstationDispatchOutcome::DryRun { task_brief: brief };
    }

    // 6. Dispatch through the existing mission_task_delegate substrate.
    //    cwd is the resolved canonical project root (downstream resolves
    //    the same way; we forward it explicitly so the inner handler does
    //    not have to re-resolve when the caller only supplied a project
    //    id).
    let mut inner_args = json!({
        "objective": brief,
        "intent": "code",
        "context_hints": [
            format!("plan:{}", plan.id),
            format!("board_task:{}", plan.board_task_id),
            format!("workstation_dispatch:v0"),
        ],
        "cwd": resolution.project_root.to_string_lossy().to_string(),
    });
    if let Some(ds) = hints.dispatch_strategy.as_deref() {
        inner_args["dispatch_strategy"] = json!(ds);
    } else {
        inner_args["dispatch_strategy"] = json!(dispatch_strategy);
    }
    // wave-23 / task 05 — forward the resolved session-trace ledger path
    // into the inner substrate so the worker can echo it back when
    // calling `mission_execution(action=*)`. The brief already nudges the
    // worker to do so; passing it here gives downstream substrates that
    // honour `session_trace_path` (today only the brief; tomorrow any
    // task_delegate substrate that proxies to mission_execution) an
    // implicit signal. Path priority: caller-supplied param > contract
    // `:session-trace-path` overlay (resolved earlier into
    // `resolved_trace_path`).
    if let Some(stp) = resolved_trace_path.as_deref() {
        inner_args["session_trace_path"] = json!(stp);
    }

    let inner_result = match task_delegate::handle(state, "mission_task_delegate", inner_args).await
    {
        Ok(r) => r,
        Err(err) => {
            // Hard error from the inner handler (panic-equivalent) —
            // surface it as a safe descriptor so the caller can route.
            return WorkstationDispatchOutcome::SafeDescriptor {
                reason: SafeDescriptorReason::ProjectRootUnresolved(format!(
                    "mission_task_delegate handler raised: {}",
                    err
                )),
                task_brief: Some(brief),
            };
        }
    };
    let inner_payload = tool_result_payload(&inner_result);
    let inner_is_error = inner_result.is_error.unwrap_or(false);
    if inner_is_error {
        return WorkstationDispatchOutcome::InnerError {
            task_brief: brief,
            inner_payload,
        };
    }

    // 7. Evidence sidecar — typed entry, source=`workstation_dispatch`.
    let mut entry = EvidenceEntry::new(
        evidence_collector::source::WORKSTATION_DISPATCH,
        evidence_collector::kind::DISPATCH,
    )
    .with_inner_dispatch(inner_payload.clone())
    .add_execution_event(EventRef::unavailable(
        "workstation-dispatch v0 wraps mission_task_delegate; live event correlation \
         is the inner handler's responsibility — bus subscription is a future task",
    ))
    .with_extra("dispatch_strategy", json!(dispatch_strategy))
    .with_extra(
        "commit_policy",
        json!(hints
            .commit_policy
            .as_deref()
            .unwrap_or(COMMIT_POLICY_SCOPED)),
    )
    .with_extra("project_id", json!(resolution.project_id))
    .with_extra(
        "project_root",
        json!(resolution.project_root.to_string_lossy().to_string()),
    )
    .with_extra("owned_files", json!(hints.owned_files.clone()))
    .with_extra("forbidden_files", json!(hints.forbidden_files.clone()))
    .with_extra(
        "acceptance_commands",
        json!(hints.acceptance_commands.clone()),
    )
    .with_extra("task_brief_preview", json!(truncate_brief_preview(&brief)));
    // wave-19 / task 07 — when the dispatch flowed through a task-contract
    // v1 file, surface the source path on the evidence ledger so observers
    // can correlate the brief preview against the on-disk SSOT. Absent
    // this annotation an audit could mistake a contract-flavoured brief
    // for a legacy natural-language brief; the field disambiguates.
    if let Some(p) = contract_source_path.as_deref() {
        entry = entry.with_extra("task_contract_source_path", json!(p.display().to_string()));
    }
    if let Some(eds) = contract_dispatch_strategy.as_deref() {
        entry = entry.with_extra("contract_dispatch_strategy", json!(eds));
    }
    // wave-23 / task 05 — when the dispatch carried a session-trace
    // ledger path, surface it on the evidence ledger so observers can
    // pivot on the same string the brief and inner args carry.
    if let Some(stp) = resolved_trace_path.as_deref() {
        entry = entry.with_extra("session_trace_path", json!(stp));
    }
    let outcome = evidence_collector::append(
        state,
        plan.id,
        project_arg_owned.as_deref(),
        cwd_arg_owned.as_deref(),
        hints.target_project.as_deref(),
        entry,
    )
    .await;
    if let AppendOutcome::Failed { error } = &outcome {
        tracing::warn!(
            plan_id = %plan.id,
            error = %error,
            "workstation_dispatch: evidence sidecar append failed"
        );
    }
    let (evidence_path, evidence_error) = outcome.into_legacy_tuple();

    WorkstationDispatchOutcome::Dispatched {
        task_brief: brief,
        // task_brief_path: future enhancement — wave-15 v0 keeps the brief
        // inline on the response. None signals the file-mirror is not yet
        // wired so callers know to read `task_brief_preview` instead.
        task_brief_path: None,
        // wave-20 / task 04 — surface the on-disk task-contract source
        // path on the response when the dispatch consumed the contract
        // directly (machine-driven mode). The legacy / rendered path
        // leaves it `None` so the wire shape stays byte-compatible with
        // wave-15..19 callers that only watch for `task_brief_preview`.
        task_contract_source_path: contract_source_path
            .as_deref()
            .map(|p| p.display().to_string()),
        evidence_path,
        evidence_error,
        inner_payload,
    }
}

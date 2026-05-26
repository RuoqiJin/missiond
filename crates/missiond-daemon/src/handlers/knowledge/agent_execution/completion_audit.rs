use anyhow::Result;
use missiond_core::event::events::ExecutionEvent;
use missiond_mcp::tools::ToolResult;
use serde_json::Value;

use crate::engine::task_completion_evidence::{
    TaskCompletionEvidenceInput, TaskCompletionEvidenceWriter,
};
use crate::state::AppState;

use super::completion_entry::{render_completion_entry, CompletionEntryFields};
use super::completion_gates::{enforce_scoped_commit_completion, enforce_task_contract_completion};
use super::completion_inputs::{parse_completion_request, CompletionRequest};
use super::completion_response::{build_completion_response, CompletionResponseFields};
use super::completion_trace::append_completion_trace_if_requested;
use super::completion_verification::evaluate_completion_verification;
use super::log_counters::{allocate_id, Counter};
use super::log_dispatch::read_dispatch_metadata_from_log;
use super::log_store::{
    append_to_block, companion_path, now_iso, project_or_target_project, read_log_file,
    resolve_project_root, touch_last_updated, write_log_file,
};
use super::log_surface::emit_execution_event;

pub(super) async fn action_complete(state: &AppState, args: &Value) -> Result<ToolResult> {
    let request = match parse_completion_request(args) {
        Ok(r) => r,
        Err(r) => return Ok(r),
    };
    let CompletionRequest {
        execution_id,
        phase,
        agent,
        summary,
        deliverables,
        verification,
        changed_files,
        staged_files,
        commit_hash,
        commit_status,
        commit_blocker,
        task_contract_path,
        task_report_path,
        verifier_status,
        verifier_notes,
        task_run_verifier_status,
        shared_memory_path,
        verifier_diagnostics,
        verified_flag,
        enforce_scoped_commit,
    } = request;

    let root = resolve_project_root(state, project_or_target_project(args)).await?;
    let path = companion_path(&root, execution_id);
    let mut file = read_log_file(&path)?;

    // Run the enforcement gate BEFORE `allocate_id` mutates the
    // id-counters block — a rejected completion must not bump the
    // counter or otherwise change the durable file.
    let scoped_commit_validation = if enforce_scoped_commit {
        match enforce_scoped_commit_completion(
            &file,
            staged_files.as_deref(),
            commit_hash.as_deref(),
            commit_status.as_deref(),
            commit_blocker.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    // wave-19 / task 08 — contract-level enforcement gate. Runs only
    // when the caller paired `enforce_scoped_commit=true` with a
    // `task_contract_path`; otherwise the contract metadata is recorded
    // verbatim with no additional checks (legacy / opt-out behaviour).
    // Daemon never shells out — we read the file off disk and use the
    // workstation_dispatch parser to project the narrow view we need.
    let task_contract_validation = if enforce_scoped_commit && task_contract_path.is_some() {
        let path_arg = task_contract_path.as_deref().unwrap();
        match enforce_task_contract_completion(
            &file,
            &root,
            path_arg,
            commit_hash.as_deref(),
            staged_files.as_deref(),
        ) {
            Ok(v) => Some(v),
            Err(err) => return Ok(err),
        }
    } else {
        None
    };

    let verification_outcome = match evaluate_completion_verification(
        &root,
        task_contract_path.as_deref(),
        task_report_path.as_deref(),
        shared_memory_path.as_deref(),
        commit_hash.as_deref(),
        verified_flag,
    ) {
        Ok(outcome) => outcome,
        Err(err) => return Ok(err),
    };
    let id = allocate_id(&mut file, Counter::Completion)?;
    let date = now_iso();

    let entry = render_completion_entry(CompletionEntryFields {
        id: &id,
        phase,
        agent,
        summary,
        deliverables,
        verification,
        date: &date,
        changed_files: changed_files.as_deref(),
        staged_files: staged_files.as_deref(),
        commit_hash: commit_hash.as_deref(),
        commit_status: commit_status.as_deref(),
        commit_blocker: commit_blocker.as_deref(),
        task_contract_path: task_contract_path.as_deref(),
        task_report_path: task_report_path.as_deref(),
        verifier_status: verifier_status.as_deref(),
        verifier_notes: verifier_notes.as_deref(),
        task_run_verifier_status: task_run_verifier_status.as_deref(),
        shared_memory_path: shared_memory_path.as_deref(),
        verifier_diagnostics: verifier_diagnostics.as_deref(),
        verified: verified_flag,
    });

    append_to_block(&mut file, "completions", &entry)?;
    touch_last_updated(&mut file)?;
    write_log_file(&path, &file)?;

    // Same dispatch-metadata projection rationale as `action_claim` —
    // surface the trio from the companion-log meta block so completion
    // consumers can route on workstation-dispatch context without reading
    // the on-disk file. Absent / legacy meta cleanly skip-serializes
    // (see ExecutionEvent::Completed doc comment).
    let meta = read_dispatch_metadata_from_log(&file);
    emit_execution_event(
        state,
        ExecutionEvent::Completed {
            execution_id: execution_id.to_string(),
            completion_id: id.clone(),
            phase: phase.to_string(),
            agent: agent.to_string(),
            at: date.clone(),
            dispatch_strategy: meta.dispatch_strategy,
            target_project: meta.target_project,
            requested_cwd: meta.requested_cwd,
        },
    )
    .await;

    let mut response = build_completion_response(CompletionResponseFields {
        completion_id: &id,
        phase,
        agent,
        date: &date,
        scoped_commit_enforced: enforce_scoped_commit,
        changed_files: changed_files.as_deref(),
        staged_files: staged_files.as_deref(),
        commit_hash: commit_hash.as_deref(),
        commit_status: commit_status.as_deref(),
        commit_blocker: commit_blocker.as_deref(),
        scoped_commit_validation: scoped_commit_validation.as_ref(),
        task_contract_path: task_contract_path.as_deref(),
        task_report_path: task_report_path.as_deref(),
        verifier_status: verifier_status.as_deref(),
        verifier_notes: verifier_notes.as_deref(),
        task_contract_validation: task_contract_validation.as_ref(),
        task_run_verifier_status: task_run_verifier_status.as_deref(),
        shared_memory_path: shared_memory_path.as_deref(),
        verifier_diagnostics: verifier_diagnostics.as_deref(),
        verified: verified_flag,
        verification_outcome: &verification_outcome,
    });

    append_completion_trace_if_requested(
        args,
        &root,
        execution_id,
        phase,
        agent,
        &id,
        &mut response,
    );

    if let Some(task_id) = args
        .get("task_id")
        .or_else(|| args.get("taskId"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let result_status = if matches!(
            task_run_verifier_status
                .as_deref()
                .or(verifier_status.as_deref()),
            Some("fail" | "failed" | "blocked" | "skipped")
        ) {
            task_run_verifier_status
                .as_deref()
                .or(verifier_status.as_deref())
                .unwrap_or("failed")
                .to_string()
        } else {
            "completed".to_string()
        };
        let writer = TaskCompletionEvidenceWriter::new(state.storage().shared_memory.clone());
        let artifact = writer
            .write_bounded(TaskCompletionEvidenceInput {
                task_id: task_id.to_string(),
                project_id: project_or_target_project(args).map(str::to_string),
                slot_id: None,
                conversation_id: Some(execution_id.to_string()),
                provider: agent.to_string(),
                result_status,
                summary: summary.to_string(),
                content: Some(format!(
                    "summary:\n{}\n\ndeliverables:\n{}\n\nverification:\n{}",
                    summary, deliverables, verification
                )),
                json: response.clone(),
                accepted_shard_id: args
                    .get("accepted_shard_id")
                    .or_else(|| args.get("acceptedShardId"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
            .await;
        match artifact {
            Ok(result) => {
                response["task_result_artifact_hash"] = Value::String(result.artifact_hash);
                response["task_result_artifact"] = result.response;
            }
            Err(err) => {
                response["task_result_artifact_error"] = Value::String(err.to_string());
            }
        }
    }

    Ok(ToolResult::json_pretty(&response))
}

use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use std::path::Path;

use super::task_verifier_auto::auto_run_task_run_verifier;

pub(super) struct CompletionVerificationOutcome {
    pub(super) verification_source: Option<&'static str>,
    pub(super) auto_verifier_summary: Option<Value>,
    pub(super) auto_verifier_status: Option<&'static str>,
    pub(super) auto_verifier_diagnostics: Option<String>,
}

impl CompletionVerificationOutcome {
    pub(super) fn none() -> Self {
        Self {
            verification_source: None,
            auto_verifier_summary: None,
            auto_verifier_status: None,
            auto_verifier_diagnostics: None,
        }
    }
}

/// wave-22 / task 02 — auto task-run verifier dispatch.
///
/// The wave21-03 caller-supplied `verified=true` escape hatch is now a
/// legacy-compat fallback. The new contract: when the writer hands every path
/// the daemon needs for an end-to-end proof (`task_contract_path`,
/// `task_report_path`, `shared_memory_path`, `commit_hash`) the daemon runs the
/// in-tree task-run verifier itself. If the caller only supplies `verified=true`
/// with missing proof paths, MissionD records the legacy claim but marks the
/// verifier source and diagnostics so reviewers can migrate the caller.
pub(super) fn evaluate_completion_verification(
    root: &Path,
    task_contract_path: Option<&str>,
    task_report_path: Option<&str>,
    shared_memory_path: Option<&str>,
    commit_hash: Option<&str>,
    verified_flag: Option<bool>,
) -> std::result::Result<CompletionVerificationOutcome, ToolResult> {
    let auto_verifier_inputs_present = task_contract_path.is_some()
        && task_report_path.is_some()
        && shared_memory_path.is_some()
        && commit_hash.is_some();

    if auto_verifier_inputs_present {
        // unwraps are safe — we just checked all four are Some.
        let tcp = task_contract_path.unwrap();
        let trp = task_report_path.unwrap();
        let smp = shared_memory_path.unwrap();
        let hash = commit_hash.unwrap();
        return match auto_run_task_run_verifier(root, tcp, trp, smp, hash) {
            Ok(summary) => Ok(CompletionVerificationOutcome {
                verification_source: Some("daemon-auto-verifier"),
                auto_verifier_summary: Some(summary),
                auto_verifier_status: Some("passed"),
                auto_verifier_diagnostics: None,
            }),
            Err(err) => Err(err),
        };
    }

    if verified_flag == Some(true) {
        // Legacy caller-supplied claim. Record it but flag in the
        // diagnostic which path was missing so the writer agent can
        // upgrade the next dispatch.
        let mut missing: Vec<&'static str> = Vec::new();
        if task_contract_path.is_none() {
            missing.push("task_contract_path");
        }
        if task_report_path.is_none() {
            missing.push("task_report_path");
        }
        if shared_memory_path.is_none() {
            missing.push("shared_memory_path");
        }
        if commit_hash.is_none() {
            missing.push("commit_hash");
        }
        return Ok(CompletionVerificationOutcome {
            verification_source: Some("legacy-caller-claim"),
            auto_verifier_summary: None,
            auto_verifier_status: Some("unknown"),
            auto_verifier_diagnostics: Some(format!(
                "verified=true accepted as legacy_verified_claim because the daemon-side auto-verifier requires all four of [task_contract_path, task_report_path, shared_memory_path, commit_hash]; missing: {:?}. Migrate the dispatch envelope to supply every path so the daemon can compute the verdict itself (wave22-02).",
                missing,
            )),
        });
    }

    Ok(CompletionVerificationOutcome::none())
}

use serde_json::{json, Value};

use super::completion_verification::CompletionVerificationOutcome;

pub(super) struct CompletionResponseFields<'a> {
    pub(super) completion_id: &'a str,
    pub(super) phase: &'a str,
    pub(super) agent: &'a str,
    pub(super) date: &'a str,
    pub(super) scoped_commit_enforced: bool,
    pub(super) changed_files: Option<&'a [String]>,
    pub(super) staged_files: Option<&'a [String]>,
    pub(super) commit_hash: Option<&'a str>,
    pub(super) commit_status: Option<&'a str>,
    pub(super) commit_blocker: Option<&'a str>,
    pub(super) scoped_commit_validation: Option<&'a Value>,
    pub(super) task_contract_path: Option<&'a str>,
    pub(super) task_report_path: Option<&'a str>,
    pub(super) verifier_status: Option<&'a str>,
    pub(super) verifier_notes: Option<&'a str>,
    pub(super) task_contract_validation: Option<&'a Value>,
    pub(super) task_run_verifier_status: Option<&'a str>,
    pub(super) shared_memory_path: Option<&'a str>,
    pub(super) verifier_diagnostics: Option<&'a str>,
    pub(super) verified: Option<bool>,
    pub(super) verification_outcome: &'a CompletionVerificationOutcome,
}

pub(super) fn build_completion_response(fields: CompletionResponseFields<'_>) -> Value {
    let mut response = json!({
        "status": "recorded",
        "completion_id": fields.completion_id,
        "phase": fields.phase,
        "agent": fields.agent,
        "at": fields.date,
        "scoped_commit_enforced": fields.scoped_commit_enforced,
    });
    if let Some(list) = fields.changed_files {
        response["changed_files"] = json!(list);
    }
    if let Some(list) = fields.staged_files {
        response["staged_files"] = json!(list);
    }
    if let Some(hash) = fields.commit_hash {
        response["commit_hash"] = json!(hash);
    }
    if let Some(status_val) = fields.commit_status {
        response["commit_status"] = json!(status_val);
    }
    if let Some(blocker) = fields.commit_blocker {
        response["commit_blocker"] = json!(blocker);
    }
    if let Some(v) = fields.scoped_commit_validation {
        response["scoped_commit_validation"] = v.clone();
    }
    if let Some(tcp) = fields.task_contract_path {
        response["task_contract_path"] = json!(tcp);
    }
    if let Some(trp) = fields.task_report_path {
        response["task_report_path"] = json!(trp);
    }
    if let Some(vs) = fields.verifier_status {
        response["verifier_status"] = json!(vs);
    }
    if let Some(vn) = fields.verifier_notes {
        response["verifier_notes"] = json!(vn);
    }
    if let Some(v) = fields.task_contract_validation {
        response["task_contract_validation"] = v.clone();
    }
    if let Some(trvs) = fields.task_run_verifier_status {
        response["task_run_verifier_status"] = json!(trvs);
    }
    if let Some(smp) = fields.shared_memory_path {
        response["shared_memory_path"] = json!(smp);
    }
    if let Some(vd) = fields.verifier_diagnostics {
        response["verifier_diagnostics"] = json!(vd);
    }
    if let Some(v) = fields.verified {
        response["verified"] = json!(v);
    }

    // Daemon-computed verifier outcome wins over caller-supplied
    // verifier_status / verifier_diagnostics, preserving the wave22
    // response contract while legacy callers keep the older shape.
    if let Some(src) = fields.verification_outcome.verification_source {
        response["verification_source"] = json!(src);
    }
    if let Some(status) = fields.verification_outcome.auto_verifier_status {
        response["verifier_status"] = json!(status);
    }
    if let Some(diag) = fields
        .verification_outcome
        .auto_verifier_diagnostics
        .as_ref()
    {
        response["verifier_diagnostics"] = json!(diag);
    }
    if let Some(scope_summary) = fields.verification_outcome.auto_verifier_summary.as_ref() {
        response["verified_scope_summary"] = scope_summary.clone();
        response["verified_validation"] = scope_summary.clone();
    }

    response
}

use super::completion_fields::render_string_list;
use super::log_store::lisp_quote_string;

pub(super) struct CompletionEntryFields<'a> {
    pub(super) id: &'a str,
    pub(super) phase: &'a str,
    pub(super) agent: &'a str,
    pub(super) summary: &'a str,
    pub(super) deliverables: &'a str,
    pub(super) verification: &'a str,
    pub(super) date: &'a str,
    pub(super) changed_files: Option<&'a [String]>,
    pub(super) staged_files: Option<&'a [String]>,
    pub(super) commit_hash: Option<&'a str>,
    pub(super) commit_status: Option<&'a str>,
    pub(super) commit_blocker: Option<&'a str>,
    pub(super) task_contract_path: Option<&'a str>,
    pub(super) task_report_path: Option<&'a str>,
    pub(super) verifier_status: Option<&'a str>,
    pub(super) verifier_notes: Option<&'a str>,
    pub(super) task_run_verifier_status: Option<&'a str>,
    pub(super) shared_memory_path: Option<&'a str>,
    pub(super) verifier_diagnostics: Option<&'a str>,
    pub(super) verified: Option<bool>,
}

pub(super) fn render_completion_entry(fields: CompletionEntryFields<'_>) -> String {
    // Keep the legacy 6-field shape byte-identical; every newer durability
    // handoff field is projected only when the caller supplied it.
    let mut entry = format!(
        "    ({id}\n      :phase {phase}\n      :agent {agent}\n      :summary {summary}\n      :deliverables {deliverables}\n      :verification {verification}\n      :at {date}",
        id = fields.id,
        phase = lisp_quote_string(fields.phase),
        agent = lisp_quote_string(fields.agent),
        summary = lisp_quote_string(fields.summary),
        deliverables = lisp_quote_string(fields.deliverables),
        verification = lisp_quote_string(fields.verification),
        date = lisp_quote_string(fields.date),
    );
    if let Some(list) = fields.changed_files {
        entry.push_str(&format!(
            "\n      :changed-files {}",
            render_string_list(list)
        ));
    }
    if let Some(list) = fields.staged_files {
        entry.push_str(&format!(
            "\n      :staged-files {}",
            render_string_list(list)
        ));
    }
    if let Some(hash) = fields.commit_hash {
        entry.push_str(&format!("\n      :commit-hash {}", lisp_quote_string(hash)));
    }
    if let Some(status_val) = fields.commit_status {
        entry.push_str(&format!(
            "\n      :commit-status {}",
            lisp_quote_string(status_val)
        ));
    }
    if let Some(blocker) = fields.commit_blocker {
        entry.push_str(&format!(
            "\n      :commit-blocker {}",
            lisp_quote_string(blocker)
        ));
    }
    if let Some(tcp) = fields.task_contract_path {
        entry.push_str(&format!(
            "\n      :task-contract-path {}",
            lisp_quote_string(tcp)
        ));
    }
    if let Some(trp) = fields.task_report_path {
        entry.push_str(&format!(
            "\n      :task-report-path {}",
            lisp_quote_string(trp)
        ));
    }
    if let Some(vs) = fields.verifier_status {
        entry.push_str(&format!(
            "\n      :verifier-status {}",
            lisp_quote_string(vs)
        ));
    }
    if let Some(vn) = fields.verifier_notes {
        entry.push_str(&format!(
            "\n      :verifier-notes {}",
            lisp_quote_string(vn)
        ));
    }
    if let Some(trvs) = fields.task_run_verifier_status {
        entry.push_str(&format!(
            "\n      :task-run-verifier-status {}",
            lisp_quote_string(trvs)
        ));
    }
    if let Some(smp) = fields.shared_memory_path {
        entry.push_str(&format!(
            "\n      :shared-memory-path {}",
            lisp_quote_string(smp)
        ));
    }
    if let Some(vd) = fields.verifier_diagnostics {
        entry.push_str(&format!(
            "\n      :verifier-diagnostics {}",
            lisp_quote_string(vd)
        ));
    }
    if let Some(v) = fields.verified {
        entry.push_str(&format!("\n      :verified {}", v));
    }
    entry.push(')');
    entry
}

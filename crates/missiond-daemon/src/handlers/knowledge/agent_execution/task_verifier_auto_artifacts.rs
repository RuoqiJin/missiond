use missiond_mcp::tools::{ToolError, ToolResult};
use std::path::{Path, PathBuf};

pub(super) fn resolve_verifier_artifact_path(project_root: &Path, artifact_path: &str) -> PathBuf {
    let raw = Path::new(artifact_path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        project_root.join(raw)
    }
}

pub(super) fn read_task_contract_artifact(path: &Path) -> Result<String, ToolResult> {
    // The loaded contract value itself is unused by the auto verifier; the
    // load call is intentional because workstation_dispatch owns the task
    // contract schema vocabulary and error classes.
    let _contract = match super::super::workstation_dispatch::load_task_contract(path) {
        Ok(c) => c,
        Err(e) => {
            use super::super::workstation_dispatch::TaskContractParseError as Tce;
            let (code, message) = match &e {
                Tce::Io(detail) => (
                    "TASK_CONTRACT_REQUIRED",
                    format!(
                        "task_contract_path `{}` is not readable: {}",
                        path.display(),
                        detail
                    ),
                ),
                _ => (
                    "TASK_CONTRACT_MALFORMED",
                    format!(
                        "task_contract_path `{}` failed schema parse: {}",
                        path.display(),
                        e.reason()
                    ),
                ),
            };
            return Err(ToolResult::structured_error(
                ToolError::new(code, message).with_suggestion(
                    "ensure the path resolves under the project root and the file is a valid `missiond.task-contract.v1` Lisp form",
                ),
            ));
        }
    };

    std::fs::read_to_string(path).map_err(|e| {
        ToolResult::structured_error(ToolError::new(
            "TASK_CONTRACT_REQUIRED",
            format!(
                "task_contract_path `{}` became unreadable mid-verification: {}",
                path.display(),
                e
            ),
        ))
    })
}

pub(super) fn read_report_artifact(path: &Path) -> Result<String, ToolResult> {
    std::fs::read_to_string(path).map_err(|e| {
        ToolResult::structured_error(
            ToolError::new(
                "TASK_REPORT_REQUIRED",
                format!(
                    "task_report_path `{}` is not readable: {}",
                    path.display(),
                    e
                ),
            )
            .with_suggestion(
                "ensure the path resolves under the project root and the writer wrote the report-contract v1 file",
            ),
        )
    })
}

pub(super) fn read_shared_memory_artifact(path: &Path) -> Result<String, ToolResult> {
    std::fs::read_to_string(path).map_err(|e| {
        ToolResult::structured_error(
            ToolError::new(
                "SHARED_MEMORY_REQUIRED",
                format!(
                    "shared_memory_path `{}` is not readable: {}",
                    path.display(),
                    e
                ),
            )
            .with_suggestion(
                "ensure the path resolves under the project root and the wave shared-memory ledger exists",
            ),
        )
    })
}

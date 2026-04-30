use missiond_mcp::tools::{error_codes, ToolError, ToolResult};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn resolve_preflight_inspect_dir(
    root: &Path,
    args: &Value,
) -> std::result::Result<PathBuf, ToolResult> {
    let cwd_arg = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    match cwd_arg {
        Some(cwd) => {
            let candidate = PathBuf::from(cwd);
            let abs = if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            };
            let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            let canon_abs = match abs.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return Err(ToolResult::structured_error(ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!("cwd `{}` does not exist or is not accessible: {}", cwd, e),
                    )));
                }
            };
            if !canon_abs.starts_with(&canon_root) {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        format!(
                            "cwd `{}` resolves outside the project root `{}`",
                            cwd,
                            root.display()
                        ),
                    )
                    .with_suggestion("supply a path inside the project, or omit `cwd`"),
                ));
            }
            Ok(canon_abs)
        }
        None => Ok(root.to_path_buf()),
    }
}

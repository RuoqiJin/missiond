use super::*;
use crate::handlers::knowledge::workstation_dispatch;

// ───────────────────────────────────────────────────────────────────────
// wave-19 / task 06 — plan-runner task-contract emitter v0
//
// Opt-in: callers pass `emit_task_contract=true` (or
// `task_contract_mode="emit"|"emit_dry_run"`). When the resolved
// dispatch shape is workstation-eligible (target=mission_task_delegate
// + non-empty objective), the runner writes a task-contract v1 Lisp
// sidecar BEFORE handing off to the workstation-dispatch substrate.
// Path convention:
//
//   <project_root>/.missiond/tasks/generated/<plan_id>/<node_id>.lisp
//
// (Single-node executes pass `node_id="root"`; DAG nodes use the
// PLAN.lisp `:id`.)
//
// Default behaviour is byte-compatible: when neither knob is set the
// runner skips emission entirely and the response payload omits the
// task-contract fields. The renderer is OUT OF PROCESS — we surface
// `render_command` (a `node scripts/render-claudecode-task.mjs ...`
// invocation) instead of shelling out from Rust, which keeps the
// daemon free of Node dependency.
//
// Failure semantics: emit BEFORE dispatch. If the write fails, the
// dispatch is REFUSED with a structured `task_contract_emit_failed`
// status — the contract is the SSOT, so a missing contract MUST NOT
// be papered over by a successful inner call. Dry-run (`emit_dry_run`
// or the existing `dry_run=true` flag) returns the contract path
// without touching the inner substrate.
// ───────────────────────────────────────────────────────────────────────

/// Canonical task-contract emit modes. Mirrors the MCP descriptor enum.
pub(crate) const TASK_CONTRACT_MODE_OFF: &str = "off";
pub(crate) const TASK_CONTRACT_MODE_EMIT: &str = "emit";
pub(crate) const TASK_CONTRACT_MODE_EMIT_DRY_RUN: &str = "emit_dry_run";

/// Resolved emission policy after parsing the `emit_task_contract` /
/// `task_contract_mode` args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskContractEmitMode {
    /// Default — no emission, byte-compatible with pre-wave19 behaviour.
    Off,
    /// Emit the contract AND let the dispatch proceed.
    Emit,
    /// Emit the contract but skip the actual inner dispatch (preview).
    EmitDryRun,
}

impl TaskContractEmitMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TaskContractEmitMode::Off => TASK_CONTRACT_MODE_OFF,
            TaskContractEmitMode::Emit => TASK_CONTRACT_MODE_EMIT,
            TaskContractEmitMode::EmitDryRun => TASK_CONTRACT_MODE_EMIT_DRY_RUN,
        }
    }

    pub(crate) fn is_enabled(self) -> bool {
        !matches!(self, TaskContractEmitMode::Off)
    }

    pub(crate) fn is_dry_run(self) -> bool {
        matches!(self, TaskContractEmitMode::EmitDryRun)
    }
}

/// Parse the opt-in args. Precedence:
///   1. `task_contract_mode` (string, "off"|"emit"|"emit_dry_run")
///   2. `emit_task_contract` (boolean, true ⇒ Emit, false ⇒ Off)
///   3. default ⇒ Off
///
/// Returns `Err(structured)` for malformed values so the caller surfaces
/// a typo (`task_contract_mode="emi"`) instead of silently falling back
/// to Off.
pub(crate) fn parse_task_contract_emit_mode(
    args: &Value,
) -> std::result::Result<TaskContractEmitMode, ToolResult> {
    if let Some(raw) = args.get("task_contract_mode") {
        let s = match raw.as_str() {
            Some(s) => s.trim(),
            None => {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "task_contract_mode must be a string",
                    )
                    .with_suggestion("expected one of: \"off\", \"emit\", \"emit_dry_run\""),
                ));
            }
        };
        return match s {
            TASK_CONTRACT_MODE_OFF => Ok(TaskContractEmitMode::Off),
            TASK_CONTRACT_MODE_EMIT => Ok(TaskContractEmitMode::Emit),
            TASK_CONTRACT_MODE_EMIT_DRY_RUN => Ok(TaskContractEmitMode::EmitDryRun),
            other => Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("task_contract_mode `{}` is not supported", other),
                )
                .with_suggestion("expected one of: \"off\", \"emit\", \"emit_dry_run\""),
            )),
        };
    }
    match args.get("emit_task_contract").and_then(|v| v.as_bool()) {
        Some(true) => Ok(TaskContractEmitMode::Emit),
        Some(false) | None => Ok(TaskContractEmitMode::Off),
    }
}

// ───────────────────────────────────────────────────────────────────────
// wave-20 / task 04 — machine-driven dispatch v0
//
// The wave-19 / task 06 emitter writes a task-contract v1 Lisp sidecar at
// `<project_root>/.missiond/tasks/generated/<plan_id>/<node_id>.lisp`
// BEFORE handing off to the workstation substrate. The wave-19 / task 07
// consumer (`run_workstation_dispatch_with_contract`) is already capable
// of loading + parsing that file and overlaying the contract onto the
// brief, but the production wiring still passes `task_contract_path =
// None` — the consumer has been dormant. Wave-20 / task 04 adds an
// opt-in mode (`dispatch_contract_mode="machine"` or the boolean
// shorthand `render_markdown=false`) that activates the consumer wiring:
// when enabled AND emission produced a path AND the dispatch routes
// through the workstation substrate, the runner forwards the resolved
// contract path so the brief is built FROM the on-disk Lisp SSOT.
//
// Default mode (`rendered`) preserves wave-15..19 behaviour byte-for-
// byte: the brief is built from the in-memory hints and the optional
// `render_command` lets a caller render the markdown out of process.
// In machine mode the markdown rendering is NOT load-bearing — the
// `render_command` is still surfaced as compatibility metadata, but
// observers can prove the Lisp drove the dispatch via the new
// `task_contract_source_path` field on the workstation response.
//
// Failure semantics (machine mode):
//   * malformed contract on disk → `SafeDescriptor` from the consumer
//     (status="skipped_malformed_task_contract"). MUST NOT fall back to
//     `claude -p` or the unscoped prompt path.
//   * emission disabled / not eligible → falls back to legacy rendered
//     dispatch (machine mode is a no-op without an emitted contract;
//     authors who insist on machine SSOT pair `dispatch_contract_mode=
//     machine` with `task_contract_mode="emit"`).
// ───────────────────────────────────────────────────────────────────────

/// Canonical dispatch-contract modes. Mirrors the MCP descriptor enum.
pub(crate) const DISPATCH_CONTRACT_MODE_RENDERED: &str = "rendered";
pub(crate) const DISPATCH_CONTRACT_MODE_MACHINE: &str = "machine";

/// Resolved dispatch-contract mode after parsing the
/// `dispatch_contract_mode` / `render_markdown` args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchContractMode {
    /// Default — wave-15..19 byte-shape. Brief is built from in-memory
    /// hints. The optional `render_command` lets a caller render the
    /// markdown brief out of process; the markdown is the load-bearing
    /// artifact for human review.
    Rendered,
    /// Machine SSOT — when an emitted task-contract path exists AND the
    /// dispatch routes through the workstation substrate, the consumer
    /// loads + parses the on-disk Lisp and overlays it onto the brief.
    /// The markdown becomes optional compatibility metadata.
    Machine,
}

impl DispatchContractMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            DispatchContractMode::Rendered => DISPATCH_CONTRACT_MODE_RENDERED,
            DispatchContractMode::Machine => DISPATCH_CONTRACT_MODE_MACHINE,
        }
    }

    pub(crate) fn is_machine(self) -> bool {
        matches!(self, DispatchContractMode::Machine)
    }
}

/// Parse the dispatch-contract mode opt-in args. Precedence:
///   1. `dispatch_contract_mode` (string, "rendered" | "machine")
///   2. `render_markdown` (boolean, `false` ⇒ Machine, `true` ⇒ Rendered)
///   3. default ⇒ Rendered (wave-15..19 byte-compat)
///
/// Returns `Err(structured)` for malformed string values so a typo
/// (`dispatch_contract_mode="machin"`) fails fast rather than silently
/// degrading to `Rendered`.
pub(crate) fn parse_dispatch_contract_mode(
    args: &Value,
) -> std::result::Result<DispatchContractMode, ToolResult> {
    if let Some(raw) = args.get("dispatch_contract_mode") {
        let s = match raw.as_str() {
            Some(s) => s.trim(),
            None => {
                return Err(ToolResult::structured_error(
                    ToolError::new(
                        error_codes::INVALID_PARAM,
                        "dispatch_contract_mode must be a string",
                    )
                    .with_suggestion("expected one of: \"rendered\", \"machine\""),
                ));
            }
        };
        return match s {
            DISPATCH_CONTRACT_MODE_RENDERED => Ok(DispatchContractMode::Rendered),
            DISPATCH_CONTRACT_MODE_MACHINE => Ok(DispatchContractMode::Machine),
            other => Err(ToolResult::structured_error(
                ToolError::new(
                    error_codes::INVALID_PARAM,
                    format!("dispatch_contract_mode `{}` is not supported", other),
                )
                .with_suggestion("expected one of: \"rendered\", \"machine\""),
            )),
        };
    }
    match args.get("render_markdown").and_then(|v| v.as_bool()) {
        Some(false) => Ok(DispatchContractMode::Machine),
        Some(true) | None => Ok(DispatchContractMode::Rendered),
    }
}

/// Lightweight projection of a workstation-dispatch task surface —
/// independent of the wave-15 `WorkstationDispatchHints` struct so the
/// emitter does not own any reformatting that the dispatch layer cares
/// about. Owned by the emitter; populated by the caller from either the
/// single-node merged hints or the DAG node hints.
#[derive(Debug, Clone, Default)]
pub(crate) struct TaskContractInputs {
    pub objective: String,
    pub scope: Option<String>,
    pub owned_files: Vec<String>,
    pub forbidden_files: Vec<String>,
    pub acceptance_commands: Vec<String>,
    pub commit_policy: Option<String>,
    pub dispatch_strategy: String,
    pub target: String,
    pub target_project: Option<String>,
    pub requested_cwd: Option<String>,
    /// wave-23 / task 05 — optional session-trace ledger path emitted
    /// onto the contract as `:session-trace-path "..."`. Default
    /// (None) preserves the wave-15..22 contract byte shape: the field
    /// is only rendered when the caller explicitly opted in.
    pub session_trace_path: Option<String>,
}

/// Escape a string for inclusion inside a Lisp double-quoted literal.
/// We only need to handle backslash + double-quote — task-contract
/// readers go through the shared MissionD Lisp parser which treats
/// every other byte literally.
pub(crate) fn lisp_escape_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // Newlines in a Lisp string literal are valid; preserve verbatim.
            other => out.push(other),
        }
    }
    out
}

/// Render a non-empty string vector as a bracketed string list:
///   `["a" "b"]`
pub(crate) fn render_lisp_string_list(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(&lisp_escape_string(s));
        out.push('"');
    }
    out.push(']');
    out
}

/// Build a task-contract v1 Lisp document from the resolved inputs.
///
/// Mirrors `.missiond/tasks/schema/task-contract-v1.lisp`:
///   - schema = "missiond.task-contract.v1"
///   - kind   = "code-alignment" (workstation-dispatchable nodes carry
///              code intent; if/when the emitter learns to project
///              read-only briefs we'll add "review" branching here)
///   - status = "ready"
///   - owner  = "claudecode"
///   - commit = (:required true :scope-check write-scope-only ...)
///
/// `node_id` doubles as the contract id (`<plan-uuid-prefix>-<node>`)
/// so `check-task-contract.mjs --all` can pivot on a stable identifier
/// after sweeping `.missiond/tasks/generated/`.
pub(crate) fn build_task_contract_lisp(
    plan_id: uuid::Uuid,
    node_id: &str,
    board_task_id: &str,
    inputs: &TaskContractInputs,
) -> String {
    let plan_short = plan_id.to_string().chars().take(8).collect::<String>();
    let contract_id = format!("plan-{}-node-{}", plan_short, sanitize_for_id(node_id));
    let title = format!(
        "Plan {} node {} — workstation task contract",
        plan_id, node_id
    );
    let commit_message = format!(
        "feat(plan-node): execute node {} for plan {}",
        node_id, plan_id
    );

    let mut out = String::new();
    out.push_str(";; Generated by MissionD plan-runner (wave-19 / task 06).\n");
    out.push_str(&format!(
        ";; plan_id = {}\n;; board_task_id = {}\n;; node_id = {}\n\n",
        plan_id, board_task_id, node_id
    ));
    out.push_str(&format!("(task {}\n", contract_id));
    out.push_str("  :schema \"missiond.task-contract.v1\"\n");
    out.push_str(&format!("  :title \"{}\"\n", lisp_escape_string(&title)));
    out.push_str("  :kind code-alignment\n");
    out.push_str("  :status ready\n");
    out.push_str("  :owner \"claudecode\"\n");
    out.push_str(&format!(
        "  :dispatch-strategy \"{}\"\n",
        lisp_escape_string(&inputs.dispatch_strategy)
    ));
    out.push_str(&format!(
        "  :goal \"{}\"\n",
        lisp_escape_string(inputs.objective.trim())
    ));

    if let Some(scope) = inputs
        .scope
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!("  :scope \"{}\"\n", lisp_escape_string(scope)));
    }

    // :write-scope is required and non-empty per task-contract v1. When
    // the caller did not declare `owned_files`, fall back to a single
    // sentinel that the checker will reject — better to fail loudly on
    // the contract than silently let the worker stage anything.
    out.push_str("  :write-scope\n    ");
    if inputs.owned_files.is_empty() {
        out.push_str("[]\n");
    } else {
        out.push_str(&render_lisp_string_list(&inputs.owned_files));
        out.push('\n');
    }

    out.push_str("  :must-not-touch\n    ");
    out.push_str(&render_lisp_string_list(&inputs.forbidden_files));
    out.push('\n');

    if !inputs.acceptance_commands.is_empty() {
        out.push_str("  :acceptance\n    ");
        out.push_str(&render_lisp_string_list(&inputs.acceptance_commands));
        out.push('\n');
    } else {
        // task-contract v1 demands `:acceptance` non-empty — surface a
        // sentinel so the contract round-trips through the checker as
        // an authoring failure rather than silently dispatching.
        out.push_str("  :acceptance\n    []\n");
    }

    let commit_policy = inputs
        .commit_policy
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("scoped");
    out.push_str("  :commit\n");
    out.push_str("    (:required true\n");
    out.push_str(&format!(
        "     :message \"{}\"\n",
        lisp_escape_string(&commit_message)
    ));
    out.push_str("     :scope-check write-scope-only\n");
    out.push_str(&format!(
        "     :policy \"{}\")\n",
        lisp_escape_string(commit_policy)
    ));

    if let Some(tp) = inputs
        .target_project
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :target-project \"{}\"\n",
            lisp_escape_string(tp)
        ));
    }
    if let Some(cwd) = inputs
        .requested_cwd
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :requested-cwd \"{}\"\n",
            lisp_escape_string(cwd)
        ));
    }
    // wave-23 / task 05 — emit `:session-trace-path` when the plan-runner
    // opted into trace forwarding. The field is read back by
    // `workstation_dispatch::ParsedTaskContract` so a downstream caller
    // that loads the contract (machine mode) can re-derive the path
    // without re-supplying the arg.
    if let Some(stp) = inputs
        .session_trace_path
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        out.push_str(&format!(
            "  :session-trace-path \"{}\"\n",
            lisp_escape_string(stp)
        ));
    }
    out.push_str(&format!(
        "  :target \"{}\"\n",
        lisp_escape_string(&inputs.target)
    ));
    out.push_str(&format!("  :plan-id \"{}\"\n", plan_id));
    out.push_str(&format!("  :node-id \"{}\"\n", lisp_escape_string(node_id)));
    out.push_str(")\n");
    out
}

/// Sanitise an arbitrary node id into the lowercase kebab-friendly form
/// task-contract v1 demands of `:id`. Unknown characters collapse to
/// `-`; collapsing repeats keeps the result readable.
fn sanitize_for_id(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            out.push(ch);
            prev_dash = false;
        } else if ch == '-' || ch.is_whitespace() || ch == '/' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            // Drop everything else conservatively (no random unicode in ids).
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}

/// Compute the on-disk path for a generated task contract. Caller-resolved
/// project root MUST already be canonical (we do not invent one here).
pub(crate) fn task_contract_path(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
    node_id: &str,
) -> PathBuf {
    project_root
        .join(".missiond")
        .join("tasks")
        .join("generated")
        .join(plan_id.to_string())
        .join(format!("{}.lisp", sanitize_for_id(node_id)))
}

/// Build the deterministic `node scripts/render-claudecode-task.mjs ...`
/// invocation paired with each generated contract. Surfaced on the
/// response so a caller can render the markdown brief without the
/// daemon shelling out to Node.
pub(crate) fn render_command_for(contract_path: &std::path::Path) -> String {
    format!(
        "node scripts/render-claudecode-task.mjs --force {}",
        contract_path.display()
    )
}

/// Write the generated contract to `<project_root>/.missiond/tasks/generated/
/// <plan_id>/<node_id>.lisp` atomically (tmp → rename).
///
/// Returns the absolute path on success. Any IO failure surfaces a
/// structured `anyhow::Error` so the caller can refuse the dispatch.
pub(crate) async fn write_task_contract(
    state: &AppState,
    plan_id: uuid::Uuid,
    node_id: &str,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
    body: &str,
) -> Result<PathBuf> {
    let project_root = resolve_project_root(
        &state.project_registry,
        project_arg,
        cwd_arg,
        target_project_arg,
    )
    .await?;
    write_task_contract_under_root(&project_root, plan_id, node_id, body)
}

/// Inner half of [`write_task_contract`] that takes an already-resolved
/// project root. Split out so unit tests can exercise the on-disk
/// contract without materialising a full [`AppState`] (the same
/// pattern [`resolve_project_root`] tests use against
/// [`SharedProjectRegistry`]).
pub(crate) fn write_task_contract_under_root(
    project_root: &std::path::Path,
    plan_id: uuid::Uuid,
    node_id: &str,
    body: &str,
) -> Result<PathBuf> {
    let path = task_contract_path(project_root, plan_id, node_id);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("task contract path missing parent: {}", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| anyhow!("mkdir {}: {}", dir.display(), e))?;
    let tmp = path.with_extension("lisp.tmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| anyhow!("write tmp {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| anyhow!("rename {} -> {}: {}", tmp.display(), path.display(), e))?;
    Ok(path)
}

/// Whether the resolved dispatch shape is eligible for task-contract
/// emission. Mirrors the workstation-dispatch eligibility floor:
/// target must be `mission_task_delegate` and the objective must be
/// non-empty. Other gates (owned-files / scope) are NOT enforced here
/// because the emitter writes a contract that surfaces them as
/// authoring violations through `check-task-contract.mjs`.
pub(crate) fn is_task_contract_eligible(target: &str, objective: Option<&str>) -> bool {
    if target != "mission_task_delegate" {
        return false;
    }
    objective.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

/// The result of a single task-contract emission attempt — surfaced
/// verbatim onto the response payload regardless of dispatch outcome.
#[derive(Debug, Clone)]
pub(crate) struct TaskContractEmissionRecord {
    pub mode: TaskContractEmitMode,
    pub eligible: bool,
    /// Set when `eligible=false` so the caller can read the skip reason.
    pub skip_reason: Option<String>,
    /// Set when emission succeeded.
    pub path: Option<PathBuf>,
    /// Set when emission attempted but failed.
    pub error: Option<String>,
    /// Set when emission succeeded.
    pub render_command: Option<String>,
}

impl TaskContractEmissionRecord {
    pub(crate) fn off() -> Self {
        Self {
            mode: TaskContractEmitMode::Off,
            eligible: false,
            skip_reason: None,
            path: None,
            error: None,
            render_command: None,
        }
    }

    pub(crate) fn skipped(mode: TaskContractEmitMode, reason: &str) -> Self {
        Self {
            mode,
            eligible: false,
            skip_reason: Some(reason.to_string()),
            path: None,
            error: None,
            render_command: None,
        }
    }

    pub(crate) fn ok(mode: TaskContractEmitMode, path: PathBuf) -> Self {
        let cmd = render_command_for(&path);
        Self {
            mode,
            eligible: true,
            skip_reason: None,
            path: Some(path),
            error: None,
            render_command: Some(cmd),
        }
    }

    pub(crate) fn failed(mode: TaskContractEmitMode, error: String) -> Self {
        Self {
            mode,
            eligible: true,
            skip_reason: None,
            path: None,
            error: Some(error),
            render_command: None,
        }
    }

    pub(crate) fn is_failure(&self) -> bool {
        self.error.is_some()
    }

    /// Project the record onto a JSON block to be merged into the
    /// response payload. Returns `None` when the emitter was OFF and
    /// nothing observable happened — keeps the response byte-shape
    /// identical to the pre-wave19 baseline for the default path.
    pub(crate) fn to_response_block(&self) -> Option<Value> {
        if matches!(self.mode, TaskContractEmitMode::Off)
            && self.path.is_none()
            && self.error.is_none()
        {
            return None;
        }
        let mut map = serde_json::Map::new();
        map.insert("task_contract_mode".to_string(), json!(self.mode.as_str()));
        map.insert("task_contract_eligible".to_string(), json!(self.eligible));
        if let Some(reason) = &self.skip_reason {
            map.insert("task_contract_skip_reason".to_string(), json!(reason));
        }
        if let Some(path) = &self.path {
            map.insert(
                "task_contract_path".to_string(),
                json!(path.display().to_string()),
            );
        }
        if let Some(cmd) = &self.render_command {
            map.insert("render_command".to_string(), json!(cmd));
        }
        if let Some(err) = &self.error {
            map.insert("task_contract_error".to_string(), json!(err));
        }
        Some(Value::Object(map))
    }
}

/// Drive one emission pass given resolved inputs. Always returns a
/// record; never panics, never silently swallows IO failure (failures
/// land on `record.error`).
pub(crate) async fn emit_task_contract(
    state: &AppState,
    plan_id: uuid::Uuid,
    board_task_id: &str,
    node_id: &str,
    mode: TaskContractEmitMode,
    inputs: &TaskContractInputs,
    project_arg: Option<&str>,
    cwd_arg: Option<&str>,
    target_project_arg: Option<&str>,
) -> TaskContractEmissionRecord {
    if !mode.is_enabled() {
        return TaskContractEmissionRecord::off();
    }
    if !is_task_contract_eligible(&inputs.target, Some(&inputs.objective)) {
        return TaskContractEmissionRecord::skipped(
            mode,
            "target is not mission_task_delegate or objective is empty",
        );
    }
    let body = build_task_contract_lisp(plan_id, node_id, board_task_id, inputs);
    match write_task_contract(
        state,
        plan_id,
        node_id,
        project_arg,
        cwd_arg,
        target_project_arg,
        &body,
    )
    .await
    {
        Ok(path) => TaskContractEmissionRecord::ok(mode, path),
        Err(e) => TaskContractEmissionRecord::failed(mode, e.to_string()),
    }
}

/// Build a `TaskContractInputs` view from the wave-15 workstation hint
/// projection. Used by the single-node `action_execute_internal` path
/// where the merged hints live; the DAG path constructs the inputs
/// directly from the per-node `WorkstationDispatchHints`.
pub(crate) fn task_contract_inputs_from_hints(
    hints: &workstation_dispatch::WorkstationDispatchHints,
    target: &str,
    dispatch_strategy: &str,
) -> TaskContractInputs {
    TaskContractInputs {
        objective: hints.objective.clone().unwrap_or_default(),
        scope: hints.scope.clone(),
        owned_files: hints.owned_files.clone(),
        forbidden_files: hints.forbidden_files.clone(),
        acceptance_commands: hints.acceptance_commands.clone(),
        commit_policy: hints.commit_policy.clone(),
        dispatch_strategy: dispatch_strategy.to_string(),
        target: target.to_string(),
        target_project: hints.target_project.clone(),
        requested_cwd: hints.requested_cwd.clone(),
        session_trace_path: None,
    }
}

/// wave-23 / task 05 — variant of `task_contract_inputs_from_hints` that
/// also stamps an optional session-trace ledger path onto the projection
/// so the contract emitter writes `:session-trace-path "..."` into the
/// generated Lisp.
///
/// Why a separate function rather than extending the existing one's
/// signature: out-of-scope callers (`plan_dag.rs`) bind to the existing
/// 3-arg form and cannot be edited under this wave's contract. A wrapper
/// keeps the legacy surface stable and lets the in-scope plan.rs
/// surface forward the trace path cleanly.
pub(crate) fn task_contract_inputs_from_hints_with_trace(
    hints: &workstation_dispatch::WorkstationDispatchHints,
    target: &str,
    dispatch_strategy: &str,
    session_trace_path: Option<&str>,
) -> TaskContractInputs {
    let mut out = task_contract_inputs_from_hints(hints, target, dispatch_strategy);
    out.session_trace_path = session_trace_path
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    out
}

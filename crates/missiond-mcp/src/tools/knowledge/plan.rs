use crate::ToolDefinition;
use serde_json::{json, Map, Value};

/// Build a single property descriptor `{"type": ..., "description": ...}` —
/// optionally with an `enum` constraint. Centralising construction here keeps
/// the schema-builder readable while sidestepping `json!` macro recursion
/// limits (default 128) when the property bag grows past ~30 entries.
fn prop(ty: &str, description: &str) -> Value {
    json!({"type": ty, "description": description})
}

fn prop_enum(ty: &str, description: &str, variants: &[&str]) -> Value {
    json!({
        "type": ty,
        "enum": variants,
        "description": description,
    })
}

fn prop_no_type(description: &str) -> Value {
    json!({"description": description})
}

fn prop_object_open(description: &str) -> Value {
    json!({
        "type": "object",
        "description": description,
        "additionalProperties": true,
    })
}

fn build_properties() -> Value {
    let mut p: Map<String, Value> = Map::new();

    p.insert("action".into(), prop_enum(
        "string",
        "manager action — see Lisp future-surface mission_plan",
        &[
            "compile", "list", "get", "by_task",
            "approve", "mark", "supersede",
            "execute", "record_evidence",
        ],
    ));

    p.insert("directive_id".into(), prop(
        "string",
        "[compile] directive id; sonnet mode loads sexp_text from version_chain head (or `directive_version`). Default approval gate requires directive.status ∈ {approved, compiled}.",
    ));

    p.insert("board_task_id".into(), prop(
        "string",
        "[compile|by_task] board_tasks.id (TEXT FK). Required for sonnet compile (anchor) and for any persist=true (FK NOT NULL).",
    ));

    p.insert("persist".into(), prop(
        "boolean",
        "[compile] insert a row (default false). Requires board_task_id. dry_run inserts as draft; sonnet inserts as awaiting_approval with compiler_model + compiled_from.",
    ));

    p.insert("compiler_mode".into(), prop_enum(
        "string",
        "[compile] dry_run (default, no LLM, same envelope as before); sonnet asks the plan-compiler actor (Sonnet) to emit a PLAN sexp anchored to board_task_id. See intent-intent-layer.lisp :: role plan-compiler.",
        &["dry_run", "sonnet"],
    ));

    p.insert("directive_version".into(), prop(
        "integer",
        "[compile sonnet] specific directive version (default = version_chain head).",
    ));

    p.insert("allow_unapproved".into(), prop(
        "boolean",
        "[compile sonnet] override approval gate. When true, the compiler runs against directive.status outside {approved, compiled}; the response flags `approval_gate_overridden=true`.",
    ));

    p.insert("target_project".into(), prop(
        "string",
        "[compile sonnet | execute] for compile this is prompt context only. For execute internal mission_task_delegate it is treated as cwd if it looks like a path; for execute internal mission_execution it is forwarded as `project`. Auto-selection v1: when omitted, runner extracts from plan.sexp_text :target-project / :target_project / :project hints; explicit arg still wins.",
    ));

    p.insert("parallelism".into(), prop(
        "string",
        "[compile sonnet | execute] for compile sonnet this is a hint for the planner (e.g. `serial`, `agent-team`, `mixed`) surfaced inside the Sonnet prompt. For execute (auto-selection v1) it is also consumed by the plan-runner: value `agent-team` (also resolvable from plan.sexp_text :parallelism hint) maps dispatch_strategy to `agent-team` (lower precedence than explicit dispatch_strategy arg or :dispatch-strategy hint), and when the resolved target is `mission_task_delegate` the runner injects the literal「使用 agent-team提高效率」hint into the delegated objective (no-duplicate).",
    ));

    p.insert("acceptance".into(), prop_no_type(
        "[compile sonnet] string or array of acceptance criteria woven into the planner prompt.",
    ));

    p.insert("constraints".into(), prop_no_type(
        "[compile sonnet] string or array of constraints woven into the planner prompt.",
    ));

    p.insert("plan_id".into(), prop(
        "string",
        "[get|approve|mark|execute|record_evidence] plan UUID",
    ));

    p.insert("status".into(), prop_enum(
        "string",
        "[list filter | mark target] PlanStatus",
        &["draft", "awaiting_approval", "approved", "executing", "succeeded", "failed", "superseded"],
    ));

    p.insert("limit".into(), prop(
        "integer",
        "[list] cap result count (1-500, default 50)",
    ));

    p.insert("old_plan_id".into(), prop(
        "string",
        "[supersede] plan to mark superseded",
    ));

    p.insert("new_plan_id".into(), prop(
        "string",
        "[supersede] replacement plan UUID (recorded in result only)",
    ));

    p.insert("target".into(), prop_enum(
        "string",
        "[execute] routing target — bridge mode hands back next_call; internal mode dispatches inside MissionD. OPTIONAL under plan-runner auto-selection v1: when omitted, runner scans plan.sexp_text for :target / :target-tool / :tool hints (case-insensitive substring: `mission_execution`/`execution`→mission_execution; `task_delegate`/`claudecode`/`code-alignment`→mission_task_delegate; `flow_run`/`flow` + a resolvable :flow-id→mission_flow_run). Source-resolution precedence is explicit_arg > plan_hint > missing; the response surfaces target_source. If parser cannot derive a safe target and caller didn't pass one, the existing MISSING_PARAM structured error is returned with a suggestion to add `target` arg or PLAN hint fields.",
        &["mission_execution", "mission_task_delegate", "mission_flow_run"],
    ));

    p.insert("execute_mode".into(), prop_enum(
        "string",
        "[execute] bridge (default) returns a next_call descriptor; internal asks the plan-runner to dispatch the target handler inside the daemon and append evidence",
        &["bridge", "internal"],
    ));

    p.insert("scheduler_mode".into(), prop_enum(
        "string",
        "[execute] default (current single-node v0 runner) | dag_v1 (Wave 12 minimal DAG scheduler, upgraded to wave-based runtime in Wave 13/02). dag_v1 only parses explicit `(node :id ... :target ... :depends-on [...])` forms inside PLAN.lisp; supported per-node fields: id / target / objective / depends-on / condition / failure-policy / timeout-ms / dispatch-strategy / target-project / requested-cwd / flow-id. failure-policy ∈ {fail-fast (default), continue}; unsupported fields are preserved into node_hint_summary.unsupported_fields and never silently dropped. Requires execute_mode=internal. v2 runtime drives ready nodes through a tokio JoinSet up to `max_parallel_nodes` per wave (default 1 = strict-sequential v1 behaviour); each per-node state transition (ready->running, running->{succeeded|failed}, pending->skipped) appends one plan_dag_node_dispatch evidence entry tagged with `state_transition`. Response surfaces scheduler_mode / node_count / max_parallel_nodes / node_results[] / skipped_nodes[] / paused_nodes[] / paused_node_ids[] / review_question_ids[] / aggregate_status / concurrency_plan. Wave 16 / Task 04 — per-node `:review-gate \"question-event\"` (with optional `:review-action` and `:review-text`) pauses the node and emits `QuestionEvent::Created` (deterministic id = `review:plan:<plan_id>:v<plan_version>:<action>:<node-id-hash>`, default action `plan-node`) instead of dispatching the target tool; downstream stays Pending; aggregate_status flips to `dag_paused` / runner_status to `review_gate_paused` when paused-only, plan status untouched. Bus publish failure still pauses (refuse to dispatch past a failed gate) and surfaces the warning under `bus_publish_warnings` + per-node `review_question_warning`. Auto-resume is OUT OF SCOPE here — the wave-16 / task 02 `QuestionEvent::Resolved` listener handles that.",
        &["default", "dag_v1"],
    ));

    p.insert("max_parallel_nodes".into(), prop(
        "integer",
        "[execute scheduler_mode=dag_v1] Wave 13/02 wave-scheduler concurrency budget. Default 1 (preserves the v1 strict-sequential contract — every wave dispatches exactly one ready node). Values >1 let the scheduler hand up to that many ready nodes to a tokio JoinSet per wave; ready-set selection is sorted by node id for deterministic test output. 0 / negative are clamped to 1. Each spawned task clones AppState (cheap, all Arc fields) and the plan; per-node evidence writes are serialised through the scheduler's main task so the on-disk sidecar stays consistent under concurrency. Has no effect when scheduler_mode!=dag_v1.",
    ));

    p.insert("dispatch_strategy".into(), prop_enum(
        "string",
        "[execute] workstation-dispatch-record strategy. Surfaced in the response and the plan_runner_dispatch evidence entry. Unknown values are normalised to `unknown`. Internal mode forwards dispatch_strategy to mission_execution(action=open), where the companion log now persists this field. Auto-selection v1 precedence: explicit_arg > plan_hint :dispatch-strategy > :parallelism mapping > default `unknown`; keyword mapping (case-insensitive substring): `agent-team`→agent-team, `code-alignment`/`fresh-session`→fresh-code-alignment, `lisp`/`architecture`/`resident`→resident-lisp, anything else→unknown (never hard-fails). Response surfaces dispatch_strategy_source ∈ {explicit_arg, plan_hint, default}.",
        &["resident-lisp", "fresh-code-alignment", "agent-team", "mixed", "prompt-fallback", "unknown"],
    ));

    p.insert("cwd".into(), prop(
        "string",
        "[execute internal mission_task_delegate | compile persist=true write_file=true] working directory passed through to mission_task_delegate. wave-14 file-first writer also accepts it as the project-root signal — must be ABSOLUTE (relative paths are refused; no process-cwd fallback per intent-worker.lisp :: project-root-spawn-cwd).",
    ));

    p.insert("requested_cwd".into(), prop(
        "string",
        "[execute internal mission_execution] working directory metadata persisted on the companion log when present (workstation-dispatch-record :requested-cwd). Auto-selection v1: when omitted, runner extracts from plan.sexp_text :requested-cwd / :requested_cwd / :cwd hints; explicit arg still wins.",
    ));

    p.insert("objective".into(), prop(
        "string",
        "[execute internal mission_task_delegate] override the auto-derived objective. Auto-selection v1 derivation order when omitted: plan.sexp_text :objective hint > :summary hint > first non-empty line of plan.sexp_text. When dispatch_strategy resolves to agent-team under target=mission_task_delegate, the runner additionally injects the literal Chinese hint「使用 agent-team提高效率」into the delegated objective (without duplicating if already present).",
    ));

    p.insert("intent".into(), prop_enum(
        "string",
        "[execute internal mission_task_delegate] task intent (default `code`); strict whitelist mirrored from mission_task_delegate",
        &["code", "ops", "research", "general"],
    ));

    p.insert("execution_id".into(), prop(
        "string",
        "[execute internal mission_execution] caller-supplied execution_id (default `plan-<plan_id>`)",
    ));

    p.insert("parent_design".into(), prop(
        "string",
        "[execute internal mission_execution] override parent-design ref (default `directive/<id>` if plan has source_directive_id, else `plan/<plan_id>`)",
    ));

    p.insert("scope".into(), prop(
        "string",
        "[execute internal mission_execution] override the human-readable scope string",
    ));

    p.insert("owner".into(), prop(
        "string",
        "[execute internal mission_execution] execution owner (default `plan-runner`)",
    ));

    p.insert("flow_id".into(), prop(
        "string",
        "[execute internal mission_flow_run] existing flow id. Auto-selection v1: when omitted, runner extracts it from plan.sexp_text :flow-id / :flow_id hints; explicit arg still wins. plan.sexp_text → flow YAML compilation is still future, so the caller (or PLAN hint) must point at an already-registered flow id.",
    ));

    p.insert("params".into(), prop_object_open(
        "[execute internal mission_flow_run] forwarded as the flow params object",
    ));

    p.insert("priority".into(), prop(
        "string",
        "[execute internal mission_task_delegate] passthrough priority",
    ));

    p.insert("timeout_secs".into(), prop(
        "integer",
        "[execute internal mission_task_delegate] passthrough timeout",
    ));

    p.insert("dry_run".into(), prop(
        "boolean",
        "[execute internal] when true, return the would-be inner args without dispatching (does NOT mutate plan status, does NOT write evidence). Also recognised under scheduler_mode=dag_v1 to return the DAG plan + topological order + projected concurrency_plan (the wave layout the v2 wave-scheduler would launch given max_parallel_nodes) without dispatching nodes or writing per-node evidence.",
    ));

    p.insert("evidence".into(), prop_no_type(
        "[record_evidence] arbitrary JSON: tool_calls / event_log / test outputs / execution log refs",
    ));

    p.insert("evidence_kind".into(), prop(
        "string",
        "[record_evidence] (Wave 12 evidence-collector v0) optional canonical taxonomy tag for the entry — one of `dispatch`/`verification`/`git_diff`/`commit`/`note` (open enum, arbitrary strings accepted). When supplied (alongside or instead of `source`), the action wraps the legacy `{\"evidence\": …}` payload with `schema_version=\"v0\"` + the supplied `kind` + a default `source=\"record_evidence_manual\"`. When BOTH `evidence_kind` and `source` are absent, the historical untagged shape is preserved byte-for-byte for backward compatibility.",
    ));

    p.insert("source".into(), prop(
        "string",
        "[record_evidence] (Wave 12 evidence-collector v0) optional canonical source tag for the entry (e.g. `record_evidence_manual`, `plan_runner_dispatch`, `plan_dag_node_dispatch`, or any caller-defined string). Same routing as `evidence_kind`: presence triggers the typed wrap (with `kind` defaulting to `note` if not supplied); absence preserves the legacy untagged wire form.",
    ));

    p.insert("project".into(), prop(
        "string",
        "[record_evidence|execute|compile persist=true write_file=true] project id (registry-resolved root); defaults to CWD. wave-14 file-first writer additionally treats it as the primary signal for project-root resolution.",
    ));

    p.insert("write_file".into(), prop(
        "boolean",
        "[compile persist=true] (wave-14 file-first SSOT) mirror the compiled PLAN sexp to `<project_root>/.missiond/plans/<topic>/PLAN.lisp` after the DB row is committed. Default false. Topic defaults to `board_task_id` when not supplied. DB row is NEVER rolled back on file failure — response surfaces status=\"partial\" + file_write_error in that case.",
    ));

    p.insert("overwrite_file".into(), prop(
        "boolean",
        "[compile persist=true write_file=true] allow replacing an existing PLAN.lisp at the target path (default false → atomic refusal).",
    ));

    p.insert("topic".into(), prop(
        "string",
        "[compile persist=true write_file=true] file-first SSOT topic segment used to derive `.missiond/plans/<topic>/PLAN.lisp`. Defaults to `board_task_id`. Sanitized (alnum / `_` / `-`).",
    ));

    p.insert("review_gate_policy".into(), prop_enum(
        "string",
        "[compile persist=true] (wave-14 review gate auto-create v1) controls automatic QuestionEvent::Created emission AFTER a successful PLAN.lisp file-first write. `manual` (default) keeps the legacy explicit-emit path (`emit_review_question=true`) the only way to fire an event; `emit_question` auto-fires when `write_file=true` AND the file landed (`file_written=true`); `off` suppresses BOTH the auto-emit and the legacy bool. Response always echoes the resolved policy. Auto-emit is fire-and-forget on the bus (never blocks, never auto-approves, never waits). Bus failures surface `review_question_warning` + the deterministic id for caller retry / manual resolution.",
        &["manual", "emit_question", "off"],
    ));

    p.insert("emit_review_question".into(), prop(
        "boolean",
        "[compile persist=true review_gate_policy=manual] (wave-11 explicit-emit path) fire one QuestionEvent::Created after the plan row is committed. Best-effort; bus failures surface `review_question_warning` instead of failing the compile. Ignored when `review_gate_policy=emit_question` (auto-emit takes over) or `review_gate_policy=off` (suppression).",
    ));

    p.insert("review_question_text".into(), prop(
        "string",
        "[compile persist=true emit_review_question=true | review_gate_policy=emit_question] free-form prompt echoed back in the response payload (`review_question_text`); the bus event itself only carries the deterministic id.",
    ));

    p.insert("review_question_id".into(), prop(
        "string",
        "[compile persist=true | approve | mark | supersede] deterministic question-id override. On compile, replaces the auto-derived id (`review:plan:<id>:v<version>:compile[:<topic-hash>]`). On approve / mark / supersede WITHOUT `review_decision`, opts the action into emitting a follow-up QuestionEvent::Resolved (or DecisionResolved) with the supplied id — same fire-and-forget, bus-failure-warns semantics (legacy quiet path). On approve / mark / supersede WITH `review_decision`, switches to the wave-15 explicit-resolution bridge: validates the deterministic id (scope=plan, artifact=plan_id (or old_plan_id for supersede), version=plan.version, action ∈ {compile|approve|mark|supersede}) BEFORE mutating state; `review_decision=approved` runs the manager transition, `rejected`/`needs_changes` skip it. Absent → no resolution emit (legacy quiet).",
    ));

    // ── wave-15 / task 05 — workstation-dispatch v0 schema ──────────────
    //
    // These knobs are opt-in. The runner only invokes the workstation-
    // dispatch substrate when `workstation_dispatch=true` (or the PLAN.lisp
    // hint `:workstation-dispatch true`) AND the resolved target is
    // `mission_task_delegate`. Anything else falls through to the legacy
    // plan-runner internal dispatch contract documented above.
    p.insert("workstation_dispatch".into(), prop(
        "boolean",
        "[execute internal target=mission_task_delegate] Wave 15 / Task 05 — opt into workstation-dispatch v0. When true (or PLAN.lisp carries `:workstation-dispatch true`) the runner builds a scoped task brief (objective / scope / owned-files / forbidden-files / acceptance-commands / commit policy / agent-team hint when dispatch_strategy=agent-team) and dispatches via the existing `mission_task_delegate` substrate (NEVER `claude -p`). Project root is resolved via `resolve_target_project_root` (project > absolute cwd > target_project; relative cwd refused; no process-cwd fallback). On safety failure (target!=mission_task_delegate, project root unresolved, missing objective) the runner returns a structured `workstation_dispatch_status=skipped_*` descriptor instead of silently falling back. Response surfaces workstation_dispatch_status / dispatch_strategy / task_brief_preview / inner_result / evidence_path. Wave 16 / Task 03 conservative auto-inference: when omitted the runner auto-enables workstation dispatch ONLY when ALL of the following hold: resolved target = `mission_task_delegate`, dispatch_strategy ∈ {fresh-code-alignment, resident-lisp, agent-team, mixed}, objective non-empty, AND at least one scoping signal present (owned_files, scope, target_project, or requested_cwd). Explicit `false` always suppresses inference. The response always carries `workstation_dispatch_source` ∈ {explicit_arg, plan_hint, inferred, disabled, not_applicable} and `workstation_dispatch_inference_reason` (when set).",
    ));

    p.insert("scope".into(), prop(
        "string",
        "[execute internal workstation_dispatch=true] free-form additional bounds spliced into the task brief's `## Scope` section. Plan-hint fallback: PLAN.lisp `:scope`. Caller wins.",
    ));

    p.insert("owned_files".into(), prop_no_type(
        "[execute internal workstation_dispatch=true] string or array of file paths the delegated task is allowed to stage / commit. Spliced into the task brief's `## Owned files` section. Plan-hint fallback: PLAN.lisp `:owned-files [\"a.rs\" \"b.rs\"]` (also accepts paren list and bareword run). Caller wins. Capped at 32 entries; overflow surfaces in the response.",
    ));

    p.insert("forbidden_files".into(), prop_no_type(
        "[execute internal workstation_dispatch=true] string or array of file paths the delegated task MUST NOT touch. Spliced into the task brief's `## Forbidden files` section (omitted from the brief when empty). Plan-hint fallback: PLAN.lisp `:forbidden-files [...]`. Capped at 32 entries.",
    ));

    p.insert("acceptance_commands".into(), prop_no_type(
        "[execute internal workstation_dispatch=true] string or array of acceptance commands the delegated task must pass before commit (`cargo test ...`, `git diff --check`, ...). Spliced into the task brief's `## Acceptance commands` section. Plan-hint fallback: PLAN.lisp `:acceptance-commands [...]`. Capped at 32 entries.",
    ));

    p.insert("commit_policy".into(), prop(
        "string",
        "[execute internal workstation_dispatch=true] commit handoff policy. Default `scoped` — the brief always carries the literal reminder `do not stage or commit outside the owned files declared above`. Plan-hint fallback: PLAN.lisp `:commit-policy`. Caller wins.",
    ));

    // ── wave-15 / task 04 — explicit review-resolution bridge schema ────
    p.insert("review_decision".into(), prop_enum(
        "string",
        "[approve | mark | supersede] (wave-15 explicit-resolution bridge) explicit decision attached to the supplied `review_question_id`. Required when `review_question_id` is supplied; absence with the id triggers a structured MISSING_PARAM error. `approved` performs the manager transition (`plan_update_status(Approved)` for approve, `plan_update_status(<status>)` for mark, `plan_supersede(old, new)` for supersede); `rejected` keeps the plan at its current status and emits Resolved/rejected; `needs_changes` keeps it in review/draft and surfaces a `next_step` hint with Resolved/needs_changes. NOT auto-approve and NOT a poll for a QuestionEvent::Resolved answer — the helper consumes only this caller-supplied input.",
        &["approved", "rejected", "needs_changes"],
    ));

    p.insert("review_actor".into(), prop(
        "string",
        "[approve | mark | supersede review_decision=*] (wave-15) free-form identity of the resolver. Echoed onto the response payload (`review_actor`) so callers can correlate the decision with whoever made it; never used for authentication.",
    ));

    p.insert("review_note".into(), prop(
        "string",
        "[approve | mark | supersede review_decision=*] (wave-15) free-form reason / next-step note. Echoed onto the response payload (`review_note`) and surfaced to downstream consumers as the human-readable resolution context.",
    ));

    Value::Object(p)
}

pub fn definitions() -> Vec<ToolDefinition> {
    let schema = json!({
        "type": "object",
        "required": ["action"],
        "properties": build_properties(),
    });
    vec![ToolDefinition::new(
        "mission_plan",
        "plan 表 manager — 9 actions (compile/list/get/by_task/approve/mark/supersede/execute/record_evidence)。\
         compile 默认 compiler_mode=\"dry_run\"（不调 LLM，行为同旧版）；compiler_mode=\"sonnet\" 时是 plan-compiler actor v0：\
         读取 directive (version_chain head 或显式 directive_version) + board_task，调 Sonnet 生成 PLAN sexp，\
         校验括号 / 顶层 head / board_task 锚点，persist=true 时落库 status=awaiting_approval、\
         compiler_model=\"claude-sonnet\"、compiled_from=\"directive/<id>:<version>\" 或 \"board_task/<id>\"。\
         默认要求 directive.status ∈ {approved, compiled}；可显式 allow_unapproved=true 调试。\
         list/get/by_task/approve/mark/supersede 为 store-backed full；\
         execute 为 plan-runner v0：默认 execute_mode=\"bridge\" 返回 next_call descriptor（runner_status=\"bridge_only\"），\
         向后兼容；execute_mode=\"internal\" 时 MissionD 内部 dispatch 目标 handler，\
         成功后写 plan_runner_dispatch 证据并把 plan 标记 executing。\
         target ∈ {mission_execution, mission_task_delegate, mission_flow_run}；\
         dispatch_strategy ∈ {resident-lisp|fresh-code-alignment|agent-team|mixed|prompt-fallback|unknown}\
         （未知值归一化为 unknown，记入响应 + sidecar，且在 internal target=mission_execution 时\
         转发给 mission_execution(action=open) 持久化进 companion log）。\
         plan-runner auto-selection v1: target 可省略，runner 从 plan.sexp_text 解析\
         :target / :target-tool / :tool / :flow-id / :dispatch-strategy / :parallelism /\
         :target-project / :requested-cwd / :objective / :summary 等 hint；解析优先级 explicit_arg > plan_hint > default(unknown)；\
         响应新增 target_source ∈ {explicit_arg, plan_hint, missing}、dispatch_strategy_source ∈ {explicit_arg, plan_hint, default}、\
         plan_hint_summary（仅含解析到的字段）；当 parallelism=agent-team（或 hint 解析为 agent-team）且 target=mission_task_delegate 时，\
         runner 在 objective 中注入字面提示「使用 agent-team提高效率」（已含则不重复）；\
         dispatch_strategy 关键字映射：agent-team→agent-team、code-alignment/fresh-session→fresh-code-alignment、lisp/architecture/resident→resident-lisp，未识别归一化 unknown 不硬失败；\
         若 parser 无法导出安全 target 且 caller 未传，仍返回原 MISSING_PARAM 结构化错误（suggestion 提示新增 target arg 或 PLAN hint）。\
         scheduler_mode=\"dag_v1\" (Wave 12 / Task 02 起，Wave 13 / 02 升级到 v2 runtime): 在 v0 单节点 runner 之上启用 DAG scheduler，\
         只解析 PLAN.lisp 中显式 `(node :id ... :target ... :depends-on [...])` 节点；支持字段 id/target/objective/depends-on/\
         condition/failure-policy/timeout-ms/dispatch-strategy/target-project/requested-cwd/flow-id；\
         failure-policy ∈ {fail-fast (默认), continue}；不支持字段保留进 node_hint_summary 不静默丢弃；\
         v2 runtime: tokio JoinSet 驱动的 wave-based 调度器，每 wave 取最多 max_parallel_nodes 个 ready 节点并发 dispatch (默认 1 = 严格顺序 v1 兼容)；\
         每节点 state transition (ready->running, running->{succeeded|failed}, pending->skipped) 都写一条 plan_dag_node_dispatch 证据；\
         响应字段: scheduler_mode / node_count / max_parallel_nodes / node_results[] / skipped_nodes[] / aggregate_status / concurrency_plan / topological_order；\
         dry_run=true 只返 DAG + concurrency_plan 不 dispatch。\
         record_evidence 写 sidecar `<project>/.missiond/v2/plans/<plan_id>.evidence.json`；\
         Wave 12 evidence-collector v0: 新增 evidence_kind / source 两个可选参数 — 当至少一个被传入时,\
         entry 会被 wrap 为带 `schema_version=\"v0\"` + canonical `source` + canonical `kind` 的 typed 形态\
         (kind 默认 `note`, source 默认 `record_evidence_manual`); 两个都不传时仍保留 legacy `{\"evidence\": …}` wire form,\
         向后兼容。\
         wave-14 file-first SSOT: compile persist=true 时再传 write_file=true 即把 compiled_sexp 镜像到 \
         `<project_root>/.missiond/plans/<topic>/PLAN.lisp` (ArtifactKind::Plan, atomic, 默认拒覆, \
         overwrite_file=true 替换); topic 默认取 board_task_id; project root 解析强制走 \
         resolve_target_project_root (project > absolute cwd > target_project, 禁止 process cwd fallback); \
         DB 行已写但 file 写失败 → status=\"partial\" + file_write_error, 不回滚 row; \
         成功响应附 file_written / file_path / file_sha256 / file_bytes / file_created / file_overwritten。\
         wave-14 review gate auto-create v1: compile persist=true 时再传 review_gate_policy=\"emit_question\" \
         即在 file_written=true 后自动 fire 一条 QuestionEvent::Created (deterministic id = \
         review:plan:<id>:v<version>:compile:<topic-hash>); review_gate_policy=\"manual\" (默认) \
         保留 wave-11 显式 emit_review_question=true 路径; review_gate_policy=\"off\" 同时压制两者。\
         不实现 UI / 不等回答 / 不自动 approve; bus 失败 surface review_question_warning + 确定性 id 供重试。\
         approve / mark / supersede 接收 review_question_id → 触发 QuestionEvent::Resolved (或 DecisionResolved)。\
         响应总附 review_gate_policy / review_question_emitted (+ review_question_id / review_question_warning when applicable)。\
         wave-15 / task 04 review-resolution bridge v0: approve / mark / supersede 同时传 review_question_id + review_decision \
         (approved | rejected | needs_changes) 时，先 validate envelope (scope=plan, artifact=plan_id 或 supersede 用 old_plan_id, \
         version=plan.version, action ∈ {compile|approve|mark|supersede}) 再决定: approved → 跑对应 manager 转换 \
         (plan_update_status / plan_supersede); rejected → 保持当前 status + status=\"review_rejected\"; \
         needs_changes → 保持当前 status + status=\"review_needs_changes\" + next_step。\
         失败 (REVIEW_ID_MALFORMED / REVIEW_SCOPE_MISMATCH / REVIEW_SCOPE_UNSUPPORTED / REVIEW_ARTIFACT_MISMATCH / \
         STALE_REVIEW_VERSION / REVIEW_ACTION_UNSUPPORTED) 在 mutate 前 fail-fast。\
         不实现 UI / 不等 QuestionEvent::Resolved 回答 / 不自动 approve; bus 失败转 review_question_warning, DB 已 commit 时不回滚。\
         可选 review_actor + review_note 仅作 audit 字符串透传到 response。\
         wave-15 / task 05 workstation-dispatch v0: execute internal target=mission_task_delegate 时再传 workstation_dispatch=true \
         (或 PLAN.lisp 写 :workstation-dispatch true) 即启用 workstation-dispatch 路径 — runner 用 \
         scope / owned_files / forbidden_files / acceptance_commands / commit_policy / dispatch_strategy 字段 \
         拼装 scoped task brief，注入到 mission_task_delegate.objective (绝不 shell out claude -p)，\
         project root 强制走 resolve_target_project_root (禁止 join relative cwd)，\
         dispatch_strategy=agent-team 时 brief 内一次性插入字面提示「使用 agent-team提高效率」；\
         安全失败 (target 错 / project root 未解 / 缺 objective) 返结构化 workstation_dispatch_status=skipped_* descriptor，\
         不静默 fallback prompt mode；响应附 workstation_dispatch_status / dispatch_strategy / task_brief_preview / inner_result / evidence_path。\
         同样的 hint contract 在 scheduler_mode=dag_v1 节点上生效 (per-node `:workstation-dispatch true`)，\
         workstation-dispatch 节点 dispatch 走相同 substrate，evidence source=workstation_dispatch。\
         wave-16 / task 03 conservative auto-inference: 当 caller 未传 workstation_dispatch 且 PLAN/node 也无 \
         :workstation-dispatch hint 时，runner 仅在五项条件同时满足时自动启用 workstation-dispatch — \
         resolved target = mission_task_delegate，dispatch_strategy ∈ {fresh-code-alignment, resident-lisp, agent-team, mixed}，\
         objective 非空，且至少一个 scoping signal (owned_files / scope / target_project / requested_cwd) 出现，\
         caller 未显式 workstation_dispatch=false。`mission_execution` / `mission_flow_run` 永不自动推断；target / project root 未解析时不推断；\
         显式 false 始终压制推断。响应始终附带 workstation_dispatch_source ∈ {explicit_arg, plan_hint, inferred, disabled, not_applicable} \
         + workstation_dispatch_inference_reason (when set)；DAG 节点同样规则 (per-node 字段)。\
         Lisp 源: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract / :dispatch-strategy-consumer \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-compiler / plan-runner \
         + intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4 plan-authoring / s5 plan-review-gate / s6 execution-runner \
         + intent-flow.lisp :: F-workstation-dispatch-policy \
         + intent-worker.lisp :: claudecode-workstation-orchestration \
         + intent-memory.lisp :: directive-layer :: file-first-artifacts :: plan-lisp。",
        schema,
    )]
}

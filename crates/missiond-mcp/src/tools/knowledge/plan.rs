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
        "[execute] default (current single-node v0 runner) | dag_v1 (Wave 12 minimal DAG scheduler, upgraded to wave-based runtime in Wave 13/02). dag_v1 only parses explicit `(node :id ... :target ... :depends-on [...])` forms inside PLAN.lisp; supported per-node fields: id / target / objective / depends-on / condition / failure-policy / timeout-ms / dispatch-strategy / target-project / requested-cwd / flow-id. failure-policy ∈ {fail-fast (default), continue}; unsupported fields are preserved into node_hint_summary.unsupported_fields and never silently dropped. Requires execute_mode=internal. v2 runtime drives ready nodes through a tokio JoinSet up to `max_parallel_nodes` per wave (default 1 = strict-sequential v1 behaviour); each per-node state transition (ready->running, running->{succeeded|failed}, pending->skipped) appends one plan_dag_node_dispatch evidence entry tagged with `state_transition`. Response surfaces scheduler_mode / node_count / max_parallel_nodes / node_results[] / skipped_nodes[] / paused_nodes[] / paused_node_ids[] / review_question_ids[] / retry_plan[] / aggregate_status / concurrency_plan. Wave 16 / Task 04 — per-node `:review-gate \"question-event\"` (with optional `:review-action` and `:review-text`) pauses the node and emits `QuestionEvent::Created` (deterministic id = `review:plan:<plan_id>:v<plan_version>:<action>:<node-id-hash>`, default action `plan-node`) instead of dispatching the target tool; downstream stays Pending; aggregate_status flips to `dag_paused` / runner_status to `review_gate_paused` when paused-only, plan status untouched. Bus publish failure still pauses (refuse to dispatch past a failed gate) and surfaces the warning under `bus_publish_warnings` + per-node `review_question_warning`. Auto-resume is OUT OF SCOPE here — the wave-16 / task 02 `QuestionEvent::Resolved` listener handles that. Wave 16 / Task 05 — per-node bounded retry policy: `:retry-count N` declares N **additional** attempts beyond the first; `:max-attempts N` declares N **total** attempts (parser converts to additional retries); optional `:retry-delay-ms M` adds an inter-attempt sleep capped at 60s. Defaults: no retry. Cap: 3 total attempts (extras above the cap are clamped). Negative / non-numeric / `:max-attempts 0` fails fast with `error_code=INVALID_PARAM`. Each attempt writes its own `ready->running` + `running->{succeeded|failed}` evidence pair tagged with the actual attempt number; final state is `succeeded` if any attempt passes, otherwise `failed` (taint propagates from the FINAL failure, fail-fast trips only after retries exhaust). Workstation-dispatch safe-descriptor refusals (unsupported_target / project_root_unresolved / missing_objective) are deterministic policy refusals and are NEVER retried — the per-node response carries `retry.non_retryable=true`. Dry-run shows `retry_plan[]` (only nodes that opted in) without dispatching. Wave 17 / Task 03 — deterministic acceptance evaluator: every node may declare `:acceptance-mode` (`inner_status` / `evidence_keys` / `manual`), `:acceptance-evidence-keys [...]`, and `:acceptance-commands [...]`. After a successful inner dispatch the scheduler runs a PURE evaluator (no shell, no subprocess) and surfaces the result under `node_results[].acceptance` + a dedicated `succeeded -> acceptance_{accepted|rejected|manual_required}` evidence row. Accepted keeps the node `succeeded`; rejected flips it to `failed` (taint propagates, fail-fast trips per `:failure-policy`); manual_required pauses the node with a deterministic `acceptance:plan:<plan_id>:v<version>:<node_id>` id (separate id space from wave-16 review-gate pauses; the wave-17 / task 01 paused-node resume helper deliberately does NOT consume these). Declaring `:acceptance-commands` without a typed `:acceptance-mode` defaults to `manual_required` — the scheduler refuses to execute shell commands authored in PLAN.lisp. Nodes that declare no acceptance hints preserve the wave-13 succeed-on-dispatch contract (the `acceptance` block is omitted from the response).",
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
        "[execute internal target=mission_task_delegate] Wave 15 / Task 05 — opt into workstation-dispatch v0. When true (or PLAN.lisp carries `:workstation-dispatch true`) the runner builds a scoped task brief (objective / scope / owned-files / forbidden-files / acceptance-commands / commit policy / completion-handoff (scoped commit) / agent-team hint when dispatch_strategy=agent-team) and dispatches via the existing `mission_task_delegate` substrate (NEVER `claude -p`). Project root is resolved via `resolve_target_project_root` (project > absolute cwd > target_project; relative cwd refused; no process-cwd fallback). On safety failure (target!=mission_task_delegate, project root unresolved, missing objective) the runner returns a structured `workstation_dispatch_status=skipped_*` descriptor instead of silently falling back. Response surfaces workstation_dispatch_status / dispatch_strategy / task_brief_preview / inner_result / evidence_path. Wave 16 / Task 03 conservative auto-inference: when omitted the runner auto-enables workstation dispatch ONLY when ALL of the following hold: resolved target = `mission_task_delegate`, dispatch_strategy ∈ {fresh-code-alignment, resident-lisp, agent-team, mixed}, objective non-empty, AND at least one scoping signal present (owned_files, scope, target_project, or requested_cwd). Explicit `false` always suppresses inference. The response always carries `workstation_dispatch_source` ∈ {explicit_arg, plan_hint, inferred, disabled, not_applicable} and `workstation_dispatch_inference_reason` (when set). Wave 17 / Task 07 scoped-commit handoff default: every workstation-dispatch brief now carries a `## Completion handoff (scoped commit)` section that instructs the worker to call `mission_execution(action=complete)` with `enforce_scoped_commit=true`. Code briefs (owned_files non-empty) demand `commit_status=\"committed\"` + `commit_hash` + `staged_files`; read-only briefs (owned_files empty) accept `commit_status=\"not-required\"` plus a written explanation in `summary`. The response always carries `scoped_commit_required=true` + `scoped_commit_policy=\"enforced-on-complete\"` so observers can pin the invariant. The daemon NEVER runs git itself — the worker performs the scoped commit and reports back. Legacy `mission_execution(action=complete)` callers keep `enforce_scoped_commit` defaulting to `false`; this default is opt-in at the brief layer ONLY.",
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
        "[execute internal workstation_dispatch=true] string or array of acceptance commands the delegated task must pass before commit (`cargo test ...`, `git diff --check`, ...). Spliced into the task brief's `## Acceptance commands` section. Plan-hint fallback: PLAN.lisp `:acceptance-commands [...]`. Capped at 32 entries. Wave 17 / Task 03: in scheduler_mode=dag_v1 the per-node `:acceptance-commands` list is now ALSO consumed by the deterministic acceptance phase that runs after a successful inner dispatch — but the scheduler NEVER executes the commands. Without a typed `:acceptance-mode` companion the gate surfaces as `acceptance_status=\"manual_required\"`, the commands round-trip into `node_results[].acceptance.commands` + the `succeeded -> acceptance_manual_required` evidence row, and the node pauses with a deterministic `acceptance:plan:<plan_id>:v<version>:<node_id>` id (distinct from the wave-16 review-gate id space).",
    ));

    p.insert("commit_policy".into(), prop(
        "string",
        "[execute internal workstation_dispatch=true] commit handoff policy. Default `scoped` — the brief always carries the literal reminder `do not stage or commit outside the owned files declared above`. Plan-hint fallback: PLAN.lisp `:commit-policy`. Caller wins. Wave 17 / Task 07: independent of this string, every workstation-dispatch brief also carries a `## Completion handoff (scoped commit)` section that pins the worker's `mission_execution(action=complete)` arguments (`enforce_scoped_commit=true` always; `commit_status` defaults to `committed` for code briefs and `not-required` for read-only briefs).",
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

    // ── wave-18 / task 07 — review automation policy v0 ─────────────────
    //
    // Explicit autonomy knob ORTHOGONAL to the wave-14 `review_gate_policy`
    // (which controls EMISSION). Default `manual` preserves the wave-15
    // behaviour byte-for-byte. `suggest` returns a deterministic
    // suggestion without mutation. `auto_safe` may auto-promote to
    // `approved` only when ALL safety rules pass — never auto-rejects,
    // never calls an LLM. Caller-supplied `review_decision` ALWAYS wins.
    p.insert("review_automation_policy".into(), prop_enum(
        "string",
        "[approve | mark | supersede] (wave-18 / task 07 review automation policy v0) explicit autonomy knob for the resolution surface. ORTHOGONAL to `review_gate_policy` (which controls EMISSION). `manual` (default) keeps the wave-15 behaviour: caller-supplied `review_decision` is the only authority and the response is byte-identical with pre-wave-18 callers. `suggest` makes the handler compute a deterministic suggestion (`suggested_review_decision`) and surfaces it WITHOUT mutating the plan. `auto_safe` may auto-promote to `approved` ONLY when ALL deterministic safety rules pass: producer ran in deterministic/dry-run mode (plan.compiler_model is None — i.e. dry-run), no file write OR file hash matches the supplied expected_file_sha256, no protected source/target, no unresolved conflicts, and the caller explicitly opted in via this knob. NEVER auto-rejects. NEVER calls an LLM. Caller-supplied `review_decision` ALWAYS wins (the policy never overrides explicit decisions). `mark` only auto-promotes when the requested target status is `approved`; other targets surface the suggestion only. `supersede` is intentionally NEVER auto-promoted (destructive transition). Response always carries `review_automation_policy` / `review_automation_status` / `suggested_review_decision` / `automation_reasons[]` when the knob was supplied.",
        &["manual", "suggest", "auto_safe"],
    ));

    p.insert("expected_file_sha256".into(), prop(
        "string",
        "[approve | mark | supersede review_automation_policy=auto_safe] (wave-18 / task 07) optional caller-supplied SHA-256 the deterministic safety inspector requires to match the on-disk PLAN.lisp hash. Pure additive guard: when the plan compile landed via the file-first writer the caller can capture `file_sha256` from the compile response and replay it here so an unexpected on-disk modification blocks `auto_safe`. Absent → strict-matching disabled (no file write attempted under approve/mark/supersede in v0; the rule still surfaces a passing audit row when omitted).",
    ));

    // ── wave-17 / task 01 — PLAN-DAG paused-node resume hook ────────────
    //
    // These knobs are opt-in. They route `mission_plan(action=execute,
    // execute_mode=internal)` through the dedicated paused-node resume
    // helper instead of the standard execute pipeline. Only resumes
    // nodes that paused via the deterministic
    // `review:plan:<plan_id>:v<version>:plan-node:<node-hash>` envelope
    // wave-16 / task 04 emits.
    p.insert("resume_review_question_id".into(), prop(
        "string",
        "[execute internal] (wave-17 / task 01) deterministic plan-node review question id from a wave-16 paused dispatch. Layout: `review:plan:<plan_id>:v<version>:plan-node:<sha256(node_id)[..16]>`. When supplied (with `resume_review_decision`), `mission_plan(action=execute)` routes through the paused-node resume helper: validates the envelope (plan id match, plan version match, action=plan-node, node-hash maps to exactly one node carrying `:review-gate \"question-event\"`), then on `approved` re-dispatches the paused node fresh (attempt 1; paused is non-terminal, not a failed attempt). `rejected` / `needs_changes` keep the node paused, write an audit row, and surface a next_step. Only resumes ONE node — downstream nodes still pending after the original paused dispatch stay pending until a follow-up execute call. Not general auto-approve: non-plan-node ids fail validation. `mission_plan` returns the existing wave-15 manager-side resolution input (`review_question_id` + `review_decision`) for compile/approve/mark/supersede actions; the resume input is a SEPARATE field set so the same request never accidentally triggers both paths.",
    ));

    p.insert("resume_review_decision".into(), prop_enum(
        "string",
        "[execute internal resume_review_question_id=*] (wave-17 / task 01) explicit decision attached to the supplied `resume_review_question_id`. Required when the id is supplied; absence with the id triggers a structured MISSING_PARAM error. `approved` re-dispatches the paused node fresh (attempt 1) and writes `paused -> resume_approved` + `ready -> running` + `running -> {succeeded|failed}` evidence; `rejected` keeps the node paused with `paused -> resume_rejected` evidence; `needs_changes` keeps it paused with `paused -> resume_needs_changes` evidence and a `next_step` recommendation in the response. Same vocabulary as the wave-15 review_decision contract.",
        &["approved", "rejected", "needs_changes"],
    ));

    p.insert("resume_actor".into(), prop(
        "string",
        "[execute internal resume_review_question_id=*] (wave-17 / task 01) free-form identity of the resolver. Echoed onto the response payload (`resume_actor`) and into the audit evidence so callers can correlate the decision with whoever made it; never used for authentication.",
    ));

    p.insert("resume_note".into(), prop(
        "string",
        "[execute internal resume_review_question_id=*] (wave-17 / task 01) free-form reason / next-step note. Echoed onto the response payload (`resume_note`) and into the audit evidence as the human-readable resume context.",
    ));

    // ── wave-17 / task 03 — PLAN-DAG deterministic acceptance evaluator ─
    //
    // These knobs only fire under `scheduler_mode=dag_v1` and only after
    // a successful inner dispatch. The acceptance evaluator is PURE
    // (no shell, no subprocess) — it inspects the node hints + the
    // inner payload and decides one of:
    //   * accepted          → node terminates `succeeded`
    //   * rejected          → node terminates `failed` (taint propagates,
    //                          fail-fast trips per `:failure-policy`)
    //   * manual_required   → node terminates `paused` with a deterministic
    //                          `acceptance:plan:<plan_id>:v<version>:<node_id>` id
    //                          (separate id space from wave-16 review-gate
    //                          pauses; wave-17 / task 01 resume helper
    //                          deliberately does NOT consume these)
    //   * not_evaluated     → no hints declared; preserve wave-13
    //                          succeed-on-dispatch contract
    //
    // The PLAN-side hint contract:
    //   `:acceptance-mode "inner_status" | "evidence_keys" | "manual"`
    //   `:acceptance-evidence-keys ["k1" "k2" ...]`  (only for evidence_keys)
    //   `:acceptance-commands ["cargo test" ...]`    (declared only —
    //                                                  NEVER executed)
    p.insert("acceptance_mode".into(), prop_enum(
        "string",
        "[execute scheduler_mode=dag_v1] (wave-17 / task 03) declarative acceptance evaluator mode (PLAN.lisp hint `:acceptance-mode`). `inner_status` accepts when the inner dispatch returned Ok and the payload carries no explicit error signal; `evidence_keys` accepts when the inner payload contains every key declared in `:acceptance-evidence-keys` (descends into well-known nested holders like `evidence` / `inner_result`); `manual` always pauses. Absent / unrecognised values fall through to the conservative default: any `:acceptance-commands` declared without a typed mode triggers `manual_required` (the scheduler NEVER runs shell commands from PLAN.lisp). Unrecognised raw values surface in `node_hint_summary.unsupported_fields` so typos are loud.",
        &["inner_status", "evidence_keys", "manual"],
    ));

    p.insert("acceptance_evidence_keys".into(), prop_no_type(
        "[execute scheduler_mode=dag_v1 acceptance_mode=evidence_keys] (wave-17 / task 03) string or array of required keys (PLAN.lisp hint `:acceptance-evidence-keys`). The evaluator scans the inner payload — top-level object first, then well-known nested holders (`evidence`, `typed_evidence`, `inner_result`, `inner_dispatch`, `result`) — and accepts only when every required key is present. Empty list under `acceptance_mode=evidence_keys` degrades to `manual_required` so the typo is loud. Wave 18 / Task 03: in cross-node fan-in mode (`:acceptance-requires \"evidence_keys\"`) the SAME list is used to inspect the source node's `inner_payload`.",
    ));

    // ── wave-18 / task 03 — PLAN-DAG cross-node acceptance fan-in ──────
    //
    // These knobs are PLAN.lisp node hints (parsed from `(node ...)`)
    // and are NOT plumbed as top-level args. They opt the node's
    // acceptance phase into a conservative dependency on prior nodes'
    // terminal status / evidence. Documented here so callers can pin
    // the shape against `node_results[].acceptance.fan_in` without
    // hunting through the source.
    //
    // PLAN.lisp surface:
    //   `:acceptance-depends-on ["node-a" "node-b"]`
    //   `:acceptance-requires "all_succeeded" | "any_succeeded" | "evidence_keys"`
    //   `:acceptance-source-node "node-a"`   (only under `evidence_keys`)
    //
    // Constraints (validator raises structured errors otherwise):
    //   * Every entry in `:acceptance-depends-on` MUST be a declared
    //     plan node id AND a (transitive) `:depends-on` ancestor of
    //     the current node — acceptance deps NEVER silently change
    //     dispatch order.
    //   * Non-empty `:acceptance-depends-on` requires a recognised
    //     `:acceptance-requires` mode.
    //   * `evidence_keys` mode requires `:acceptance-source-node` to
    //     point at one of the deps.
    //
    // Evaluator behaviour:
    //   * `all_succeeded` — fan-in passes when every listed node is
    //     `Succeeded`.
    //   * `any_succeeded` — fan-in passes when at least one listed node
    //     is `Succeeded`.
    //   * `evidence_keys` — fan-in passes when the source node's
    //     `inner_payload` contains every key declared in
    //     `:acceptance-evidence-keys` (descends into the same
    //     well-known nested holders the wave-17 evidence_keys mode
    //     uses).
    //
    // Interaction with the wave-17 per-node evaluator:
    //   * Per-node `Rejected` / `ManualRequired` always dominates;
    //     fan-in is recorded for audit but does NOT promote a node.
    //   * Per-node `Accepted` / `NotEvaluated` paired with a fan-in
    //     PASS keeps / promotes the node to `Accepted`.
    //   * Fan-in FAIL flips the node to `Rejected` (taint propagates,
    //     fail-fast trips per `:failure-policy`).
    //
    // Surface (per-node response + evidence row):
    //   `node_results[].acceptance.fan_in = {
    //       mode, source_nodes, passed, reason
    //   }`
    //   evidence extra fields:
    //     `acceptance_fan_in` / `acceptance_fan_in_mode` /
    //     `acceptance_fan_in_source_nodes` /
    //     `acceptance_fan_in_passed` / `acceptance_fan_in_reason`.

    p.insert("acceptance_depends_on".into(), prop_no_type(
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 03) array of prior node ids the current node's acceptance phase depends on. Spelled `:acceptance-depends-on` in PLAN.lisp. Every entry MUST be a (transitive) `:depends-on` ancestor of the current node — the validator raises a structured error otherwise so acceptance deps cannot silently change execution order.",
    ));

    p.insert("acceptance_requires".into(), prop_enum(
        "string",
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 03) cross-node acceptance fan-in mode. Spelled `:acceptance-requires` in PLAN.lisp. `all_succeeded` accepts when every node in `:acceptance-depends-on` is Succeeded; `any_succeeded` accepts when at least one is Succeeded; `evidence_keys` reads `:acceptance-evidence-keys` from the `:acceptance-source-node`'s inner payload. Required when `:acceptance-depends-on` is non-empty; unrecognised values fail the build with a structured error.",
        &["all_succeeded", "any_succeeded", "evidence_keys"],
    ));

    p.insert("acceptance_source_node".into(), prop(
        "string",
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 03) single source node id for `:acceptance-requires \"evidence_keys\"` fan-in. Spelled `:acceptance-source-node` in PLAN.lisp. MUST appear in the same node's `:acceptance-depends-on` list.",
    ));

    // ── wave-17 / task 04 — PLAN-DAG conservative rollback descriptors ──
    //
    // These knobs only fire under `scheduler_mode=dag_v1` and only on
    // a node's FINAL failed attempt (after retries exhaust / non-retryable
    // refusal). The rollback evaluator NEVER runs destructive shell
    // commands on its own — it either records intent (`descriptor` mode)
    // or hands a scoped task brief to the wave-15 workstation substrate
    // (`workstation` mode, ONLY when every safety condition holds).
    //
    // Defaults:
    //   * `:rollback-policy` absent OR `"none"` → no rollback at all
    //                                              (preserves pre-wave17
    //                                              behaviour).
    //   * No safety violation: every gate must hold for `workstation`
    //                          mode — otherwise the node surfaces
    //                          `rollback_status="refused"` with the
    //                          failing condition spelled out.
    //
    // SafeDescriptor refusals from the rollback substrate (substrate-
    // side `MissingObjective` / `ProjectRootUnresolved` / etc.) collapse
    // to `rollback_status="refused"` and are NEVER retried.
    //
    // Failure-policy interaction: the rollback pass runs AFTER the
    // final failed attempt and BEFORE downstream taint propagation.
    // Downstream behaviour stays governed by the existing
    // `:failure-policy` (`fail-fast` / `continue`); the rollback
    // outcome NEVER changes whether downstream nodes are tainted /
    // aborted.
    p.insert("rollback_policy".into(), prop_enum(
        "string",
        "[execute scheduler_mode=dag_v1] (wave-17 / task 04) per-node rollback policy (PLAN.lisp hint `:rollback-policy`). `none` (default) preserves the pre-wave17 contract — failed node taints downstream per `:failure-policy`, no rollback descriptor / dispatch. `descriptor` records the rollback intent (objective + owned files + acceptance commands + brief preview) on the response and an audit row, but NEVER dispatches. `workstation` opts into automatic rollback dispatch through the wave-15 workstation-dispatch substrate — only when ALL safety gates pass: target-project (or requested-cwd) resolved, `:rollback-objective` non-empty, `:rollback-owned-files` non-empty, `:dispatch-strategy` on the safe whitelist {fresh-code-alignment, resident-lisp, agent-team, mixed}. Otherwise surfaces as `rollback_status=\"refused\"` with the failing condition. Unrecognised raw values fall back to absent (no rollback) and surface in `node_hint_summary.unsupported_fields` so typos are loud. The rollback pass NEVER alters failure-policy semantics: downstream taint propagation runs identically afterwards. Wave 18 / Task 04: when a failed node ALSO declares `:rollback-cascade \"plan\"` or `\"dispatch-safe\"`, the cascade evaluator runs AFTER the node-local rollback and folds an ordered compensation plan onto `node_results[].rollback.cascade` (every plan node carrying `:compensates \"<failed-node-id>\"`, ordered by `:rollback-after`). `plan` mode never dispatches; `dispatch-safe` only dispatches compensation nodes that pass the wave-17 / task 04 safety gate (descriptor-only or absent compensations stay `descriptor_ready` — never silently promoted). Refusals are non-retryable; the cascade NEVER prompts the fallback path.",
        &["none", "descriptor", "workstation"],
    ));

    p.insert("rollback_objective".into(), prop(
        "string",
        "[execute scheduler_mode=dag_v1 rollback_policy=descriptor|workstation] (wave-17 / task 04) free-form objective for the rollback brief (PLAN.lisp hint `:rollback-objective`). Required (non-empty) for `workstation` mode — its absence is one of the safety refusals. Echoed verbatim under `descriptor` mode so observers / out-of-band tooling can act on the intent.",
    ));

    p.insert("rollback_owned_files".into(), prop_no_type(
        "[execute scheduler_mode=dag_v1 rollback_policy=workstation] (wave-17 / task 04) string or array of file paths the rollback task is allowed to stage / commit (PLAN.lisp hint `:rollback-owned-files`). Required (>= 1 entry) for `workstation` mode — workstation dispatch with no owned files would let the rollback agent touch arbitrary parts of the tree, which is the exact thing the scoped-commit invariant exists to prevent. Surfaced verbatim under `descriptor` mode too.",
    ));

    p.insert("rollback_acceptance_commands".into(), prop_no_type(
        "[execute scheduler_mode=dag_v1 rollback_policy=descriptor|workstation] (wave-17 / task 04) string or array of acceptance commands the rollback task must pass before commit (PLAN.lisp hint `:rollback-acceptance-commands`). Surfaced verbatim into the rollback brief AND the descriptor + evidence row, BUT the scheduler NEVER executes them (mirrors the wave-17 / task 03 acceptance-commands invariant — `acceptance_commands_executed=false` is pinned on every rollback evaluation).",
    ));

    // ── wave-18 / task 04 — PLAN-DAG conservative cascade rollback ──────
    //
    // These knobs only fire under `scheduler_mode=dag_v1` and only on
    // a node's FINAL failed attempt, AFTER the wave-17 / task 04 node-local
    // rollback evaluator. The cascade evaluator is conservative:
    //
    //   * Defaults to `none` — node-local rollback behaviour is preserved
    //     byte-for-byte for plans that did not opt in.
    //   * `plan` mode ONLY records intent (which compensation nodes WOULD
    //     run, in what order). NEVER dispatches.
    //   * `dispatch-safe` mode dispatches a compensation node ONLY when
    //     that compensation node's own rollback safety gates pass
    //     (`:rollback-policy "workstation"` + every wave-17 / task 04 gate).
    //     SafeDescriptor / safety-gate refusals are recorded as
    //     `refused`, NEVER retried, and the cascade NEVER prompts the
    //     fallback path.
    //
    // Compensation discovery: every plan node carrying
    // `:compensates "<failed-node-id>"` is a candidate. Ordering
    // follows the topological order induced by `:rollback-after`
    // (cycle-tolerant — falls back to declaration order when a typo
    // creates a cycle so the cascade never deadlocks).
    p.insert("compensates".into(), prop(
        "string",
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 04) failed-node id this node compensates for. Spelled `:compensates \"<failed-node-id>\"` in PLAN.lisp. Pure metadata: declaring `:compensates` does NOT make this node dispatch automatically — only the cascade evaluator (running AFTER the named node fails AND only when the failed node opted into `:rollback-cascade`) consults the field.",
    ));

    p.insert("rollback_cascade".into(), prop_enum(
        "string",
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 04) per-node cascade rollback mode (PLAN.lisp hint `:rollback-cascade`). `none` (default) preserves the wave-17 / task 04 byte shape — only the node-local rollback runs. `plan` records the ordered cascade plan (every compensation node carrying `:compensates \"<this-failed-node>\"`) on the response + an audit row, but NEVER dispatches. `dispatch-safe` records the same plan AND, for every compensation node whose own rollback safety gates pass (`:rollback-policy \"workstation\"` + the wave-17 / task 04 gates), dispatches it through the wave-15 workstation substrate. SafeDescriptor / safety-gate refusals are non-retryable; the cascade NEVER prompts the fallback path. Compensation nodes that did not opt into `:rollback-policy \"workstation\"` (descriptor-only or absent) stay `descriptor_ready` — `dispatch-safe` will NOT promote a non-workstation compensation to a dispatch. Unrecognised raw values fall back to `none` and surface in `node_hint_summary.unsupported_fields` so typos are loud. Cascade ordering follows `:rollback-after` (cycle-tolerant — typos fall back to declaration order so the cascade never deadlocks).",
        &["none", "plan", "dispatch-safe"],
    ));

    p.insert("rollback_after".into(), prop_no_type(
        "[PLAN.lisp node hint only — not a top-level arg] (wave-18 / task 04) ordering hint for cascade compensation order. Spelled `:rollback-after [\"node-a\" \"node-b\"]` in PLAN.lisp (also accepts paren list and bareword run). When two compensation nodes both declare `:compensates` for the same failed node, the cascade evaluator runs them in the topological order induced by `:rollback-after`. NOT promoted to a real `:depends-on` — cascade ordering MUST NOT silently change forward dispatch order. Cycles in the `:rollback-after` graph fall back to declaration order so a typo never deadlocks the cascade.",
    ));

    // ── wave-17 / task 02 — PLAN-DAG claim / lease discipline ──────────
    //
    // These knobs add claim/lease coordination to PLAN DAG node dispatch.
    // The scheduler binds to the wave12-01 mission_execution coordination
    // model: same `scopes_overlap` predicate, same lease semantics. There
    // is NO new lock service — the per-DAG-run registry exists only
    // for the lifetime of one execute call.
    p.insert("claim_lease_secs".into(), prop(
        "integer",
        "[execute internal scheduler_mode=dag_v1] (wave-17 / task 02) per-node claim lease duration in seconds. Default 1800 (30 min). Clamped to [60, 14400] (1 min .. 4 hr) so authoring typos cannot stall the wave loop with a 0s lease nor pin a scope for the rest of the day. Mirrors the wave12-01 mission_execution lease defaults so the two surfaces print identical timestamps. Surfaced on every response (live + dry-run) under `claim_lease_secs`.",
    ));

    p.insert("claimer_name".into(), prop(
        "string",
        "[execute internal scheduler_mode=dag_v1] (wave-17 / task 02) identity stamped onto every plan-DAG claim record. Default `plan-dag-scheduler`. Empty / whitespace-only normalises to the default so a blank form field doesn't poison the audit log. Matches the wave12-01 `:claimer` vocabulary so audit dashboards can grep one identity across companion-log claims AND DAG claims. Echoed onto every response under `claimer_name`.",
    ));

    p.insert("enforce_claims".into(), prop(
        "boolean",
        "[execute internal scheduler_mode=dag_v1] (wave-17 / task 02) opt into strict claim enforcement. Default `false` (backward-compatible — claim metadata still surfaces on the evidence + response so observers can tell the discipline ran, but conflicts NEVER block dispatch). When `true`, an unresolvable scope overlap fails the node fast with structured `CLAIM_CONFLICT` (the inner handler is NEVER invoked); the per-node response carries `state=failed` + `reason=CLAIM_CONFLICT...` + `inner_payload.claim_status=conflict` + the conflicting claim snapshot. Reuses the same `scopes_overlap` predicate as wave12-01 (`mission_execution`) and wave16-06 (`enforce_scoped_commit_completion`) so the three call sites cannot drift. Per-node lifecycle becomes `pending -> claimed -> running -> {succeeded|failed} -> released` with one evidence row per transition; release timestamps land on the `claimed -> released` row. Dry-run shows planned claims under `planned_claims[]` without mutating.",
    ));

    // ── wave-17 / task 05 — DAG finalize + distill trigger v0 ──────────
    //
    // These knobs are opt-in. Without `finalize_plan=true` the response
    // shape stays byte-identical with the wave-17 / task 04 baseline; the
    // existing per-aggregate `plan_update_status` side-effect already runs
    // unconditionally so backward-compat callers see no behaviour change.
    p.insert("finalize_plan".into(), prop(
        "boolean",
        "[execute scheduler_mode=dag_v1] (wave-17 / task 05) opt into the explicit finalization pass. Default `false` (preserves the wave-17 / task 04 byte-shape — only the existing plan_update_status side-effect runs). When `true`, the response carries an additional `finalization` block describing the aggregate -> plan_status mapping (`dag_succeeded` → succeeded; `dag_failed` / `dag_partial` → failed; `dag_paused` → preserves current status — the runner refuses to claim success while a node awaits review) plus an audit `dag_finalized` evidence row with the same projection. Setting `distill_on_success=true` requires this knob to be `true` too — distill is gated on a successful finalization.",
    ));

    p.insert("distill_on_success".into(), prop(
        "boolean",
        "[execute scheduler_mode=dag_v1 finalize_plan=true] (wave-17 / task 05) when true AND `finalize_plan=true` AND the aggregate resolves to `dag_succeeded` AND the plan FSM update lands on `succeeded`, automatically invoke `mission_workflow(action=distill, plan_id=<plan>)` after the finalization step. The response surfaces a `finalization.distill` block with `triggered` + `reason` + `distill_mode` + the inner workflow payload under `result`. Distill failure surfaces a non-fatal `warning` on the same block — the plan final state is NEVER downgraded when distill errors (per the wave-17 / task 05 brief: distill failure does NOT corrupt the plan final state). Project / cwd / target_project signals are forwarded verbatim so the distill handler resolves the same project root the DAG run wrote evidence into. `distill_on_success=true` without `finalize_plan=true` returns INVALID_PARAM (fail-fast: silently ignoring the request would mask caller intent).",
    ));

    p.insert("distill_mode".into(), prop_enum(
        "string",
        "[execute scheduler_mode=dag_v1 finalize_plan=true distill_on_success=true] (wave-17 / task 05) forwarded verbatim to `mission_workflow(action=distill)`. Default `dry_run` (the conservative preview: emits a `(workflow-draft …)` descriptor, no LLM, no persistence). Set `sonnet` to invoke the workflow-distiller actor v0 (Sonnet over plan + evidence sidecar). Allowlist matches `workflow.rs::parse_distill_mode` so the two surfaces cannot drift. Validation runs even in dry-run / `distill_on_success=false` so a typo (`sonet`) fails fast on the next live run instead of being silently ignored.",
        &["dry_run", "sonnet"],
    ));

    // ── wave-18 / task 05 — cross-plan distill chain v0 ────────────────
    //
    // These knobs run AFTER the wave-17 finalize+distill pass and let
    // the caller mark this plan as a contributor to a named chain that
    // spans multiple successful plans. Conservative: nothing fires
    // unless `finalize_plan=true` AND the inner finalization resolves
    // to `final_plan_status="succeeded"`. Chain failures NEVER downgrade
    // the underlying plan finalization — they only surface a warning.
    p.insert("distill_chain_id".into(), prop(
        "string",
        "[execute scheduler_mode=dag_v1 finalize_plan=true] (wave-18 / task 05) opt into the cross-plan distill chain by tagging this plan's chain entry with the supplied id. Free-form caller-chosen string (e.g. `chain:wave18-finalize-loop`). Required to be non-blank when supplied; blank / whitespace collapses to absent. When omitted but other chain knobs are set, runner derives a deterministic fallback (`chain:auto:plan-<plan_id>`) so the audit row never carries an empty id. Echoed on response as `distill_chain_id` plus inside `finalization.distill_chain.chain_id`. Multiple plans pinning the same id form a chain — each contributes ONE evidence row to its OWN sidecar; readers correlate by id. Per CLAUDE.md fast-fail: any chain knob without `finalize_plan=true` returns INVALID_PARAM.",
    ));

    p.insert("distill_chain_mode".into(), prop_enum(
        "string",
        "[execute scheduler_mode=dag_v1 finalize_plan=true] (wave-18 / task 05) controls whether the chain orchestrator invokes `mission_workflow(action=distill)` after recording the chain entry. Default `record_only` (no workflow call — only the evidence sidecar entry is appended; the chain serves purely as a cross-plan audit ledger). `dry_run` forwards to `mission_workflow(action=distill, distill_mode=\"dry_run\")` after the chain row is appended (preview, no LLM, no persistence). `sonnet` forwards to `mission_workflow(action=distill, distill_mode=\"sonnet\")` and runs the workflow-distiller actor v0 — required to be set EXPLICITLY (the chain orchestrator never auto-promotes record_only to sonnet). Allowlist mirrors `workflow.rs::parse_distill_mode` plus the chain-only `record_only` literal. Validation runs eagerly so typos (`sonett`) fail fast even when no other chain knob was supplied. Distill failures (handler error, workflow-distiller refusal) surface a non-fatal `distill_chain_warning` on the response — the wave-17 finalization block is preserved verbatim per the brief: chain failure NEVER corrupts the plan final state.",
        &["record_only", "dry_run", "sonnet"],
    ));

    p.insert("distill_chain_name".into(), prop(
        "string",
        "[execute scheduler_mode=dag_v1 finalize_plan=true] (wave-18 / task 05) optional human-readable label for the chain (e.g. `wave18-finalize-loop` or `release-rc1-distill`). Echoed back under `finalization.distill_chain.chain_name` and stamped onto the evidence row. When `distill_chain_mode=\"dry_run\"|\"sonnet\"` is also set, the chain runner forwards this value as `mission_workflow(action=distill).name` so a persisted distill row carries the chain label — caller can still pre-populate `name` separately to override. Blank / whitespace collapses to absent.",
    ));

    // ── wave-18 / task 06 — autonomous PLAN field inference v0 ──────────
    //
    // Conservative deterministic helper that infers a small set of PLAN
    // DAG fields when the caller / PLAN.lisp / evidence-sidecar carry
    // enough signal. NEVER calls an LLM; NEVER mutates over a caller-
    // supplied value (conflicts surface on `conflicts[]` and stay as
    // suggestions); ONLY high-confidence inferences are applied under
    // `apply_safe` (low / medium always degrade to suggestions).
    p.insert("infer_plan_fields".into(), prop_enum(
        "string",
        "[execute] (wave-18 / task 06) opt into autonomous PLAN field inference. Default `off` preserves the legacy byte-shape — no inference, no response augmentation. `preview` runs the deterministic engine over plan.sexp_text + plan.compiled_from + the most-recent 16 evidence-sidecar entries and returns ONLY the inference block (status=`inference_preview`, runner_status=`inference_preview_no_dispatch`); the underlying execute pipeline is NOT invoked, no plan FSM transitions. `apply_safe` runs the same engine, then augments caller args with every field whose confidence is `high` AND whose caller-side slot is empty, then proceeds with execute exactly as if the caller had passed the augmented args. Caller-supplied values ALWAYS win — conflicts (caller value differs from inferred) land on `conflicts[]` and are NEVER mutated. Six fields supported in v0: `target` / `dispatch_strategy` / `target_project` / `owned_files` / `acceptance_mode` / `workstation_dispatch`. Sources scanned: `plan_sexp` (PLAN.lisp `:keyword` hints — high confidence), `compiled_from` (directive provenance string keyword scan — medium confidence), `evidence_sidecar` (historical `plan_runner_dispatch` / `workstation_dispatch` entries — medium / high based on multi-entry agreement). Response surfaces under `plan_field_inference`: `mode` / `inference_status` / `inferred_fields[]` (each {field, value, confidence, source, detail}) / `suggested_fields[]` (sub-threshold) / `conflicts[]` ({field, caller_value, inferred_value, confidence, source}) / `evidence_sources[]` (which knobs the engine scanned). NEVER calls an LLM; deterministic heuristics only. The block is also attached to dispatched responses under `apply_safe` so observers can audit which fields auto-filled.",
        &["off", "preview", "apply_safe"],
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
         wave-17 / task 07 scoped-commit handoff default: 每条 workstation-dispatch brief 现在固定包含 \
         `## Completion handoff (scoped commit)` 段，指示 worker 在调 mission_execution(action=complete) 时必须传 \
         enforce_scoped_commit=true。Code brief (owned_files 非空) 要求 commit_status=\"committed\" + commit_hash + staged_files; \
         read-only brief (owned_files 空) 默认 commit_status=\"not-required\" 并要求在 summary 写明原因。\
         响应在 dispatched / dry_run / inner_returned_error / safe-descriptor 全分支固定附带 \
         scoped_commit_required=true + scoped_commit_policy=\"enforced-on-complete\"，便于观察方一行 assert。\
         daemon 永不跑 git，worker 自己 commit 并把 hash 报回。LEGACY mission_execution(action=complete) 调用者 \
         enforce_scoped_commit 仍默认 false，向后兼容；本次只在 brief 层 opt-in。Future work: git pre-commit hook 在 \
         worktree 强制 staged path 子集化 owned_files (本任务不实现，列入待办)。\
         wave-17 / task 01 PLAN-DAG paused-node resume hook: execute internal 时再传 \
         resume_review_question_id (= 上一次 wave-16 paused dispatch 返回的 \
         `review:plan:<plan_id>:v<version>:plan-node:<sha256(node_id)[..16]>` id) + \
         resume_review_decision (approved | rejected | needs_changes) 即路由到 plan-node 恢复 helper — \
         先 validate envelope (plan id 一致, plan version 一致, action=plan-node, \
         node-hash 唯一映射到一个仍带 :review-gate \"question-event\" 的节点)，\
         approved → 以 fresh attempt 1 重 dispatch 那一个节点 (paused 不算失败 attempt) 并写 \
         paused→resume_approved + ready→running + running→{succeeded|failed} 三条 evidence；\
         rejected → 节点保持 paused, 写 paused→resume_rejected evidence, surface next_step；\
         needs_changes → 同上但 paused→resume_needs_changes + 不同 next_step。只 resume 这一个节点, \
         downstream pending 节点不动 (后续 execute 推进)。非 plan-node id 在 validation 阶段 fail-fast。\
         同一份 review:plan:* 但 action 不是 plan-node 时仍走 wave-15 的 review_question_id + review_decision \
         manager-action bridge — 两个 input field set 严格分离, 同一请求绝不会同时触发。\
         wave-16 / task 02 review-resolution listener 同步扩展: 收到 QuestionEvent::Resolved 且 \
         id scope=plan + action=plan-node 时, listener 自动调用同一 resume helper (approved → 重 dispatch；\
         rejected/needs_changes → 写 evidence keep paused)，非 plan-node id 保留旧行为。\
         未识别 resolution 字符串永远 ignore + warn, 绝不 dispatch。\
         wave-17 / task 02 PLAN-DAG claim/lease discipline: scheduler_mode=dag_v1 时再传 \
         claim_lease_secs (默认 1800, clamp [60, 14400]) / claimer_name (默认 plan-dag-scheduler) / \
         enforce_claims (默认 false, 向后兼容) — 调度器在 pending 与 running 之间插入 claimed 中间态, \
         绑定 wave12-01 mission_execution coordination 模型 (复用 scopes_overlap 谓词, 不引入新 lock service)。\
         Node scope 派生优先级: :owned-files (每个文件单独成 scope token) > :scope (整段字符串) > \
         plan/<plan_id>/node/<node_id> 合成 fallback (永不为空)。enforce_claims=true 时遇 overlap 立即 \
         fail-fast 节点 (state=failed, reason=CLAIM_CONFLICT, 内层 handler 永不调用); enforce_claims=false 时 \
         best-effort 记 claim metadata 但绝不 block dispatch。每个节点 lifecycle 变成 \
         pending -> claimed -> running -> {succeeded|failed} -> released, 每次转移一条 evidence; \
         claim_id / claimer / claim_scopes / claim_scope_source / claim_acquired_at / claim_lease_expires_at 写在 \
         pending->claimed evidence; claim_released_at / claim_terminal_state 写在 claimed->released evidence。\
         dry_run 在响应里附 planned_claims[] (含每个节点的 claim_id / scopes / scope_source / lease_secs / enforce_claims) \
         不 mutate。响应总附 planned_claims / claim_lease_secs / claimer_name / enforce_claims, 调用方可对比 dry-run vs live。\
         wave-17 / task 04 PLAN-DAG conservative rollback descriptors: scheduler_mode=dag_v1 时节点可声明 \
         :rollback-policy `none|descriptor|workstation`, :rollback-objective, :rollback-owned-files, \
         :rollback-acceptance-commands。默认无 :rollback-policy 或 \"none\" → 不做任何 rollback (preserves \
         pre-wave17 behaviour)。`descriptor` → 仅记录意图 + brief preview, 永不 dispatch; `workstation` → \
         仅当所有安全条件满足 (target_project 或 requested_cwd 已解析、:rollback-objective 非空、\
         :rollback-owned-files >=1 条、:dispatch-strategy 在安全白名单 {fresh-code-alignment / resident-lisp / \
         agent-team / mixed}) 才通过 wave-15 workstation-dispatch substrate dispatch rollback task; 否则 \
         rollback_status=\"refused\" + 失败条件原因。Substrate-side SafeDescriptor 拒绝同样 collapse 到 refused, \
         永不 retry。每次 rollback 评估都 emit failed -> rollback_{not_requested|descriptor_ready|dispatched|\
         refused|failed} 一条 evidence; node_results[].rollback 块包含 policy / status / reason / objective / \
         owned_files / acceptance_commands / acceptance_commands_executed=false (pinned, 证明永不执行) / \
         task_brief_preview (有则) / inner_result (workstation dispatched / failed 时)。Failure-policy 交互: \
         rollback 在最终失败 attempt **之后**, downstream taint propagation **之前**; 下游行为仍由 \
         existing :failure-policy 决定, rollback 永不 alter taint / fail-fast。\
         wave-18 / task 04 PLAN-DAG conservative cascade rollback: 节点可声明 \
         :rollback-cascade `none|plan|dispatch-safe` (默认 none, 保留 wave-17/04 byte shape), \
         :compensates `<failed-node-id>` (其它节点声明此字段表示 compensate 该 failed node), \
         :rollback-after [...] (compensation 节点之间可选 ordering hint, 永不提升为 :depends-on)。\
         `plan` 模式仅记录 ordered compensation plan (每个 compensation 节点的 descriptor + brief preview), \
         绝不 dispatch; `dispatch-safe` 模式仅在每个 compensation 节点自身 rollback safety gates 全过 \
         (:rollback-policy \"workstation\" + 全部 wave-17/04 gates) 时才 dispatch, 否则 refused 不 retry。\
         Compensation 节点没有 :rollback-policy \"workstation\" (descriptor 或缺省) 在 dispatch-safe 模式下 \
         也只记 descriptor_ready, 绝不被自动升级 dispatch。SafeDescriptor / safety-gate 拒绝同样 collapse 到 \
         refused, 永不 prompt fallback。Cascade ordering 用 :rollback-after 拓扑排序 (cycle-tolerant, \
         typo 出现 cycle 时 fallback 到 declaration order 防 deadlock)。Cascade 结果挂在 \
         node_results[].rollback.cascade + evidence.rollback_cascade_* 字段 \
         (mode / cascade_root / compensations[] / reason)。绝不 arbitrary rollback code execution: \
         cascade 评估者只 dispatch wave-15 workstation substrate, 绝不直接 shell out 或 prompt fallback。\
         wave-18 / task 05 cross-plan distill chain v0: scheduler_mode=dag_v1 finalize_plan=true 时再传 \
         distill_chain_id (任意非空字符串, 缺省时按 plan id 派生 chain:auto:plan-<plan_id>) + \
         distill_chain_mode (record_only [默认] / dry_run / sonnet) + 可选 distill_chain_name 即把当前 \
         plan 标记为跨 plan 蒸馏链的一员。仅在 wave-17 finalization 决出 final_plan_status=succeeded \
         时才执行链动作; failed/paused/no-finalize 一律 skip 并在响应里 surface skip reason \
         (skipped_plan_not_succeeded / skipped_no_finalization)。record_only 仅向当前 plan 的 \
         evidence sidecar append 一条 distill_chain_record 行 (kind=distill_chain_record, \
         schema_version=v0, 不调 workflow distill); dry_run / sonnet 在 append 之前调用 \
         mission_workflow(action=distill, distill_mode=...), 把 inner result 挂在 \
         finalization.distill_chain.distill_result + 顶层 distill_result 短路。sonnet 必须显式传入, \
         链 runner 永不自动从 record_only 升格 sonnet。Append 永远 additive (底层 writer 永不覆盖), \
         不同 plan 的链条目分散在各自 sidecar 里, reader 通过 chain_id 串联; 当前 plan 内已存在的 \
         同 chain_id 行被计入 chain_index_in_plan (UX 字段, 不影响判定)。任意 chain knob 但缺 \
         finalize_plan=true 立即返 INVALID_PARAM (fast-fail, 与 wave-17 distill_on_success 同政策)。\
         distill_chain_mode 严白名单 [record_only, dry_run, sonnet], 即使 chain knob 缺也校验, 防止 \
         typo 漏到下一次实战; sonet/sonett 立即 INVALID_PARAM。链失败 (workflow handler 返 error / \
         sidecar 写失败) 仅 surface distill_chain_warning + status=record_failed|recorded_with_distill_warning, \
         绝不下调 plan finalization (与 wave-17 distill 失败同政策: 链失败永不污染 plan final state)。\
         响应固定附 distill_chain_status / distill_chain_id / finalization.distill_chain {triggered, \
         status, chain_id, chain_id_source, chain_mode, [chain_name], [chain_index_in_plan], \
         [distill_result], [warning], [evidence_path], [evidence_error]}。\
         Lisp 源: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract / :dispatch-strategy-consumer \
         + intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-compiler / plan-runner \
         + intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s4 plan-authoring / s5 plan-review-gate / s6 execution-runner \
         + intent-flow.lisp :: F-workstation-dispatch-policy \
         + intent-worker.lisp :: claudecode-workstation-orchestration \
         + intent-memory.lisp :: directive-layer :: file-first-artifacts :: plan-lisp。",
        schema,
    )]
}

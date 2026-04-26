;; Wave 20 / Task 04 — Machine-driven task contract dispatch v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp

(report wave20-04-machine-driven-dispatch-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave20-04-machine-driven-dispatch-v0"
  :status done
  :commit_hash "681c95df3eaf807f0c2c4cd278da593d69ecb76d"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
      :exit_code 0
      :ok true
      :notes "75 passed (1 wave-20 surface test added on top of wave-19 baseline 74)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "235 passed (10 wave-20 dispatch_contract_mode parser + machine-mode response surface tests added on top of wave-20/07 baseline 225)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
      :exit_code 0
      :ok true
      :notes "249 passed (4 wave-20 TaskContractDispatchCtx machine-mode tests added on top of wave-20/07 baseline 245)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
      :exit_code 0
      :ok true
      :notes "52 passed (2 wave-20 forwarding tests added on top of wave-20/07 baseline 50)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1246 passed (baseline 1230 + 16 new wave-20/04 tests across plan / plan_dag / workstation_dispatch / unified_entry)")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed (plan schema descriptor extension exercised via test_plan_actions_match_lisp)")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "26 task contracts OK")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in scoped files")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "staged scope guard OK (5 staged files inside :write-scope, no :must-not-touch overlap)")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp"
      :exit_code 0
      :ok true
      :notes "post-commit verifier OK against commit 681c95df3eaf")]

  :scope_deviations []

  :notes
    "wave20-04 wires the wave-19 / task 07 dormant consumer (run_workstation_dispatch_with_contract) into the active dispatch path via an opt-in dispatch_contract_mode. New surface: DispatchContractMode enum {Rendered (default), Machine} parsed from `dispatch_contract_mode` (string allowlist) or boolean shorthand `render_markdown` (false ⇒ Machine). Default `Rendered` preserves the wave-15..19 byte-shape exactly — every legacy caller keeps the same response wire shape (verified by the existing 1230 daemon tests still passing). Machine mode behaviour: when a wave-19 emitter wrote a contract for THIS dispatch AND the dispatch routes through workstation substrate, the runner forwards the resolved on-disk path so the consumer loads + parses the task.lisp directly (overlaying every non-empty contract field onto the brief — contract is the SSOT). The brief preview now carries the wave-19/07 `## Source contract` preamble and the response surfaces a new field `task_contract_source_path` (Dispatched-only, machine-only) that proves the Lisp drove the brief — observers can pin the SSOT invariant by asserting `dispatch_contract_mode==\"machine\" && task_contract_source_path.starts_with(\"<project_root>/.missiond/tasks/generated/\")`. Markdown rendering becomes optional compatibility metadata: render_command is still surfaced when emit succeeds (callers can run the renderer out of process for human review), but it is no longer load-bearing for the dispatch — the on-disk Lisp is. Failure semantics (machine mode): malformed contract on disk → SafeDescriptor (status=`skipped_malformed_task_contract`) — REFUSED to fall back to `claude -p` or the legacy natural-language brief, because silently downgrading would defeat the machine SSOT contract. Emitter OFF / node ineligible → falls back to legacy rendered dispatch (machine mode is a no-op for that dispatch — authors who insist on machine SSOT pair `dispatch_contract_mode=machine` with `task_contract_mode=\"emit\"`). Backward compatibility: the Dispatched variant grew a single new field (`task_contract_source_path: Option<String>`) — every existing pattern match either uses `..` (rollback / cascade paths in plan_dag.rs) or was updated in lockstep (3 test fixtures). DAG path: TaskContractDispatchCtx now captures `dispatch_contract_mode` once at the scheduler entry point and clones it into every per-node dispatch — per-node mode overrides are intentionally NOT supported (would defeat the cross-node SSOT). Per-node response payload gains `dispatch_contract_mode` for symmetry with single-node. Unified entry forwards both new knobs (`dispatch_contract_mode` + `render_markdown`) AND the wave-19 emitter knobs (`task_contract_mode` + `emit_task_contract`) — discovered the wave-19 knobs were never plumbed through unified_entry; closed that gap as part of this task so the unified pipeline can drive the full machine-driven loop end-to-end. Strict allowlist: typo in `dispatch_contract_mode` (e.g. `\"machin\"`) returns INVALID_PARAM with suggestion — the runner never silently degrades. MCP descriptor pins the same allowlist + documents the SafeDescriptor failure semantics so the agent surface mirrors the runtime contract.")

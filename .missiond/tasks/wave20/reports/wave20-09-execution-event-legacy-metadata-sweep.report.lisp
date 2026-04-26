;; Wave 20 / Task 09 — ExecutionEvent legacy metadata sweep.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-09-execution-event-legacy-metadata-sweep.lisp

(report wave20-09-execution-event-legacy-metadata-sweep
  :schema "missiond.report-contract.v1"
  :task_id "wave20-09-execution-event-legacy-metadata-sweep"
  :status done
  :commit_hash "6e01e3fe572f99fb09a4c08d2fdaaf3ee6c5b24a"
  :files_changed
    ["crates/missiond-core/src/event/events/execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-core --lib"
      :exit_code 0
      :ok true
      :notes "327 passed (302 baseline + 25 new wave-20/09 tests: 8 legacy-JSON deserialize tests + 8 byte-identical-to-legacy serialize tests + 8 with-metadata round-trip tests + 1 partial-metadata sweep across Heartbeat/Audited/Repaired). Existing serde_round_trip_for_each_variant + kind_returns_non_empty_for_every_variant + payload_hint_is_small_for_all_variants updated in place to construct the new variants with the optional trio set to None.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "84 passed (68 baseline + 16 new wave-20/09 tests: 8 *_event_inherits_dispatch_trio_from_companion_log + 8 *_event_omits_dispatch_trio_for_legacy_log covering Heartbeat/Released/DeviationRecorded/DecisionRecorded/IssueRecorded/Audited/Repaired/StaleClaim — same projection chain the runtime emit paths now use).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1304 passed (baseline 1288 + 16 new agent_execution tests; rest of daemon suite untouched).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build (78 baseline warnings unchanged; sqlx future-incompat note unchanged).")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK (no .missiond/v2/*.lisp touched per must-not-touch).")
     (:command "git diff --check -- crates/missiond-core/src/event/events/execution.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in scoped files.")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "shared-memory check OK after wave20-09 claim + completion entries appended.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-09-execution-event-legacy-metadata-sweep.lisp"
      :exit_code 0
      :ok true
      :notes "post-commit verifier OK against commit 6e01e3fe572f99fb09a4c08d2fdaaf3ee6c5b24a.")]

  :scope_deviations []

  :notes
    "wave20-09 sweeps the 8 legacy ExecutionEvent variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) and brings them up to the same workstation-dispatch projection contract that Opened (wave-11), Claimed/Completed (wave-18/02) and PlanNodeStateChanged (wave-14/02) already carry. Each variant gains three optional fields (`dispatch_strategy`, `target_project`, `requested_cwd`) wired with `#[serde(default, skip_serializing_if = \"Option::is_none\")]` so legacy producers / consumers stay byte-identical with the original wire form when the trio is absent. Inheritance: every emit site in `agent_execution.rs` now calls `read_dispatch_metadata_from_log(&file)` against the post-write `LogFile` handle (the same helper Claimed/Completed already use) and forwards the trio onto the event — so a single companion-log meta block drives every event a given execution emits, and observers downstream can route on dispatch context without re-loading the on-disk file. Variant inventory + size impact: Heartbeat 5→8, Released 5→8 (note Released keeps the existing `summary: Option<String>` without skip-serialize so the legacy 5-field shape always includes summary), DeviationRecorded 4→7, DecisionRecorded 4→7, IssueRecorded 4→7, Audited 4→7, Repaired 3→6, StaleClaim 4→7. Serde backward compat is double-pinned: (1) `*_deserializes_legacy_json_without_dispatch_metadata` proves old JSON parses to None; (2) `*_without_dispatch_metadata_serializes_byte_identical_to_legacy` asserts the wire payload matches the original key set + count exactly; (3) `*_with_dispatch_metadata_round_trips` proves new shape preserves verbatim. Plus a partial-metadata sweep across Heartbeat/Audited/Repaired pins skip-serialize per individual field so a regression on any one optional surfaces immediately. Audit emits the Audited event followed by per-stale-claim StaleClaim events — both now share the same `meta` snapshot (cloned per emit) so a single meta read serves both. No live LLM, no DB schema migration, no new MCP tool, no .missiond/v2/*.lisp edits (wave20-10 backfill window). All must-not-touch paths verified untouched (plan.rs / plan_dag.rs / workstation_dispatch.rs / scripts/**)."

)

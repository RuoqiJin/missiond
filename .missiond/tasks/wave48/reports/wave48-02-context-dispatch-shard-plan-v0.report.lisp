;; Wave 48 task report.
;; Schema: missiond.report-contract.v1

(report wave48-02-context-dispatch-shard-plan-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave48-02-context-dispatch-shard-plan-v0"
  :status done
  :commit_hash "5c3f30d3"
  :files_changed
    [".missiond/tasks/wave48/context-pack.lisp"
     ".missiond/tasks/wave48/shared-memory.lisp"
     ".missiond/tasks/wave48/session-trace.lisp"
     ".missiond/tasks/wave48/reports/wave48-02-context-dispatch-shard-plan-v0.report.lisp"]
  :acceptance_results
    [(:command "node scripts/check-context-pack.mjs .missiond/tasks/wave48/context-pack.lisp"
              :exit_code 0 :ok true
              :note "1 pack, 10 entries (4 from wave48-01 + bootstrap, 6 appended this task: 3 observations, 2 shard-proposals, 1 conflict).")
     (:command "node scripts/check-task-report.mjs .missiond/tasks/wave48/reports/wave48-02-context-dispatch-shard-plan-v0.report.lisp"
              :exit_code 0 :ok true
              :note "Report contract v1 validation passed after replacing the post-commit hash placeholder.")
     (:command "git diff --check -- .missiond/tasks/wave48/context-pack.lisp .missiond/tasks/wave48/reports/wave48-02-context-dispatch-shard-plan-v0.report.lisp"
              :exit_code 0 :ok true
              :note "No whitespace errors in the wave48-02 write scope.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave48/wave48-02-context-dispatch-shard-plan-v0.lisp"
              :exit_code 0 :ok false
              :note "Verifier flagged HEAD=5c3f30d3 because the integrator bundled wave48-01-shard-clear-stale-dyn-pin + wave48-02-shard-blueprint-checker-pin implementation files into the same commit that recorded this report. Mismatch is recorded in :scope_deviations rather than rewritten via amend, since 5c3f30d3 is already published. wave48-02 entries themselves stayed inside the declared write-scope.")]
  :scope_deviations
    [(:path ".missiond/v3/missiond-blueprint.lisp"
            :reason "Bundled into 5c3f30d3 by the integrator as part of wave48-02-shard-blueprint-checker-pin (single-owner hotspot must merge atomically with wave48-01-shard-clear-stale-dyn-pin). Not authored by this investigation task; outside :write-scope.")
     (:path "scripts/check-v3-workstation-config-isomorphism.mjs"
            :reason "Bundled into 5c3f30d3 alongside the blueprint pin (same shard). Outside :write-scope for the read-only investigation contract.")
     (:path "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            :reason "Bundled into 5c3f30d3 as the wave48-01-shard-clear-stale-dyn-pin implementation. wave48-02 contract explicitly excluded crates/**.")
     (:path "crates/missiond-core/src/db/traits.rs"
            :reason "Bundled into 5c3f30d3 as the BoardStore::clear_board_task_assignee trait method for wave48-01-shard-clear-stale-dyn-pin. Outside :write-scope.")
     (:path "crates/missiond-core/src/db/pg/board.rs"
            :reason "Bundled into 5c3f30d3 as the PG impl of clear_board_task_assignee for wave48-01-shard-clear-stale-dyn-pin. Outside :write-scope.")]
  :notes "Read-only context-pack investigation completing the wave48 dispatch-shard plan for dynamic-slot restart recovery. Three structurally separable shards were identified with NO write-scope overlap, and one terminate-side alternative was rejected as a conflict.\n\nRecommended shard split (and dispatch groups):\n  * dispatch-group A (parallel-safe within owner=claudecode):\n    - wave48-01-shard-clear-stale-dyn-pin (already proposed by wave48-01) — write-scope: crates/missiond-daemon/src/engine/intent_engine/autopilot.rs, crates/missiond-core/src/db/traits.rs, crates/missiond-core/src/db/pg/board.rs.\n    - wave48-02-shard-blueprint-checker-pin (single-owner hotspot, MUST land in the same merge as wave48-01) — write-scope: .missiond/v3/missiond-blueprint.lisp, scripts/check-v3-workstation-config-isomorphism.mjs. Captures the new 'clear stale dyn-pin' invariant in the V3 surface; the workstation-config checker requireAll must add the matching string the wave48-01 autopilot.rs change introduces.\n  * dispatch-group B (depends-on group A merged):\n    - wave48-02-shard-recovery-smoke — write-scope: scripts/check-v3-request-flow-smoke.mjs only. Adds an opt-in --restart-during-dispatch sub-mode that delegates a long task, kills the daemon mid-flight, restarts, and asserts the dead-pin clears and re-dispatch lands on a fresh idle coder slot.\n\nRejected via conflict (wave48-02-conflict-terminate-side-cleanup): a 'clear pins on dynamic-slot terminate' shard would touch autopilot.rs (reap_expired_dynamic_slots), compute_slot.rs (spawn_failed + user_terminated branches), and db/pg/board.rs in parallel with wave48-01's autopilot.rs + db/pg/board.rs hotspot. Hard merge conflict + isomorphism risk + redundant: wave48-01's dispatch-time existence check already covers BOTH restart-wipe and TTL-reap (see observation wave48-02-obs-ttl-same-trap), so the dispatch-side fix is strictly preferred.\n\nSingle-owner hotspots flagged: (1) .missiond/v3/missiond-blueprint.lisp + scripts/check-v3-workstation-config-isomorphism.mjs (workstation-config isomorphism is single-owner; blueprint text is projected into autopilot.rs by the checker, so they MUST land together). (2) crates/missiond-daemon/src/engine/intent_engine/autopilot.rs (already owned by wave48-01; do not split a sibling shard onto the same file).\n\nContext-pack entries appended this task (all under :task wave48-02-context-dispatch-shard-plan-v0):\n  - observation wave48-02-obs-ttl-same-trap (seq 5) — TTL reaping and daemon restart produce the identical dangling-pin condition.\n  - observation wave48-02-obs-blueprint-checker-coupling (seq 6) — workstation-config isomorphism is a single-owner hotspot.\n  - observation wave48-02-obs-smoke-coverage-gap (seq 7) — wave47 --execute-real-dispatch only proves happy path; restart-recovery smoke is missing.\n  - shard-proposal wave48-02-shard-blueprint-checker-pin (seq 8).\n  - shard-proposal wave48-02-shard-recovery-smoke (seq 9).\n  - conflict wave48-02-conflict-terminate-side-cleanup (seq 10).\n\nIntegration-plan recommendation for the next wave (NOT appended in this read-only task — left for the integrator):\n  :accepted-shards [clear-stale-dyn-pin blueprint-checker-pin recovery-smoke]\n  :dispatch-groups [A B]\nThe three shards' write-scopes are pairwise disjoint as required by check-context-pack.mjs::firstScopeOverlap, so they can co-exist in a single integration-plan.\n\nReport :commit_hash carries 5c3f30d3, the integration commit that recorded this wave48-02 report and landed the accepted group-A implementation/code-isomorphism shards."
  :verification_tier local)

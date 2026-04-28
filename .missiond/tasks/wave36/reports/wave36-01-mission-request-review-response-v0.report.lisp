;; Wave 36 task report.
;; Schema: missiond.report-contract.v1

(report wave36-01-mission-request-review-response-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave36-01-mission-request-review-response-v0"
  :status done
  :commit_hash "3937a738a236"
  :agent_commit_hash "37421f4ae3af"
  :final_commit_hash "3937a738a236"
  :verified_commit_hash "3937a738a236"
  :parent_patches
    [(:commit "3937a738a236"
      :kind hotfix-other
      :reason "Codex live smoke showed response=approve_intent approved the directive but left request-local plan.lisp absent, forcing a second mission_request advance call. Parent hotfix aligned the adapter with the V3 user loop: approve intent now delegates to mission_directive approve, then immediately runs unified_entry s4 plan-authoring and projects plan.lisp for the same request."
      :files [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"])]
  :files_changed
    [".missiond/v3/missiond-blueprint.lisp"
     "crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"]
  :acceptance_results
    [(result :command "cargo test -p missiond-daemon handlers::knowledge::request::tests"
             :exit_code 0
             :ok true
             :note "67 passed; 0 failed; 0 ignored after parent hotfix. Adds 27 new tests covering parse_respond_decision (response/decision aliases, missing/unknown errors), RespondDecision classification (requires_directive_ref / requires_plan_ref / record_only), extract_lisp_keyword_string|int helpers, resolve_directive_ref/plan_ref (explicit-arg-wins, artifact fallback, blocked-when-missing), build_respond_plan_compile_args (request_id default board_task anchor + explicit board_task pass-through), next_event_seq seq+1 / max+1 / unrelated-name filtering, event_path_for_seq six-digit padding, build_review_event_lisp shape (dispatched approve_intent, blocked execute_plan with no refs, recorded reject_plan), next_action_for vocabulary, and parse_event_seq_from_filename strictness.")
     (result :command "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
             :exit_code 0
             :ok true
             :note "1 passed; mission_request remains registered after the action enum and description-only schema extension.")
     (result :command "cargo check -p missiond-daemon"
             :exit_code 0
             :ok true
             :note "Builds clean. Net additions are pure helpers (RespondDecision enum, parse_respond_decision, extract_lisp_keyword_string/int, resolve_directive_ref/plan_ref, build_respond_plan_compile_args, next_event_seq, event_path_for_seq, build_review_event_lisp, next_action_for) plus action_respond which calls super::directive::handle / super::plan::handle / super::unified_entry::run_pipeline without modifying any of those inner handlers.")
     (result :command "cargo check -p missiond-mcp"
             :exit_code 0
             :ok true
             :note "Description-only + enum-extension schema change. additionalProperties=true preserved; new properties (response, decision, note, directive_id, plan_id) are documented in the schema and pass strict-enum validation in action_respond.")
     (result :command "node scripts/check-lisp-blueprint-compression.mjs"
             :exit_code 0
             :ok true
             :note "v1 manifest + v3 blueprint parse and pass section + artifact + state-machine checks; new (review-response ...) form is nested under (unified-entry ...) and does not break compression invariants.")
     (result :command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "Architecture-lisp guard accepts the new review-response form including decisions vector, decision-routing rules, ref-resolution policy, event-ledger declaration, response-shape, and non-goals.")
     (result :command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "No NUL bytes in any of the three write-scope files.")
     (result :command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "No whitespace errors / conflict markers in the staged diff.")
     (result :command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp --mode staged"
             :exit_code 0
             :ok true
             :note "task-scope-guard staged OK (3 staged files, all inside write-scope).")
     (result :command "node scripts/verify-task-contract.mjs .missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"
             :exit_code 0
             :ok true
             :note "task-contract verify OK against worker commit 37421f4ae3af — commit message matches contract :commit :message and all changed files lie inside :write-scope. Parent hotfix commit 3937a738a236 is recorded in :parent_patches and keeps the same three-file write surface.")]
  :notes
    ["V3 review-response contract: extended (unified-entry ...) with a (review-response ...) form declaring action=respond inputs, the six legal decisions (approve_intent, reject_intent, ask_question, approve_plan, reject_plan, execute_plan), per-decision routing rules, ref-resolution policy (explicit-arg first, artifact-extracted fallback, blocked when missing), event-ledger schema (lifecycle-event v1 under .missiond/requests/<id>/events/<seq>.event.lisp with monotonic local sequence), response-shape, and explicit non-goals (never auto-approves, never spawns workstation work, never invents an id, never edits inner directive/plan/unified_entry handlers)."
     "respond action inputs: request_id (required), response or decision (required, enum), optional note, optional execute (or execute_after_approval alias), optional approved_directive_id / directive_id + directive_version for intent decisions, optional approved_plan_id / plan_id for plan decisions, optional board_task_id for the approve_intent plan-authoring continuation (defaults to request_id for request-local dry-run plan projection), and the same project / cwd / target_project root-resolution fields as status."
     "respond response shape: { status: ok|blocked, action: respond, mode, request_id, request_path, artifact_paths, artifact_exists, respond_result: { decision, outcome: recorded|dispatched|blocked, event_path, event_seq, event_sha256, event_bytes, execute, next_action, directive_id?, directive_version?, plan_id?, inner_action?, blocked_reason?, note? }, review_packet, next_action, v3_contract, projection?, pipeline_result? }. The wrapper preserves existing start/advance/status response shape and only adds respond as a peer entry point."
     "Persisted ref requirements: approve_intent / reject_intent require a directive ref (id + version) — explicit approved_directive_id|directive_id + directive_version, OR :directive_id|:id + :directive_version|:version parsed from the request-local intent-alignment.lisp; missing both ⇒ structured blocked response. approve_plan / reject_plan / execute_plan require a plan ref (id) — explicit approved_plan_id|plan_id OR :plan_id|:id parsed from the request-local plan.lisp; missing both ⇒ structured blocked response. mission_request never fabricates an id; the blocked next_action describes how to obtain one."
     "Why execution remains explicitly gated: approve_plan never sets execute=true regardless of the caller's flag — it dispatches to mission_plan(action=approve) only, and the blueprint requires response=execute_plan as a separate decision. execute_plan additionally requires execute=true (or omitted execute alongside response=execute_plan); execute=false alongside response=execute_plan returns blocked. The execute branch routes through super::unified_entry::run_pipeline with approved_plan_id + execute=true, so mission_plan execute's existing scoped-write / risk gates remain authoritative — mission_request never spawns workstation slots itself."
     "Event ledger: every respond call appends a request-local lifecycle-event Lisp file under .missiond/requests/<request_id>/events/<seq>.event.lisp using atomic_write_artifact and a monotonically-increasing local sequence (next_event_seq scans existing 000NNN.event.lisp filenames, ignoring stray names, and picks max+1). Event kinds map to outcomes: review_response_dispatched (approve/execute success), review_response_recorded (record-only routes), review_response_blocked (missing ref, execute=false on execute_plan, request.lisp absent, or inner surface returned a structured error). Refs/notes/blocked_reason/inner_action are stamped into the :payload only when present so blocked events never invent fields."
     "review_packet derivation remains pure: after respond returns, the wrapper re-reads request-local artifact existence and re-derives review_packet via derive_review_packet, propagating effective_execute (true only on a successful execute_plan dispatch) so the returned packet correctly reflects ExecuteRequested vs AwaitingPlanApproval."
     "Parent hotfix 3937a738a236 closes the live-smoke gap in the original worker implementation: response=approve_intent now approves the directive and immediately invokes unified_entry s4 plan-authoring, then run_projection writes .missiond/requests/<request_id>/plan.lisp. The returned review_packet therefore advances to awaiting_plan_approval without requiring the caller to issue a separate mission_request(action=advance) call."
     "MCP schema/description: action enum extended to start|advance|status|respond; new properties response (enum), decision (alias), note, directive_id (alias), plan_id (alias) added with full descriptions. additionalProperties=true preserved. The tool description spells out the per-decision routing, ref-resolution policy, blocked-response semantics, and event-ledger path so external callers can answer review_packet without reading the implementation."
     "Tests are AppState-free: parse_respond_decision, RespondDecision::{requires_directive_ref|requires_plan_ref|record_only}, extract_lisp_keyword_string|int, resolve_directive_ref|plan_ref, next_event_seq, parse_event_seq_from_filename, event_path_for_seq, build_review_event_lisp, and next_action_for all take pure inputs. The blocked / dispatched / recorded paths are pinned by structural assertions on the rendered event Lisp body so the audit trail format is contractual, not incidental."
     "Files committed (write-scope only): .missiond/v3/missiond-blueprint.lisp, crates/missiond-daemon/src/handlers/knowledge/request.rs, crates/missiond-mcp/src/tools/knowledge/request.rs. Shared-memory / lifecycle / session-trace / report file are protocol artifacts the orchestrator commits separately."])

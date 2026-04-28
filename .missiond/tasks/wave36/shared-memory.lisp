;; Wave 36 shared-memory ledger.

(shared-memory wave36
  :schema "missiond.shared-memory.v1"
  :wave wave36
  :created-at "2026-04-28T22:56:00+08:00"
  :sequence 5

  (observation
    :id wave36-theme-001
    :task wave36-01-mission-request-review-response-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T22:56:00+08:00"
    :touched [".missiond/tasks/wave36/manifest.lisp"
              ".missiond/tasks/wave36/context-atlas.lisp"
              ".missiond/tasks/wave36/pattern-cards.lisp"
              ".missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"]
    :summary "Wave36 theme: make mission_request continue the human review loop by accepting explicit review responses as the unified entry adapter, while preserving existing directive/plan approval gates.")

  (observation
    :id wave36-bootstrap-001
    :task wave36-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-28T14:55:47Z"
    :touched [".missiond/claudecode/wave36-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave36-01-claim-003
    :task wave36-01-mission-request-review-response-v0
    :agent claudecode
    :seq 3
    :at "2026-04-28T15:05:00Z"
    :touched [".missiond/tasks/wave36/wave36-01-mission-request-review-response-v0.lisp"
              ".missiond/claudecode/wave36-01-mission-request-review-response-v0.md"
              ".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Claim wave36-01: extending mission_request with action=respond adapter that records review events under .missiond/requests/<id>/events/, gates on persisted directive/plan refs (or explicit args), and routes approve/execute through existing mission_directive / unified_entry / mission_plan paths without bypassing approval gates or spawning workstation work.")

  (completion
    :id wave36-01-completion-004
    :task wave36-01-mission-request-review-response-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T15:17:58Z"
    :touched [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Completed wave36-01 at commit 37421f4ae3af. Added (review-response ...) form under unified-entry; added action_respond + RespondDecision/RespondOutcome + parse_respond_decision + resolve_directive_ref/plan_ref + next_event_seq + build_review_event_lisp + next_action_for in request.rs (all pure helpers + one async dispatcher); extended MCP schema with action=respond, response/decision enums and aliases. 25 new pure tests pin decision parsing, ref resolution, event sequencing, blocked responses, and no-execute-by-default behaviour. Acceptance: cargo test daemon (65 ok) + mcp surfaces (1 ok) + cargo check daemon/mcp + blueprint compression + architecture-lisp + perl NUL scan + git diff --check + task-scope-guard + verify-task-contract all green.")

  (observation
    :id wave36-01-parent-hotfix-005
    :task wave36-01-mission-request-review-response-v0
    :agent codex-orchestrator
    :seq 5
    :at "2026-04-28T15:34:12Z"
    :touched [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Parent hotfix commit 3937a738a236 after live smoke: response=approve_intent no longer stops after directive approval; it immediately runs unified_entry s4 plan-authoring and request-local plan.lisp projection, so the next review_packet asks for plan approval from the same mission_request entry.")

  (observation
    :id wave36-01-parent-hotfix-006
    :task wave36-01-mission-request-review-response-v0
    :agent codex-orchestrator
    :seq 6
    :at "2026-04-28T16:18:40Z"
    :touched [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"]
    :summary "Parent hotfix commit d34759b0e2b7 after live IPC smoke: approve_intent creates/reuses a hidden BoardTask anchor when board_task_id is omitted, approve_plan materializes request-local plan.lisp into a draft Plan row when plan_id is omitted, and the smoke verified approve_plan reused the same BoardTask anchor before routing through mission_plan approve."))

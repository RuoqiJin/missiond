;; Wave 35 task report.
;; Schema: missiond.report-contract.v1

(report wave35-01-mission-request-review-packet-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave35-01-mission-request-review-packet-v0"
  :status done
  :commit_hash "e285ae43e458"
  :files_changed
    [".missiond/v3/missiond-blueprint.lisp"
     "crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"]
  :acceptance_results
    [(result :command "cargo test -p missiond-daemon handlers::knowledge::request::tests"
             :exit_code 0
             :ok true
             :note "39 passed; 0 failed; 0 ignored. Includes 19 new review_packet tests covering classify_review_state, allowed_responses, build_review_artifact_preview UTF-8 boundary, derive_review_packet for all 5 states, parse_execute_requested aliases, extract_mode_from_request_lisp.")
     (result :command "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
             :exit_code 0
             :ok true
             :note "1 passed; mission_request remains registered with the new review_packet description.")
     (result :command "cargo check -p missiond-daemon"
             :exit_code 0
             :ok true
             :note "Builds clean; new code only adds pure helpers and one missiond_core::util::safe_byte_truncate import.")
     (result :command "cargo check -p missiond-mcp"
             :exit_code 0
             :ok true
             :note "Description-only change; schema preserves additionalProperties=true and existing action enum.")
     (result :command "node scripts/check-lisp-blueprint-compression.mjs"
             :exit_code 0
             :ok true
             :note "v1 manifest + v3 blueprint parse and pass section + artifact + state-machine checks.")
     (result :command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "Architecture lisp guard accepts the new review-packet form under unified-entry.")
     (result :command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "No NUL bytes in any of the three write-scope files.")
     (result :command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "No whitespace errors / conflict markers in the staged diff.")
     (result :command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp --mode staged"
             :exit_code 0
             :ok true
             :note "task-scope-guard staged OK (3 staged files, all inside write-scope).")
     (result :command "node scripts/verify-task-contract.mjs .missiond/tasks/wave35/wave35-01-mission-request-review-packet-v0.lisp"
             :exit_code 0
             :ok true
             :note "task-contract verify OK against e285ae43e458 — commit message matches contract :commit :message and all changed files lie inside :write-scope.")]
  :notes
    ["V3 review-packet contract: added a (review-packet ...) form under (unified-entry ...) declaring fields, states, state-derivation rules, allowed_responses tables for human-interactive vs trusted-agent, preview-policy (480-byte safe_byte_truncate), and the explicit non-goal that this is display surface only. The implementation-map note for mission_request was extended to mention the review_packet projection."
     "Response shape: mission_request start/advance now embed `review_packet` in the wrapper response; mission_request status embeds it next to artifact_paths/artifact_exists. The packet carries state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed."
     "State derivation rules implemented in classify_review_state: plan.lisp + execute=true → execute_requested (only state where execute_allowed=true); plan.lisp alone → awaiting_plan_approval; intent-alignment.lisp without plan.lisp → awaiting_intent_approval; no artifacts but projection_target present → intent_drafting; otherwise → received."
     "No auto-approval: review_packet is a pure projection of request-local artifact existence + the latest pipeline projection target. mission_request never calls mission_directive approve, never calls mission_plan approve, never sets approve_directive_id / approved_plan_id, never spawns workstation slots. execute_allowed=true only when the caller already passed execute=true alongside an existing plan.lisp — mission_plan execute is still the actual execution surface."
     "UTF-8 safety: artifact_preview uses missiond_core::util::safe_byte_truncate (480-byte cap). Tests pin: a 60-char × 3-byte CJK string at max=80 truncates to exactly 78 bytes / 26 chars, and every byte index in the preview is a char_boundary. derive_review_packet_uses_safe_byte_truncation_for_cjk_preview pins the wrapper-level invariant on a 200-character CJK fixture."
     "Tests are AppState-free: helpers (classify_review_state, allowed_responses_for, build_review_artifact_preview, derive_review_packet, parse_execute_requested, extract_mode_from_request_lisp) take pure inputs. File-read injection uses a closure parameter so derive_review_packet stays unit-testable without disk IO."
     "MCP description updated to document the new review_packet object while preserving additionalProperties=true and the existing start|advance|status enum. No new properties were added to the input schema; review_packet is purely an output shape."])

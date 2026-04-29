;; Wave 44 task contract.

(task wave44-01-request-local-artifact-roots-v0
  :schema "missiond.task-contract.v1"
  :title "v3 request local artifact roots v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier smoke
  :dispatch-group "A"
  :estimated-minutes 60
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave44/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave44/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "After wave43 proved the live mission_request path, remove the remaining default artifact-root drift: request-local .missiond/requests/<request_id>/ artifacts are the V3 truth; legacy .missiond/alignment/<topic>/ and .missiond/plans/<plan_id>/ compatibility writers must be explicit opt-in and must not pollute the worktree during the default live smoke."

  :write-scope
    ["scripts/check-v3-request-flow-smoke.mjs"
     ".missiond/v3/missiond-blueprint.lisp"
     "crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"]

  :must-not-touch
    ["packages/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/router/**"
     ".missiond/tasks/wave28/**"
     ".missiond/tasks/wave29/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/**"
     ".missiond/tasks/wave32/**"
     ".missiond/tasks/wave33/**"
     ".missiond/tasks/wave34/**"
     ".missiond/tasks/wave35/**"
     ".missiond/tasks/wave36/**"
     ".missiond/tasks/wave37/**"
     ".missiond/tasks/wave38/**"
     ".missiond/tasks/wave39/**"
     ".missiond/tasks/wave40/**"
     ".missiond/tasks/wave41/**"
     ".missiond/tasks/wave42/**"
     ".missiond/tasks/wave43/**"
     ".missiond/tasks/wave44/manifest.lisp"
     ".missiond/tasks/wave44/context-atlas.lisp"
     ".missiond/tasks/wave44/pattern-cards.lisp"
     ".missiond/tasks/wave44/wave44-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Update the V3 blueprint first: intent-alignment and plan request-local paths are SSOT; .missiond/alignment/<topic>/ and .missiond/plans/<plan_id>/ are legacy compatibility projections only, not default mission_request output."
     "Add or document an explicit mission_request compatibility-writer switch (preferred name: compat_write_file) while preserving the existing write_file alias for old callers. Default mission_request live flow must not forward write_file=true to mission_directive or mission_plan."
     "In crates/missiond-daemon/src/handlers/knowledge/request.rs, make start and respond approve_intent route request-local projections without legacy compatibility writes by default. If compat_write_file=true (or legacy write_file=true) is explicitly supplied, keep forwarding write_file=true to the inner compatibility writer."
     "In crates/missiond-mcp/src/tools/knowledge/request.rs, expose compat_write_file in the schema and describe write_file as a legacy alias. The user-facing description must say request-local artifacts are always the V3 review surface; compatibility roots are opt-in."
     "Update scripts/check-v3-request-flow-smoke.mjs --live-ipc so the default live smoke does not pass write_file=true. It must assert that the checked request_id creates request-local artifacts but does not create .missiond/alignment/<request_id>/ or any .missiond/plans/*/PLAN.lisp containing the smoke objective. Keep --cleanup scoped to the request-local directory."
     "Optional but preferred: add a --compat-write-file flag to the smoke checker that intentionally exercises the legacy opt-in path and reports the compatibility artifacts separately, without adding it to the aggregate default."
     "Preserve wave43 behavior: default and --dry-fixture remain daemon-free, --live-ipc still stops at awaiting_execution and never calls execute_plan."
     "If this task reveals that no Rust behavior change is needed because omission of write_file already prevents compatibility writes, still update Lisp/MCP/checker so future callers do not learn the wrong default from docs or smoke examples."]

  :acceptance
    ["node scripts/check-v3-request-flow-smoke.mjs --dry-fixture"
     "node scripts/check-v3-request-flow-smoke.mjs"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave44-request-local-artifacts-v0 --cleanup"
     "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --request-id wave44-request-local-artifacts-v0-json --cleanup --json"
     "node scripts/check-v3-code-isomorphism-complete.mjs"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "cargo test -p missiond-daemon handlers::knowledge::request::tests"
     "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
     "perl -ne 'exit 1 if /\\x00/' scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs"
     "git diff --check -- scripts/check-v3-request-flow-smoke.mjs .missiond/v3/missiond-blueprint.lisp crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs"]

  :commit
    (:required true
     :message "feat(v3): make request artifacts local by default"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Whether compat_write_file was added or existing behavior only needed doc/checker correction."
     "Default live IPC side effects: request-local artifacts, DB rows, and absence of legacy compatibility files."
     "Any optional legacy compat smoke behavior."
     "Acceptance command results."])

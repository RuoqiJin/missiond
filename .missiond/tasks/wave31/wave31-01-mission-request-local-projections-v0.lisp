;; Wave 31 task contract.

(task wave31-01-mission-request-local-projections-v0
  :schema "missiond.task-contract.v1"
  :title "mission_request request-local projections v0"
  :kind code-alignment
  :status ready
  :owner "claudecode"
  :depends-on []
  :dispatch-strategy "fresh-code-alignment"
  :verification-tier local
  :dispatch-group "A"
  :estimated-minutes 45
  :heartbeat-minutes 10
  :session-trace-writable true
  :context-atlas-path ".missiond/tasks/wave31/context-atlas.lisp"
  :pattern-card-path ".missiond/tasks/wave31/pattern-cards.lisp"
  :router-policy-path ".missiond/router/router-policy-v1.lisp"
  :router-backend-registry-path ".missiond/router/router-backend-registry-v1.lisp"
  :goal "Advance mission_request from request.lisp + initial event + compatibility pipeline into request-local Lisp projections, so a single request directory can hold request.lisp, intent-alignment.lisp, plan.lisp, events, receipts, and reports."

  :write-scope
    ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :must-not-touch
    ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
     "crates/missiond-daemon/src/handlers/mod.rs"
     "crates/missiond-mcp/src/tools/mod.rs"
     "scripts/**"
     ".missiond/v1/**"
     ".missiond/v2/**"
     ".missiond/research/**"
     ".missiond/tasks/schema/**"
     ".missiond/tasks/wave30/**"
     ".missiond/tasks/wave31/manifest.lisp"
     ".missiond/tasks/wave31/dispatch-plan.lisp"
     ".missiond/tasks/wave31/context-atlas.lisp"
     ".missiond/tasks/wave31/pattern-cards.lisp"
     ".missiond/tasks/wave31/wave31-*.lisp"
     ".missiond/claudecode/**"]

  :requirements
    ["Keep mission_request(action=start) conservative: it may write request-local projections, but must not auto-approve intent or plan and must not dispatch workstation work directly."
     "After the existing unified_entry call returns, project stable inner compile payloads into request-local files. Directive compile stages write .missiond/requests/<request_id>/intent-alignment.lisp from compiled_sexp or compiled_sexp_preview. Plan compile stages write .missiond/requests/<request_id>/plan.lisp from compiled_sexp or compiled_sexp_preview."
     "Use the existing request_paths + atomic_write_artifact flow. Respect overwrite_file for these projections. If projection is not possible because the pipeline stage is execute/error/missing sexp, return a clear projection status in the mission_request wrapper instead of failing the whole call."
     "mission_request(action=status) should expose request-local artifact paths and existence booleans for request, intent_alignment, plan, events_dir, receipts_dir, and reports_dir. It should still read request.lisp as before."
     "Update the MCP tool schema/description only if new response fields or args need to be documented; keep additionalProperties=true for compatibility."
     "Update .missiond/v3/missiond-blueprint.lisp implementation-map note/status if the code alignment moves beyond the current partial v0."
     "Add focused unit tests in request.rs for pure extraction/projection helpers, including directive preview projection, plan preview projection, no-sexpr no-op status, and status artifact existence shape if practical without constructing AppState."]

  :acceptance
    ["cargo test -p missiond-daemon handlers::knowledge::request::tests"
     "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
     "cargo check -p missiond-daemon"
     "cargo check -p missiond-mcp"
     "node scripts/check-lisp-blueprint-compression.mjs"
     "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
     "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
     "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"]

  :commit
    (:required true
     :message "feat(architecture): project request-local lisp artifacts"
     :scope-check write-scope-only)

  :report
    ["Commit hash."
     "Projection behavior by pipeline stage."
     "Response/status fields added."
     "Whether blueprint status/note changed."
     "Acceptance command results."])

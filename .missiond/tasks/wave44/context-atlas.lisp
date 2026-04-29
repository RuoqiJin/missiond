;; Wave 44 dispatch-time context atlas.

(context-atlas wave44-v3-request-local-artifact-roots-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave44
  :goal "Make mission_request default live artifacts request-local only; compatibility artifact roots are opt-in."
  :read-order [".missiond/claudecode/wave44-shared-preamble.md"
               ".missiond/tasks/wave44/context-atlas.lisp"
               ".missiond/tasks/wave44/pattern-cards.lisp"
               ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
               ".missiond/tasks/wave43/reports/wave43-01-v3-request-live-ipc-smoke-v0.report.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               "crates/missiond-daemon/src/handlers/knowledge/request.rs"
               "crates/missiond-mcp/src/tools/knowledge/request.rs"
               "scripts/check-v3-request-flow-smoke.mjs"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "Architecture authority. Update this before code so request-local artifacts are the default V3 truth and compat paths are legacy projections."
      :grep ["artifact intent-alignment"
             ":compat-path"
             "surface mission_request"
             "surface mission_directive"
             "surface mission_plan"])
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "mission_request adapter that currently forwards write_file into inner directive/plan surfaces when callers supply it."
      :grep ["normalize_start_args"
             "build_respond_plan_compile_args"
             "write_file"
             "run_projection"
             "request_paths_for"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "Public MCP schema and long-form description. Add compat_write_file and clarify write_file legacy alias."
      :grep ["write_request_file"
             "write_file"
             "MissionD v3 unified request entry"
             "compatibility"])
    (file "scripts/check-v3-request-flow-smoke.mjs"
      :purpose "Live IPC smoke added in wave43. Make default live smoke request-local-only and assert absence of legacy compat files."
      :grep ["runLiveIpcSmoke"
             "write_file: true"
             "cleanup"
             "live_ipc"
             "request_id"])))

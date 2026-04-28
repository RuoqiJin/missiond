;; Wave 31 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave31-request-entry
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave31
  :goal "Advance the V3 unified request entry toward request-local Lisp artifacts."
  :read-order [".missiond/claudecode/wave31-shared-preamble.md"
               ".missiond/tasks/wave31/context-atlas.lisp"
               ".missiond/tasks/wave31/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave31/wave31-01-mission-request-local-projections-v0.lisp"]

  (global-anchors
    (file "crates/missiond-daemon/src/handlers/knowledge/request.rs"
      :purpose "New mission_request daemon handler; owns request.lisp, initial lifecycle event, and wrapper response."
      :grep ["action_start"
             "wrap_pipeline_result"
             "tool_result_payload"
             "build_request_lisp"
             "build_event_lisp"
             "request_paths"])
    (file "crates/missiond-mcp/src/tools/knowledge/request.rs"
      :purpose "MCP schema for mission_request action=start|advance|status."
      :grep ["write_request_file"
             "approved_directive_id"
             "approved_plan_id"
             "additionalProperties"])
    (file "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :purpose "Compatibility pipeline response shape; read-only unless contract explicitly expands scope."
      :grep ["run_pipeline"
             "pipeline_stage"
             "artifact_refs"
             "build_artifact_refs"])
    (file "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
      :purpose "Directive compile response keys used for intent-alignment projection."
      :grep ["compiled_sexp_preview"
             "compiled_sexp"
             "file_path"
             "maybe_write_directive_artifact"])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "Plan compile response keys used for plan.lisp projection."
      :grep ["compiled_sexp_preview"
             "compiled_sexp"
             "sexp_hash_preview"
             "maybe_write_plan_artifact"])
    (file "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
      :purpose "Existing atomic writer and project-root resolver; reuse instead of inventing a second writer."
      :grep ["atomic_write_artifact"
             "resolve_writer_project_root"
             "sanitize_topic_segment"])
    (file "scripts/check-lisp-blueprint-compression.mjs"
      :purpose "V3 blueprint validator. Update only if blueprint-required sections change."
      :grep ["mission_request"
             "mission-request"
             "lifecycle-event"])))

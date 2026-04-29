;; Wave 38 dispatch-time context atlas.
;; Read-only guidance for the worker. Task contract remains the source of truth.

(context-atlas wave38-workflow-methodology-artifact-v0
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave38
  :goal "Make compile_methodology write_file project an enriched V3 workflow artifact instead of a raw methodology-source mirror."
  :read-order [".missiond/claudecode/wave38-shared-preamble.md"
               ".missiond/tasks/wave38/context-atlas.lisp"
               ".missiond/tasks/wave38/pattern-cards.lisp"
               ".missiond/v3/missiond-blueprint.lisp"
               ".missiond/tasks/wave38/wave38-01-workflow-methodology-artifact-v0.lisp"]

  (global-anchors
    (file ".missiond/v3/missiond-blueprint.lisp"
      :purpose "V3 contract for workflow artifact, mission_workflow implementation map, and compression checks."
      :grep ["artifact workflow"
             "surface mission_workflow"
             "compile_methodology reads methodology Lisp"
             "canonicalizes methodology source"])
    (file "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
      :purpose "mission_workflow implementation. compile_methodology currently persists YAML then maybe_write_workflow_artifact with source content; render_workflow_artifact_sexp already supports enriched V3 workflow artifacts."
      :grep ["async fn action_compile_methodology"
             "async fn action_compile_deterministic"
             "maybe_write_workflow_artifact"
             "render_workflow_artifact_sexp"
             "render_workflow_steps"
             "extract_steps_with_lines"
             "build_generated_yaml"])
    (file "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
      :purpose "MCP schema/tool description for write_file semantics and workflow surface contract."
      :grep ["compile_methodology"
             "write_file"
             "canonicalizes the methodology source"
             "enriched V3 workflow artifact"])
    (file "scripts/check-v3-workflow-isomorphism.mjs"
      :purpose "Lisp/code checker that must pin the updated blueprint/code/MCP strings and dry fixture."
      :grep ["compile_methodology reads methodology Lisp"
             "distill persist+write_file writes an enriched V3 workflow artifact"
             "render_workflow_artifact_sexp"
             "buildFixture"])))

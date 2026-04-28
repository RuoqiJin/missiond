;; Worker report for wave31-01-mission-request-local-projections-v0.
;; Schema: missiond.report-contract.v1

(report wave31-01-mission-request-local-projections-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave31-01-mission-request-local-projections-v0"
  :status done
  :commit_hash "27a6e423da6226b68a44588948cc0c2a3358f647"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
     "crates/missiond-mcp/src/tools/knowledge/request.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::request::tests"
      :exit_code 0 :ok true
      :detail "19 tests passed (4 pre-existing + 15 new projection/path/existence helpers); 1687 filtered.")
     (:command "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered"
      :exit_code 0 :ok true
      :detail "1 test passed; mission_request still registered alongside directive/plan/workflow.")
     (:command "cargo check -p missiond-daemon"
      :exit_code 0 :ok true
      :detail "compiles clean; no new warnings introduced by request.rs (pre-existing crate warnings unchanged).")
     (:command "cargo check -p missiond-mcp"
      :exit_code 0 :ok true
      :detail "compiles clean; schema description updated, additionalProperties=true preserved.")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
      :exit_code 0 :ok true
      :detail "v1 manifest + v3 blueprint structure check OK after implementation-map note update.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "architecture-lisp shape OK (1 file scanned).")
     (:command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no NUL bytes in any of the three write-scope files.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/request.rs crates/missiond-mcp/src/tools/knowledge/request.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no trailing-whitespace / mixed-indent diff complaints in the staged write scope.")]

  :projection_behavior
    (:by_pipeline_stage
      ((stage "s1_message_intake|s3_alignment_review_gate"
         :target "intent_alignment"
         :writes ".missiond/requests/<request_id>/intent-alignment.lisp"
         :sexp-source "compiled_sexp -> compiled_sexp_preview")
       (stage "s4_plan_authoring|s5_plan_review_gate"
         :target "plan"
         :writes ".missiond/requests/<request_id>/plan.lisp"
         :sexp-source "compiled_sexp -> compiled_sexp_preview")
       (stage "s6_execution_runner"
         :target nil
         :status "skipped_execute_stage"
         :note "execution stage carries no compile sexp; projection is a deliberate no-op.")
       (stage "pipeline-error / inner is_error=true"
         :target nil
         :status "skipped_pipeline_error"
         :note "preserves the inner error envelope without overwriting prior request-local artifacts.")
       (stage "missing compiled_sexp / compiled_sexp_preview"
         :target "intent_alignment|plan"
         :status "skipped_no_sexp"
         :note "projection target classified, but the inner payload had nothing stable to project.")
       (stage "advance without request_id / unresolved project root"
         :target "intent_alignment|plan"
         :status "skipped_no_request_id|skipped_no_project_root"
         :note "projection requires both an id-shaped request_id and a resolvable project root.")
       (stage "atomic write failure"
         :target "intent_alignment|plan"
         :status "write_failed"
         :note "atomic_write_artifact error surfaced verbatim under projection.error; whole call still succeeds."))
      :writer "atomic_write_artifact (file_artifacts.rs); honors overwrite_file flag for the projection too.")

  :response_status_fields
    (:wrapper_response_added_keys
       ["projection.status (written | skipped_execute_stage | skipped_pipeline_error | skipped_unknown_stage | skipped_no_sexp | skipped_no_request_id | skipped_no_project_root | write_failed)"
        "projection.target (intent_alignment | plan, omitted when status carries no target)"
        "projection.sexp_source (compiled_sexp | compiled_sexp_preview)"
        "projection.path / sha256 / bytes / created / overwritten (only on Written)"
        "projection.error (only on WriteFailed)"]
     :status_action_added_keys
       ["artifact_paths { request, intent_alignment, plan, events_dir, receipts_dir, reports_dir }"
        "artifact_exists { same six keys, bool }"]
     :request_artifacts_change
       "request_artifacts.artifact_paths now reuses build_artifact_paths_json so start and status surface identical key shapes.")

  :blueprint_change
    (:file ".missiond/v3/missiond-blueprint.lisp"
     :section "implementation-map / surface mission_request"
     :status_kept "code-aligned-partial"
     :note_changed true
     :note_summary "Now describes the v0 projection: writes request.lisp + initial event, runs unified_entry, projects compiled_sexp / compiled_sexp_preview into request-local intent-alignment.lisp / plan.lisp via atomic_write_artifact, surfaces projection status, and exposes artifact paths + existence booleans on action=status. Status string left at code-aligned-partial because workstation dispatch + DB schema migration remain explicit non-goals.")

  :notes
    "Pure projection helpers (classify_projection_target / extract_projected_sexp / plan_projection / build_artifact_paths_json / build_artifact_existence_with) are unit-tested without AppState. The IO glue run_projection wires plan_projection to atomic_write_artifact and is covered by the existing daemon integration tests (still pass) plus the lift assertion extract_pipeline_meta_reads_decorator_sibling which pins the unified_entry decorator's sibling-content contract."

  :trace_references
    [".missiond/tasks/wave31/shared-memory.lisp :: wave31-01-claim-003"
     ".missiond/tasks/wave31/session-trace.lisp :: wave31-01-trace-preamble-read-004"])

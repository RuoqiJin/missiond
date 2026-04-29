;; Wave 38 task report.
;; Schema: missiond.report-contract.v1

(report wave38-01-workflow-methodology-artifact-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave38-01-workflow-methodology-artifact-v0"
  :status done
  :commit_hash "5b8fa97ebc98"
  :files_changed
    [".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-workflow-isomorphism.mjs"
     "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
  :acceptance_results
    [(result :command "node scripts/check-v3-workflow-isomorphism.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "v3 workflow Lisp/code isomorphism check OK against the dry fixture; fixture extended with build_methodology_match_rules + the new methodology projection call site + the three test fn names + the MCP write_file copy.")
     (result :command "node scripts/check-v3-workflow-isomorphism.mjs"
             :exit_code 0
             :ok true
             :note "real-tree run pins blueprint wording (persist+write_file projects through render_workflow_artifact_sexp; source_kind=methodology; :status compiled; instead of canonicalizing the raw methodology source) and code-level helper/fixture names (build_methodology_match_rules + 3 test functions).")
     (result :command "cargo test -p missiond-daemon handlers::knowledge::workflow::tests:: --quiet"
             :exit_code 0
             :ok true
             :note "163 passed, 0 failed (was 160 before this wave); the 3 new tests are build_methodology_match_rules_includes_flow_id_and_source_hash, methodology_compile_renders_v3_workflow_artifact_not_raw_source, methodology_compile_review_required_status_when_no_steps.")
     (result :command "cargo test -p missiond-mcp test_directive_plan_workflow_surfaces_registered --quiet"
             :exit_code 0
             :ok true
             :note "1 passed; mission_workflow surface still registered after the write_file description rewrite.")
     (result :command "node scripts/check-lisp-blueprint-compression.mjs"
             :exit_code 0
             :ok true
             :note "v1 manifest + v3 blueprint compression contract still holds after extending the mission_workflow :note.")
     (result :command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "blueprint architecture-lisp check OK on the updated file.")
     (result :command "perl -ne 'exit 1 if /\\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workflow-isomorphism.mjs crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             :exit_code 0
             :ok true
             :note "no NUL bytes in any of the four touched files.")
     (result :command "git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workflow-isomorphism.mjs crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             :exit_code 0
             :ok true
             :note "no whitespace-error or conflict markers in the staged write-scope files.")]
  :notes "Closes the workflow Lisp-isomorphism gap left in V3 for compile_methodology.\n\nV3 artifact shape for compile_methodology write_file: a single (workflow ...) form rendered by the existing render_workflow_artifact_sexp helper, identical in shape to the distill projection. The methodology branch has no Workflow DB row, so :workflow_id is stamped with the deterministic generated flow_id (e.g. \"methodology-<stem>-v0\" or the explicit output_flow_id) instead of a UUID; :source_plans is the empty vector (no plan); :match_rules is built by the new build_methodology_match_rules helper and packs source_kind=\"methodology\" / compiler=\"deterministic-v0\" / compiler_version / compiler_status / flow_id / source_hash / source_path / generated_at so reviewers can correlate the .lisp artifact with the generated YAML; :steps re-runs render_workflow_steps on the methodology body (same step extractor distill uses); :status is :compiled when the methodology has executable (step …) forms and :compiled_review_required when it has none; :body preserves the methodology Lisp body verbatim. The raw methodology source is therefore retained inside the V3 envelope, not published as the on-disk artifact.\n\nDB-free + backward-compatible: no Workflow row insert / no schema migration / no workflow_id UUID is generated for the methodology branch. The YAML compiler (build_generated_yaml + atomic_write into .missiond/generated/flows/<flow_id>.yaml), run_methodology dispatch, the review-gate / auto-Sonnet policy stack, and the wave-14 fallback that downgrades to status=\"partial\" when topic is missing all stay byte-identical. write_file=false still returns YAML/preview only; dry_run still returns a lint preview; no action dispatches workstation work. distill (dry_run + sonnet) write_file behavior is untouched — both branches now share the same enriched V3 envelope helper.\n\nRust tests / checker fixtures added: three focused unit tests in handlers::knowledge::workflow::tests — build_methodology_match_rules_includes_flow_id_and_source_hash (asserts every required field of the new helper), methodology_compile_renders_v3_workflow_artifact_not_raw_source (asserts the rendered artifact is the (workflow …) envelope, carries flow_id / source_hash / source_kind=methodology / extracted steps / :status :compiled, AND is not byte-equal to the raw methodology source), methodology_compile_review_required_status_when_no_steps (asserts the no-step branch downgrades to :status :compiled_review_required while still emitting the V3 envelope). check-v3-workflow-isomorphism.mjs gains 9 new blueprint/handler needles plus 3 new MCP needles plus an updated dry-fixture that covers the new helper + tests + MCP wording, and 163 daemon workflow tests + 1 mcp surface-registration test pass."
  :verification_tier local)

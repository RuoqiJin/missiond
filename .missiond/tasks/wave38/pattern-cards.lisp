;; Wave 38 dispatch-time pattern cards.

(pattern-cards wave38-workflow-methodology-artifact-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave38

  (card lisp-artifact-projection-over-source-mirror
    :use-for [wave38-01-workflow-methodology-artifact-v0]
    :summary "When a V3 artifact contract exists, write_file should publish the contract artifact, not mirror intermediate source text."
    :recipe ["Update V3 blueprint wording first so code follows the Lisp contract."
             "Reuse existing render_*_artifact helpers when available instead of inventing a second file format."
             "Keep the old DB/YAML side effect stable; only change the optional file-first projection."
             "Pin code and MCP docs in the isomorphism checker."]
    :known-good ["scripts/task-runner-finalize-report.mjs"
                 "crates/missiond-daemon/src/handlers/knowledge/workflow.rs :: render_workflow_artifact_sexp"])

  (card no-db-row-does-not-mean-no-artifact-id
    :use-for [wave38-01-workflow-methodology-artifact-v0]
    :summary "A DB-free methodology branch can still have a stable file artifact id derived from flow_id/source hash."
    :recipe ["Do not add a DB migration just to satisfy the artifact shape."
             "Use generated flow_id and source_hash in :match_rules / metadata so reviewers can correlate YAML and Lisp."
             "Keep review resolution mode=methodology and db_transition=false."
             "Add tests that distinguish enriched workflow artifact bytes from raw methodology source bytes."]
    :known-good ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs :: workflow_methodology_receipt_does_not_fake_db_state"
                 "crates/missiond-daemon/src/handlers/knowledge/workflow.rs :: render_workflow_artifact_sexp_keeps_draft_without_steps_explicit"]))

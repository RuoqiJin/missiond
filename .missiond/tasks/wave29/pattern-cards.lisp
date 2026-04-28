;; Wave 29 dispatch-time pattern cards.
;; This file is read-only guidance for workers. wave29-02 will introduce the
;; durable schema/checker and stable pattern-card files.

(pattern-cards wave29-runner-efficiency
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave29

  (card schema-checker
    :use-for [wave29-01-context-atlas-schema-v0
              wave29-02-pattern-card-schema-v0
              wave29-05-verification-receipt-schema-v0]
    :recipe ["Create a small Lisp schema file with status/checker/read-only guarantees."
             "Create a Node checker using scripts/lib/missiond_lisp.mjs; no ad hoc S-expression parsing."
             "Support --dry-fixture, --json, and file inputs. Add named exports for downstream reuse."
             "Fixture mix: happy path, missing required field, malformed path, enum drift, duplicate id, traversal/absolute path rejection."
             "Acceptance starts with the new checker --dry-fixture, then existing adjacent dry fixtures, then git diff --check."]
    :known-good ["scripts/check-task-runner-manifest.mjs"
                 "scripts/check-router-dispatch-descriptor.mjs"
                 "scripts/check-router-backend-registry.mjs"])

  (card node-cli-readonly
    :use-for [wave29-03-runner-wave-prep-v0
              wave29-06-ready-queue-planner-v0]
    :recipe ["Keep CLI read/write effects explicit in usage text."
             "Use named imports from existing checkers instead of duplicating schema rules."
             "Make deterministic output byte-stable; sort ids, object keys, diagnostics, and fixture output."
             "Expose pure functions for fixture and downstream reuse; gate main() on direct invocation when imports are needed."
             "Self-audit for no dispatch/no spawn/no git mutation/no network/no LLM when the task claims read-only planning."]
    :known-good ["scripts/plan-task-runner.mjs"
                 "scripts/render-wave-briefs.mjs"
                 "scripts/verify-task-runner-batch.mjs"])

  (card report-lineage
    :use-for [wave29-04-parent-hotfix-lineage-v1]
    :recipe ["Treat worker commit, parent hotfix commit, final verified commit, and report commit hash as separate roles."
             "Do not amend worker commits. The final report :commit_hash should point at the final commit when parent patches exist."
             "Record :agent_commit_hash for the worker's original commit and :parent_patches for each parent hotfix."
             "Batch verification may match memory completion text against any lineage hash, but verification should resolve the final/verified hash."]
    :known-good [".missiond/tasks/wave28/reports/wave28-02-task-runner-plan-cli-v0.report.lisp"
                 "scripts/check-task-report.mjs"
                 "scripts/verify-task-run.mjs"
                 "scripts/verify-task-runner-batch.mjs"])

  (card cross-layer-smoke
    :use-for [wave29-07-runner-efficiency-smoke-v1]
    :recipe ["Prefer layer-local fixtures plus one synthetic manifest that crosses every layer."
             "Each regression should fail near the owning layer; avoid one opaque bash mega-runner."
             "Pin productive-only, context navigation, preamble-read trace, lineage, receipts, and ready-queue semantics in the same synthetic wave."
             "If no Rust files changed in the wave, use smoke-tier Node/Lisp acceptance and explicitly skip cargo."])

  (card large-file-navigation
    :use-for [wave29-01-context-atlas-schema-v0
              wave29-06-ready-queue-planner-v0]
    :recipe ["Read atlas anchors before broad scans."
             "For plan.rs, use rg anchors and partial reads around mod task_runner_dry_run and tests; avoid whole-file reads unless needed."
             "For generated or unusual files, prefer rg --text or /usr/bin/grep -na when binary detection appears."
             "Do not place raw NUL bytes in source files; use textual escapes such as \\u0000 so search tools keep files searchable."]))

;; Wave 40 dispatch-time pattern cards.

(pattern-cards wave40-report-preservation-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave40

  (card sparse-report-projection
    :use-for [wave40-01-parent-hotfix-report-preservation-v0]
    :summary "A parent hotfix finalizer should patch lineage fields over the worker report, not rebuild a minimal report."
    :recipe ["Start with the Lisp/report-contract wording so the code has a precise authority."
             "Parse the worker report and preserve existing non-lineage keyword/value pairs."
             "Replace or insert only the lineage fields: :commit_hash, :agent_commit_hash, :final_commit_hash, :verified_commit_hash, :parent_patches, and the unioned :files_changed."
             "Preserve :acceptance_results by default; appended verification commands should add entries, not replace worker proof."]
    :known-good ["scripts/task-runner-finalize-report.mjs :: finalizeReportSource"
                 "scripts/task-runner-parent-hotfix.mjs :: planParentHotfixFromSource"])

  (card regression-from-wave39
    :use-for [wave40-01-parent-hotfix-report-preservation-v0]
    :summary "The concrete failure class is wave39 parent hotfix finalization losing rich worker report fields."
    :recipe ["Build the dry fixture from a synthetic rich report rather than editing historical wave39 files."
             "Include two acceptance results and at least one :notes entry in the worker report."
             "Assert the finalized report still validates and still contains the worker proof plus the parent-hotfix lineage fields."
             "Keep minimal old reports supported so wave30-era fixtures remain valid."]
    :known-good [".missiond/tasks/wave39/reports/wave39-01-task-scoped-lifecycle-event-files-v0.report.lisp"
                 "scripts/task-runner-finalize-report.mjs :: runFixtures"])

  (card generic-lisp-field-preservation
    :use-for [wave40-01-parent-hotfix-report-preservation-v0]
    :summary "Prefer a generic property-preservation path so future report-contract extensions do not need finalizer rewrites."
    :recipe ["Avoid hard-coding only today's optional fields if a small generic property renderer is practical."
             "Unknown existing keyword fields should survive finalization unless they are one of the explicit patched lineage fields."
             "Keep output deterministic enough for fixtures; exact original whitespace does not need to be preserved."]
    :known-good ["scripts/lib/missiond_lisp.mjs :: readKeywordProps"
                 "scripts/check-task-report.mjs :: optional report fields"]))

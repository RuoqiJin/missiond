;; Pattern card: report-lineage
;;
;; Recipe for tracking commit lineage when a worker commit is followed by one
;; or more parent hotfix commits. Distilled from the wave28-02 task report
;; (.missiond/tasks/wave28/reports/wave28-02-task-runner-plan-cli-v0.report.lisp)
;; and the wave29-04 lineage extension (scripts/check-task-report.mjs +
;; scripts/verify-task-run.mjs + scripts/verify-task-runner-batch.mjs).

(pattern-card report-lineage
  :schema "missiond.pattern-card.v1"
  :version "v1"
  :purpose "Reproduce the MissionD parent-hotfix lineage recipe so a report covering a worker commit followed by parent patches keeps the original worker hash, the final commit hash, and the verified hash distinguishable across the report / verifier / batch verifier surfaces."
  :summary "Treat worker commit, parent hotfix commit, final verified commit, and report :commit_hash as separate roles. Never amend the worker commit. The report ships :agent_commit_hash + :parent_patches alongside :commit_hash so verifiers can join all three roles."

  :use-for [wave29-04-parent-hotfix-lineage-v1]

  :recipe
    ["1. Identify the four commit roles BEFORE writing the report. (a) :agent_commit_hash — the worker's original commit, never rewritten; (b) :commit_hash — the final commit hash (= :agent_commit_hash when no parent patches exist, otherwise the last parent hotfix); (c) :final_commit_hash — explicit alias for :commit_hash when parent patches exist; (d) :verified_commit_hash — the hash that scripts/verify-task-contract.mjs validated against (typically equal to :final_commit_hash)."
     "2. NEVER amend the worker commit. If a hook or lint failure surfaces after the worker commit lands, create a NEW commit (parent hotfix) on the same write-scope. Amending breaks lineage AND breaks any downstream verifier that already pinned :agent_commit_hash."
     "3. Record each parent hotfix in :parent_patches as (:commit <hash> :kind <category> :reason <prose> :files [<paths>]). Common :kind values: lint-cleanup, ts6133-cleanup, hook-repair, scope-guard-fix. Each parent patch MUST stay inside the original task's :write-scope; if a hotfix needs new paths it belongs in a follow-up task contract, not a parent patch."
     "4. The report's :commit_hash field MUST point at the FINAL commit when parent patches exist. wave28-02's :commit_hash = 302330a (final), :agent_commit_hash = 954116e513c5 (worker), :final_commit_hash = 302330a, :verified_commit_hash = 302330a. The batch verifier compares the memory completion entry's hash text against ANY of the lineage hashes."
     "5. wave29-04's lineage extension makes :agent_commit_hash and :parent_patches OPTIONAL fields with full back-compat: reports without lineage continue to validate. scripts/check-task-report.mjs adds validateCommitLineage; scripts/verify-task-run.mjs exposes agentCommitHash / finalCommitHash / verifiedCommitHash; scripts/verify-task-runner-batch.mjs uses commitHashesAgree to accept any lineage hash from memory ledgers."
     "6. After the parent hotfix lands, re-run scripts/verify-task-contract.mjs <task.lisp> against the FINAL commit. The verifier walks the on-disk diff between HEAD and the task's pre-commit baseline, so it naturally validates the cumulative effect of worker + parent patches."
     "7. Append a fresh trace event (:kind commit, :commit_hash <final>) to session-trace.lisp ONLY for the final commit. Parent hotfix events SHOULD be recorded as :kind hotfix or :kind observation so the session-trace remains a single authoritative timeline rather than a mix of in-flight and final hashes."]

  :known-good [".missiond/tasks/wave28/reports/wave28-02-task-runner-plan-cli-v0.report.lisp"
               "scripts/check-task-report.mjs"
               "scripts/verify-task-run.mjs"
               "scripts/verify-task-runner-batch.mjs"
               ".missiond/tasks/schema/report-contract-v1.lisp"]

  :anti-pattern
    ["Amending the worker commit to fix a lint failure. This rewrites the agent commit hash, invalidates any downstream verifier that pinned :agent_commit_hash, and silently drops the failure history."
     "Writing :commit_hash = <worker-commit> when a parent patch exists. Verifiers compare :commit_hash against the on-disk HEAD; a stale worker hash makes the verifier always fail with a confusing 'commit not found' message."
     "Recording the parent patch as a separate task report. Lineage belongs to the original task — splitting it across reports breaks the back-link from the verified hash to the worker commit."]

  :non-goals
    ["Replacing the report :commit_hash semantics. The single :commit_hash field remains authoritative for the FINAL commit; lineage fields are additive metadata."
     "Auto-rewriting commits. The pattern is hand-authored discipline reinforced by checker validation, not a refactoring tool. wave29-04 does NOT introduce any commit-mutation tooling."]

  :notes
    "Memory-ledger completion text is fuzzy: it may contain any of the lineage hashes depending on when the agent reported. The batch verifier's commitHashesAgree() helper accepts ANY of [:agent_commit_hash, :commit_hash, :final_commit_hash, :verified_commit_hash] as a match for memory text — the verification authority remains the final/verified hash, but memory text gets the benefit of the doubt.")

;; Wave29-07 runner-efficiency smoke v1 report.

(report wave29-07-runner-efficiency-smoke-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave29-07-runner-efficiency-smoke-v1"
  :status done
  :owner "claudecode"
  :commit_hash "08bf1a65d1d9"
  :files_changed ["scripts/check-context-atlas.mjs"
                  "scripts/check-pattern-card.mjs"
                  "scripts/prepare-task-runner-wave.mjs"
                  "scripts/check-task-report.mjs"
                  "scripts/check-verification-receipt.mjs"
                  "scripts/plan-task-runner.mjs"
                  "scripts/render-wave-briefs.mjs"
                  "scripts/verify-task-runner-batch.mjs"]
  :acceptance_results
    [(:command "node scripts/check-context-atlas.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-pattern-card.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/prepare-task-runner-wave.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-report.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-verification-receipt.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/plan-task-runner.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/render-wave-briefs.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture" :exit_code 0 :ok true)
     (:command "node scripts/check-task-contract.mjs --all" :exit_code 0 :ok true)
     (:command "git diff --check -- scripts/check-context-atlas.mjs scripts/check-pattern-card.mjs scripts/prepare-task-runner-wave.mjs scripts/check-task-report.mjs scripts/check-verification-receipt.mjs scripts/plan-task-runner.mjs scripts/render-wave-briefs.mjs scripts/verify-task-runner-batch.mjs" :exit_code 0 :ok true)
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp --mode staged" :exit_code 0 :ok true)
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-07-runner-efficiency-smoke-v1.lisp" :exit_code 0 :ok true)]
  :notes "Cross-layer smoke v1 added across 8 in-scope script files. Each layer carries one wave29-07-prefixed fixture/assertion pinning a cross-layer invariant: (A) check-context-atlas wave29-07-loop-smoke-real-wave29-atlas-validates: real .missiond/tasks/wave29/context-atlas.lisp validates clean via in-process readContextAtlasFile + validateAtlasObject (21 → 22 fixtures). (B) check-pattern-card wave29-07-loop-smoke-5-seed-cards-validate: all 5 wave29-02 seed pattern files under .missiond/patterns/*.pattern.lisp validate clean via readPatternCardFile + validateCardObject (14 → 15 fixtures). (C) prepare-task-runner-wave wave29-07-loop-smoke-preamble-read-trace-emitted: prep CLI emits a (trace-event :kind read :files [...]) referencing the manifest's shared_preamble_path; assertion uses parseLisp + readKeywordProps + nodeToStringArray for mechanical structural verification (10 → 11 fixtures). (D) check-task-report wave29-07-loop-smoke-hotfix-lineage-pin: synthetic report with worker hash aaaaaaa as :commit_hash but parent_patches[-1].commit=bbbbbbb is rejected by the wave29-04 final-hash drift rule (48 → 49 fixtures). The PASS direction is already pinned by wave29-04 wave28-02 lineage exemplar; together they pin the invariant from both sides. (E) check-verification-receipt wave29-07-loop-smoke-receipt-reuse-conservative: composite reuse-helper case with 5 internal sub-checks — matching all 4 rules → reusable=true; wrong commit (cafebabe vs 1234567abc) → false; wrong command (--different-flag vs --dry-fixture) → false; wrong tier (local CANNOT cover smoke) → false; non-zero exit (1 vs 0) → false. ANY single sub-check failure fails the smoke (16 structural + 7 → 16 structural + 8 reuse-helper = 23 → 24 total). (F) plan-task-runner wave29-07-loop-smoke-ready-queue-saves-time-on-unbalanced-dag: 4-node unbalanced DAG (anchor 10min/A, slow-peer 90min/A, fast-follower 5min/B deps anchor, medium-tail 20min/C deps anchor) where --schedule ready-queue produces wave_duration_savings_minutes > 0 AND aggregate_idle_window_savings_minutes > 0; the same source under default --schedule group-barrier MUST NOT carry the ready_queue field (additive-only contract pinned via Object.hasOwn) (19 → 20 fixtures). (G) render-wave-briefs wave29-07-loop-smoke-thin-brief-includes-context-anchors: synthetic task contract with both :context-atlas-path and :pattern-card-path causes the thin brief to include both paths verbatim plus the ## Context Navigation section header (11 → 12 fixtures). (H) verify-task-runner-batch wave29-07-loop-smoke-cross-layer-batch-verifies: 3-node manifest where each report carries wave29-04 lineage fields (worker + parent_patches + final commit_hash matching trailing parent_patches[-1].commit), every shared-memory completion cites the final hash, every git stub commit aligns; foo-node receives a wave29-05 receipt with matching commit/command/tier; verifier returns aggregate_status=all_green AND receipt_coverage populated with reusable_count=1 for foo (16 → 17 fixtures). All baseline fixtures across all 8 files stay byte-identically green. NO cargo command in any acceptance — verification_tier=smoke. NO child_process / spawn / git mutation / network / LLM in any layer. Imports added: prepare-task-runner-wave.mjs gained parseLisp/isList/head/readKeywordProps/nodeText/nodeToStringArray from missiond_lisp.mjs for the structured trace assertion; check-context-atlas.mjs and check-pattern-card.mjs runFixtures gained a `run`-style branch that calls the named exports against on-disk files, used only by the wave29-07 fixtures."
  :acceptance_summary "Layers A-H cross-layer smoke pinned. 8 dry-fixture suites green at expected counts (22, 15, 11, 49, 24, 20, 12, 17). check-task-contract --all 105 tasks green. git diff --check clean across 8 paths. scope-guard staged OK (8 files). verify-task-contract OK against final commit 08bf1a65d1d9.")

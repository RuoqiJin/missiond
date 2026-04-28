;; Wave 29 dispatch-time context atlas.
;; This file is read-only guidance for workers. wave29-01 will introduce the
;; durable schema/checker for future context-atlas files.

(context-atlas wave29-runner-efficiency
  :schema "missiond.context-atlas.dispatch.v0"
  :wave wave29
  :goal "Reduce navigation misses by giving workers stable code anchors, grep keywords, and file intent before implementation."
  :read-order [".missiond/claudecode/wave29-shared-preamble.md"
               ".missiond/tasks/wave29/context-atlas.lisp"
               ".missiond/tasks/wave29/pattern-cards.lisp"
               ".missiond/tasks/wave29/<task-id>.lisp"]

  (global-anchors
    (file "scripts/lib/missiond_lisp.mjs"
      :purpose "Shared Lisp reader and property helpers used by all Node checkers."
      :grep ["export function parseLisp"
             "export function readKeywordProps"
             "export function nodeToStringArray"
             "export function pathMatchesAny"])
    (file "scripts/check-task-contract.mjs"
      :purpose "Task contract validator; wave29 prep added context-atlas-path and pattern-card-path validation."
      :grep ["validateDispatchMetadata"
             "validateNavigationMetadata"
             "validateRepoRelativePathField"
             "ALLOWED_VERIFICATION_TIER"])
    (file "scripts/render-claudecode-task.mjs"
      :purpose "Single-task renderer and shared preamble source."
      :grep ["renderNavigationContext"
             "renderSharedPreamble"
             "renderThinTask"
             "Load the context atlas / pattern card before broad repository search"])
    (file "scripts/render-wave-briefs.mjs"
      :purpose "Batch thin-brief renderer from task-runner manifests."
      :grep ["function renderManifest"
             "function resolveTaskContractPath"
             "function writeSharedPreamble"
             "runFixturesPatched"])
    (file "scripts/check-task-runner-manifest.mjs"
      :purpose "Manifest schema/checker and named exports consumed by planner/renderer/verifier."
      :grep ["export function projectManifest"
             "export function readManifestFile"
             "export function validateManifestObject"
             "FORBIDDEN_PRODUCTIVE_KINDS"])
    (file "scripts/plan-task-runner.mjs"
      :purpose "Read-only plan CLI; current output is group-barrier batches plus critical path and overlap diagnostics."
      :grep ["export function planFromManifestFile"
             "export function planFromManifestObject"
             "function longestFrom"
             "function collectOverlapDiagnostics"
             "formatPlanLisp"
             "runFixtures"])
    (file "scripts/check-task-report.mjs"
      :purpose "Report checker; wave29 prep added optional commit-lineage fields."
      :grep ["validateReport"
             "validateCommitLineage"
             ":agent_commit_hash"
             ":parent_patches"])
    (file "scripts/verify-task-run.mjs"
      :purpose "Single task report/memory/commit verifier; exports loader and verification helpers."
      :grep ["export function loadReport"
             "agentCommitHash"
             "finalCommitHash"
             "verifiedCommitHash"
             "export function verifyRun"
             "commitHashesAgree"])
    (file "scripts/verify-task-runner-batch.mjs"
      :purpose "Manifest-wide verifier; joins node contract/report/shared-memory/git evidence."
      :grep ["export function verifyNode"
             "report.verifiedCommitHash"
             "verificationHash"
             "export function aggregateResults"
             "export function verifyManifest"])
    (file "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
      :purpose "Large Rust hotspot. Prefer anchors over full reads."
      :grep ["mod task_runner_dry_run"
             "parse_task_runner_mode"
             "attach_task_runner_block"
             "compute_runner_block"
             "fn longest_from"
             "task_runner_loop_smoke_pins_wave28_invariants"
             "strip_rust_comments_and_strings"]))

  (task-focus
    (task wave29-01-context-atlas-schema-v0
      :first-reads ["scripts/check-task-runner-manifest.mjs"
                    "scripts/check-task-contract.mjs"
                    "scripts/lib/missiond_lisp.mjs"]
      :avoid ["Do not read plan.rs unless a fixture needs Rust-anchor examples."])
    (task wave29-02-pattern-card-schema-v0
      :first-reads ["scripts/check-task-runner-manifest.mjs"
                    "scripts/check-router-dispatch-descriptor.mjs"
                    ".missiond/tasks/schema/task-contract-v1.lisp"]
      :avoid ["Do not invent a new parser; reuse missiond_lisp helpers."])
    (task wave29-03-runner-wave-prep-v0
      :first-reads ["scripts/render-wave-briefs.mjs"
                    "scripts/render-claudecode-task.mjs"
                    "scripts/check-session-trace.mjs"
                    "scripts/check-task-report.mjs"]
      :avoid ["Do not change task contract schema; wave29 prep fields already exist."])
    (task wave29-04-parent-hotfix-lineage-v1
      :first-reads ["scripts/check-task-report.mjs"
                    "scripts/verify-task-run.mjs"
                    "scripts/verify-task-runner-batch.mjs"
                    ".missiond/tasks/wave28/reports/wave28-02-task-runner-plan-cli-v0.report.lisp"]
      :avoid ["Do not amend old commits; model parent patch lineage explicitly."])
    (task wave29-05-verification-receipt-schema-v0
      :first-reads ["scripts/verify-task-runner-batch.mjs"
                    "scripts/check-task-report.mjs"
                    "scripts/check-task-runner-manifest.mjs"]
      :avoid ["Do not make receipts authoritative for source facts; they cache verified command evidence only."])
    (task wave29-06-ready-queue-planner-v0
      :first-reads ["scripts/plan-task-runner.mjs"
                    "scripts/check-task-runner-manifest.mjs"
                    ".missiond/tasks/schema/task-runner-manifest-v1.lisp"]
      :avoid ["Keep current group-barrier output backward compatible; add ready-queue/phase output as additive fields or an explicit flag."])
    (task wave29-07-runner-efficiency-smoke-v1
      :first-reads ["scripts/plan-task-runner.mjs"
                    "scripts/render-wave-briefs.mjs"
                    "scripts/verify-task-runner-batch.mjs"
                    "scripts/check-task-report.mjs"]
      :avoid ["No cargo unless a preceding task touched Rust; this wave is Node/Lisp orchestration only."])))

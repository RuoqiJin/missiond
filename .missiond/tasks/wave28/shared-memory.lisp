;; Wave 28 shared-memory ledger.

(shared-memory wave28
  :schema "missiond.shared-memory.v1"
  :wave wave28
  :created-at "2026-04-28T00:00:00+08:00"
  :sequence 1

  (observation
    :id wave28-bootstrap-001
    :task wave28-01-task-runner-manifest-schema-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T00:00:00+08:00"
    :touched []
    :summary "Wave 28 switches dispatch mechanics to productive-only workers: archive/backfill/index stay orchestrator-owned, worker briefs render thin, and shared boilerplate lives in wave28-shared-preamble.md. Theme: task-contract runner v0 as the layer before intent-alignment.lisp -> PLAN.lisp -> workflow.lisp closed loop.")

  (claim
    :id wave28-01-claim-001
    :task wave28-01-task-runner-manifest-schema-v0
    :agent claudecode
    :seq 2
    :at "2026-04-28T08:00:00+08:00"
    :touched [".missiond/tasks/schema/task-runner-manifest-v1.lisp"
              "scripts/check-task-runner-manifest.mjs"]
    :summary "Beginning wave28-01: introducing task-runner-manifest schema v1 + read-only checker (orchestration metadata only; not a worker-task surface, not a runtime backend switch).")

  (completion
    :id wave28-01-completion-001
    :task wave28-01-task-runner-manifest-schema-v0
    :agent claudecode
    :seq 3
    :at "2026-04-28T08:35:00+08:00"
    :touched [".missiond/tasks/schema/task-runner-manifest-v1.lisp"
              "scripts/check-task-runner-manifest.mjs"]
    :summary "Wave28-01 done at commit 145645c. Schema head (task-runner-manifest <id> :schema :wave :brief_mode :shared_preamble_path :productive_only ...) + (node :task_id :depends_on :verification_tier :dispatch_group :estimated_minutes :heartbeat_minutes :write_scope ...) entries. Default overlap_policy=reject for productive same-group; warn opt-in available. 20 fixtures / 15 categories PASS. Named exports SCHEMA / MANIFEST_HEAD / NODE_HEAD / BRIEF_MODES / VERIFICATION_TIERS / OVERLAP_POLICIES / FORBIDDEN_PRODUCTIVE_KINDS / projectManifest / readManifestFile / validateManifestObject ready for wave28-02 / 03 / 05."
    :refs [wave28-01-claim-001])

  (claim
    :id wave28-02-claim-001
    :task wave28-02-task-runner-plan-cli-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T12:55:00+08:00"
    :touched ["scripts/plan-task-runner.mjs"]
    :summary "Beginning wave28-02: read-only CLI scripts/plan-task-runner.mjs that consumes a task-runner manifest + task contracts and emits a deterministic runner plan (topological batches respecting dispatch_group, critical-path minutes, write-scope overlap diagnostics, verification-tier summary). Pure planning — no spawn / git / network / LLM. Imports validateManifestObject + projectManifest + readManifestFile + named enums from wave28-01.")

  (claim
    :id wave28-03-claim-001
    :task wave28-03-wave-brief-batch-renderer-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T13:00:00+08:00"
    :touched ["scripts/render-wave-briefs.mjs"
              "scripts/render-claudecode-task.mjs"]
    :summary "Beginning wave28-03: batch renderer scripts/render-wave-briefs.mjs that consumes a task-runner manifest and emits one shared preamble plus thin ClaudeCode briefs per productive worker node. Adds named exports (loadSingleTask / renderTask / renderSharedPreamble / deriveSharedPreamblePath) and gates main() on direct invocation in scripts/render-claudecode-task.mjs so the new batch script imports renderer functions in-process — never shells out. Wave28-02 owns scripts/plan-task-runner.mjs (must-not-touch overlap = none).")

  (completion
    :id wave28-02-completion-001
    :task wave28-02-task-runner-plan-cli-v0
    :agent claudecode
    :seq 6
    :at "2026-04-28T13:12:00+08:00"
    :touched ["scripts/plan-task-runner.mjs"]
    :summary "Wave28-02 done at commit 954116e. scripts/plan-task-runner.mjs (read-only plan CLI) emits deterministic JSON or Lisp output: schema/manifest_path/wave/productive_only/batches/critical_path_minutes/total_estimated_minutes/max_parallel_width/overlap_diagnostics/verification_tier_counts. Topological batches respect dispatch_group boundaries (nodes from different groups never share a batch). Cycle errors return structured {ok:false, error:cycle, cycle:[...]}; missing-task-contract errors return {error:missing_task, missing:[...]}; schema-invalid manifests short-circuit on validateManifestObject. 12 dry-fixtures across 11 categories PASS. Zero shell-out / git / network / LLM call sites (audited via grep). Reuses wave28-01 readManifestFile + projectManifest + validateManifestObject + VERIFICATION_TIERS + OVERLAP_POLICIES + SCHEMA. Named exports planFromManifestFile / planFromManifestObject / PLAN_SCHEMA ready for wave28-04 / 05 / 06."
    :refs [wave28-02-claim-001])

  (completion
    :id wave28-03-completion-001
    :task wave28-03-wave-brief-batch-renderer-v0
    :agent claudecode
    :seq 7
    :at "2026-04-28T13:30:00+08:00"
    :touched ["scripts/render-wave-briefs.mjs"
              "scripts/render-claudecode-task.mjs"]
    :summary "Wave28-03 done at commit c4a9850. scripts/render-wave-briefs.mjs reads a wave28-01 manifest, writes one preamble at <manifest>:shared_preamble_path (skipped when present unless --force; --force overwrites), then writes one thin brief per productive node at .missiond/claudecode/<task-id>.md. Renderer never shells out: imports loadSingleTask / renderTask / renderSharedPreamble / deriveSharedPreamblePath (NEW named exports) from scripts/render-claudecode-task.mjs (main() now gated on direct invocation). Defence in depth: archive/backfill/index/lisp-backfill nodes hard-rejected even if smuggled past wave28-01's checker. 9 dry-fixtures across 9 categories PASS (preamble-from-scratch, thin-brief-omits-boilerplate, deterministic-paths, skip-when-present, force-overwrites, archive-id reject, backfill-id reject, index-kind reject, backward-compat smoke). Existing 6 render-claudecode-task fixtures stay green byte-identical. check-task-contract --all OK (98 tasks). git diff --check clean. Real-world smoke (against wave28-01 + wave28-02 contracts inside tmp dir) confirmed 2 thin briefs + 1 preamble emitted. verify-task-contract OK against c4a9850."
    :refs [wave28-03-claim-001])

  (claim
    :id wave28-05-claim-001
    :task wave28-05-task-runner-batch-verifier-v0
    :agent claudecode
    :seq 8
    :at "2026-04-28T14:00:00+08:00"
    :touched ["scripts/verify-task-runner-batch.mjs"
              "scripts/verify-task-run.mjs"]
    :summary "Beginning wave28-05: NEW scripts/verify-task-runner-batch.mjs that takes a wave28-01 manifest, walks each productive node, and joins task-contract + report + shared-memory completion + commit hash via wave23-03 verifier helpers. Reuses wave28-01 readManifestFile + validateManifestObject; reuses wave23-03 verifyRun + loadContract + loadReport + loadLedger + readCommit; surgical Edit on scripts/verify-task-run.mjs ONLY to add named exports if not already present. Defence-in-depth skip for non-productive pseudo-nodes (FORBIDDEN_PRODUCTIVE_KINDS or productive_only=false). Read-only: no git mutation, no shell beyond existing readCommit, no network, no LLM.")

  (claim
    :id wave28-04-claim-001
    :task wave28-04-mission-plan-task-runner-dry-run-surface-v0
    :agent claudecode
    :seq 9
    :at "2026-04-28T14:05:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Beginning wave28-04: surfacing mission_plan execute task-runner manifest dry-run. New optional args task_runner_manifest_path + task_runner_mode (off|dry_run; apply/auto/unknown -> INVALID_PARAM before plan lookup). Pure-Rust manifest parser + projector (mirrors wave28-02 plan output keys); NO spawn / Node / shell / git. Mirrors wave27-03 split: compute_runner_block + attach_task_runner_block post-pass after attach_router_recommendation_block. applied=false hard-coded literal. Off-mode no-I/O even when both new args + all 4 wave27 router args supplied. Runs in parallel with wave28-05 (no overlap).")

  (completion
    :id wave28-05-completion-001
    :task wave28-05-task-runner-batch-verifier-v0
    :agent claudecode
    :seq 10
    :at "2026-04-28T14:30:00+08:00"
    :touched ["scripts/verify-task-runner-batch.mjs"]
    :summary "Wave28-05 done at commit fe70a5f. NEW scripts/verify-task-runner-batch.mjs (1161 lines) — read-only batch verifier that consumes a wave28-01 task-runner manifest and joins each productive node against its task contract + report + shared-memory completion + git commit. scripts/verify-task-run.mjs NOT touched (zero diff) — every helper required (loadContract, loadReport, loadLedger, loadLedgerFromSource, loadReportFromSource, verifyRun, readCommit, commitHashMatches, commitHashesAgree, loadContractFromSource) was already a named export from wave23-03 / wave23-06; wave23-03 19 fixture + 15 helper baseline preserved byte-identically. Output schema missiond.task-runner-batch-verification.v0 with alphabetically-sorted top-level keys + sorted task_id arrays for byte-identical determinism. Defence-in-depth pseudo-node skip (kind ∈ FORBIDDEN_PRODUCTIVE_KINDS OR id substring match OR productive_only=false ⇒ skipped silently, counted in skipped_nodes for telemetry). Per-node delegates to wave23-03 verifyRun for the actual contract/report/memory/commit cross-checks; batch verifier itself only adds the manifest walk + commit-hash-vs-memory-summary cross-check + result aggregation. 8 dry-fixtures PASS (all-green, missing report, missing memory, commit hash mismatch, pseudo-node skipped, productive_only=false skip-all, deterministic JSON, contract message mismatch via verifyRun). Real-world smoke against wave28-01 (commit 145645c) returned aggregate_status=all_green, verified_nodes=1/1. check-task-contract --all OK (98 tasks). git diff --check clean. verify-task-contract OK against fe70a5fcad04. Read-only invariant honored: no git mutation, no shell beyond readCommit (git rev-parse | git log | git show), no network, no LLM. Named exports SCHEMA / isPseudoNode / expectedPathsFor / extractCommitHashFromText / verifyNode / aggregateResults / verifyManifest ready for wave28-06 loop smoke."
    :refs [wave28-05-claim-001])

  (completion
    :id wave28-04-completion-001
    :task wave28-04-mission-plan-task-runner-dry-run-surface-v0
    :agent claudecode
    :seq 11
    :at "2026-04-28T14:35:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Wave28-04 done at commit dde6f61df560. New module task_runner_dry_run inside handlers::knowledge::plan (~640 LOC) + 13 new tests inside handlers::knowledge::plan::tests + 2 new MCP args (task_runner_mode enum off|dry_run, task_runner_manifest_path string). parse_task_runner_mode rejects apply / auto / unknown / non-string with structured INVALID_PARAM BEFORE plan lookup. attach_task_runner_block splices a `task_runner` response block when mode=dry_run; off/default mode is byte-identical to wave-15..27 baseline AND does ZERO file I/O even when task_runner_manifest_path is supplied (Off-path early-return precedes any read). Block fields: schema (literal `missiond.task-runner-plan.v0`), manifest_status ∈ {used, missing, unreadable, malformed}, applied (LOCKED Value::Bool(false)), manifest_path (echo), task_runner_warning (degraded only), wave, productive_only, batches (topological by depth + dispatch_group), critical_path_minutes, total_estimated_minutes, verification_tier_counts ({local, smoke, full} pre-seeded), overlap_diagnostics (severity follows :overlap_policy — reject → error, warn → warning). Pure-Rust S-expression tokeniser (mirrors wave24-04 in-module pattern); minimal subset extracting only the projection-relevant fields, tolerates unknown keys for forward-compat. NO spawn / Node / shell / git / mission_task_delegate / LLM. Cross-wave invariants: applied=false hard-coded Value::Bool(false), is_boolean() asserted alongside == false in 3 tests; combined Off-mode invariant test (task_runner_off_with_runner_args_and_router_args_does_no_file_io) supplies BOTH wave-28 task_runner args AND ALL FOUR wave-24..27 router args with bogus paths and asserts NONE of the bogus paths exist after the call AND the response is byte-identical to baseline. Test totals: cargo test -p missiond-daemon --bin missiond → 1680 passed (1667 baseline + 13 new); cargo test -p missiond-mcp --lib → 17 passed (baseline preserved). check-task-contract --all OK (98 tasks). git diff --check clean. task-scope-guard staged OK (2 staged files). verify-task-contract OK against dde6f61df560. Edits inside :write-scope only; zero matches against :must-not-touch (plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs / .missiond/v2/** / .missiond/tasks/** / .missiond/claudecode/** / scripts/**)."
    :refs [wave28-04-claim-001])

  (claim
    :id wave28-06-claim-001
    :task wave28-06-task-runner-loop-smoke-v0
    :agent claudecode
    :seq 12
    :at "2026-04-28T15:00:00+08:00"
    :touched ["scripts/check-task-runner-manifest.mjs"
              "scripts/plan-task-runner.mjs"
              "scripts/render-wave-briefs.mjs"
              "scripts/verify-task-runner-batch.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Beginning wave28-06: cross-layer smoke pinning the same synthetic productive-only manifest through 5 layers (wave28-01 checker, wave28-02 plan CLI, wave28-03 brief renderer, wave28-04 daemon dry-run surface, wave28-05 batch verifier). Adds a small number of wave28-06-named fixtures to each of the 4 Node scripts (raising baselines 20→21+, 12→13+, 9→10+, 8→9+) plus one exhaustive Rust smoke test inside handlers::knowledge::plan::tests (raising daemon baseline 1680→1681). Pins productive-only invariant (archive/backfill absent from briefs + batch accounting), verification-tier propagation (full only on final smoke), heartbeat metadata propagation into thin briefs, applied=false literal, no-execution (no shell/git/network/LLM), and determinism (byte-identical re-run). Audits assemble forbidden patterns from string parts to avoid self-trip (wave24-06 / wave25-05 / wave26-06 / wave27-06 lesson).")

  (completion
    :id wave28-06-completion-001
    :task wave28-06-task-runner-loop-smoke-v0
    :agent claudecode
    :seq 13
    :at "2026-04-28T15:45:00+08:00"
    :touched ["scripts/check-task-runner-manifest.mjs"
              "scripts/plan-task-runner.mjs"
              "scripts/render-wave-briefs.mjs"
              "scripts/verify-task-runner-batch.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Wave28-06 done at commit 47177130da69. Cross-layer smoke pinning the wave28 task-contract runner v0 surface across all 5 layers. Added 6 wave28-06-named fixtures across the 4 Node scripts (Layer A: 20→22 / 15→16 categories; Layer B: 12→13; Layer C: 9→11 / 9→10 categories; Layer D: 8→9) AND 1 exhaustive Rust smoke test task_runner_loop_smoke_pins_wave28_invariants (Layer E: daemon baseline 1680→1681). Pinned cross-layer invariants: 1 (same productive-only manifest validates clean through all 5 layers); 2 (productive-only — defence-in-depth at schema + renderer + verifier + daemon-batch-id substring check); 3 (verification-tier counts: full=1 only on final smoke node, local=2, smoke=0 — pinned at plan CLI + daemon dry-run); 4 (heartbeat_minutes literal `10` echoed in thin brief; preamble OR brief carries boilerplate guidance); 5 (applied=Value::Bool(false) literal + wave27-06-style self-audit grep on plan.rs active source for forbidden patterns assembled from string fragments per wave24-06/25-05/26-06/27-06 lesson); 6 (byte-identical determinism on re-plan + re-render); 7 (no LLM/no network — covered by self-audit). CLI/Rust parity proof: same synthetic 3-node manifest (alpha + beta + final-smoke; dispatch_groups A/B/C; verification_tier local/local/full; estimated_minutes 30/25/45; heartbeat_minutes 10/10/10) drives consistent semantics across all 5 layers. Test totals: cargo test -p missiond-daemon --bin missiond → 1681 passed; cargo test -p missiond-mcp --lib → 17 passed (baseline preserved). Acceptance: all 11 commands exit=0 (4 dry-fixtures + 2 cargo test + 1 cargo build + 1 check-task-contract + 1 git diff --check + 1 check-missiond-hooks + 1 task-scope-guard). check-task-report OK against the new report. verify-task-contract OK against 47177130da69. Edits inside :write-scope only; zero matches against :must-not-touch (plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs / crates/missiond-mcp/src/tools/knowledge/plan.rs / .missiond/v2/** / .missiond/tasks/wave27/** / .missiond/tasks/wave28/wave28-*.lisp / dispatch-plan.lisp / .missiond/claudecode/** / scripts/check-task-contract.mjs / render-claudecode-task.mjs / verify-task-run.mjs)."
    :refs [wave28-06-claim-001]))

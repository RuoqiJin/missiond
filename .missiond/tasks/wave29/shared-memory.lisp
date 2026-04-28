;; Wave 29 shared-memory ledger.

(shared-memory wave29
  :schema "missiond.shared-memory.v1"
  :wave wave29
  :created-at "2026-04-28T16:00:00+08:00"
  :sequence 1

  (observation
    :id wave29-bootstrap-001
    :task wave29-01-context-atlas-schema-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T16:00:00+08:00"
    :touched [".missiond/tasks/wave29/manifest.lisp"
              ".missiond/tasks/wave29/context-atlas.lisp"
              ".missiond/tasks/wave29/pattern-cards.lisp"]
    :summary "Wave29 theme: runner-efficiency v1. Dispatch remains productive-only, no archive/backfill/index workers. New focus: structured navigation (context atlas), pattern-card execution recipes, parent-hotfix commit lineage, verification receipts, ready-queue planning, and cross-layer smoke without unnecessary cargo when Rust is not touched.")

  (claim
    :id wave29-01-claim-001
    :task wave29-01-context-atlas-schema-v0
    :agent claudecode
    :seq 2
    :at "2026-04-28T14:10:39+08:00"
    :touched [".missiond/tasks/schema/context-atlas-v1.lisp"
              "scripts/check-context-atlas.mjs"]
    :summary "Begin context atlas schema + checker work; will mirror wave28-01 manifest schema/checker conventions.")

  (claim
    :id wave29-02-claim-001
    :task wave29-02-pattern-card-schema-v0
    :agent claudecode
    :seq 3
    :at "2026-04-28T14:11:30+08:00"
    :touched [".missiond/tasks/schema/pattern-card-v1.lisp"
              "scripts/check-pattern-card.mjs"
              ".missiond/patterns/schema-checker.pattern.lisp"
              ".missiond/patterns/node-cli-readonly.pattern.lisp"
              ".missiond/patterns/report-lineage.pattern.lisp"
              ".missiond/patterns/cross-layer-smoke.pattern.lisp"
              ".missiond/patterns/large-file-navigation.pattern.lisp"]
    :summary "Begin wave29-02: define missiond.pattern-card.v1 schema, ship Node checker reusing scripts/lib/missiond_lisp.mjs, and seed 5 reusable pattern cards under .missiond/patterns. Schema must accept the existing dispatch-time pattern-cards.lisp shape unmodified and validate the 5 seed cards. Group A parallel with wave29-01 + wave29-04 on disjoint files.")

  (claim
    :id wave29-04-claim-001
    :task wave29-04-parent-hotfix-lineage-v1
    :agent claudecode
    :seq 4
    :at "2026-04-28T14:30:00+08:00"
    :touched ["scripts/check-task-report.mjs"
              "scripts/verify-task-run.mjs"
              "scripts/verify-task-runner-batch.mjs"
              ".missiond/tasks/schema/report-contract-v1.lisp"]
    :summary "Begin wave29-04: parent hotfix lineage v1. Add :agent_commit_hash + :parent_patches optional fields with full back-compat across check-task-report, verify-task-run, verify-task-runner-batch, plus schema. Group A parallel with wave29-01 + wave29-02 on disjoint files.")

  (completion
    :id wave29-01-completion-001
    :task wave29-01-context-atlas-schema-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T14:17:06+08:00"
    :touched [".missiond/tasks/schema/context-atlas-v1.lisp"
              "scripts/check-context-atlas.mjs"]
    :refs [wave29-01-claim-001]
    :summary "wave29-01 done at commit 1a5b01f9c292. Context-atlas schema v1 + read-only Node checker shipped; 21 dry fixtures across 13 categories pass; existing wave29 dispatch atlas validates clean; named exports SCHEMA / ATLAS_HEAD / projectAtlas / readContextAtlasFile / validateAtlasObject ready for wave29-03 + wave29-06.")

  (completion
    :id wave29-02-completion-001
    :task wave29-02-pattern-card-schema-v0
    :agent claudecode
    :seq 6
    :at "2026-04-28T14:35:00+08:00"
    :touched [".missiond/tasks/schema/pattern-card-v1.lisp"
              "scripts/check-pattern-card.mjs"
              ".missiond/patterns/schema-checker.pattern.lisp"
              ".missiond/patterns/node-cli-readonly.pattern.lisp"
              ".missiond/patterns/report-lineage.pattern.lisp"
              ".missiond/patterns/cross-layer-smoke.pattern.lisp"
              ".missiond/patterns/large-file-navigation.pattern.lisp"]
    :refs [wave29-02-claim-001]
    :summary "wave29-02 done at commit 57fdc88ff135. Pattern-card schema v1 + read-only Node checker shipped; 14 dry fixtures across 13 categories pass; existing wave29 dispatch-time pattern-cards.lisp validates with 2 warnings (cards without :known-good — by design), 5 seed cards under .missiond/patterns/*.pattern.lisp validate clean; named exports SCHEMA / LEGACY_SCHEMA_DISPATCH / ACCEPTED_SCHEMAS / SET_HEAD / SINGLE_HEAD / CARD_HEAD / REQUIRED_CARD_FIELDS / RECOMMENDED_CARD_FIELDS / OPTIONAL_CARD_FIELDS / HEADER_OPTIONAL_FIELDS / projectCard / readPatternCardFile / validateCardObject ready for wave29-03 + wave29-06 + wave29-07. RECOMMENDED_CARD_FIELDS is data-driven so adding another recommended field auto-warns. Schema accepts both canonical missiond.pattern-card.v1 and legacy missiond.pattern-cards.dispatch.v0 so the wave29 dispatch file passes unmodified.")

  (completion
    :id wave29-04-completion-001
    :task wave29-04-parent-hotfix-lineage-v1
    :agent claudecode
    :seq 7
    :at "2026-04-28T14:50:00+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"
              "scripts/verify-task-run.mjs"
              "scripts/verify-task-runner-batch.mjs"]
    :refs [wave29-04-claim-001]
    :summary "wave29-04 done at commit d7e314ab86d6 (post-hotfix; worker commit was c037c63f91aa). Parent hotfix lineage v1 hardening shipped: schema documents :kind enum (lint-cleanup|doc-fix|test-fix|scope-trim|hotfix-other) + final-hash drift invariant + extended hex format (>=7 <=64 chars). check-task-report fixtures 42 → 48 (+6: wave28-02 lineage exemplar, fail-missing-commit, fail-malformed-hash, fail-drift, fail-bad-files-path, fail-bad-kind). verify-task-run fixtures 19 → 23 (+4 lineage cases) + helper cases 15 → 20 (+5 buildLineageEntries). verify-task-runner-batch fixtures 9 → 12 (+3 either-hash matching cases). All 70 real on-disk reports + 105 task contracts stay green byte-identically. Final commit d7e314ab86d6 itself exercises the v1 hardening: parent hotfix removed pre-existing TS6133 unused TRACE_SCHEMA import; this report's :parent_patches lineage records the worker commit + parent lint-cleanup explicitly.")

  (claim
    :id wave29-03-claim-001
    :task wave29-03-runner-wave-prep-v0
    :agent claudecode
    :seq 8
    :at "2026-04-28T14:30:00+08:00"
    :touched ["scripts/prepare-task-runner-wave.mjs"
              "scripts/render-wave-briefs.mjs"]
    :summary "Begin wave29-03: ship read-only-plus-file-generation prep CLI scripts/prepare-task-runner-wave.mjs that reuses wave28-01 manifest checker + wave28-03 render-wave-briefs renderManifest via named imports (will surgically export renderManifest from render-wave-briefs.mjs). Generates report skeletons + bootstrap shared-memory/session-trace entries with audit-trail preamble-read trace event. Productive-only enforcement as defence-in-depth. Group B parallel with wave29-05 + wave29-06 on disjoint files.")

  (claim
    :id wave29-06-claim-001
    :task wave29-06-ready-queue-planner-v0
    :agent claudecode
    :seq 9
    :at "2026-04-28T15:05:00+08:00"
    :touched ["scripts/plan-task-runner.mjs"
              "scripts/check-task-runner-manifest.mjs"
              ".missiond/tasks/schema/task-runner-manifest-v1.lisp"]
    :summary "Begin wave29-06: evolve plan-task-runner.mjs to add additive ready-queue/phase-barrier planner gated behind --schedule ready-queue flag. Default --schedule group-barrier preserves byte-identical baseline output (13 fixtures must stay green). Ready-queue releases nodes as soon as :depends_on edges + same-group write-scope overlap edges are satisfied, with critical-path-first priority and lex tie-break. Output adds idle_window_savings_ms aggregate and per-task ready_at/finish_at vs barrier_finish_at. NO schema field changes (recommend NONE). Group B parallel with wave29-03 + wave29-05 on disjoint files.")

  (claim
    :id wave29-05-claim-001
    :task wave29-05-verification-receipt-schema-v0
    :agent claudecode
    :seq 10
    :at "2026-04-28T15:08:00+08:00"
    :touched [".missiond/tasks/schema/verification-receipt-v1.lisp"
              "scripts/check-verification-receipt.mjs"
              "scripts/verify-task-runner-batch.mjs"]
    :summary "Begin wave29-05: define missiond.verification-receipt.v1 schema, ship Node checker reusing scripts/lib/missiond_lisp.mjs, and add optional --receipts awareness to verify-task-runner-batch.mjs (12 baseline fixtures preserved byte-identically). Group B parallel with wave29-03 + wave29-06 on disjoint files. Receipts cache command evidence (wave/task_id/commit_hash/command/exit_code/tier/timing/files); reuse rules are conservative (commit + command + zero exit + tier-cover all required); receipts are NEVER substitutes for source facts or commit verification.")

  (completion
    :id wave29-03-completion-001
    :task wave29-03-runner-wave-prep-v0
    :agent claudecode
    :seq 11
    :at "2026-04-28T14:55:00+08:00"
    :touched ["scripts/prepare-task-runner-wave.mjs"
              "scripts/render-wave-briefs.mjs"]
    :refs [wave29-03-claim-001]
    :summary "wave29-03 done at commit d36de8040bf0. Runner-wave prep CLI shipped: scripts/prepare-task-runner-wave.mjs (NEW; ~1100 lines, 10 dry fixtures across 10 categories all green) reuses readManifestFile + validateManifestObject + FORBIDDEN_PRODUCTIVE_KINDS (wave28-01) and renderManifest + FORBIDDEN_ID_SUBSTRINGS (wave28-03 + wave29-03 surgical export) via in-process named imports — NO child_process. scripts/render-wave-briefs.mjs surgically edited: added export to renderManifest function declaration + converted FORBIDDEN_ID_SUBSTRINGS to export const + 2 doc-block comments; the wave28-03 11-case / 10-category baseline remains byte-identical green (dry-fixture re-run verified). CLI flags: --manifest --out-dir --dry-run --force --json --dry-fixture. Prepared artifact types: shared preamble + thin briefs (delegated to wave28-03 renderer) + report skeletons (deterministic missiond.report-contract.v1 :status draft stubs) + bootstrap shared-memory observation entry + bootstrap session-trace pair (:kind start + :kind read referencing the shared_preamble_path). Productive-only enforcement is defence-in-depth pre-write so a single bad node aborts cleanly. Acceptance: prepare --dry-fixture (10/10), render-wave-briefs --dry-fixture (11/11 byte-identical baseline), check-task-contract --all (105 tasks), git diff --check (clean), check-missiond-hooks (aligned), task-scope-guard (2 staged inside scope), verify-task-contract (OK against d36de8040bf0), check-task-report (1 report green).")

  (completion
    :id wave29-06-completion-001
    :task wave29-06-ready-queue-planner-v0
    :agent claudecode
    :seq 12
    :at "2026-04-28T15:35:00+08:00"
    :touched ["scripts/plan-task-runner.mjs"]
    :refs [wave29-06-claim-001]
    :summary "wave29-06 done at commit 1951aaa4fe79 (post-hotfix; worker commit was 15c5267fcaa4). Ready-queue planner v0 shipped as additive --schedule ready-queue branch on plan-task-runner.mjs. Default --schedule group-barrier preserves wave28-02 baseline output BYTE-IDENTICALLY (verified via stash + diff against pre-edit binary on the wave29 manifest). Fixtures 13 → 19 (+6 wave29-06-ready-queue: default-byte-identical, flag-emits-ready-windows, overlap-blocks-window, overlap-edge-injected-reject, priority-deterministic, idle-savings-computed). Manifest schema (.missiond/tasks/schema/task-runner-manifest-v1.lisp) + manifest checker (scripts/check-task-runner-manifest.mjs) UNCHANGED — no new required or optional fields; the brief's allowance for :schedule_hint was intentionally not exercised. Top-level ready_queue field exposes: schema (missiond.task-runner-plan.ready-queue.v0), priority_rule (critical-path-remaining-desc), tie_break (task-id-lex-asc), overlap_edges, order, tasks (per-task ready_at/finish_at/barrier_finish_at/idle_window_savings_minutes/priority_minutes/gated_by/dispatch_group/estimated_minutes), aggregate_idle_window_savings_minutes, max_idle_window_savings_minutes, wave_duration_minutes, wave_duration_savings_minutes. Algorithm: effective deps = explicit :depends_on + (under reject only) overlap edges from lex-smaller→lex-larger same-group ids; topo propagation; barrier_finish_at = batchStart + own duration (per-task, not collective). Final commit 1951aaa4fe79 itself exercises the wave29-04 lineage v1 hardening — parent lint-cleanup hotfix dropped the unused `dependents` parameter from computeReadyQueue() (TS6133); this report's :parent_patches lineage records the worker commit + parent lint-cleanup explicitly. NUL-byte check clean (one regression caught + repaired mid-implementation: literal NUL between `${from}` and `${to}` template literal at line 489). check-task-runner-manifest --dry-fixture 22/22 green; check-task-contract --all 105 tasks green.")

  (completion
    :id wave29-05-completion-001
    :task wave29-05-verification-receipt-schema-v0
    :agent claudecode
    :seq 13
    :at "2026-04-28T15:25:30+08:00"
    :touched [".missiond/tasks/schema/verification-receipt-v1.lisp"
              "scripts/check-verification-receipt.mjs"
              "scripts/verify-task-runner-batch.mjs"]
    :refs [wave29-05-claim-001]
    :summary "wave29-05 done at commit ed7940f9d572. Verification-receipt schema v1 + read-only Node checker shipped; 16 structural + 7 reuse-helper dry fixtures across 16 categories pass; verify-task-runner-batch.mjs gained OPTIONAL --receipts flag + computeReceiptCoverage helper + receipt_coverage JSON field (emitted ONLY when receipts supplied); 12 wave28-05 + wave29-04 baseline fixtures BYTE-IDENTICAL green (asserted in-loop via Object.hasOwn check); 4 new wave29-05 fixtures cover matching-receipt / stale-commit / no-receipts-key-absent / full-tier-covers-local-tier; check-task-contract --all 105 tasks green; check-task-report --all 73 reports green; git diff --check clean. Schema head: (verification-receipt-schema missiond.verification-receipt.v1 ...). Required receipt fields: :wave :task_id :commit_hash :command :exit_code :tier (PLUS at least one of :duration_ms or :started_at+:finished_at). Optional receipt fields: :receipt_id :files :notes (and the alternate timing form). Conservative reuse rules (verbatim in `isReceiptReusable`): (1) hex-prefix-agree commit; (2) command exact match after .trim(); (3) :exit_code === 0; (4) tier covering full > smoke > local ASYMMETRIC. ANY failure → reuse=false. Named exports SCHEMA / SET_HEAD / SINGLE_HEAD / RECEIPT_HEAD / TIERS / projectReceipt / readVerificationReceiptFile / validateReceiptObject / isReceiptReusable (10 total) ready for wave29-06 / wave29-07 / future planners. Receipts are CACHE rules NOT verification rules — documented in code comments as load-bearing; the orchestrator MUST still verify task contract / report / memory completion / git commit even when receipts are present. One unexpected work item: TS6133 unused `opts` parameter on validateReceiptForm (originally added speculatively for future allowHeaderFields config; the actual code path derives that from `isSingle` directly) — fixed by dropping the parameter from the signature and the two call sites. One race-condition workaround: scripts/plan-task-runner.mjs appeared as `M ` staged in the index because the parallel wave29-06 worker had staged its own file in the shared workspace; resolved with `git reset HEAD scripts/plan-task-runner.mjs` (leaves wave29-06's working-tree changes intact) then re-staged my 3 declared paths so scope-guard accepted cleanly.")

  (claim
    :id wave29-07-claim-001
    :task wave29-07-runner-efficiency-smoke-v1
    :agent claudecode
    :seq 14
    :at "2026-04-28T15:45:00+08:00"
    :touched ["scripts/check-context-atlas.mjs"
              "scripts/check-pattern-card.mjs"
              "scripts/prepare-task-runner-wave.mjs"
              "scripts/check-task-report.mjs"
              "scripts/check-verification-receipt.mjs"
              "scripts/plan-task-runner.mjs"
              "scripts/render-wave-briefs.mjs"
              "scripts/verify-task-runner-batch.mjs"]
    :summary "Begin wave29-07: cross-layer smoke v1 across 8 in-scope script files. Sole node in dispatch group C (no parallel agent). Each layer gets one wave29-07-loop-smoke fixture pinning a cross-layer invariant: (A) real wave29 atlas validates via in-process projectAtlas+validateAtlasObject, (B) all 5 seed pattern files validate via readPatternCardFile+validateCardObject, (C) prep CLI emits :kind read trace event referencing the preamble path, (D) hotfix lineage drift detection rejects mismatched final commit, (E) isReceiptReusable returns false on wrong commit/command/tier/exit and true when all 4 rules align, (F) ready-queue saves >0 minutes on unbalanced DAG, (G) thin brief mentions context_atlas_path + pattern_card_path, (H) batch verifier returns all_green on cross-layer 3-node manifest with lineage + receipts populated. NO cargo (verification_tier=smoke). All edits surgical, baseline fixtures must stay byte-identically green.")

  (completion
    :id wave29-07-completion-001
    :task wave29-07-runner-efficiency-smoke-v1
    :agent claudecode
    :seq 15
    :at "2026-04-28T16:05:00+08:00"
    :touched ["scripts/check-context-atlas.mjs"
              "scripts/check-pattern-card.mjs"
              "scripts/prepare-task-runner-wave.mjs"
              "scripts/check-task-report.mjs"
              "scripts/check-verification-receipt.mjs"
              "scripts/plan-task-runner.mjs"
              "scripts/render-wave-briefs.mjs"
              "scripts/verify-task-runner-batch.mjs"]
    :refs [wave29-07-claim-001]
    :summary "wave29-07 done at commit 08bf1a65d1d9. Cross-layer smoke v1 shipped: 8 layers (A check-context-atlas 21→22, B check-pattern-card 14→15, C prepare-task-runner-wave 10→11, D check-task-report 48→49, E check-verification-receipt 23→24, F plan-task-runner 19→20, G render-wave-briefs 11→12, H verify-task-runner-batch 16→17) each pin one wave29-07-loop-smoke fixture. Cross-layer invariants pinned: (1) same synthetic wave shape consistent across atlas+pattern+prep+report+receipt+planner+renderer+batch, (2) productive-only enforced at all entry points, (3) preamble-read trace emission auditable via parsed Lisp shape (not substring), (4) parent-hotfix lineage final hash authoritative + drift rejected, (5) receipt reuse conservative across 4 rules + matching/non-matching cases, (6) ready-queue strictly positive savings on unbalanced 4-node DAG (savings > 0 minutes verified), (7) zero cargo in any acceptance, (8) real-world wave29 atlas + 5 seed pattern files validate clean via in-process named exports. NO child_process / spawn / git mutation / network / LLM. Imports added: prepare-task-runner-wave.mjs gained Lisp parser helpers for layer C; check-context-atlas + check-pattern-card runFixtures gained `run`-style branch (no impact to baseline source-fixtures). All baseline fixtures across all 8 files stay green at expected counts. Acceptance: 8 dry-fixtures green at new counts (22/15/11/49/24/20/12/17), check-task-contract --all 105 tasks green, git diff --check clean, check-missiond-hooks aligned, scope-guard staged 8 files OK, verify-task-contract OK against final commit 08bf1a65d1d9."))

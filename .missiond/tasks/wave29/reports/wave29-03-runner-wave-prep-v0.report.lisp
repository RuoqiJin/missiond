;; Wave 29 / Task 03 — Runner wave prep v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp

(report wave29-03-runner-wave-prep-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave29-03-runner-wave-prep-v0"
  :status done
  :commit_hash "d842b1d3322b"
  :agent_commit_hash "d36de8040bf0"
  :final_commit_hash "d842b1d3322b"
  :verified_commit_hash "d842b1d3322b"
  :parent_patches
    [(:commit "d842b1d3322b"
      :kind lint-cleanup
      :reason "Parent hotfix after worker exit: drop superfluous await in synchronous fixture loop to clear TS80007."
      :files ["scripts/prepare-task-runner-wave.mjs"])]
  :files_changed
    ["scripts/prepare-task-runner-wave.mjs"
     "scripts/render-wave-briefs.mjs"]

  :acceptance_results
    [(:command "node scripts/prepare-task-runner-wave.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "10 fixtures across 10 categories pass: dry-run-no-write, full-run, skeleton-shape, preamble-read trace, force-overwrites, deterministic-output, idempotent-rerun, archive-id rejected, backfill-kind rejected, bootstrap append-only.")
     (:command "node scripts/render-wave-briefs.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "wave28-03 baseline 11 fixtures across 10 categories continue to pass byte-identically; wave29-03 surgical edit only added two named exports (renderManifest + FORBIDDEN_ID_SUBSTRINGS) and a documentation block — no CLI behavior or fixture body changed.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks).")
     (:command "git diff --check -- scripts/prepare-task-runner-wave.mjs scripts/render-wave-briefs.mjs"
      :exit_code 0
      :ok true
      :notes "Whitespace clean.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "missiond hook aligned, executable.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: 2 staged files, all inside :write-scope.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-03-runner-wave-prep-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify originally OK against worker commit d36de8040bf0; post-worker parent hotfix d842b1d3322b keeps the same task commit message and stays inside write-scope, so lineage now records d842b1d3322b as final/verified.")
     (:command "node scripts/check-task-report.mjs .missiond/tasks/wave29/reports/wave29-03-runner-wave-prep-v0.report.lisp"
      :exit_code 0
      :ok true
      :notes "report self-validation green; missiond.report-contract.v1 shape OK.")]

  :notes
    ["Prepared artifact types: (1) shared preamble — delegated to wave28-03 renderer; (2) thin briefs — delegated to wave28-03 renderer; (3) report skeletons — new in wave29-03, deterministic missiond.report-contract.v1 stubs with :status draft, empty :commit_hash, empty :files_changed/:acceptance_results; (4) bootstrap shared-memory observation entry (id pattern: <wave>-bootstrap-<NNN>); (5) bootstrap session-trace pair — :kind start (id pattern: <wave>-trace-bootstrap-start-<NNN>) plus :kind read (id pattern: <wave>-trace-bootstrap-read-<NNN>) whose :files vector references the manifest's :shared_preamble_path so the wave29-shared-preamble-read audit expectation is recorded BEFORE the first worker boots."
     "Preamble-read trace evidence: a dedicated :kind read trace-event (id <wave>-trace-bootstrap-read-<NNN>) is appended to .missiond/tasks/<wave>/session-trace.lisp with :files containing only the manifest's shared_preamble_path. The kind 'read' is the canonical session-trace-v1 verb for navigation/audit reads (already used by wave29-01/02/04 worker reads). Verifiers can grep for :kind read alongside the preamble path to confirm the audit expectation was seeded."
     "render-wave-briefs.mjs edited surgically: added export to the existing renderManifest function declaration (with a wave29-03 doc-block clarifying the named-export contract) and converted the FORBIDDEN_ID_SUBSTRINGS module constant to an export const (with a doc comment). NO function body, CLI behavior, fixture seed, or fixture assertion changed — the wave28-03 11-case / 10-category baseline remains byte-identical green."
     "Reuse via named imports only — NO child_process.spawn|exec|fork. Imports: readManifestFile + validateManifestObject + FORBIDDEN_PRODUCTIVE_KINDS from check-task-runner-manifest.mjs (wave28-01); renderManifest + FORBIDDEN_ID_SUBSTRINGS from render-wave-briefs.mjs (wave28-03 + wave29-03 surgical export). The renderer transitively reuses loadSingleTask + renderTask + renderSharedPreamble from render-claudecode-task.mjs (wave28-03)."
     "Productive-only enforcement (defence-in-depth): walks every node before any write and rejects on (a) :kind in FORBIDDEN_PRODUCTIVE_KINDS or (b) :task_id containing any FORBIDDEN_ID_SUBSTRINGS substring. Pre-write enforcement so a single bad node aborts cleanly — no partial briefs/skeletons/bootstrap left behind."
     "--dry-run validates + plans but writes NOTHING — the dry-run-writes-nothing fixture asserts the tmp tree is byte-identical before/after. --force overwrites existing brief/preamble/skeleton; default skip-when-present is byte-identical to wave28-03 renderer behavior."
     "Idempotent re-run: second prepareWave() call against the same manifest writes 0 briefs / 0 skeletons (skip-when-present); only the bootstrap ledgers grow (each prep run records its own preamble-read audit expectation via append-only)."
     "Bootstrap append-only invariant: spliceBeforeFinalParen walks back from EOF skipping whitespace, asserts a closing ), then splices the new entry between the existing payload and that paren. Pre-existing entries' bytes are preserved verbatim; the bootstrap-append-preserves-existing-entries fixture asserts the prefix slice (everything up to lastIndexOf ')') is byte-identical before/after."
     "Bootstrap ordinal/seq derivation: nextOrdinalFromTracePath scans existing -bootstrap*-NNN ids and returns max+1 so re-runs do not collide on duplicate :id (which the session-trace and shared-memory checkers reject); nextMemorySeq + nextTraceSeq scan all :seq <int> literals and return max+1 so the strictly-increasing seq invariant is preserved."
     "JSON output schema: { ok: true, manifest_path, wave, briefs_written, skeletons_written, bootstrap_emitted, dry_run }. Stable shape so downstream verifiers / the wave29-07 cross-layer smoke can pin it without re-deriving."
     "Parent hotfix lineage backfilled by Codex before wave29 archive: worker commit d36de8040bf0 produced the main implementation; parent commit d842b1d3322b dropped a superfluous await in scripts/prepare-task-runner-wave.mjs after the worker exited. This report now treats d842b1d3322b as :commit_hash / :final_commit_hash / :verified_commit_hash and preserves d36de8040bf0 as :agent_commit_hash."
     "Verification tier 'local' — no full workspace build, no cargo. Acceptance is dry-fixture + check-task-contract --all + git diff --check + scope-guard + verify-task-contract, all green at worker commit d36de8040bf0 and lineage-finalized at parent hotfix commit d842b1d3322b."])

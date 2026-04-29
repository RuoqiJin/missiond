;; Wave 39 task report.
;; Schema: missiond.report-contract.v1

(report wave39-01-task-scoped-lifecycle-event-files-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave39-01-task-scoped-lifecycle-event-files-v0"
  :status done
  :commit_hash "ffad9c14fe9c"
  :files_changed
    [".missiond/tasks/schema/task-lifecycle-event-v1.lisp"
     ".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-task-lifecycle-events.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/task-runner-append-event.mjs"
     "scripts/task-runner-dispatch.mjs"
     "scripts/task-runner-next-action.mjs"
     "scripts/task-runner-submit-dispatch.mjs"
     "scripts/task-runner-wave-state.mjs"
     "scripts/verify-task-runner-batch.mjs"]
  :acceptance_results
    [(result :command "node scripts/check-task-lifecycle-events.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "18 cases pass: legacy ledger + request-local + new standalone task-scoped one-event files including missing :wave / bad commit hash / absolute :touched / mismatched filename seq / cross-dir duplicate :id / cross-dir non-monotonic :seq fixtures.")
     (result :command "node scripts/task-runner-append-event.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "7 cases pass: existing legacy --ledger-only and request-local projection and concurrent child appends still work; new --events-dir mode allocates 000001/000002.event.lisp under directory lock; hybrid --events-dir + --ledger writes both with the same :seq.")
     (result :command "node scripts/task-runner-wave-state.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "4 cases pass: ready-queue / blocked / cancelled re-dispatchable still hold; new case auto-detects .missiond/tasks/<wave>/events/, merges with the legacy ledger, dedupes the shared event id (lifecycle_event_count=2 not 3), and exposes the parent_hotfix hash from the standalone event file.")
     (result :command "node scripts/task-runner-dispatch.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "4 cases pass: emit-dispatch-events now writes BOTH the request-local one-event file AND the task-scoped one-event file via the auto-detected events-dir; events_dir_path surfaces in the descriptor.")
     (result :command "node scripts/task-runner-submit-dispatch.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "3 cases pass: dry-run shape unchanged; --apply now appends the task-scoped one-event file alongside the request-local one-event file; failed submissions still write nothing.")
     (result :command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "19 fixtures pass: existing wave30-05 lifecycle/receipt/finalized-report smoke is augmented with a wave39-01 cross-layer projection that calls appendLifecycleEvent({eventsDir, ...}) and revalidates the standalone 000001.event.lisp through the same checker.")
     (result :command "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
             :exit_code 0
             :ok true
             :note "Cross-layer dry fixture covers the new contract text: blueprint primary path, schema two-shape contract, checker validateTaskScopedLifecycleEventFile / renderTaskScopedLifecycleEventFile, append-event --events-dir / appendLifecycleEventToEventsDir / scanTaskScopedEventDirMaxSeq / appendLedgerCompatProjection, wave-state defaultEventsDirPath / readOptionalTaskScopedEventFiles / mergeLifecycleEvents, dispatch + submit eventsDirPath pass-through, batch verifier task-scoped events-dir smoke.")
     (result :command "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
             :exit_code 0
             :ok true
             :note "Real-tree run pins the same needles against the live blueprint / schema / scripts so future drift is caught.")
     (result :command "node scripts/check-lisp-blueprint-compression.mjs"
             :exit_code 0
             :ok true
             :note "v1 manifest + v3 blueprint compression contract still holds after extending the task-runner-cli :note for the task-scoped event-file primary path.")
     (result :command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             :exit_code 0
             :ok true
             :note "blueprint architecture-lisp check OK on the updated file.")
     (result :command "perl -ne 'exit 1 if /\\x00/' .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/check-task-lifecycle-events.mjs scripts/task-runner-append-event.mjs scripts/task-runner-wave-state.mjs scripts/task-runner-next-action.mjs scripts/task-runner-dispatch.mjs scripts/task-runner-submit-dispatch.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs"
             :exit_code 0
             :ok true
             :note "no NUL bytes in any of the ten touched files.")
     (result :command "git diff --check -- .missiond/v3/missiond-blueprint.lisp .missiond/tasks/schema/task-lifecycle-event-v1.lisp scripts/check-task-lifecycle-events.mjs scripts/task-runner-append-event.mjs scripts/task-runner-wave-state.mjs scripts/task-runner-next-action.mjs scripts/task-runner-dispatch.mjs scripts/task-runner-submit-dispatch.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/verify-task-runner-batch.mjs"
             :exit_code 0
             :ok true
             :note "no whitespace-error or conflict markers in the write-scope files.")]
  :notes "Closes the event-sourced lifecycle isomorphism gap in V3 by promoting task-scoped lifecycle events to first-class one-event files while keeping the legacy task-lifecycle-events.lisp ledger as a backward-compatible projection/input.\n\nStandalone task-scoped lifecycle event artifact shape: a single (lifecycle-event ...) form per file at .missiond/tasks/<wave>/events/<seq>.event.lisp, schema=missiond.task-lifecycle-event.v1 (same as the ledger so projector code stays untouched), with explicit :wave header so one file can be validated in isolation. Required fields: :schema :wave :id :task :actor_role :event_kind :commit_role :seq :at :touched :summary; optional :commit_hash :report_path :receipt_path :refs :legacy_memory_id :legacy_trace_id. Files are zero-padded numeric (000001.event.lisp, 000002.event.lisp, ...). The schema doc now declares both shapes (standalone-task-event-file as primary, legacy-task-lifecycle-event-log as compatibility).\n\nAppend-event CLI and locking/atomic-write behavior: scripts/task-runner-append-event.mjs accepts --ledger only (existing legacy path, unchanged), --events-dir only (new task-scoped events-dir mode), or both (events-dir is primary; ledger is updated as a compatibility projection with the same allocated :seq). The events-dir path locks <events-dir>/.dir.lock with O_EXCL, scans dirMax + ledgerMax to allocate seq=max+1, renders the standalone event bytes, validates them through validateLifecycleEventFiles, and creates the final file via fs.openSync(file, 'wx') so two cooperating writers cannot overwrite the same numeric file. Ledger compat projection is a nested withLedgerLock under the dir lock with consistent ordering (dir-lock then ledger-lock) to avoid deadlock with legacy ledger-only writers.\n\nWave-state / dispatch / batch verifier compatibility behavior: scripts/task-runner-wave-state.mjs auto-detects .missiond/tasks/<wave>/events/ when present, validates every standalone event file, and merges with the legacy ledger via a new mergeLifecycleEvents that dedupes by event :id (event files take precedence). The merged stream feeds the existing finalize/dispatch/blocked decision logic unchanged; lifecycle_event_count is now the unique-id count. scripts/task-runner-next-action.mjs, scripts/task-runner-dispatch.mjs, and scripts/task-runner-submit-dispatch.mjs accept --events-dir, default it to the conventional task-scoped events-dir, and pass it through to appendLifecycleEvent so dispatch_task events land in BOTH the standalone event file and the legacy ledger compat projection (and optionally the request-local one-event file when --request-id/--request-events-dir are supplied). scripts/verify-task-runner-batch.mjs gains a wave39-01 cross-layer smoke that calls appendLifecycleEvent({eventsDir, ...}) and revalidates the resulting 000001.event.lisp through the same checker.\n\nLegacy ledger backward-compat guarantees: existing --ledger-only callers keep working byte-identical (verified by the legacy-ledger-only fixture in task-runner-append-event); historical wave30-38 task-lifecycle-events.lisp files are not migrated or rewritten; the ledger schema, header shape, splice-before-final-paren append, and atomic-rename are unchanged; the projector / parent-hotfix / finalize-report code paths read events through the same eventFromNode helper and therefore see the merged stream without any changes; wave-state still auto-detects the legacy ledger as a fallback when the events-dir is missing.\n\nAcceptance command results: every command listed in the task contract completes with exit_code 0 and no diagnostics; see :acceptance_results above for the per-command notes."
  :verification_tier local)

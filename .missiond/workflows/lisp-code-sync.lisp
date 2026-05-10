;; MissionD workflow: real-time Lisp -> code isomorphism loop.
;;
;; Purpose:
;;   SSOT Lisp edits are not allowed to remain as inert declarations. A changed
;;   Lisp/checker file must enter EventBus, run the appropriate typed compile /
;;   code-isomorphism gate, and either write a synced report or create a
;;   visible BoardTask that asks the master to produce exact shards.

(workflow lisp-code-sync
  :schema "missiond.workflow.v1"
  :workflow_id lisp-code-sync
  :status active
  :source_plans [resident-codex-master final-convergence-control-plane-m6-split]
  :match_rules
    ((trigger :kind file-watch :event SystemEvent.ConfigChanged :path ".missiond/**/*.lisp")
     (trigger :kind file-watch :event SystemEvent.ConfigChanged :path ".missiond/**/*.mjs")
     (dedupe-key "lisp-code-sync:<project>:<path-hash>")
     (storm-dedupe-key "lisp-code-sync:<project>:storm-circuit"))
  :inputs [SystemEvent.ConfigChanged ProjectRegistry compiled-v3-runtime project-checker code-isomorphism-gate]
  :steps
    ((step s1 :name observe-lisp-change
       :entry [SystemEvent.ConfigChanged]
       :logic "Accept only changed paths under a registered project .missiond directory; resolve project_id/root through MissionD ProjectRegistry; unknown roots write diagnostic report only.")
     (step s2 :name compile-and-check
       :entry [project-resolution changed-path]
       :logic "For missiond, run node scripts/compile-v3-runtime.mjs --json before code-isomorphism; external projects use .missiond/check.sh when present; green gates are synced and create no worker work.")
     (step s3 :name create-sync-task
       :entry [failed-checker-result]
       :logic "Create or reuse one visible BoardTask with dedupe key lisp-code-sync:<project>:<path-hash>, auto_execute=true, and a request for evidence-plan plus exact accepted shard creation before implementation. If same-source failures exceed the storm threshold, switch to root-cause dedupe key lisp-code-sync:<project>:storm-circuit and stop spending one worker per changed path.")
     (step s4 :name report
       :entry [checker-result task-result]
       :logic "Write .missiond/v3/runtime/lisp-code-sync/<timestamp>-<path-hash>.report.lisp, expose lispCodeSync counters plus reportDirs/stormCircuitHits/recentSyncTaskCreations through mission_master_status, and leave downstream completion authority to durable final evidence plus green gates.")
     (step s5 :name self-loop-guard
       :entry [file-watcher report-dir]
       :logic "Watcher and subscription consumer must ignore runtime report paths, compiled/runtime projection, checkpoints, and cold runtime evidence, suppress unchanged content fingerprints before expensive gates, debounce repeated path events, and apply report retention/GC so lisp-code-sync reports never trigger another lisp-code-sync run.")
     (step s6 :name stale-decision-revalidation
       :entry [mission_question_list mission_master_status.lispCodeSync]
       :logic "Before showing lisp-code-sync self-loop decisions, revalidate reportDirs; if recentReports5m=0 and no project is over retention, mark the question stale_evidence/resolved_by_runtime_fix instead of asking the user to decide old facts.")
     (step s7 :name stale-boardtask-dispatch-revalidation
       :entry [autopilot-ready-task BoardTask.title BoardTask.description BoardTask.dedupe_key]
       :logic "Before selecting a slot or sending PTY input, Autopilot must detect BoardTasks that point at .missiond/v3/runtime/lisp-code-sync/** report paths and close them as resolved_by_runtime_fix/stale_evidence; stale runtime report tasks must not consume worker slots."))
  :egress [lisp-code-sync-report BoardTaskCreated master-wakeup mission_master_status]
  :runtime-surfaces
    [crates/missiond-daemon/src/engine/lisp_code_sync.rs
     crates/missiond-daemon/src/engine/master_control.rs
     crates/missiond-daemon/src/main.rs]
  :acceptance
    ((criterion c1 :rule "Editing .missiond/**/*.lisp emits SystemEvent.ConfigChanged.")
     (criterion c2 :rule "ConfigChanged is consumed by lisp-code-sync before worker delegation.")
     (criterion c3 :rule "Green code-isomorphism writes a synced report and creates no BoardTask.")
     (criterion c4 :rule "Failing code-isomorphism creates one visible deduped BoardTask.")
     (criterion c5 :rule "Implementation must still pass through exact accepted shard workflow before code mutation.")
     (criterion c6 :rule "Runtime reports under .missiond/v3/runtime/** are ignored by the watcher; unchanged content fingerprints are suppressed at watcher and subscription consumption; reports are pruned by retention/GC.")
     (criterion c7 :rule "Repeated same-source failures trip a storm circuit and reuse lisp-code-sync:<project>:storm-circuit.")
     (criterion c8 :rule "Pending lisp-code-sync self-loop decisions are revalidated before display and auto-resolved when evidence is stale.")
     (criterion c9 :rule "Autopilot revalidates stale lisp-code-sync runtime report BoardTasks before dispatch and closes them without PTY."))
  :risk-gates
    ((gate g1 :rule "lisp-code-sync never edits code directly; it compiles/checks and delegates through BoardTask/EventBus.")
     (gate g2 :rule "A failed sync task asks master for evidence-plan and exact accepted shard before any code worker.")
     (gate g3 :rule "File watcher publishes SystemEvent.ConfigChanged; sync processing subscribes to EventBus rather than bypassing it.")
     (gate g4 :rule "PTY-only completion is diagnostic; downstream completion requires durable final evidence.")
     (gate g5 :rule "Runtime report paths are ignored before EventBus publication; unchanged content fingerprints and repeated path events are suppressed before they can become sync tasks, including old queued ConfigChanged events.")
     (gate g6 :rule "Same-source task creation storms must open a root-cause circuit rather than recursively dispatching more workers.")
     (gate g7 :rule "Stale runtime report BoardTasks are resolved before slot selection, not sent to workers."))
  :completion
    ((criterion c1 :rule "A matching Lisp/checker edit is classified as synced, failed-sync-task-created, or unknown-project.")
     (criterion c2 :rule "Green code-isomorphism writes a synced report and creates no BoardTask.")
     (criterion c3 :rule "Failing code-isomorphism creates exactly one visible deduped BoardTask.")
     (criterion c4 :rule "mission_master_status exposes lispCodeSync runtime counters.")
     (criterion c5 :rule "lisp-code-sync self-generated reports do not create ConfigChanged loops.")
     (criterion c6 :rule "mission_question_list auto-resolves stale lisp-code-sync self-loop decisions using current reportDir evidence.")
     (criterion c7 :rule "dispatch_board_tasks resolves stale runtime report BoardTasks before slot selection."))
  :safety
    ((rule :id no-direct-codegen :text "lisp-code-sync never edits code directly; it compiles/checks and delegates through BoardTask/EventBus.")
     (rule :id no-broad-implementation :text "A failed sync task asks master for evidence-plan and exact shard before any code worker.")
     (rule :id eventbus-first :text "File watcher publishes SystemEvent.ConfigChanged; sync processing subscribes to EventBus rather than bypassing it.")
     (rule :id stale-decision-revalidation :text "Decision Inbox must revalidate operational evidence before display; stale lisp-code-sync self-loop questions are auto-resolved rather than shown as fresh choices.")
     (rule :id stale-boardtask-dispatch-revalidation :text "Autopilot must revalidate lisp-code-sync runtime-report BoardTasks before dispatch and close stale evidence tasks without consuming a worker slot.")))

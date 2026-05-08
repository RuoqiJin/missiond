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
     (dedupe-key "lisp-code-sync:<project>:<path-hash>"))
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
       :logic "Create or reuse one visible BoardTask with dedupe key lisp-code-sync:<project>:<path-hash>, auto_execute=true, and a request for evidence-plan plus exact accepted shard creation before implementation.")
     (step s4 :name report
       :entry [checker-result task-result]
       :logic "Write .missiond/v3/runtime/lisp-code-sync/<timestamp>-<path-hash>.report.lisp, expose lispCodeSync counters through mission_master_status, and leave downstream completion authority to durable final evidence plus green gates."))
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
     (criterion c5 :rule "Implementation must still pass through exact accepted shard workflow before code mutation."))
  :risk-gates
    ((gate g1 :rule "lisp-code-sync never edits code directly; it compiles/checks and delegates through BoardTask/EventBus.")
     (gate g2 :rule "A failed sync task asks master for evidence-plan and exact accepted shard before any code worker.")
     (gate g3 :rule "File watcher publishes SystemEvent.ConfigChanged; sync processing subscribes to EventBus rather than bypassing it.")
     (gate g4 :rule "PTY-only completion is diagnostic; downstream completion requires durable final evidence."))
  :completion
    ((criterion c1 :rule "A matching Lisp/checker edit is classified as synced, failed-sync-task-created, or unknown-project.")
     (criterion c2 :rule "Green code-isomorphism writes a synced report and creates no BoardTask.")
     (criterion c3 :rule "Failing code-isomorphism creates exactly one visible deduped BoardTask.")
     (criterion c4 :rule "mission_master_status exposes lispCodeSync runtime counters."))
  :safety
    ((rule :id no-direct-codegen :text "lisp-code-sync never edits code directly; it compiles/checks and delegates through BoardTask/EventBus.")
     (rule :id no-broad-implementation :text "A failed sync task asks master for evidence-plan and exact shard before any code worker.")
     (rule :id eventbus-first :text "File watcher publishes SystemEvent.ConfigChanged; sync processing subscribes to EventBus rather than bypassing it.")))

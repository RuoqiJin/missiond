;; MissionD — Pillar: state-machines
;; Split from intent.lisp for parallel loading
;; Parent: intent.lisp

  (pillar state-machines
    (purpose "all finite state automata governing lifecycle transitions")

    (component pty-session-state
      :target "semantic-terminal crate (external) — src/types.rs"
      (state-machine pty-session
        (states (Starting) (Idle) (SlashMenu) (Thinking) (Responding)
                (ToolRunning) (Confirming) (Error))
        (transitions
          (Starting -> Idle :trigger "prompt-detected")
          (Starting -> Error :trigger "process-crash")
          (Idle -> Thinking :trigger "spinner-detected")
          (Idle -> SlashMenu :trigger "slash-menu-detected")
          (Idle -> ToolRunning :trigger "tool-activity")
          (Idle -> Confirming :trigger "permission-dialog")
          (SlashMenu -> Idle :trigger "menu-dismissed")
          (Thinking -> Responding :trigger "output-begins")
          (Thinking -> ToolRunning :trigger "tool-hint")
          (Responding -> Idle :trigger "prompt-returns")
          (Responding -> ToolRunning :trigger "tool-invoked")
          (ToolRunning -> Idle :trigger "prompt-returns")
          (ToolRunning -> Confirming :trigger "permission-dialog")
          (Confirming -> ToolRunning :trigger "confirmed")
          (Confirming -> Idle :trigger "denied"))))

    (component board-task-status
      :target "crates/missiond-core/src/types/board.rs"
      (state-machine board-task
        (states (Open) (Running) (Done) (Failed) (Blocked))
        (transitions
          (Open -> Running :trigger "claimed")
          (Open -> Blocked :trigger "dependency-unmet")
          (Running -> Done :trigger "completed")
          (Running -> Failed :trigger "error")
          (Blocked -> Open :trigger "dependency-resolved")))
      (state-machine engineering-phase
        (states (Investigate) (Consult) (Plan) (Execute) (Finalize))
        (transitions
          (Investigate -> Consult :trigger "context-gathered")
          (Consult -> Plan :trigger "review-complete")
          (Plan -> Execute :trigger "plan-approved")
          (Execute -> Finalize :trigger "implementation-done")
          (Finalize -> Investigate :trigger "issues-found"))))

    (component task-status
      :target "crates/missiond-core/src/types/task.rs"
      (state-machine task
        (states (Queued) (Running) (Completed) (Failed))
        (transitions
          (Queued -> Running :trigger "slot-claimed")
          (Running -> Completed :trigger "result-received")
          (Running -> Failed :trigger "error-or-timeout"))))

    (component question-status
      :target "crates/missiond-core/src/types/question.rs"
      (state-machine question
        (states (Pending) (Answered) (Dismissed))
        (transitions
          (Pending -> Answered :trigger "answer-provided")
          (Pending -> Dismissed :trigger "user-dismissed"))))

    (component extraction-phase
      :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      (state-machine extraction
        (states (Idle) (Sending) (WaitingForIdleness) (Complete))
        (transitions
          (Idle -> Sending :trigger "extraction-triggered")
          (Sending -> WaitingForIdleness :trigger "content-sent")
          (WaitingForIdleness -> Complete :trigger "slot-idle")
          (WaitingForIdleness -> Idle :trigger "timeout")
          (Complete -> Idle :trigger "reset")))))


(genome missiond-autopilot
  :schema "missiond.genome.v1"
  :activation shadow

  (organ autopilot
    (tissue wakeup
      :receptors [BoardEvent::TaskCreated "BoardEvent::Updated(status=open)" "BoardEvent::StatusChanged(new_status=open)" SlotEvent::BecameIdle]
      :allow-atoms [autopilot.match-board-wakeup autopilot.match-slot-wakeup]
      :allow-effects [NotifyAutopilotDispatch Log Metric Noop]
      :budgets (:max-causation-depth 10 :max-events-per-correlation 128 :max-cell-runtime-ms 300000 :idempotency-cache-size 4096)
      (molecule board-task-created
        :on BoardEvent::TaskCreated
        :atoms [autopilot.match-board-wakeup]
        :effects [NotifyAutopilotDispatch])
      (molecule board-updated-open
        :on "BoardEvent::Updated(status=open)"
        :atoms [autopilot.match-board-wakeup]
        :effects [NotifyAutopilotDispatch])
      (molecule board-status-open
        :on "BoardEvent::StatusChanged(new_status=open)"
        :atoms [autopilot.match-board-wakeup]
        :effects [NotifyAutopilotDispatch])
      (molecule slot-became-idle
        :on SlotEvent::BecameIdle
        :atoms [autopilot.match-slot-wakeup]
        :effects [NotifyAutopilotDispatch]))

    (tissue tick
      :receptors [SystemTick::Autopilot60s]
      :allow-atoms [autopilot.match-system-tick]
      :allow-effects [RunAutopilotTick Log Metric Noop]
      :budgets (:max-causation-depth 10 :max-events-per-correlation 128 :max-cell-runtime-ms 300000 :idempotency-cache-size 4096)
      (molecule autopilot-60s-tick
        :on SystemTick::Autopilot60s
        :atoms [autopilot.match-system-tick]
        :effects [RunAutopilotTick]))

    (tissue dispatch
      :receptors [Command::NotifyAutopilotDispatch]
      :allow-atoms [autopilot.match-notify-command]
      :allow-effects [RunBoardDispatch Log Metric Noop]
      :budgets (:max-causation-depth 10 :max-events-per-correlation 128 :max-cell-runtime-ms 300000 :idempotency-cache-size 4096)
      (molecule notify-to-board-dispatch
        :on Command::NotifyAutopilotDispatch
        :atoms [autopilot.match-notify-command]
        :effects [RunBoardDispatch]))

    (tissue watchdog
      :receptors [IncidentEvent::Reported RuntimeGuard::Quarantine]
      :allow-atoms [autopilot.match-quarantine autopilot.match-runtime-error]
      :allow-effects [PublishBoardEvent AddBoardTaskNote UpdateBoardTask Log Metric Noop]
      :budgets (:max-causation-depth 10 :max-events-per-correlation 128 :max-cell-runtime-ms 300000 :idempotency-cache-size 4096)
      (molecule quarantine-incident
        :on RuntimeGuard::Quarantine
        :atoms [autopilot.match-quarantine]
        :effects [Log Metric])
      (molecule runtime-incident
        :on IncidentEvent::Reported
        :atoms [autopilot.match-runtime-error]
        :effects [Log Metric Noop]))))

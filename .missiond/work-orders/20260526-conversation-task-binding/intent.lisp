(intent conversation-task-binding
  :schema "missiond.work-order.intent.v1"
  :created_at "2026-05-26T17:20:00+08:00"
  :summary "Fix MissionD worker completion so reused provider slots cannot bind an old completed conversation/final to a newly dispatched BoardTask."
  :problem "A real iOS/Jarvis client-channel smoke created BoardTask f0708d99-ae99-4fc0-bd12-f0ccd9fc9716 for dispatch hint normalization, but Autopilot closed it with a stale durable final from previous conversation d15c186c... about Jarvis monitor endpoints."
  :goal "Completion authority must be scoped to the current task and current provider session: dispatch must not rebind completed historical conversations, and durable final selection must reject conversations ended before the task claim time."
  :constraints ["Lisp first"
                "No broad refactor"
                "Add focused regression tests"
                "Deploy to Mac mini through GitHub pull and local build after commit"])

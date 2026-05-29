(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260528134502-Pin-xjpcode-text-only-paid-CLI-p"
  :events ((event created :at "2026-05-28T13:45:02.203Z" :actor missiond-work-order)
           (event scope-expanded
             :at "2026-05-29T14:00:00+08:00"
             :actor codex
             :reason "Jarvis intent/plan authoring also needs xjpcode text-only provider env, Rust author adapter, deploy env propagation, and checker pins.")
           (event smoke-generalized
             :at "2026-05-29T18:35:00+08:00"
             :actor codex
             :reason "Intent/plan smoke must accept configured paid CLI author metadata rather than hard-coding Codex-only author fields.")
           (event smoke-direct-answer-terminal
             :at "2026-05-29T18:45:00+08:00"
             :actor codex
             :reason "Grounded direct answers complete with answer_delta/result_artifact/final and should not be forced through BoardTask dispatch semantics.")))

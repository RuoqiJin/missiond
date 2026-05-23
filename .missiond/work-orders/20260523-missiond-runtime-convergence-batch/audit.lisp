(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "20260523-missiond-runtime-convergence-batch"
  :events ((event created :at "2026-05-23T10:40:00+08:00" :actor codex
             :note "Batch work-order added because the implemented runtime batch spans Jarvis intent/plan streaming, task-result artifact closure, typed V3 runtime projection, and conversation ingestion hardening.")
           (event accepted :at "2026-05-23T10:40:00+08:00" :actor codex
             :note "Scope is constrained to already-staged MissionD runtime/checker/SSOT files and verified by focused Rust tests plus V3 static gates.")))

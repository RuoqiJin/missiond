(intent
  :id "20260526-jarvis-monitor-endpoints"
  :summary "Pin canonical Jarvis monitor endpoints in runtime context so workers do not guess local ports."
  :trigger "Client-channel audit found a false low-priority diagnostic caused by probing a wrong local monitor port."
  :scope ["crates/missiond-daemon/src/handlers/knowledge/context_gather.rs" ".missiond/v3/shards/request-runtime.lisp" ".missiond/v3/evidence/codex-boot-context.lisp" "scripts/check-v3-runtime-path-hygiene.mjs"]
  :acceptance ["runtime_environment contains canonical local and public Jarvis monitor endpoints" "runtime path hygiene checker pins the endpoint contract" "static checks pass"])

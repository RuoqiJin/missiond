(plan
  :id "20260526-jarvis-monitor-endpoints"
  :steps [
    (step ssot :action "Add canonical monitor endpoint guidance to request runtime and boot context evidence.")
    (step runtime :action "Emit monitor_endpoints in mission_context_gather runtime_environment payload.")
    (step checker :action "Pin monitor_endpoints/local_http/public_https in runtime path hygiene checker.")
    (step closure :action "Refresh behavior closure anchors and checker projections that drifted while validating the runtime-context change.")
    (step verify :action "Run runtime path hygiene, grounded dispatch, fmt check, and git diff check.")
  ]
  :write_scope [".missiond/work-orders/20260526-jarvis-monitor-endpoints/**" "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs" ".missiond/v3/shards/request-runtime.lisp" ".missiond/v3/evidence/codex-boot-context.lisp" ".missiond/v3/evidence/blueprint-notes.lisp" ".missiond/v3/shards/universe/behavior-closure.lisp" "scripts/check-v3-runtime-path-hygiene.mjs" "scripts/check-frontend-board-runtime-projection.mjs" "scripts/check-v3-autopilot-runtime-isomorphism.mjs" "scripts/check-v3-master-control-isomorphism.mjs" "crates/missiond-daemon/src/context/v3_contracts/generated.rs" "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs" "scripts/generated/v3_contracts.d.ts" "scripts/generated/v3_contracts.mjs" "scripts/generated/v3_runtime_defaults.mjs"]
  :must_not_touch ["Mac mini deployment" "XJP repo" "secrets"]
  :acceptance "No worker should need to infer Jarvis monitor port/path from repo files.")

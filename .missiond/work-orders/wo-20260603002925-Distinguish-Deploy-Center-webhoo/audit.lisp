(work-order-audit
  :schema "missiond.work-order.audit.v1"
  :id "wo-20260603002925-Distinguish-Deploy-Center-webhoo"
  :events ((event created :at "2026-06-03T00:29:25.723Z" :actor missiond-work-order)
           (event verified :at "2026-06-03T00:29:00Z" :actor codex
             :checks ["node scripts/check-v3-memory-kb-isomorphism.mjs --json"
                      "node scripts/project-v3-contracts.mjs --write && node scripts/compile-v3-runtime.mjs --json"
                      "cargo test -p missiond-daemon --bin missiond deployment_event_ -- --nocapture"
                      "cargo check -p missiond-daemon"]
             :result "V3 checker/runtime compile passed; deployment_event tests passed 12/12; cargo check passed with existing warnings.")))

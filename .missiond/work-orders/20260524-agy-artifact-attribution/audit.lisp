(audit
  :id "20260524-agy-artifact-attribution"
  :schema "missiond.work-order.audit.v1"
  :status completed
  :created-at "2026-05-24T00:00:00+08:00"
  :notes ["Created after iOS Jarvis Agy smoke showed the strict intent/plan gate and materialized context-pack path worked, but Autopilot reused an older Agy artifact from another BoardTask."
          "Implemented explicit BoardTask ID attribution for Agy durable markdown artifacts; foreign BoardTask artifacts are rejected before broad keyword matching."]
  :evidence ["cargo test -p missiond-daemon agy_artifact -- --nocapture"
             "node scripts/check-v3-agent-cli-regression.mjs --json"
             "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
             "node scripts/check-v3-code-isomorphism-complete.mjs --json"
             "node scripts/check-v3-final-convergence.mjs --json --static-only"
             "git diff --check"])

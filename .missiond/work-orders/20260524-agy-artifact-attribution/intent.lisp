(intent
  :id "20260524-agy-artifact-attribution"
  :schema "missiond.work-order.intent.v1"
  :summary "Bind Agy durable markdown artifacts to the active BoardTask before Autopilot closes worker tasks."
  :problem "Agy read-only smoke completed with a current artifact, but Autopilot closed the new BoardTask using an older Antigravity brain markdown file from a different BoardTask because broad keyword matching outranked task identity."
  :desired-outcome "Autopilot treats explicit BoardTask ID inside an Agy artifact as first-authority attribution; artifacts that declare a different BoardTask cannot close the current task."
  :constraints [ssot-first no-fallback no-rsync touched-rustfmt-only]
  :acceptance ["cargo test -p missiond-daemon agy_artifact"
               "node scripts/check-v3-agent-cli-regression.mjs --json"
               "node scripts/check-v3-final-convergence.mjs --json --static-only"])

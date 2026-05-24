(intent
  :id "20260524-agy-numbered-artifact-close"
  :schema "missiond.work-order.intent.v1"
  :summary "Allow Agy numbered markdown artifacts to close their delegated BoardTask."
  :problem "Agy read-only smoke produced a current task artifact with headings such as `## 1. Findings`, but Autopilot's output-contract matcher only accepted unnumbered headings and left the BoardTask running."
  :desired-outcome "Autopilot normalizes numbered report headings before enforcing Findings / Evidence / Recommendations / Verification, so provider formatting does not block task-result-artifact closure."
  :constraints [ssot-first no-fallback no-rsync-macmini-sync touched-rustfmt-only]
  :acceptance ["cargo test -p missiond-daemon agy_artifact"
               "cargo test -p missiond-daemon output_contract_close_blocker"
               "node scripts/check-v3-agent-cli-regression.mjs --json"
               "node scripts/check-v3-final-convergence.mjs --json --static-only"])

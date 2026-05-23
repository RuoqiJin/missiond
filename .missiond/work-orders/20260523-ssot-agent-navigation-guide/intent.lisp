(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-ssot-agent-navigation-guide"
  :objective "Add SSOT agent navigation guide for MissionD V3"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["node scripts/compile-v3-runtime.mjs --check --json"
                  "node scripts/check-v3-agent-entry-slices.mjs --json"
                  "node scripts/check-v3-capability-governance-isomorphism.mjs --json"
                  "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                  "node scripts/project-v3-contracts.mjs --check --json"
                  "node scripts/check-typed-lisp-compiler.mjs"
                  "cargo test -p missiond-daemon tool_directory"
                  "cargo test -p missiond-daemon context_slice_agent_entry_selects_by_intent"
                  "node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])

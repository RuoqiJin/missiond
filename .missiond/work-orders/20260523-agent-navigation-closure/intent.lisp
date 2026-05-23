(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-agent-navigation-closure"
  :objective "Close MissionD SSOT agent navigation system with feedback, project catalog, Board and CLI"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["node scripts/compile-v3-runtime.mjs --check --json"
                  "node scripts/check-v3-agent-entry-slices.mjs --json"
                  "node scripts/check-v3-agent-navigation-quality.mjs --json --check"
                  "node scripts/check-v3-agent-navigation-closure.mjs --json"
                  "node scripts/check-v3-capability-governance-isomorphism.mjs --json"
                  "node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                  "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                  "node scripts/check-typed-lisp-compiler.mjs"
                  "node scripts/project-frontend-board-config.mjs --check"
                  "node scripts/check-frontend-board-lisp-schema.mjs"
                  "node scripts/check-frontend-board-code-isomorphism.mjs"
                  "node scripts/check-frontend-board-runtime-projection.mjs"
                  "pnpm --dir packages/board build"
                  "cargo test -p missiond-daemon tool_directory"
                  "cargo test -p missiond-daemon agent_navigation"
                  "cargo test -p missiond-daemon context_slice"
                  "node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])

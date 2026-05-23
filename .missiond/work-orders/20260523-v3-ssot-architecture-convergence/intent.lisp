(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-v3-ssot-architecture-convergence"
  :objective "Implement MissionD V3 SSOT architecture convergence, generated runtime defaults, data-driven convergence gate, shard split, and artifact commit outbox"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                  "node scripts/check-typed-lisp-compiler.mjs"
                  "node scripts/project-v3-contracts.mjs --check --json"
                  "node scripts/compile-v3-runtime.mjs --check --json"
                  "node scripts/check-v3-semantic-checker-coverage.mjs --json"
                  "cargo test -p missiond-daemon v3_blueprint_runtime"
                  "cargo test -p missiond-daemon file_artifacts"
                  "cargo test -p missiond-daemon request::"
                  "cargo test -p missiond-core event"
                  "cargo test --workspace"
                  "pnpm --dir packages/board build"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])

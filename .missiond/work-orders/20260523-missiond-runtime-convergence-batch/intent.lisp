(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-missiond-runtime-convergence-batch"
  :objective "Converge Jarvis intent/plan streaming, typed V3 runtime projection, and conversation/runtime ingestion fixes under one auditable MissionD batch"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["cargo check -p missiond-core -p missiond-daemon"
                  "bash scripts/rustfmt-missiond.sh --check"
                  "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                  "node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json"
                  "node scripts/check-v3-workstation-pool-isomorphism.mjs --json"
                  "node scripts/check-v3-agent-cli-regression.mjs --json"
                  "node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "git diff --check"]
  :constraints ["Lisp-first"
                "work-order-gated-commit"
                "no-secret-values"
                "no-rsync-macmini-sync"
                "fast-fail-no-fallback"])

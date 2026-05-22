(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260522-runtime-config-projection"
  :objective "Add compiled typed runtime config projection and route Rust/JS runtime consumers through it first"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["dune runtest --root tools/missiond_lispc"
                  "node scripts/compile-v3-runtime.mjs --json"
                  "node scripts/check-typed-lisp-compiler.mjs --json"
                  "node scripts/check-v3-workstation-config-isomorphism.mjs --json"
                  "node scripts/check-v3-router-policy-isomorphism.mjs --json"
                  "cargo test -p missiond-daemon v3_blueprint_runtime"
                  "node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "git diff --check"]
  :constraints ["Lisp remains authoring SSOT"
                "compiled runtime JSON is generated projection output"
                "public Rust and JS loader APIs stay stable"
                "source Lisp fallback remains available"])

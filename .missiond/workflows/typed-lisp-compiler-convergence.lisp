(workflow typed-lisp-compiler-convergence
  :schema "missiond.workflow.typed-lisp-compiler-convergence.v1"
  :workflow_id typed-lisp-compiler-convergence
  :status active
  :source_plans [lisp-ssot-v3 ocaml-typed-compiler-upgrade]
  :purpose "Keep Lisp as the human/agent SSOT while moving fragile Lisp semantics from JS token scans into an OCaml structural gate."
  :inputs [lisp-files expected-surfaces workflow-files project-registry maturity-target]
  :match_rules (:project missiond :surface typed-lisp-compiler :risk medium)
  :steps [s1 s2 s3 s4 s5 s6 s7]
  :core ((step s1 :logic "run the OCaml structural gate first when toolchain exists; otherwise return a typed toolchain diagnostic and keep JS gates as compatibility wrappers")
         (step s2 :logic "parse MissionD V3, workflow, and project Lisp into typed AST nodes with stable source locations")
         (step s3 :logic "validate pillar -> function -> entry/core/egress/surface semantics, ordered steps, workflow shape, universe registry, and maturity gates")
         (step s4 :logic "emit stable JSON diagnostics and compiled JSON projections; compiled JSON is generated output and must not be hand-authored")
         (step s5 :logic "load compiled JSON from Rust only as a read-only projection cache with source-hash diagnostics and existing fallback behavior")
         (step s6 :logic "migrate external project semantic checks one sample at a time, starting with Auth domain hardening")
         (step s7 :logic "allow code-anchor JS checkers to continue validating Rust/TS/MJS file anchors after the OCaml Lisp structural gate succeeds"))
  :risk-gates [no-runtime-hot-path no-auto-toolchain-install no-handwritten-compiled-json no-silent-js-fallback]
  :completion (:checks ["node scripts/check-typed-lisp-compiler.mjs"
                        "node scripts/compile-v3-runtime.mjs --json"
                        "node scripts/check-v3-pillar-flow-schema.mjs --engine=ocaml --json"
                        "node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json"]
               :fallback "If OCaml tooling is absent, strict --engine=ocaml checks fail with OCAML_TOOLCHAIN_MISSING; default gates keep running through JS wrappers until the toolchain is installed."))

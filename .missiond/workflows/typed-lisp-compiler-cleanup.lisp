(workflow typed-lisp-compiler-cleanup
  :schema "missiond.workflow.typed-lisp-compiler-cleanup.v1"
  :workflow_id typed-lisp-compiler-cleanup
  :status active
  :owner resident-master-control
  :authority [typed-lisp-compiler-convergence project-domain-hardening]
  :source_plans [ocaml-typed-compiler-upgrade v3-runtime-ssot]
  :purpose "Finish moving fragile Lisp semantics out of JS checkers while keeping JS as wrapper and code-anchor layer."
  :inputs [ocaml-cli js-checkers compiled-json rust-loader final-convergence-status]
  :match_rules
    ((trigger :kind manual :surface typed-lisp-compiler :when "objective asks to shrink JS checker semantics")
     (trigger :kind checker :code OCAML_LISPC_FAILED)
     (trigger :kind workflow :workflow_id multi-project-domain-hardening-wave :when "domain hardening needs a typed gate"))
  :steps
    ((step s1 :id inventory-js-semantics
       :logic "List JS checkers that still parse Lisp semantics instead of calling missiond-lispc.")
     (step s2 :id extend-typed-gates
       :logic "Add OCaml validators for workflow, project maturity, universe, and project-domain hardening before deleting duplicate JS logic.")
     (step s3 :id wrapper-reduction
       :logic "Reduce JS to argument parsing, OCaml invocation, compatibility JSON shape, and code-anchor scanning.")
     (step s4 :id compiled-json-runtime
       :logic "Extend generated compiled JSON and Rust loader coverage for universe, workflows, maturity, and domain-hardening registry.")
     (step s5 :id concurrency-policy
       :logic "Enforce scripts/lib/ocaml_lispc.mjs Dune lock; forbid raw parallel dune in workflow/checker commands.")
     (step s6 :id final-gate
       :logic "Run typed compiler checks and final convergence after every cleanup shard."))
  :egress [typed-validator-shards js-wrapper-shards rust-loader-shards checker-report]
  :risk-gates
    ((gate g1 :rule "No silent JS fallback when --engine=ocaml is requested.")
     (gate g2 :rule "Compiled JSON is generated output and must not be hand-authored.")
     (gate g3 :rule "OCaml is not added to daemon hot paths.")
     (gate g4 :rule "Raw parallel dune commands are forbidden; use the locked JS wrapper."))
  :completion
    ((criterion c1 :rule "project-domain-hardening has an OCaml structural gate.")
     (criterion c2 :rule "Final convergence passes after JS wrapper reduction.")
     (criterion c3 :rule "Remaining JS semantic checks are documented as code-anchor responsibilities.")))

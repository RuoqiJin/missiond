(workflow project-ssot-convergence
  :schema "missiond.workflow.project-ssot-convergence.v1"
  :purpose "Reusable multi-project SSOT convergence muscle memory: turn an existing code project into compact Lisp SSOT plus checker-backed code mapping."
  :owner resident-master-control
  :authority v3-project-blueprint-registry
  :inputs [project-id project-root canonical-intent existing-code context-pack-path? acceptance]
  :workers
    ((codex-master :role integrator :write [board kb checkpoint context-pack] :code-write false)
     (claude-opus :role implementation :write exact-shard-only :code-write true)
     (claude-sonnet :role narrow-patch :write exact-file-region-only :code-write true)
     (gemini :role readonly-survey :write context-pack-only :code-write false))
  :core
    ((step s1 :id collect-evidence
       :logic "read current intent Lisp, manifests, public routes/tools/commands, package metadata, tests, runtime docs, and dirty worktree status")
     (step s2 :id draft-l1-index
       :logic "if missing or verbose, write a compact .missiond/intent.lisp index with identity, constraints, pillars, module links, and gates")
     (step s3 :id draft-backend-blueprint
       :logic "write .missiond/backend/<project>-backend-blueprint.lisp when Rust/API/service code exists")
     (step s4 :id draft-frontend-blueprint
       :logic "write .missiond/frontend/<project>-frontend-blueprint.lisp when UI/client code exists")
     (step s5 :id create-checkers
       :logic "add checker-first schema/code-isomorphism/runtime-projection gates before broad code refactors")
     (step s6 :id run-code-isomorphism
       :logic "map every declared public behavior to Lisp surface; mark gaps as designed/backfill, not invisible")
     (step s7 :id dispatch-backfill-workers
       :logic "create disjoint BoardTasks with context_pack_path, write_scope, must_not_touch, acceptance, model_profile, timeout_secs, completion protocol")
     (step s8 :id verify-and-report
       :logic "run project checker, build/test, diff check; write convergence report and observed dispatch-stability issues"))
  :egress [project-blueprint project-checkers boardtasks convergence-report kb-note]
  :risk-gates
    ((gate g1 :rule "Do not refactor code before checker pins current facts.")
     (gate g2 :rule "Same file and same surface have one owner.")
     (gate g3 :rule "Gemini remains read-only until scoped write smoke passes.")
     (gate g4 :rule "User dirty changes are facts; never revert them.")
     (gate g5 :rule "Code-first changes create backfill BoardTask through commit-lisp-convergence."))
  :completion
    ((criterion c1 :rule "Lisp has pillar/function/entry/core/egress/surface shape.")
     (criterion c2 :rule "Checker proves root, blueprint, surfaces, and public code anchors.")
     (criterion c3 :rule "Build/test command is known even when deferred.")
     (criterion c4 :rule "MissionD project registry can locate the project by id and root.")))

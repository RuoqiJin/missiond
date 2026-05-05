(workflow project-ssot-convergence
  :schema "missiond.workflow.project-ssot-convergence.v1"
  :purpose "Reusable multi-project SSOT convergence muscle memory: turn an existing code project into compact Lisp SSOT plus checker-backed code mapping."
  :owner resident-master-control
  :authority v3-project-blueprint-registry
  :inputs [project-id project-root canonical-intent existing-code dirty-baseline context-pack-path? acceptance]
  :workers
    ((codex-master :role integrator :write [board kb checkpoint context-pack] :code-write false)
     (claude-opus :role implementation :write exact-shard-only :code-write true)
     (claude-sonnet :role narrow-patch :write exact-file-region-only :code-write true)
     (gemini :role readonly-survey :write context-pack-only :code-write false))
  :core
    ((step s1 :id collect-evidence
       :logic "read current intent Lisp, manifests, public routes/tools/commands, package metadata, tests, runtime docs, and dirty worktree status")
     (step s1b :id choose-write-strategy
       :logic "if canonical intent is already large (>500 lines) or the repo has operator-owned dirty code outside write_scope, prefer overlay+manifest mode: preserve existing intent facts, append a compact M6 overlay, and write a bounded intent-manifest instead of full-rewriting the SSOT")
     (step s2 :id draft-l1-index
       :logic "if missing, write a compact .missiond/intent.lisp index with identity, constraints, pillars, module links, and gates; if present and large, add an M6 overlay rather than replacing the whole file")
     (step s3 :id draft-backend-blueprint
       :logic "write .missiond/backend/<project>-backend-blueprint.lisp when Rust/API/service code exists")
     (step s4 :id draft-frontend-blueprint
       :logic "write .missiond/frontend/<project>-frontend-blueprint.lisp when UI/client code exists")
     (step s5 :id create-checkers
       :logic "add checker-first schema/code-isomorphism/runtime-projection gates before broad code refactors")
     (step s6 :id run-code-isomorphism
       :logic "map every declared public behavior to Lisp surface; mark gaps as designed/backfill, not invisible")
     (step s7 :id dispatch-backfill-workers
       :logic "create disjoint BoardTasks with context_pack_path, read_scope, write_scope, must_not_touch, acceptance, model_profile, timeout_secs, completion protocol; read_scope is readable evidence, write_scope is the only writable set, and must_not_touch forbids write/stage/commit rather than reads")
     (step s7b :id swarm-target-projects-structural-pass
       :logic "Multi-project objectives MUST pass target_project_ids/targetProjectIds/target_projects/targetProjects to mission_swarm_run as a structured array. Project lists embedded only in the natural-language objective are discarded by the tool because the schema does not parse prose; the swarm then collapses target_projects to project_id only and the universe wave cannot fan out. Repro: BoardTask 31e5449c-e315-4003-ad59-c3eebd5eb837 first call resolved targets to missiond only because the prose project list was not passed structurally. The corrected dispatch passes target_project_ids = [<id>...].")
     (step s8 :id verify-and-report
       :logic "run project checker, build/test, diff check; when dirty-baseline is outside this task's ownership, run scoped diff checks for owned paths and record full diff-check blockers as diagnostics instead of formatting or touching operator code")
     (step s9 :id worker-stall-recovery
       :logic "if a ClaudeCode worker stalls after intermediate narration such as 'let me write' without file changes, interrupt once, reduce the shard to an atomic overlay/manifest patch, and record the stall as dispatch telemetry; do not retry the same full-rewrite prompt indefinitely"))
  :egress [project-blueprint project-checkers boardtasks convergence-report kb-note]
  :risk-gates
    ((gate g1 :rule "Do not refactor code before checker pins current facts.")
     (gate g2 :rule "Same file and same surface have one owner.")
     (gate g3 :rule "Gemini remains read-only until scoped write smoke passes.")
     (gate g4 :rule "User dirty changes are facts; never revert them.")
     (gate g5 :rule "Code-first changes create backfill BoardTask through commit-lisp-convergence.")
     (gate g6 :rule "If engine_hint/pool_hint cannot be honored, Autopilot records reroute_reason as a durable BoardTask note.")
     (gate g7 :rule "Evaluation workers produce structured artifacts (Findings/Evidence/Recommendations/Verification), not raw KB JSON dumps.")
     (gate g8 :rule "Rust projects must expose a project-local touched-file rustfmt checker; workers must not run cargo fmt -p or cargo fmt --all unless an explicit maintainer task owns repo-wide formatting.")
     (gate g9 :rule "Dirty worktree SSOT convergence commits must stage explicit .missiond paths only; full git diff --check failures in non-owned files are diagnostics, not permission to edit those files.")
     (gate g10 :rule "Large existing intent files should use M6 overlay+manifest unless the task explicitly owns a full SSOT rewrite.")
     (gate g11 :rule "PTY-only completion of a delegated worker BoardTask MUST NOT close unless the screen carries a structured artifact (Findings / Evidence / Recommendations / Verification / Summary heading) or durable provider final evidence is available; intermediate assistant sentences captured between the prompt return and the JSONL final write are not valid finals.")
     (gate g12 :rule "A slot may be the running owner of at most ONE BoardTask at a time. Autopilot dispatch unclaims any other BoardTask whose claim_executor still points at the slot before the new dispatch claims it; conversations.task_id is force-rebound to the incoming task so mission_conversation_query(taskId=...) always returns the current dispatch's conversation."))
  :completion
    ((criterion c1 :rule "Lisp has pillar/function/entry/core/egress/surface shape.")
     (criterion c2 :rule "Checker proves root, blueprint, surfaces, and public code anchors.")
     (criterion c3 :rule "Build/test command is known even when deferred.")
     (criterion c4 :rule "MissionD project registry can locate the project by id and root.")
     (criterion c5 :rule "Dirty-baseline handling is explicit: preserved, staged only by owned path, and reported in the manifest or convergence note.")))

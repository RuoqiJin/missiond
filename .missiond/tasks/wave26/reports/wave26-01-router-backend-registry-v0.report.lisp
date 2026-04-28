;; Wave 26 / Task 01 — Router backend readiness registry v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave26/wave26-01-router-backend-registry-v0.lisp

(report wave26-01-router-backend-registry-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave26-01-router-backend-registry-v0"
  :status done
  :commit_hash "2ac6a5b2dbd2"
  :files_changed
    [".missiond/router/router-backend-registry-v1.lisp"
     ".missiond/tasks/schema/router-backend-registry-v1.lisp"
     "scripts/check-router-backend-registry.mjs"]

  :acceptance_results
    [(:command "node scripts/check-router-backend-registry.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "router-backend-registry fixtures OK (19 cases, 13 categories). Categories: pass, fail-schema, fail-unknown-id, fail-duplicate-id, fail-status-enum, fail-runtime-allowed-literal, fail-runtime-allowed-status, fail-runtime-ready-substrate, fail-empty-blockers, fail-non-goals, fail-unknown-entry-head, fail-missing-required, fail-unknown-field. 4 pass cases (minimal current-default, all-five-backends, runtime-ready with substrate, advisory-only with optional fields) + 15 fail cases covering every rejection rule in the schema's :validation-contract.")
     (:command "node scripts/check-router-backend-registry.mjs .missiond/router/router-backend-registry-v1.lisp"
      :exit_code 0
      :ok true
      :notes "router-backend-registry check OK (1 registry, 5 backends). Seed registry passes structural validation: schema match, all 5 enum backend ids present, claudecode current-default + runtime_allowed true + non-nil substrate, four non-claudecode entries advisory-only/unavailable + runtime_allowed false + non-empty apply_blockers, every entry lists at least one non-goal.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (83 tasks). All wave22+wave23+wave24+wave25+wave26 task contracts continue to parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No drift introduced by the new schema or seed files.")
     (:command "git diff --check -- .missiond/tasks/schema/router-backend-registry-v1.lisp .missiond/router/router-backend-registry-v1.lisp scripts/check-router-backend-registry.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check exit 0 — no whitespace or conflict-marker errors in any of the 3 newly committed files.")]

  :time_sinks
    ["read background patterns (router-policy schema + check-router-policy.mjs + session-trace schema + check-session-trace.mjs + missiond_lisp.mjs helpers) to mirror established conventions"
     "audit codebase to confirm only ClaudeCode workstation orchestration is wired (handlers::knowledge::workstation_dispatch + agent_execution) — informs apply_blockers text for the four non-claudecode entries"
     "design schema to enforce two cross-wave invariants atomically: runtime_allowed=true requires status in {current-default, runtime-ready}, AND those two statuses require non-nil substrate"
     "write 19-case dry-fixture covering pass + every rejection rule in the schema's :validation-contract; tighter than the 12-case minimum but matches check-router-policy's 19-case ceiling"]

  :major_decisions
    [(:decision "Schema entry head is `backend` (singular) and registry header is `router-backend-registry`."
      :rationale "Matches the singular-entry-head idiom in router-policy v1 (entry head = `rule`) and session-trace v1 (entry head = `trace-event`). Avoids inventing a new convention.")
     (:decision "Runtime_allowed is a literal boolean atom (true|false), not a yes/on alias."
      :rationale "Matches router-policy v1's :dry-run-only / :runtime-replacement enforcement. The checker is the single source of truth for what the invariant looks like on disk; aliases would erode that.")
     (:decision "Substrate accepts string OR the literal atom nil — not a missing field."
      :rationale "Forces authors to make absence explicit. Missing substrate is structurally rejected; nil is a deliberate declaration. Pairs with the cross-wave invariant that runtime-ready/current-default REQUIRE a non-nil substrate.")
     (:decision "patch-worker is seeded as :readiness_status unavailable rather than advisory-only."
      :rationale "No router-policy rule today actually recommends patch-worker — it is a reserved enum slot. unavailable communicates this is not even an aspirational recommendation surface yet, while advisory-only is reserved for backends actively named in policy rules (deterministic-checker, verifier-worker).")
     (:decision "Named exports include RUNTIME_ALLOWED_STATUSES alongside the schema enums."
      :rationale "wave26-02 / wave26-03 will need to reproduce the cross-wave invariant in their own join logic; exporting the constant prevents the invariant from being silently re-encoded with different membership.")]

  :unexpected_work
    ["Codebase audit of handlers::knowledge::workstation_dispatch + agent_execution confirmed 0 runtime adapters exist for the four non-claudecode backends, so no seed entries needed downgrading from runtime-ready. The audit is now memorialized in the seed file's header comment so wave26-02 reviewers do not have to repeat it."]

  :blockers []

  :trace_refs
    ["wave26-01-trace-start-001"
     "wave26-01-trace-commit-001"
     "wave26-01-trace-complete-001"
     ".missiond/tasks/wave26/session-trace.lisp"]

  :notes "Schema + seed + checker only — no runtime consumer in this task per the contract. wave26-02 (router recommendation readiness join) and wave26-03 (plan-side surfacing) will import SCHEMA / BACKEND_IDS / READINESS_STATUSES / RUNTIME_ALLOWED_STATUSES / projectBackend / projectRegistry / readBackendRegistryFile from scripts/check-router-backend-registry.mjs to reuse the validated parser. The CLI is gated on import.meta.url === file://process.argv[1] (matches wave23-06 check-session-trace.mjs pattern) so importing the module never re-runs main().")

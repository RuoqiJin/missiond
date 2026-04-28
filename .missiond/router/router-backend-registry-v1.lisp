;; MissionD router-backend-registry v1 — seed registry.
;; Schema: .missiond/tasks/schema/router-backend-registry-v1.lisp
;;
;; READ-ONLY ADVISORY DATA. Nothing in MissionD reads this file at runtime
;; in wave 26. wave26-02 / wave26-03 will join the registry to the
;; recommendation pipeline (still advisory) and to the report contract;
;; future waves may add an apply gate that refuses to promote a backend
;; whose :runtime_allowed is false. ClaudeCode remains the live default.
;;
;; All 5 router-policy backend ids are listed below, each annotated with:
;;   :readiness_status   — current-default | advisory-only | runtime-ready | unavailable
;;   :runtime_allowed    — true iff status is current-default or runtime-ready
;;   :apply_blockers     — concrete reasons promotion to live dispatch is rejected
;;   :substrate          — Rust module path (for adapters that exist) or nil
;;   :non-goals          — explicit non-claims so reviewers can audit drift
;;
;; Codebase audit (2026-04-28) confirmed only ClaudeCode workstation
;; orchestration is wired in missiond-daemon::handlers::knowledge::workstation_dispatch
;; and ::agent_execution. No runtime adapter exists for missiond-llm-router,
;; deterministic-checker, patch-worker, or verifier-worker — those entries
;; are advisory-only / unavailable until later waves ship an adapter.

(router-backend-registry seed-v1
  :schema "missiond.router-backend-registry.v1"
  :version "v1"
  :description "Seed backend readiness registry distilled from the wave22+ codebase audit. ClaudeCode is the only runtime-allowed backend today; every other entry in the router-policy enum is recorded as advisory-only with apply_blockers naming the missing adapter. This file is the SSOT future apply-gate work will consult — do NOT mark a backend runtime-ready unless an adapter actually ships."

  (backend
    :id claudecode
    :readiness_status current-default
    :runtime_allowed true
    :apply_blockers []
    :substrate "missiond-daemon::handlers::knowledge::workstation_dispatch"
    :owner "claudecode"
    :adapter_path "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
    :non-goals
      ["does not select a specific Claude model slot — slot selection stays with missiond-llm-router"
       "does not imply ClaudeCode is the only acceptable runtime backend forever; later waves may promote additional adapters via this registry"
       "does not encode capacity / concurrency limits — those live with the workstation pillar, not the registry"]
    :notes "ClaudeCode workstation orchestration is the live default runtime adapter today. wave22+ trace observations show every runtime task ran through this substrate.")

  (backend
    :id missiond-llm-router
    :readiness_status advisory-only
    :runtime_allowed false
    :apply_blockers
      ["no runtime adapter shipped — missiond-daemon does not invoke a slot-resolved LLM as a task backend, only ClaudeCode workstation orchestration is wired in handlers::knowledge::workstation_dispatch"
       "router replacement is explicitly out of scope wave22+; promoting this backend would require a new daemon-side dispatcher and an apply gate (not delivered in wave26)"]
    :substrate nil
    :owner "claudecode"
    :non-goals
      ["does not promise an LLM-router backend will ever ship — the entry exists only so router policy may name it as a recommendation surface"
       "does not encode which slot (Sonnet / Opus / Haiku) would be used — slot routing is the LLM-router's internal concern, not the registry's"
       "does not replace runtime dispatch"]
    :notes "Router policy may recommend this backend for code-alignment tasks; the recommendation is observational only until a daemon-side adapter exists.")

  (backend
    :id deterministic-checker
    :readiness_status advisory-only
    :runtime_allowed false
    :apply_blockers
      ["no runtime adapter shipped — there is no daemon-side worker that takes a checker script and reports back via the workstation/agent dispatch protocol"
       "today checker scripts run only via the human-driven acceptance path inside ClaudeCode; the registry would need a worker pillar entry before this could be runtime-ready"]
    :substrate nil
    :owner "claudecode"
    :non-goals
      ["does not imply a deterministic-checker worker is staffed for any particular wave — the entry exists so router policy can recommend it for pure-script tasks"
       "does not authorize bypassing ClaudeCode authoring of the checker scripts themselves; only the validation pass is what would be advisory-routed here"
       "does not replace runtime dispatch"]
    :notes "Mirrors the wave24-01 r-deterministic-checker-tasks rule; that rule is also advisory-only.")

  (backend
    :id patch-worker
    :readiness_status unavailable
    :runtime_allowed false
    :apply_blockers
      ["no runtime adapter shipped — there is no Mechanic-style automated patch worker integrated with missiond-daemon dispatch"
       "patch-worker is a forward-looking enum slot referenced by router-policy; the substrate would need to live in jarvis-mechanic and be exposed to missiond-daemon, neither of which is in scope wave26"
       "even the surrounding apply gate (wave26+) is not built yet — promoting this backend would require both an adapter AND the gate"]
    :substrate nil
    :owner "claudecode"
    :non-goals
      ["does not commit to any particular patch-worker implementation (Mechanic vs. another); the entry exists only so router policy may name it as a class"
       "does not replace runtime dispatch"
       "does not imply patch-worker would bypass scoped-commit / write-scope enforcement once it exists"]
    :notes "Listed as unavailable rather than advisory-only because no policy rule today actually recommends it — it is reserved as an enum slot.")

  (backend
    :id verifier-worker
    :readiness_status advisory-only
    :runtime_allowed false
    :apply_blockers
      ["no runtime adapter shipped — there is no daemon-side worker that takes a commit + contract pair and reports back via the workstation/agent dispatch protocol"
       "today verifier scripts (verify-task-contract.mjs, task-scope-guard.mjs) run only via the human-driven acceptance path inside ClaudeCode; the registry would need a worker pillar entry before this could be runtime-ready"]
    :substrate nil
    :owner "claudecode"
    :non-goals
      ["does not authorize the verifier-worker to mutate the repo or the index — the role is read-only by definition"
       "does not replace runtime dispatch"
       "does not imply a verifier-worker would replace ClaudeCode for review tasks; the recommendation is observational"]
    :notes "Mirrors the wave24-01 r-post-commit-verifier rule; that rule is also advisory-only."))

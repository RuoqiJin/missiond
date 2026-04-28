;; MissionD router-dispatch-descriptor v1
;; Purpose: a Lisp-shaped, machine-checkable HANDOFF CONTRACT that pins a
;; single dispatch decision derived from (router recommendation + backend
;; readiness) for one specific task. Each descriptor names the recommended
;; backend, the registry-observed readiness, the strict apply-eligibility
;; outcome, and ALL three cross-wave no-execution invariants explicitly so
;; the daemon, planners, and downstream tooling never have to re-derive
;; them by joining three other Lisp files at runtime.
;;
;; Loadbearing rule: a router-dispatch-descriptor is NOT a runtime backend
;; switch. The three invariants below are LOCKED to single legal values so
;; this entire schema cannot be mis-used as a "promote to live dispatch"
;; lever, even by accident:
;;
;;   :dry_run_only           — literal atom true   (locked)
;;   :runtime_replacement    — literal atom false  (locked)
;;   :no_execution           — literal atom true   (locked)
;;
;; Any descriptor whose three invariants do not match the locked values is
;; a structural error — there is no escape hatch in v1. wave27-02 emits
;; descriptors via a CLI; wave27-03/04/05/06 surface them in plans /
;; reports / renderer / smoke-test contexts. None of those wires read this
;; schema as a "switch backend" signal.
;;
;; Backend ids and readiness statuses MUST stay aligned with their
;; upstream enums:
;;   backend ids        — router-policy v1 BACKEND_CLASSES + router-backend-registry v1 BACKEND_IDS
;;   readiness statuses — router-backend-registry v1 READINESS_STATUSES + the synthetic
;;                        `unknown` fallback wave26-02/03 emit when a backend lookup fails.
;; Drift between any of those enums is a structural error.

(router-dispatch-descriptor-schema missiond.router-dispatch-descriptor.v1
  :version "v1"
  :status "code-aligned — schema + read-only checker scripts/check-router-dispatch-descriptor.mjs (wave27-01); intentionally NOT wired to runtime dispatch. wave27-02 emits descriptors via a CLI; wave27-03/04/05/06 surface them on plans / reports / renderer / smoke. The three no-execution invariants (:dry_run_only / :runtime_replacement / :no_execution) are LOCKED to single legal values so the descriptor cannot be promoted to live dispatch by reinterpretation."
  :checker "scripts/check-router-dispatch-descriptor.mjs"
  :seed nil

  (purpose
    "Pin a single dispatch handoff decision per task as a structured Lisp record instead of free-form prose."
    "Carry the three no-execution invariants explicitly on every descriptor so the daemon never has to infer them by joining policy + registry + report contracts at runtime."
    "Surface the strict apply-eligibility outcome (and ALL blockers when ineligible) so downstream tooling can decide whether to even *show* a runtime-promote affordance — without ever owning the apply decision."
    "Echo the source paths consulted (recommendation schema id + policy file + backend registry file + optional trace index) so reviewers can audit how each descriptor was derived.")

  (file-shape
    :file "<emitted by wave27-02 CLI; piped or stored ad hoc>"
    :form (router-dispatch-descriptor <descriptor-id>
            :schema "missiond.router-dispatch-descriptor.v1"
            :task_id <string>
            :recommended_backend <backend-id-atom>
            :router_confidence <confidence-atom>
            :backend_readiness_status <readiness-atom>
            :backend_runtime_allowed <literal-bool>
            :router_apply_eligible <literal-bool>
            :router_apply_blockers [<string> ...]
            :dry_run_only true
            :runtime_replacement false
            :no_execution true
            :source_recommendation_schema <string>
            :source_policy_path <repo-relative-path>
            :source_backend_registry_path <repo-relative-path>
            ;; optional
            :source_trace_index_path <repo-relative-path>
            :notes <string>
            :generated_at <iso-8601-string>
            :generator <string>))

  (entry-heads
    [router-dispatch-descriptor])

  (backend-ids
    [claudecode missiond-llm-router deterministic-checker patch-worker verifier-worker])

  (confidence-levels
    [high medium low])

  (readiness-statuses
    [current-default advisory-only runtime-ready unavailable unknown])

  (status-meaning
    (current-default  "Backend is the live default runtime adapter today; carries :backend_runtime_allowed true, but is NEVER apply-eligible without an explicit runtime-ready opt-in upstream.")
    (runtime-ready    "Backend has a shipped runtime adapter and may be apply-eligible when ALL eligibility-gates pass.")
    (advisory-only    "Backend is referenced as a recommendation surface only — no runtime adapter is shipped; never apply-eligible.")
    (unavailable      "Backend cannot be invoked at all in the current deployment; never apply-eligible.")
    (unknown          "Synthetic fallback wave26-02/03 emit when the backend lookup against the registry fails; always carries :backend_runtime_allowed false and at least one blocker."))

  (descriptor-contract
    (:task_id "string; matches a real (task <id> ...) form id. Must be non-empty.")
    (:recommended_backend "atom; one of the 5 backend ids in (backend-ids ...) above. Mirrors the wave24-04/wave25-02 router recommendation output.")
    (:router_confidence "atom; one of high | medium | low. Mirrors the wave25-02 router_confidence enum.")
    (:backend_readiness_status "atom; one of the 5 readiness statuses in (readiness-statuses ...) above (registry-derived 4 + synthetic unknown). Mirrors the wave26-04 router_backend_readiness_status enum.")
    (:backend_runtime_allowed "literal atom true or false (NEVER a string). Mirrors the wave26-01 :runtime_allowed atom for the recommended backend, or `false` when readiness is unknown / advisory-only / unavailable.")
    (:router_apply_eligible "literal atom true or false (NEVER a string). MUST be true ONLY when ALL of the following hold: :backend_readiness_status = runtime-ready (current-default alone is insufficient — explicit runtime-ready opt-in required), :backend_runtime_allowed = true, :router_confidence = high, AND :router_apply_blockers is empty. Any other shape MUST set this to false.")
    (:router_apply_blockers
      "vector of non-empty strings stating concrete reasons live promotion is rejected for THIS descriptor. Each entry mirrors the wave26-01 :apply_blockers vector / the wave26-02/03 synthetic blockers."
      "MAY be empty () ONLY when :router_apply_eligible is true."
      "MUST be non-empty (at least one reason) when :router_apply_eligible is false.")
    (:dry_run_only        "LOCKED to the literal atom true. Cross-wave invariant — descriptor existence never authorises live dispatch. Strings (\"true\") and any other value are rejected.")
    (:runtime_replacement "LOCKED to the literal atom false. Cross-wave invariant — runtime backend replacement is out of scope through wave 27. Strings (\"false\") and any other value are rejected.")
    (:no_execution        "LOCKED to the literal atom true. Cross-wave invariant — emitting / consuming a descriptor MUST NOT execute the recommended backend. Strings (\"true\") and any other value are rejected.")
    (:source_recommendation_schema "string; identifier of the upstream router recommendation schema (e.g. \"missiond.router-recommendation.v0\"). Must be non-empty.")
    (:source_policy_path "string; repo-relative path to the router-policy file consulted (no leading '/' or '~', no '..' traversal).")
    (:source_backend_registry_path "string; repo-relative path to the router-backend-registry file consulted (no leading '/' or '~', no '..' traversal).")
    (:source_trace_index_path "OPTIONAL string; repo-relative path to the trace-index file consulted to score :router_confidence (no leading '/' or '~', no '..' traversal).")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation.")
    (:generated_at "OPTIONAL ISO-8601 timestamp string identifying when the descriptor was emitted (e.g. \"2026-04-28T11:23:05+08:00\").")
    (:generator "OPTIONAL string identifying which CLI / handler emitted the descriptor (e.g. \"scripts/recommend-task-backend.mjs\" or \"missiond-daemon::router::dispatch_descriptor\")."))

  (required-descriptor-fields
    [:schema
     :task_id
     :recommended_backend
     :router_confidence
     :backend_readiness_status
     :backend_runtime_allowed
     :router_apply_eligible
     :router_apply_blockers
     :dry_run_only
     :runtime_replacement
     :no_execution
     :source_recommendation_schema
     :source_policy_path
     :source_backend_registry_path])

  (optional-descriptor-fields
    [:source_trace_index_path
     :notes
     :generated_at
     :generator])

  (locked-invariants
    (:dry_run_only        true)
    (:runtime_replacement false)
    (:no_execution        true))

  (eligibility-gate
    "router_apply_eligible = true REQUIRES ALL of:"
    "  - backend_readiness_status = runtime-ready (current-default is REJECTED — explicit runtime-ready opt-in required)"
    "  - backend_runtime_allowed = true (literal atom)"
    "  - router_confidence = high"
    "  - router_apply_blockers = () (empty)"
    "Any other combination MUST set router_apply_eligible to false; the checker rejects descriptors that violate this gate."
    "router_apply_eligible = false is ALWAYS legal regardless of the other fields, provided router_apply_blockers is non-empty."
    "Note: this gate intentionally REJECTS current-default → eligible. The current default backend is the live runtime today, but promoting any other backend (or asserting runtime apply-eligibility) requires explicit runtime-ready opt-in upstream.")

  (validation-contract
    :file-must-have-header [:schema]
    :enum-checked
      [:recommended_backend :router_confidence :backend_readiness_status]
    :literal-bool-fields
      [:backend_runtime_allowed :router_apply_eligible :dry_run_only :runtime_replacement :no_execution]
    :locked-literal-fields
      [(:dry_run_only true)
       (:runtime_replacement false)
       (:no_execution true)]
    :path-fields
      [:source_policy_path :source_backend_registry_path :source_trace_index_path]
    :rejects
      ["schema mismatch (:schema not missiond.router-dispatch-descriptor.v1)"
       "missing any required field on the descriptor"
       "unknown descriptor field"
       ":recommended_backend not in the 5-element backend-ids enum"
       ":router_confidence not in the 3-element confidence-levels enum"
       ":backend_readiness_status not in the 5-element readiness-statuses enum"
       ":backend_runtime_allowed not the literal atom true or false (strings are rejected)"
       ":router_apply_eligible not the literal atom true or false (strings are rejected)"
       ":dry_run_only not the literal atom true (strings or false are rejected — locked invariant)"
       ":runtime_replacement not the literal atom false (strings or true are rejected — locked invariant)"
       ":no_execution not the literal atom true (strings or false are rejected — locked invariant)"
       ":router_apply_eligible true while :backend_readiness_status is NOT runtime-ready (current-default is REJECTED — explicit opt-in required)"
       ":router_apply_eligible true while :backend_runtime_allowed is NOT literal true"
       ":router_apply_eligible true while :router_confidence is NOT high"
       ":router_apply_eligible true while :router_apply_blockers is non-empty"
       ":router_apply_eligible false while :router_apply_blockers is empty (must explain at least one rejection reason)"
       ":router_apply_blockers not a vector of non-empty strings"
       ":task_id missing or empty"
       ":source_recommendation_schema missing or empty"
       "absolute or ~ or .. path inside :source_policy_path / :source_backend_registry_path / :source_trace_index_path"
       "unknown entry head at top level (only `router-dispatch-descriptor` allowed)"]
    :no-prose
      "descriptor entries are S-expressions; narrative belongs in :notes strings only.")

  (checker-contract
    :input "stdin (--stdin) OR ad hoc descriptor.lisp files"
    :modes [single-file stdin dry-fixture]
    :flags [--json --stdin --dry-fixture]
    :json-shape "{ ok, files, errors[], descriptors_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any input that is not a single (router-dispatch-descriptor ...) form (or a sequence of them)"
       "any locked-invariant field whose value is not the locked literal"]
    :non-goal
      "checker does NOT call git, does NOT shell out, does NOT touch the network or any LLM, and does NOT verify that the recommended backend is actually invocable. Producing the descriptor is wave27-02; surfacing it on plans / reports / renderer / smoke is wave27-03 / 04 / 05 / 06; this checker validates structure only.")

  (cross-wave-invariant
    "Router dispatch descriptors are advisory output only. No wave reads these to alter live dispatch."
    "Every descriptor MUST declare :dry_run_only true AND :runtime_replacement false AND :no_execution true. Any other shape is a structural error."
    ":router_apply_eligible true REQUIRES the strict eligibility-gate above (runtime-ready + runtime_allowed=true + confidence=high + zero blockers). current-default alone is NEVER sufficient — explicit runtime-ready opt-in is required so the descriptor cannot be silently promoted by a backend whose adapter is the live default but whose readiness has not been opt-in confirmed."
    "When :router_apply_eligible is false the descriptor MUST list at least one concrete blocker so reviewers can see WHY apply was rejected.")

  (non-goals
    "The schema does not score backends or pick a winner — recommendation scoring is the wave24-03 / wave25-02 recommendation CLI's job."
    "The schema does not invoke any backend or apply any change — it is purely a handoff record."
    "The schema does not encode cost / latency / capacity hints — those belong in a future v2 if needed."
    "The schema does not verify the source files referenced by :source_policy_path / :source_backend_registry_path actually exist — the checker only validates path *shape*. wave27-02 / 06 are responsible for joining descriptors to live source files."))

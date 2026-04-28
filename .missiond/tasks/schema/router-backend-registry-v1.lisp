;; MissionD router-backend-registry v1
;; Purpose: a Lisp-shaped, machine-checkable registry describing each backend
;; the router enum may name and DECLARING WHETHER THAT BACKEND IS RUNTIME-READY,
;; so router recommendations (wave24+) can explain whether the recommended
;; backend is actually wired into the daemon, advisory-only, currently the
;; default, or simply unavailable. Registry files are a data contract — the
;; checker is the line of defence between "router suggested it" and "the
;; daemon could actually run it."
;;
;; Loadbearing rule: this registry does NOT replace runtime dispatch and is NOT
;; consumed at runtime in this wave. wave26-01 ships schema + seed + checker
;; only. The registry exists so future apply-gate work (later waves) can refuse
;; to promote a recommendation whose backend is not :runtime_allowed true.
;;
;; Backend ids MUST stay aligned with router-policy v1's backend enum:
;;   claudecode | missiond-llm-router | deterministic-checker
;;   patch-worker | verifier-worker
;; — drift between the two enums is a structural error.

(router-backend-registry-schema missiond.router-backend-registry.v1
  :version "v1"
  :status "code-aligned — schema + seed registry + read-only checker scripts/check-router-backend-registry.mjs (wave26-01); intentionally NOT wired to runtime dispatch"
  :checker "scripts/check-router-backend-registry.mjs"
  :seed ".missiond/router/router-backend-registry-v1.lisp"

  (purpose
    "Make backend readiness a machine-checkable Lisp data contract instead of free-form prose embedded in router policy notes."
    "Pin the cross-wave invariant that only :runtime_allowed true backends may ever be promoted to live dispatch — and only when their :readiness_status is current-default or runtime-ready."
    "Surface :apply_blockers so reviewers can see WHY a backend is not runtime-ready without parsing prose; populated even for advisory-only and unavailable entries."
    "Enumerate the substrate (Rust module path or planned location) per backend so wave26-02 / wave26-03 can join recommendations to the actual runtime adapter — or report nil when no adapter exists yet.")

  (file-shape
    :file ".missiond/router/<registry>.lisp"
    :form (router-backend-registry <registry-id>
            :schema "missiond.router-backend-registry.v1"
            :version <string>
            :description <string>
            <backend> ...))

  (header-fields
    [:schema :version :description])

  (entry-heads
    [backend])

  (backend-ids
    [claudecode missiond-llm-router deterministic-checker patch-worker verifier-worker])

  (readiness-statuses
    [current-default advisory-only runtime-ready unavailable])

  (status-meaning
    (current-default  "Backend is the live default runtime adapter today; :runtime_allowed MUST be true and :substrate MUST be a non-nil Rust module path.")
    (runtime-ready    "Backend has a shipped runtime adapter and may be promoted to live dispatch; :runtime_allowed MUST be true and :substrate MUST be a non-nil Rust module path.")
    (advisory-only    "Backend is referenced in router policy as a recommendation surface only — no runtime adapter is shipped; :runtime_allowed MUST be false and :apply_blockers MUST be non-empty.")
    (unavailable      "Backend cannot be invoked at all in the current deployment (e.g. the substrate has not been built, the adapter is out of scope this wave); :runtime_allowed MUST be false and :apply_blockers MUST be non-empty."))

  (runtime-allowed-statuses
    [current-default runtime-ready])

  (backend-entry-contract
    (:id "one of the 5 enumerated backend ids; unique within a registry file")
    (:readiness_status "one of current-default | advisory-only | runtime-ready | unavailable")
    (:runtime_allowed "literal boolean atom true or false; MUST be true iff :readiness_status is current-default or runtime-ready")
    (:apply_blockers
      "vector of non-empty strings stating concrete reasons the router-recommended backend cannot be promoted to live dispatch."
      "MAY be empty when :readiness_status is current-default or runtime-ready."
      "MUST be non-empty (at least one reason) when :readiness_status is advisory-only or unavailable.")
    (:substrate
      "string identifying the runtime adapter / surface that would be invoked (e.g. a Rust module path), OR the literal atom nil when no adapter exists."
      "MUST be a non-nil string when :readiness_status is current-default or runtime-ready."
      "MAY be nil when :readiness_status is advisory-only or unavailable.")
    (:non-goals
      "non-empty vector of strings stating what this backend entry explicitly does NOT claim — every entry MUST list at least one non-goal so reviewers can audit drift.")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation")
    (:owner "OPTIONAL string identifying the team / pillar that owns the adapter")
    (:adapter_path "OPTIONAL repo-relative path to the planned or shipped adapter source"))

  (required-backend-fields
    [:id :readiness_status :runtime_allowed :apply_blockers :substrate :non-goals])

  (optional-backend-fields
    [:notes :owner :adapter_path])

  (validation-contract
    :file-must-have-header [:schema :version]
    :unique-per-file [:id]
    :enum-checked [:id :readiness_status]
    :id-style "lowercase kebab; must match ^[a-z][a-z0-9-]*$ AND be in (backend-ids ...) above"
    :rejects
      ["schema mismatch (:schema not missiond.router-backend-registry.v1)"
       "duplicate backend :id within a file"
       ":id not in the 5-element backend-ids enum"
       ":readiness_status not in the 4-element readiness-statuses enum"
       ":runtime_allowed not the literal atom true or false"
       ":runtime_allowed true while :readiness_status is NOT current-default and NOT runtime-ready"
       ":readiness_status runtime-ready or current-default WITHOUT a non-nil :substrate"
       ":readiness_status advisory-only or unavailable WITH an empty :apply_blockers vector"
       ":non-goals empty or missing"
       "unknown entry head at top level (only `backend` allowed)"
       "missing any required field on a backend entry"
       "unknown backend field"]
    :no-prose "registry entries are S-expressions; narrative belongs in :notes / :apply_blockers strings only")

  (checker-contract
    :input ".missiond/router/*.lisp + ad hoc files"
    :modes [single-file dry-fixture]
    :flags [--json --dry-fixture]
    :json-shape "{ ok, files, errors[], backends_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any registry whose backend list does not consist solely of `backend` entries"]
    :non-goal
      "checker does NOT consume the registry at runtime, does NOT promote any recommendation, and does NOT verify that a declared :substrate Rust module actually exists. wave26-02 / wave26-03 are responsible for joining the registry to recommendations and to runtime; this checker validates structure only.")

  (cross-wave-invariant
    "Backend readiness is the single source of truth for whether a router recommendation may ever be promoted to live dispatch. Until later waves wire an apply gate, this is enforced by review against :runtime_allowed."
    ":runtime_allowed true REQUIRES :readiness_status in (current-default runtime-ready). Any other combination is a structural error."
    "A backend with :readiness_status runtime-ready REQUIRES a non-nil :substrate so the apply gate can name a concrete Rust module to invoke.")

  (non-goals
    "The schema does not score backends or pick a winner — backend selection is the recommendation CLI's job (wave24-03)."
    "The schema does not encode cost / latency / capacity hints — those belong in a future v2 if needed."
    "The schema does not require the runtime adapter to exist; it only encodes whether the registry CLAIMS the adapter is ready."))

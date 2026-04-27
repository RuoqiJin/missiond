;; MissionD router-policy v1
;; Purpose: a Lisp-shaped, machine-checkable policy file describing which
;; backend should be RECOMMENDED for a given task shape. Policy files are
;; data contracts so future router work can reason about backend selection
;; without parsing prose. THIS SCHEMA DOES NOT REPLACE RUNTIME DISPATCH.
;;
;; Loadbearing rule: every policy file is :dry-run-only true and
;; :runtime-replacement false. Recommendations are advisory output only;
;; the live MissionD plan/dispatch path is unchanged by anything in this
;; file. Any policy that claims runtime replacement is a checker error.
;;
;; Scope: schema + seed policy + read-only checker. There is intentionally
;; NO Rust-side consumer in this wave — wave24-03 builds the read-only CLI,
;; wave24-04 surfaces dry-run hints in plans, wave24-06 wires the smoke
;; harness. Each later wave reads policy files via the same checker.

(router-policy-schema missiond.router-policy.v1
  :version "v1"
  :status "code-aligned — schema + seed policy + read-only checker scripts/check-router-policy.mjs (wave24-01); intentionally NOT wired to runtime dispatch"
  :checker "scripts/check-router-policy.mjs"
  :seed ".missiond/router/router-policy-v1.lisp"

  (purpose
    "Make router backend recommendations a machine-checkable Lisp data contract instead of free-form prose."
    "Enumerate the backend classes a recommendation may name; reject unknown classes structurally."
    "Pin the cross-wave invariant that router output is advisory only — :dry-run-only true and :runtime-replacement false are required, and policies claiming runtime replacement fail validation.")

  (file-shape
    :file ".missiond/router/<policy>.lisp"
    :form (router-policy <policy-id>
            :schema "missiond.router-policy.v1"
            :version <string>
            :dry-run-only true
            :runtime-replacement false
            :description <string>
            <rule> ...))

  (header-fields
    [:schema :version :dry-run-only :runtime-replacement])

  (entry-heads
    [rule])

  (backend-classes
    [claudecode missiond-llm-router deterministic-checker patch-worker verifier-worker])

  (backend-meaning
    (claudecode             "ClaudeCode interactive worker; the current default runtime backend.")
    (missiond-llm-router    "MissionD LLM router invoking a slot-resolved model (Sonnet, Opus, etc.).")
    (deterministic-checker  "Pure-script checker (e.g. scripts/check-*.mjs) — no LLM, no network.")
    (patch-worker           "Mechanic-style automated patch worker that applies a deterministic edit and re-runs cargo.")
    (verifier-worker        "Read-only verifier worker that confirms a commit matches a contract; never writes."))

  (rule-contract
    (:id "stable kebab-id, unique within a policy file (e.g. r-docs-to-claudecode)")
    (:priority "non-negative integer; unique within a policy file; lower number wins ties")
    (:when "non-empty list of predicate clauses, each (kind <pattern>) | (dispatch_strategy <pattern>) | (owner <pattern>) | (status <pattern>) | (path-glob <glob>) | (any <clauses...>) | (all <clauses...>)")
    (:recommend "(:backend <backend-class> :reasoning <string>) — backend MUST be from the enum above; reasoning is single-line prose justifying the choice")
    (:non-goals "non-empty vector of strings stating what this rule explicitly does NOT claim — every rule MUST list at least one non-goal so reviewers can audit drift")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation"))

  (required-rule-fields
    [:id :priority :when :recommend :non-goals])

  (optional-rule-fields
    [:notes])

  (predicate-heads
    [kind dispatch_strategy dispatch-strategy owner status path-glob any all])

  (predicate-meaning
    (kind                "Match task :kind atom (or vector of atoms via (any ...)).")
    (dispatch_strategy   "Match task :dispatch-strategy atom; underscore alias for legacy callers.")
    (dispatch-strategy   "Match task :dispatch-strategy atom; canonical kebab-case form.")
    (owner               "Match task :owner string.")
    (status              "Match task :status atom.")
    (path-glob           "Match any path in task :write-scope against a glob pattern.")
    (any                 "Logical OR across nested clauses; non-empty.")
    (all                 "Logical AND across nested clauses; non-empty."))

  (validation-contract
    :file-must-have-header [:schema :version :dry-run-only :runtime-replacement]
    :unique-per-file [:id :priority]
    :enum-checked [:recommend.backend]
    :id-style "lowercase kebab/dot/underscore; must match ^[a-z0-9][a-z0-9._-]*$"
    :priority-style ":priority is a non-negative integer"
    :rejects
      ["schema mismatch (:schema not missiond.router-policy.v1)"
       ":dry-run-only missing or not literal true"
       ":runtime-replacement missing or not literal false"
       "duplicate :id within a policy file"
       "duplicate :priority within a policy file"
       "rule missing any of the required fields"
       "rule :recommend missing :backend or :reasoning"
       "rule :recommend :backend not in the backend-class enum"
       "rule :non-goals empty or missing"
       "rule :when empty or contains a clause whose head is not in the predicate enum"
       "unknown entry head at top level (only `rule` allowed)"]
    :no-prose "policy entries are S-expressions; narrative belongs in :notes / :reasoning strings only")

  (checker-contract
    :input ".missiond/router/*.lisp + ad hoc files"
    :modes [single-file dry-fixture]
    :flags [--json --dry-fixture]
    :json-shape "{ ok, files, errors[], rules_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any policy claiming :runtime-replacement true (this is the cross-wave invariant — router output is advisory only)"
       "any policy missing :dry-run-only true at top level"
       "any rule whose :non-goals is missing or empty"
       "predicate clauses with no head atom (malformed predicate)"]
    :non-goal
      "checker does NOT consume policy files at runtime, does NOT replace dispatch logic, and does NOT score / select rules. Selection / scoring is a separate concern handled by a future read-only recommendation CLI (wave24-03) that ALSO honors :dry-run-only true.")

  (cross-wave-invariant
    "Router policy is advisory output only. No wave reads these files at runtime to alter dispatch."
    "Every policy file MUST declare :dry-run-only true AND :runtime-replacement false at the top level."
    "Every rule MUST list :non-goals so reviewers can detect scope drift toward live dispatch.")

  (non-goals
    "The schema does not define a scoring function — picking a single recommendation from multiple matching rules is the recommendation CLI's job, not the schema's."
    "The schema does not enforce that recommended backends are actually available in any deployment; it only enforces that the named backend is in the enumerated class set."
    "The schema does not encode cost, latency, or capacity hints — those belong in a future v2 if needed."))

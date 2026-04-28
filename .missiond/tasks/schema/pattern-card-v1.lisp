;; MissionD pattern-card v1
;; Purpose: a Lisp-shaped, machine-checkable record for reusable
;; implementation recipes ("pattern cards"). Pattern cards are
;; READ-ONLY guidance — they capture how a recurring kind of work
;; (schema checker, read-only Node CLI, report lineage, cross-layer
;; smoke, large-file navigation) is best implemented in this repo,
;; and they point at concrete exemplar files via :known-good. They
;; never grant scope beyond a task contract: a worker is still bound
;; by the task contract's :write-scope and :must-not-touch.
;;
;; The schema accepts two equivalent shapes:
;;   1. A container of multiple cards (mirrors the wave29 dispatch-time
;;      pattern-cards.lisp shape):
;;        (pattern-cards <set-id>
;;          :schema "missiond.pattern-card.v1"   ; or legacy
;;                                              ; "missiond.pattern-cards.dispatch.v0"
;;          :version "v1"
;;          :wave <wave-id>?                     ; optional
;;          (card <card-id> :use-for [...] :recipe [...] :known-good [...])
;;          ...)
;;
;;   2. A single card per file (the .missiond/patterns/*.pattern.lisp
;;      seed format):
;;        (pattern-card <card-id>
;;          :schema "missiond.pattern-card.v1"
;;          :version "v1"
;;          :purpose "..."
;;          :use-for [<task-id-pattern> ...]
;;          :recipe ["..." ...]
;;          :known-good ["repo/relative/path" ...])
;;
;; Loadbearing rule: pattern cards are advisory implementation recipes
;; only. They MUST stay repo-relative and MUST NOT be cited as scope
;; expansion. The wave29-02 checker validates structure / shape /
;; path safety / non-empty recipe / unique :id. It never grants
;; permission to edit any file outside the task contract.

(pattern-card-schema missiond.pattern-card.v1
  :version "v1"
  :status "code-aligned — schema + read-only checker scripts/check-pattern-card.mjs (wave29-02); seed cards live under .missiond/patterns/. Schema accepts the existing wave29 dispatch-time pattern-cards.lisp shape unmodified by allowing the legacy :schema value missiond.pattern-cards.dispatch.v0."
  :checker "scripts/check-pattern-card.mjs"
  :seed nil

  (purpose
    "Capture how a recurring kind of work is best implemented in this repo as a structured Lisp record instead of free-form prose embedded in task briefs."
    "Make :known-good exemplar paths machine-checkable so a worker can jump straight to a working reference instead of grepping the whole tree."
    "Pin the cross-wave invariant that pattern cards are READ-ONLY guidance and MUST NOT be cited as scope expansion. The task contract :write-scope / :must-not-touch remain authoritative."
    "Allow both the wave29 dispatch-time multi-card container and the per-file single-card seed format so the dispatch file and the .missiond/patterns/*.pattern.lisp files share one validator.")

  (file-shape
    :file ".missiond/patterns/<card-id>.pattern.lisp (single-card seed) OR .missiond/tasks/<wave>/pattern-cards.lisp (multi-card dispatch container)"
    :form-multi (pattern-cards <set-id>
                  :schema "missiond.pattern-card.v1"
                  :version "v1"
                  :wave <wave-id>
                  (card <card-id>
                        :use-for [<task-id-pattern> ...]
                        :recipe [<step-string> ...]
                        :known-good [<repo-relative-path> ...]
                        :purpose <string>
                        :summary <string>
                        :anti-pattern [<string> ...]
                        :non-goals [<string> ...]
                        :notes <string>)
                  ...)
    :form-single (pattern-card <card-id>
                   :schema "missiond.pattern-card.v1"
                   :version "v1"
                   :purpose <string>
                   :summary <string>
                   :use-for [<task-id-pattern> ...]
                   :recipe [<step-string> ...]
                   :known-good [<repo-relative-path> ...]
                   :anti-pattern [<string> ...]
                   :non-goals [<string> ...]
                   :notes <string>))

  (container-heads
    [pattern-card pattern-cards])

  (entry-heads-inside-pattern-cards
    [card])

  (legacy-schema-aliases
    "missiond.pattern-cards.dispatch.v0")

  (canonical-schema
    "missiond.pattern-card.v1")

  (header-contract
    (:schema "literal string; MUST be \"missiond.pattern-card.v1\" for new pattern cards. The legacy value \"missiond.pattern-cards.dispatch.v0\" is accepted on the multi-card container so the wave29 dispatch-time pattern-cards.lisp validates unmodified.")
    (:version "OPTIONAL string; conventionally \"v1\". Provided so future v2 emissions can carry an explicit marker without bumping :schema.")
    (:wave "OPTIONAL kebab id matching ^[a-z][a-z0-9-]*$; only meaningful on the multi-card container.")
    (:purpose "OPTIONAL non-empty string when present (single-card form prefers this; multi-card form documents purpose in :notes or in the surrounding file comments).")
    (:summary "OPTIONAL non-empty string when present.")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation."))

  (card-contract
    (:id "non-empty kebab id matching ^[a-z][a-z0-9-]*$. Required as the second form of (card <id> ...) inside the multi-card container, OR as the second form of (pattern-card <id> ...) for the single-card form. The :id keyword is also accepted; if both the second-form id and a :id keyword are present they MUST agree.")
    (:use-for "REQUIRED vector of task-id patterns this card applies to. Each entry MUST match ^[a-z0-9][a-z0-9._-]*$ (the same shape as task-runner-manifest task ids) so a downstream tool can join cards to task contracts. The :use_for underscore alias is also accepted.")
    (:recipe "REQUIRED vector of non-empty step strings describing the implementation pattern. MUST contain at least one step. Empty :recipe is a structural error — a card without steps offers no guidance.")
    (:known-good "RECOMMENDED vector of repo-relative path strings (no leading '/' or '~', no '..' traversal). SHOULD point at exemplar implementations of this pattern. Missing or empty :known-good is a warning, not an error — some patterns (e.g. cross-layer-smoke, large-file-navigation in the wave29 dispatch file) are seeded before a single canonical exemplar exists. The :known_good underscore alias is also accepted. wave29-02 enforces :known-good presence at the seed-card layer (.missiond/patterns/*.pattern.lisp), not at the schema layer.")
    (:purpose "OPTIONAL non-empty string when present (single-card form prefers this).")
    (:summary "OPTIONAL non-empty string when present.")
    (:anti-pattern "OPTIONAL vector of non-empty strings describing what NOT to do. The :anti_pattern underscore alias is also accepted.")
    (:non-goals "OPTIONAL vector of non-empty strings explicitly excluded from the recipe.")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation."))

  (alias-table
    (:use-for       :use_for)
    (:known-good    :known_good)
    (:anti-pattern  :anti_pattern)
    (:non-goals     :non_goals))

  (required-card-fields
    [:id :use-for :recipe])

  (recommended-card-fields
    [:known-good])

  (optional-card-fields
    [:purpose :summary :anti-pattern :non-goals :notes])

  (validation-contract
    :file-must-have-header [:schema]
    :unique-per-file [:id]
    :enum-checked
      [:schema]
    :path-fields
      [:known-good]
    :id-pattern-fields
      [:use-for]
    :rejects
      ["schema mismatch (:schema not missiond.pattern-card.v1 or the legacy missiond.pattern-cards.dispatch.v0)"
       "missing any required card field (:id :use-for :recipe)"
       "unknown card field (after alias normalization)"
       ":id missing or malformed (must match ^[a-z][a-z0-9-]*$)"
       "duplicate card :id within a single file"
       ":use-for entry not matching ^[a-z0-9][a-z0-9._-]*$"
       "empty :recipe vector (a card with no steps offers no guidance)"
       ":recipe entry that is empty / non-string"
       ":known-good entry that is empty / non-string / absolute / contains '..' traversal / starts with '~'"
       ":anti-pattern / :non-goals entry that is empty / non-string"
       "second-form id and :id keyword present but disagree"
       "unknown entry head at top level (only `pattern-cards` or `pattern-card` allowed)"
       "unknown entry head inside (pattern-cards ...) (only `card` allowed)"]
    :warns
      ["missing :known-good keyword entirely (acceptable for cards seeded before any exemplar exists; flagged so we notice when an exemplar exists and should be linked)"
       "empty :known-good vector (acceptable for newly-seeded cards but flagged so we notice when an exemplar exists and should be linked)"
       ":known-good path not present on disk (warning only — the schema does not own the exemplar files)"]
    :no-prose
      "card entries are S-expressions; narrative belongs in :notes or surrounding file comments only.")

  (checker-contract
    :input "stdin (--stdin) OR ad hoc <file>.lisp paths"
    :modes [single-file stdin dry-fixture]
    :flags [--json --stdin --dry-fixture]
    :json-shape "{ ok, files, errors[], warnings[], cards_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any input that is not (pattern-cards ...) or (pattern-card ...)"]
    :non-goal
      "checker does NOT call git, does NOT shell out, does NOT touch the network or any LLM, and does NOT verify that referenced :use-for task ids exist on disk. wave29-02 validates structure only; downstream tooling can join cards to task contracts.")

  (cross-wave-invariant
    "Pattern cards are advisory READ-ONLY implementation recipes. They never grant scope beyond the task contract."
    "The schema accepts BOTH the wave29 dispatch-time multi-card container shape (pattern-cards <set-id> ...) AND the per-file single-card seed shape (pattern-card <card-id> ...). Both shapes share the same card validation rules so one validator covers both."
    "The legacy :schema value \"missiond.pattern-cards.dispatch.v0\" remains accepted on the multi-card container so the existing wave29 pattern-cards.lisp file passes the v1 checker WITHOUT modification."
    "Empty :recipe is a structural error: a card with no steps offers no guidance. Empty :known-good is a warning so newly-seeded cards can land before their exemplar exists."
    ":use-for entries MUST match the same task-id shape as task-runner-manifest task ids. This makes a future card-to-task join mechanically straightforward.")

  (non-goals
    "The schema does not score recipes or pick a winner — it only validates shape."
    "The schema does not require :known-good paths to exist on disk; missing files are a warning, not an error."
    "The schema does not encode model / backend hints — pattern cards are language-agnostic implementation recipes."
    "The schema does not replace task contracts. A worker MUST still respect the task contract's :write-scope and :must-not-touch even when a card's :known-good entries point at files outside that scope."
    "The schema does not encode the underscore vs kebab style decision; aliases are normalized so both shapes round-trip cleanly through the checker."))

;; MissionD verification-receipt v1
;; Purpose: a Lisp-shaped, machine-checkable cache of command-evidence
;; receipts so the orchestrator can REUSE already-run smoke / full
;; verification evidence across a wave instead of blindly repeating
;; expensive checks. Receipts are produced by a worker AFTER a
;; verification command exits, and they record exactly which command
;; ran, against which commit, with which exit code, in which tier.
;;
;; Loadbearing rule: a verification receipt is ADVISORY ACCELERATION
;; ONLY. It is NOT a substitute for source facts and is NOT a
;; substitute for commit verification. The orchestrator MUST still
;; verify the task contract, the report contract, the shared-memory
;; completion entry, and the git commit even when a matching receipt
;; is present. Receipts only let the orchestrator SKIP the actual
;; re-execution of a deterministic verification command when the
;; conservative reuse rules below all hold.
;;
;; Conservative reuse rules (mirrored verbatim in the
;; `isReceiptReusable` helper exported from the wave29-05 checker):
;;   1. receipt's :commit_hash MUST match the queried commit
;;      (exact OR via the standard hex-prefix-agree rule used by
;;      the wave29-04 lineage helpers — the longer hash starts with
;;      the shorter hash AND the shorter is at least 7 hex chars).
;;   2. receipt's :command MUST equal the queried command exactly
;;      (string equality after .trim()). No fuzzy / argv-reorder /
;;      env-aware matching — receipts cache a SPECIFIC command line.
;;   3. receipt's :exit_code MUST equal 0. Any non-zero exit means
;;      the prior evidence is invalid for reuse — re-run the command.
;;   4. receipt's :tier MUST match the queried tier OR be wider:
;;        full   covers full | smoke | local
;;        smoke  covers smoke | local
;;        local  covers local
;;      i.e. a `full` receipt can satisfy a `smoke` reuse query, but
;;      a `local` receipt CANNOT satisfy a `smoke` query. This pins
;;      the rule that smoke / full evidence is strictly stronger
;;      than local-only evidence and never the other way around.
;; Any of (1) (2) (3) (4) failing → reuse=false. Receipts are
;; throwaway caches; when in doubt, re-run.

(verification-receipt-schema missiond.verification-receipt.v1
  :version "v1"
  :status "code-aligned — schema + read-only checker scripts/check-verification-receipt.mjs (wave29-05); verify-task-runner-batch.mjs gained an OPTIONAL --receipts <file.lisp> flag and a new receipt_coverage field in its JSON output. Receipts NEVER substitute for source facts; the batch verifier MUST still verify task contract, report, memory completion, and git commit even when receipts are loaded."
  :checker "scripts/check-verification-receipt.mjs"
  :seed nil

  (purpose
    "Cache deterministic verification command evidence (smoke / full dry-fixture or other re-runnable commands) so a wave can reuse last run's exit code instead of re-paying the cost when the underlying commit + command + tier all still apply."
    "Pin the cross-wave invariant that receipts are advisory acceleration ONLY — they are NEVER a substitute for source facts or for commit verification."
    "Provide a single canonical reuse helper (isReceiptReusable) so wave29-06 / wave29-07 / future planner tasks all encode the conservative rules in exactly the same place. Receipts that fail any of the four conservative rules MUST be treated as if no receipt existed."
    "Allow batching a wave's receipts into one (verification-receipt-set ...) container so a wave's per-task receipts ride together; per-file single-receipt records also remain valid for ad-hoc capture.")

  (file-shape
    :file ".missiond/tasks/<wave>/verification-receipts.lisp (multi-receipt container, recommended) OR .missiond/tasks/<wave>/receipts/<task-id>.receipt.lisp (per-task single-receipt form)"
    :form-multi (verification-receipt-set <set-id>
                  :schema "missiond.verification-receipt.v1"
                  :version "v1"
                  :wave <wave-id>
                  :generated_at <iso-8601-string>
                  (receipt <receipt-id>
                          :wave <wave-id-string>
                          :task_id <task-id-string>
                          :commit_hash <hex-string>
                          :command <command-string>
                          :exit_code <integer>
                          :tier <local|smoke|full>
                          :started_at <iso-8601-string>
                          :finished_at <iso-8601-string>
                          :duration_ms <non-negative-integer>
                          :files [<repo-relative-path-string> ...]
                          :notes <string>)
                  ...)
    :form-single (verification-receipt <receipt-id>
                   :schema "missiond.verification-receipt.v1"
                   :version "v1"
                   :wave <wave-id-string>
                   :task_id <task-id-string>
                   :commit_hash <hex-string>
                   :command <command-string>
                   :exit_code <integer>
                   :tier <local|smoke|full>
                   :started_at <iso-8601-string>
                   :finished_at <iso-8601-string>
                   :duration_ms <non-negative-integer>
                   :files [<repo-relative-path-string> ...]
                   :notes <string>))

  (container-heads
    [verification-receipt-set verification-receipt])

  (entry-heads-inside-receipt-set
    [receipt])

  (canonical-schema
    "missiond.verification-receipt.v1")

  (header-contract
    (:schema "REQUIRED literal string \"missiond.verification-receipt.v1\" on either container.")
    (:version "OPTIONAL string; conventionally \"v1\". Provided so future v2 emissions can carry an explicit marker without bumping :schema.")
    (:wave "OPTIONAL kebab id matching ^[a-z][a-z0-9-]*$; only meaningful on the multi-receipt container header.")
    (:generated_at "OPTIONAL ISO-8601 timestamp recording when the container was emitted.")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation."))

  (receipt-contract
    (:id "non-empty kebab id matching ^[a-z][a-z0-9._-]*$. Required as the second form of (receipt <id> ...) inside the multi-receipt container, OR as the second form of (verification-receipt <id> ...) for the single-receipt form. The :receipt_id keyword is also accepted; if both the second-form id and a :receipt_id keyword are present they MUST agree. When both are absent the checker derives a stable id from {wave}-{task_id}-{commit_hash[:7]}-{tier}.")
    (:wave "REQUIRED non-empty wave id string matching ^[a-z][a-z0-9-]*$. MUST be the prefix of :task_id (defence against accidentally-mixed receipt files).")
    (:task_id "REQUIRED non-empty task id string matching ^[a-z0-9][a-z0-9._-]*$ (mirrors task-runner-manifest task ids). MUST start with the receipt's :wave value followed by '-'.")
    (:commit_hash "REQUIRED non-empty hex string matching ^[0-9a-f]{7,64}$ (case-insensitive; mirrors the wave29-04 lineage hex pattern).")
    (:command "REQUIRED non-empty string. The exact command line that produced the receipt. Whitespace is preserved as-recorded; reuse comparisons use a trimmed equality test.")
    (:exit_code "REQUIRED integer (Number.isInteger). May be negative or zero or positive — only zero counts as reusable evidence (rule 3 in the schema header).")
    (:tier "REQUIRED enum {local | smoke | full}. Mirrors wave28-01 VERIFICATION_TIERS exactly.")
    (:started_at "REQUIRED-OR-DERIVED ISO-8601 timestamp. EITHER :started_at + :finished_at MUST BOTH be present OR :duration_ms MUST be present (or both forms — they are not mutually exclusive). The checker rejects a receipt that has neither timing form.")
    (:finished_at "See :started_at — required as a pair when timing is encoded as start/finish.")
    (:duration_ms "Non-negative integer (Number.isInteger AND >= 0). Either :duration_ms OR (:started_at + :finished_at) is required; both forms are allowed. When BOTH are present the checker does NOT cross-check them numerically — they are independently advisory.")
    (:files "OPTIONAL vector of repo-relative path strings (no leading '/' or '~', no '..' traversal). When present, these are the files the verified command read or wrote (advisory only — the checker does not enforce existence).")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation."))

  (required-receipt-fields
    [:wave :task_id :commit_hash :command :exit_code :tier])

  (timing-required-one-of
    [:started_at+:finished_at :duration_ms])

  (optional-receipt-fields
    [:receipt_id :files :notes])

  (validation-contract
    :file-must-have-header [:schema]
    :unique-per-file [:receipt-id]
    :enum-checked
      [:schema :tier]
    :path-fields
      [:files]
    :id-pattern-fields
      [:wave :task_id :receipt-id]
    :rejects
      ["schema mismatch (:schema not missiond.verification-receipt.v1)"
       "missing any required receipt field (:wave :task_id :commit_hash :command :exit_code :tier)"
       "missing both timing forms (need :duration_ms OR (:started_at + :finished_at))"
       ":started_at present without :finished_at (or vice versa)"
       "unknown receipt field (after structural normalization)"
       ":wave malformed (must match ^[a-z][a-z0-9-]*$)"
       ":task_id malformed (must match ^[a-z0-9][a-z0-9._-]*$)"
       ":task_id does not start with :wave + '-' (stale wave/task mismatch — defence against accidentally-mixed receipt files)"
       ":commit_hash malformed (must match ^[0-9a-f]{7,64}$ case-insensitive)"
       ":command empty / non-string"
       ":exit_code non-integer"
       ":tier not in enum {local|smoke|full}"
       ":duration_ms negative / non-integer"
       ":started_at / :finished_at not a valid ISO-8601 timestamp"
       ":files entry that is empty / non-string / absolute / contains '..' traversal / starts with '~'"
       "duplicate :receipt_id within a single file (or across the input set)"
       "unknown entry head at top level (only `verification-receipt-set` or `verification-receipt` allowed)"
       "unknown entry head inside (verification-receipt-set ...) (only `receipt` allowed)"]
    :no-prose
      "receipt entries are S-expressions; narrative belongs in :notes or surrounding file comments only.")

  (checker-contract
    :input "stdin (--stdin) OR ad hoc <file>.lisp paths"
    :modes [single-file stdin dry-fixture]
    :flags [--json --stdin --dry-fixture]
    :json-shape "{ ok, files, errors[], warnings[], receipts_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any input that is not (verification-receipt-set ...) or (verification-receipt ...)"]
    :non-goal
      "checker does NOT call git, does NOT shell out, does NOT touch the network or any LLM, and does NOT verify that referenced :files exist on disk. wave29-05 validates structure + reuse-rule helper only; downstream tooling can join receipts to actual git commits.")

  (reuse-helper-contract
    :name "isReceiptReusable"
    :signature "(receipt, { commit_hash, command, tier }) -> boolean"
    :rules
      ["1. receipt.exit_code === 0 (any non-zero exit invalidates reuse)"
       "2. receipt.commit_hash matches query commit_hash via hex-prefix-agree rule (longer starts with shorter; shorter >= 7 hex chars)"
       "3. receipt.command.trim() === query.command.trim() (no fuzzy matching)"
       "4. tier covering: full covers {local, smoke, full}; smoke covers {local, smoke}; local covers {local}"]
    :never
      ["never substitute for source facts (the four reuse conditions are CACHE rules, NOT verification rules)"
       "never substitute for commit verification (the orchestrator MUST still verify the git commit)"
       "never reused when ANY rule fails — return false and re-run the command"])

  (cross-wave-invariant
    "Verification receipts are ADVISORY ACCELERATION ONLY. They never replace task-contract, report, shared-memory, or git verification."
    "The conservative reuse rules (commit + command + zero-exit + tier-cover) MUST all hold for `isReceiptReusable` to return true. Any failure → reuse=false."
    "Tier covering is ASYMMETRIC: full > smoke > local. A `local` receipt CANNOT satisfy a smoke or full reuse query."
    "The checker does NOT cross-check :started_at/:finished_at against :duration_ms. They are independently advisory; the schema's job is structural validation, not arithmetic reconciliation."
    ":task_id MUST start with :wave-prefix-+'-' so a misplaced receipt (e.g. a wave28 receipt accidentally pasted into a wave29 receipts file) fails the checker before it ever reaches the planner.")

  (non-goals
    "The schema does not mandate where receipts live — both per-wave container files and per-task single-receipt files are valid; the recommendation is the container form so a wave's receipts batch into one file."
    "The schema does not mandate the units of :duration_ms (it is advisory); the comparison helpers never read :duration_ms."
    "The schema does not require :files to exist on disk; missing :files entries are NOT a warning. Receipts are caches, not file-existence assertions."
    "The schema does not encode the result of any future tier-promotion policy — that is a planner concern, not a receipt concern."))

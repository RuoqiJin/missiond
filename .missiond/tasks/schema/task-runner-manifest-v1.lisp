;; MissionD task-runner-manifest v1
;; Purpose: a Lisp-shaped, machine-checkable batch descriptor that names
;; the productive worker tasks for one wave, their dependency edges,
;; verification tier, dispatch group, write-scope, brief mode, and a
;; pointer to the shared preamble. The manifest is consumed by the
;; wave28-02 plan CLI, the wave28-03 brief renderer, and the wave28-05
;; batch verifier. It is NOT a worker task surface and NOT a runtime
;; backend switch — it is orchestration metadata only.
;;
;; Loadbearing rule: this manifest does NOT replace the dispatch-plan.lisp
;; that wave28 uses today, and it does NOT authorise any backend swap.
;; Every node in the manifest MUST point at a real (task <id> ...) form
;; under .missiond/tasks/<wave>/. Archive / backfill / index orchestrator
;; tasks MUST NOT appear when :productive_only is true (the cross-wave
;; default for wave28+) — those stay orchestrator-owned per the wave28
;; dispatch-plan policy.
;;
;; Verification tiers MUST stay aligned with task-contract v1's
;; :verification-tier enum (local | smoke | full). Drift is a structural
;; error.

(task-runner-manifest-schema missiond.task-runner-manifest.v1
  :version "v1"
  :status "code-aligned — schema + read-only checker scripts/check-task-runner-manifest.mjs (wave28-01); intentionally NOT wired to runtime dispatch and NOT a worker-task surface. wave28-02 emits/loads manifests via a CLI; wave28-03 renders briefs from them; wave28-05 verifies batch outcomes against them."
  :checker "scripts/check-task-runner-manifest.mjs"
  :seed nil

  (purpose
    "Make the wave-level batch of productive worker tasks a structured Lisp record instead of free-form prose embedded in a dispatch-plan note."
    "Pin the cross-wave invariant that productive-only manifests MUST exclude archive / backfill / index categories — those remain orchestrator-owned per the wave28 dispatch-plan policy."
    "Encode dependency edges and dispatch groups so the wave28-02 plan CLI can derive a deterministic execution order without re-parsing each task contract."
    "Encode write-scope per node so the wave28-05 batch verifier can detect same-file overlaps inside a single dispatch group; the default policy REJECTS overlap among productive tasks because two parallel agents writing the same file means a coordination bug.")

  (file-shape
    :file "<emitted by wave28-02 CLI; stored ad hoc; not seeded under .missiond/tasks/schema/>"
    :form (task-runner-manifest <manifest-id>
            :schema "missiond.task-runner-manifest.v1"
            :wave <wave-id>
            :brief_mode <brief-mode-atom>
            :shared_preamble_path <repo-relative-path>
            :productive_only <literal-bool>
            ;; optional manifest-level fields
            :overlap_policy <overlap-policy-atom>
            :description <string>
            :generated_at <iso-8601-string>
            :generator <string>
            <node> ...))

  (header-fields
    [:schema :wave :brief_mode :shared_preamble_path :productive_only])

  (entry-heads
    [node])

  (brief-modes
    [thin full preamble])

  (verification-tiers
    [local smoke full])

  (overlap-policies
    [reject warn])

  (overlap-policy-meaning
    (reject "Default productive-only policy: two nodes in the same dispatch_group MUST NOT declare overlapping write_scope paths. Same-file overlap is a structural error and the checker fails. Parallel agents writing the same file is a coordination bug.")
    (warn   "Opt-in relaxation: same-file overlap inside a dispatch_group is reported as a warning instead of an error. Reserved for explicitly serialized retry/repair manifests where two nodes intentionally share a target. Productive-only manifests SHOULD prefer reject."))

  (dispatch-group-style
    "non-empty atom or string identifying a peer group. Convention: a single uppercase letter (A | B | C | ...) mirrored from the wave dispatch-plan, but any non-empty kebab-case identifier is accepted. Nodes with the same :dispatch_group are eligible to run in parallel; nodes in different :dispatch_group strings are serialized by the plan CLI according to :depends_on edges.")

  (forbidden-productive-categories
    [archive backfill index lisp-backfill])

  (forbidden-productive-id-substrings
    ["-archive-" "-backfill-" "-index" "lisp-backfill"])

  (header-contract
    (:schema "literal string \"missiond.task-runner-manifest.v1\"; missing or mismatched schema is a structural error.")
    (:wave "non-empty kebab id matching ^[a-z][a-z0-9-]*$; SHOULD match the directory name under .missiond/tasks/<wave>/.")
    (:brief_mode "atom; one of the 3 brief-modes (thin | full | preamble). thin / preamble manifests REQUIRE a non-empty :shared_preamble_path so the renderer can locate shared boilerplate.")
    (:shared_preamble_path "string; repo-relative path to the shared preamble Markdown (no leading '/' or '~', no '..' traversal). The checker WARNS when the file does not exist on disk; missing path with brief_mode=thin|preamble is an error.")
    (:productive_only "literal atom true or false (NEVER a string). When true the checker rejects any node whose id, kind, or category names an archive / backfill / index orchestrator task. wave28+ default is true.")
    (:overlap_policy "OPTIONAL atom; one of reject | warn. Default is reject. Overrides only the same-dispatch_group write-scope overlap rule; per-node validation is unaffected.")
    (:description "OPTIONAL free-form prose for humans; never load-bearing for validation.")
    (:generated_at "OPTIONAL ISO-8601 timestamp string identifying when the manifest was emitted.")
    (:generator "OPTIONAL string identifying which CLI / handler emitted the manifest (e.g. \"scripts/plan-task-runner.mjs\")."))

  (node-contract
    (:task_id "non-empty kebab id matching ^[a-z0-9][a-z0-9._-]*$; MUST be unique within a manifest. SHOULD reference a real (task <id> ...) form under .missiond/tasks/<wave>/.")
    (:depends_on "vector of node :task_id values; every entry MUST resolve to another node in the SAME manifest. Self-edges and cycles are structural errors.")
    (:verification_tier "atom; one of local | smoke | full. Mirrors task-contract v1's :verification-tier enum; drift is a structural error.")
    (:dispatch_group "non-empty atom or string. Nodes sharing this value are eligible to run in parallel; the overlap-policy applies within a single :dispatch_group.")
    (:estimated_minutes "positive integer (literal atom; NOT a string). Zero or negative is a structural error.")
    (:heartbeat_minutes "positive integer (literal atom; NOT a string). Zero or negative is a structural error.")
    (:write_scope "vector of repo-relative path strings (no leading '/' or '~', no '..' traversal). May be empty only when the node is a pure verification probe; productive nodes SHOULD declare at least one path so the batch verifier can audit overlap.")
    (:notes "OPTIONAL free-form prose for humans; never load-bearing for validation.")
    (:owner "OPTIONAL string identifying the team / pillar that owns the task (e.g. \"claudecode\" | \"codex\").")
    (:kind "OPTIONAL atom mirroring task-contract :kind; the productive-only gate inspects this field when present (archive | backfill | index | lisp-backfill is rejected when :productive_only is true)."))

  (required-header-fields
    [:schema :wave :brief_mode :shared_preamble_path :productive_only])

  (optional-header-fields
    [:overlap_policy :description :generated_at :generator])

  (required-node-fields
    [:task_id :depends_on :verification_tier :dispatch_group :estimated_minutes :heartbeat_minutes :write_scope])

  (optional-node-fields
    [:notes :owner :kind])

  (defaults
    (:overlap_policy reject)
    (:productive_only true))

  (validation-contract
    :file-must-have-header [:schema :wave :brief_mode :shared_preamble_path :productive_only]
    :unique-per-manifest [:task_id]
    :enum-checked
      [:brief_mode :verification_tier :overlap_policy]
    :literal-bool-fields
      [:productive_only]
    :positive-int-fields
      [:estimated_minutes :heartbeat_minutes]
    :path-fields
      [:shared_preamble_path :write_scope]
    :rejects
      ["schema mismatch (:schema not missiond.task-runner-manifest.v1)"
       "missing any required header field"
       "missing any required node field"
       "unknown header field"
       "unknown node field"
       ":brief_mode not in the 3-element brief-modes enum"
       ":verification_tier not in the 3-element verification-tiers enum (must mirror task-contract v1)"
       ":overlap_policy not in the 2-element overlap-policies enum"
       ":productive_only not the literal atom true or false (strings are rejected)"
       ":estimated_minutes not a positive integer literal atom (zero, negative, fractional, or string forms are rejected)"
       ":heartbeat_minutes not a positive integer literal atom (zero, negative, fractional, or string forms are rejected)"
       "absolute or ~ or .. path inside :shared_preamble_path"
       "absolute or ~ or .. path inside any :write_scope entry"
       "duplicate :task_id within a manifest"
       ":depends_on entry referencing a non-existent node id in the SAME manifest"
       ":depends_on self-edge (a node depending on itself)"
       ":productive_only true with a node whose id / kind matches archive / backfill / index / lisp-backfill (orchestrator-owned per dispatch-plan policy)"
       "write-scope overlap among nodes in the SAME :dispatch_group when :overlap_policy is reject (default)"
       ":wave / :task_id / :dispatch_group with malformed id shape"
       "unknown entry head at top level (only `node` allowed inside the manifest)"]
    :warns
      [":shared_preamble_path file not present on disk (warning only — the schema does not own the preamble file)"
       "write-scope overlap among nodes in the SAME :dispatch_group when :overlap_policy is warn (advisory)"]
    :no-prose
      "manifest entries are S-expressions; narrative belongs in :description / :notes strings only.")

  (checker-contract
    :input "stdin (--stdin) OR ad hoc manifest.lisp files"
    :modes [single-file stdin dry-fixture]
    :flags [--json --stdin --dry-fixture]
    :json-shape "{ ok, files, errors[], warnings[], manifests_validated, nodes_validated }"
    :rejects
      ["everything in :validation-contract :rejects above"
       "any input that is not a single (task-runner-manifest ...) form (or a sequence of them)"]
    :non-goal
      "checker does NOT call git, does NOT shell out, does NOT touch the network or any LLM, and does NOT verify that the referenced (task <id> ...) forms exist on disk. The wave28-05 batch verifier does that join. wave28-01 validates structure only.")

  (cross-wave-invariant
    "Task-runner manifests are advisory orchestration metadata only. No wave reads these to alter live dispatch."
    ":productive_only true REQUIRES that no node names an archive / backfill / index orchestrator task. The wave28 dispatch-plan policy keeps those orchestrator-owned; the manifest checker enforces the same boundary structurally."
    "Same-file write-scope overlap among nodes in the SAME :dispatch_group is REJECTED by default. Two parallel agents writing the same file is a coordination bug. The :overlap_policy warn opt-in is reserved for explicitly serialized retry/repair manifests."
    "Verification-tier strings MUST mirror task-contract v1's :verification-tier enum exactly. Drift is a structural error; renaming the enum requires a coordinated v2 schema bump.")

  (non-goals
    "The schema does not score backends or pick a winner — backend selection stays in the router-recommendation surface (wave24-03 / wave25-02)."
    "The schema does not invoke any backend or apply any change — it is purely a batch descriptor."
    "The schema does not require the referenced task contracts to exist on disk; the checker validates manifest *shape* only. wave28-05 is responsible for the cross-file join."
    "The schema does not encode cost / latency / capacity hints — those belong in a future v2 if needed."
    "The schema does not replace the wave dispatch-plan.lisp; it complements it. dispatch-plan is hand-authored orchestrator policy; manifests can be emitted by tooling for batch verification."))

;; MissionD task-contract v1
;; Purpose: make Lisp the machine-readable SSOT for task dispatch while
;; Markdown remains a ClaudeCode-compatible rendered view.

(task-contract-schema missiond.task-contract.v1
  :version "v1"
  :status "code-aligned — checker + renderer + verifier + scope-guard + hooks-installer scripts implemented; v2 default-on hooks doctor preflight surfaced in renderer + installer (wave22-01); wave23-02 adds optional :session-trace-writable opt-in for trace-ledger writers and renderer auto-detects sibling session-trace.lisp; wave24-05 adds optional :router-policy-path field and renderer surfaces a Router Policy (advisory) section, auto-detecting .missiond/router/router-policy-v1.lisp when the field is absent — runtime dispatch is unchanged; wave25-04 extends the renderer's Router Policy section with two read-only command lines (check-router-policy + recommend-task-backend, parameterized by the resolved policy path AND the current task source path) and the Report Contract section with a MAY-language note about the wave25-02 optional router report fields — recommendation stays advisory and the renderer never shells out; wave26-05 adds optional :router-backend-registry-path field and the renderer extends the existing Router Policy (advisory) section to also render a `check-router-backend-registry` line when the registry resolves AND to append `--backend-registry <registry-path>` to the wave25-04 recommend-task-backend command when BOTH the policy AND the registry resolve, and extends the Report Contract section with a sub-bullet enumerating the wave26-04 readiness fields — section keeps 'advisory' / 'dry-run only' literals and gains an explicit 'MUST NOT switch backend' phrase; the renderer never shells out; wave27-05 extends the same Router Policy (advisory) section to ALSO surface two read-only `scripts/build-router-dispatch-descriptor.mjs` command lines (default Lisp output and a pipe-to `scripts/check-router-dispatch-descriptor.mjs --stdin` form) when BOTH the policy AND the registry resolve, adds the literal 'no execution' phrase next to existing 'advisory' / 'dry-run only' / 'MUST NOT switch backend' literals, and extends the Report Contract section with a sub-bullet enumerating the wave27-04 optional dispatch-descriptor report fields with MAY-language; no new task-contract field is added because the dispatch descriptor is an ephemeral artifact generated on demand by the wave27-02 CLI from existing inputs (task + policy + registry); wave28 dispatch-efficiency adds optional :verification-tier / :dispatch-group / :estimated-minutes / :heartbeat-minutes metadata and renderer --brief-mode full|thin|preamble so shared boilerplate is emitted once per wave instead of repeated in every worker brief"
  :checker "scripts/check-task-contract.mjs"
  :renderer "scripts/render-claudecode-task.mjs"
  :verifier "scripts/verify-task-contract.mjs"
  :scope-guard "scripts/task-scope-guard.mjs"
  :pre-commit-hook ".githooks/pre-commit"
  :hooks-installer "scripts/install-missiond-hooks.mjs"
  :hooks-doctor "scripts/check-missiond-hooks.mjs"

  (purpose
    "S-expressions carry the machine contract: scope, forbidden files, dependencies, acceptance, commit policy, and report requirements."
    "Markdown is an execution view rendered from the contract for current ClaudeCode ergonomics."
    "MissionD plan-runner can later consume this contract directly without depending on Markdown parsing.")

  (required-task-fields
    [:schema :title :kind :status :owner :goal :write-scope :must-not-touch :acceptance :commit])

  (field-contract
    (:schema "must equal missiond.task-contract.v1")
    (:id "second form of (task <id> ...); lowercase kebab/dot/underscore id")
    (:kind "code-alignment | lisp-only | docs | review | smoke | ops")
    (:status "draft | ready | running | blocked | done | archived")
    (:owner "agent or workstation class")
    (:depends-on "vector of task ids; optional")
    (:dispatch-strategy "fresh-code-alignment | resident-lisp | agent-team | mixed | manual; optional")
    (:write-scope "non-empty vector of repo-relative paths or globs")
    (:must-not-touch "vector of repo-relative paths or globs; may be empty but must be explicit")
    (:requirements "ordered vector of human-readable implementation requirements")
    (:acceptance "non-empty vector of shell commands or verifier commands")
    (:commit "property list: :required boolean + :message + :scope-check")
    (:verification-tier "OPTIONAL enum: local | smoke | full. Dispatch planner / brief renderer uses this to keep routine workers on local checks and reserve full workspace cargo/build runs for smoke/final tasks.")
    (:dispatch-group "OPTIONAL compact id for true parallel batch grouping. It is orchestration metadata, not a dependency; :depends-on remains authoritative.")
    (:estimated-minutes "OPTIONAL positive integer atom. Used for dispatch planning and bottleneck review; checker rejects zero/non-integer values.")
    (:heartbeat-minutes "OPTIONAL positive integer atom. Thin briefs render it as a shared-memory heartbeat expectation so stalled workers can be detected early.")
    (:session-trace-writable
      "OPTIONAL boolean (default false). When true, the rendered brief instructs the worker that this task is permitted to append factual (trace-event ...) entries to .missiond/tasks/<wave>/session-trace.lisp as a shared coordination output, in addition to its own :write-scope. Default behaviour: workers MUST NOT write to session-trace.lisp unless this flag is true. The session-trace ledger remains MissionD-owned factual telemetry — it never replaces the worker's per-task :write-scope, and prose explanations still belong in the report contract.")
    (:router-policy-path
      "OPTIONAL repo-relative path (default absent). When provided, the renderer emits a 'Router Policy (advisory)' section pointing at this router-policy-v1 Lisp file so human readers / ClaudeCode workers can consult the dry-run policy that distilled prior trace observations. When absent, the renderer auto-detects .missiond/router/router-policy-v1.lisp; if neither path resolves on disk, the section is omitted. The rendered section is strictly informational — it MUST contain the literal words 'advisory' and 'dry-run only', MUST NOT instruct workers to change backend, and the renderer never shells out to the recommendation CLI. wave25-04: the rendered section now ALSO carries two read-only command lines as text — `node scripts/check-router-policy.mjs <policy-path>` and `node scripts/recommend-task-backend.mjs --task <THIS_TASK_LISP> --policy <policy-path> --json` (parameterized by the resolved policy path AND the task source path) — so a human or worker can copy-paste them; they remain advisory commands only and the renderer never executes them.")
    (:router-backend-registry-path
      "OPTIONAL repo-relative path (default absent). wave26-05: when provided, the renderer extends the existing wave24-05 + wave25-04 'Router Policy (advisory)' section to ALSO surface the wave26-01 backend readiness registry. Auto-detect precedence: explicit :router-backend-registry-path on the task contract wins; otherwise the renderer auto-detects .missiond/router/router-backend-registry-v1.lisp; if neither path resolves on disk, the registry context is omitted (the rest of the section continues to render unchanged). When the registry resolves, the renderer appends a read-only `node scripts/check-router-backend-registry.mjs <registry-path>` command line to the section. When BOTH the policy AND the registry resolve, the renderer modifies the wave25-04 `recommend-task-backend` command to also include `--backend-registry <registry-path>` (the policy-only command remains rendered byte-identical when the registry does not resolve, preserving wave25-04 backward compatibility). The section continues to carry the literals 'advisory' and 'dry-run only' verbatim and explicitly says backend dispatch MUST NOT be switched based on rendered text. The renderer NEVER shells out — every command in the section is rendered text only and is presented as opt-in inspection for humans / workers."))

  (commit-contract
    :required-fields [:required :scope-check]
    :required-when-commit-required [:message]
    :scope-check-values [write-scope-only none not-required]
    :rule "When :required true, staged files must be inside :write-scope unless the task explicitly records a deviation.")

  (renderer-contract
    :input ".missiond/tasks/**/*.lisp"
    :output ".missiond/claudecode/<task-id>.md"
    :default "full brief; refuse overwrite unless --force"
    :brief-modes
      [(full "legacy-compatible verbose brief with all shared protocol sections in every file")
       (thin "task-specific brief only; points at .missiond/claudecode/<wave>-shared-preamble.md for shared-memory/report/session-trace/router/commit boilerplate")
       (preamble "shared boilerplate rendered once per wave; no task input required")]
    :machine-context-rendered
      ["task :kind / :status / :owner"
       ":dispatch-strategy when present"
       ":verification-tier / :dispatch-group / :estimated-minutes / :heartbeat-minutes when present"
       ":depends-on as code-fenced ids"
       "shared-memory ledger path .missiond/tasks/<wave>/shared-memory.lisp when the file exists on disk"
       "expected report-contract path .missiond/tasks/<wave>/reports/<task-id>.report.lisp when the wave id is derivable"
       "pre-commit scoped-index guard line `node scripts/task-scope-guard.mjs --task <task.lisp> --mode staged` immediately after the git-add step when :commit :required is true"
       "MISSIOND_TASK_CONTRACT=<task.lisp> env-var prefix on the rendered git commit line when :commit :required is true (mirrors the .githooks/pre-commit activation contract)"
       "verify-task-contract command line in the Commit section when :commit :required is true"
       "literal '使用 agent-team提高效率' rendered exactly once in a 'Dispatch Note' section when :dispatch-strategy is agent-team"
       "default-on hooks-doctor preflight block immediately before the staged-guard / git-add commands when :commit :required is true; emits the read-only `node scripts/check-missiond-hooks.mjs --json` line and the explicit `node scripts/install-missiond-hooks.mjs --install` opt-in line; the renderer never mutates git config and never substitutes for the operator running --install"
       "session-trace section (wave23-02): when a sibling .missiond/tasks/<wave>/session-trace.lisp file exists on disk, the renderer emits a 'Session Trace' section pointing at the path and reminding workers that the trace ledger is the canonical fact log (schema missiond.session-trace.v1). When the task contract sets :session-trace-writable true, the section also tells the worker they MAY append (trace-event ...) entries; otherwise the section explicitly states the worker MUST NOT write to session-trace.lisp. Auto-detection only — no contract change required for tasks that merely co-exist with the trace ledger."
       "router-policy section (wave24-05): when the task contract carries :router-policy-path <path>, the renderer emits a 'Router Policy (advisory)' section pointing at that path; otherwise it auto-detects .missiond/router/router-policy-v1.lisp and renders the section if the file exists on disk. The section is rendered between Session Trace and Commit and MUST contain the literal words 'advisory' and 'dry-run only'; it MUST NOT instruct ClaudeCode to change backend. Renderer never shells out to scripts/recommend-task-backend.mjs — the recommendation CLI is opt-in for humans and tooling, not a renderer dependency."
       "router-policy commands (wave25-04): the same Router Policy (advisory) section now ALSO renders two read-only command lines as text — `node scripts/check-router-policy.mjs <policy-path>` and `node scripts/recommend-task-backend.mjs --task <THIS_TASK_LISP> --policy <policy-path> --json` — both parameterized by the resolved policy path AND the current task contract source path. The commands are presented as 'you may run these to inspect the policy / dry-run recommendation' and stay strictly advisory; the section's literal 'advisory' and 'dry-run only' guarantees still hold and no shell-out is added to the renderer."
       "router-backend-registry context (wave26-05): the same Router Policy (advisory) section is extended to ALSO surface the wave26-01 backend readiness registry when it resolves. Auto-detect precedence: explicit :router-backend-registry-path on the task contract wins; otherwise the renderer auto-detects .missiond/router/router-backend-registry-v1.lisp; if neither resolves the registry context is omitted (the rest of the section continues to render unchanged). When the registry resolves, the renderer appends a read-only `node scripts/check-router-backend-registry.mjs <registry-path>` line. When BOTH the policy AND the registry resolve, the renderer modifies the wave25-04 `recommend-task-backend` command to also include `--backend-registry <registry-path>` (when only the policy resolves the wave25-04 command stays rendered byte-identical for backward compat). The section keeps 'advisory' and 'dry-run only' literals verbatim and gains an explicit 'MUST NOT switch backend' phrase. Renderer never shells out — every command stays rendered text only."
       "report-contract router-fields note (wave25-04): when the rendered brief includes a Report Contract section, the renderer appends a brief MAY-language note pointing at the wave25-02 optional router-recommendation report fields (:recommended_backend / :router_confidence / :router_policy_path / :router_dry_run_only / :router_applied / :router_reasons / :router_trace_index_path) so workers know they can populate them when they observe a recommendation. The note is advisory ('MAY' not 'MUST') because the report fields themselves are optional; their cross-wave invariants (router_dry_run_only=true literal, router_applied=false literal) are checker-enforced in scripts/check-task-report.mjs and re-stated in the bullet text."
       "report-contract router-readiness note (wave26-05): when the rendered brief includes a Report Contract section, the renderer appends an additional sub-bullet group enumerating the wave26-04 optional readiness fields (:router_backend_readiness_status enum current-default|advisory-only|runtime-ready|unavailable|unknown, :router_backend_runtime_allowed literal bool, :router_apply_eligible literal bool, :router_apply_blockers vector of non-empty strings, :router_backend_registry_path repo-relative path) so workers know they MAY populate them when they observe a backend readiness registry. MAY-language only; cross-wave invariants are checker-enforced in scripts/check-task-report.mjs."
       "router-dispatch-descriptor commands (wave27-05): the same Router Policy (advisory) section is extended further so that when BOTH the policy AND the registry resolve, the renderer appends a dedicated dispatch-descriptor sub-section carrying TWO read-only command lines as text — `node scripts/build-router-dispatch-descriptor.mjs --task <THIS_TASK_LISP> --policy <policy-path> --backend-registry <registry-path>` (default Lisp output, suitable for piping) and the same command piped into `node scripts/check-router-dispatch-descriptor.mjs --stdin` (the wave27-01 checker only parses Lisp from stdin, so the rendered pipe form intentionally drops --json). The sub-section adds the literal phrase 'no execution' next to the existing wave24-05 'advisory' / 'dry-run only' and wave26-05 'MUST NOT switch backend' literals. The renderer NEVER executes either command — they are rendered text only and the dispatch descriptor remains an ephemeral artifact generated on demand. No new task-contract field is added (rationale: the descriptor is derived from existing inputs — task + policy + registry — so a static :router-dispatch-descriptor-path field would create stale-by-design state). When only the policy resolves (no registry), the wave26-05 ordering stays byte-identical and the dispatch-descriptor sub-section is omitted."
       "report-contract router-dispatch-descriptor note (wave27-05): when the rendered brief includes a Report Contract section, the renderer appends an additional sub-bullet group enumerating the wave27-04 optional dispatch-descriptor fields (:router_dispatch_descriptor_path repo-relative path, :router_dispatch_descriptor_status enum eligible|current-default|advisory-only|registry-missing|unavailable|unknown, :router_dispatch_backend enum claudecode|missiond-llm-router|deterministic-checker|patch-worker|verifier-worker, :router_dispatch_eligible literal bool, :router_dispatch_no_execution literal `true` only — cross-wave invariant — false AND quoted-string both rejected by the checker, :router_dispatch_blockers vector of non-empty strings) so workers know they MAY populate them when they observe a dispatch descriptor. MAY-language only; cross-wave invariants are checker-enforced in scripts/check-task-report.mjs."]
    :dispatch-efficiency-rendering
      ["thin mode preserves task-specific Goal / Ownership / Must Not Touch / Requirements / Acceptance / Commit / Report sections but omits repeated Shared Memory / Report Contract / Session Trace / Router Policy boilerplate"
       "preamble mode renders that shared boilerplate once, including heartbeat, report, session-trace, router no-execution boundaries, and commit protocol"
       "full mode remains the default for backward compatibility and existing fixtures"]
    :backward-compatibility
      ["existing fields (kind/status/owner/dispatch_strategy/depends_on/Goal/Ownership/Must Not Touch/Requirements/Acceptance Commands/Commit/Report) keep their prior wording and ordering"
       "new sections (Dispatch Note, Shared Memory, Report Contract, verify-task-contract command) are additive and conditional"
       "scoped commit guard v2 (task-scope-guard --mode staged + MISSIOND_TASK_CONTRACT prefix) extends the existing Commit section in place; renders only when :commit :required is true and never replaces the git add or git commit lines"
       "default-on hooks-doctor preflight block (wave22-01) extends the Commit section in place above the existing git-add / staged-guard / git-commit fenced block; renders only when :commit :required is true; never mutates git config; only adds doctor + opt-in install lines"
       "router-policy section (wave24-05) is additive and conditional: it sits between the Session Trace and Commit sections and only renders when :router-policy-path resolves or .missiond/router/router-policy-v1.lisp exists on disk; section text is purely informational (literal 'advisory' and 'dry-run only') and never prescribes a backend switch; renderer never shells out to recommend-task-backend.mjs"
       "router-policy commands (wave25-04) extend the existing Router Policy (advisory) section in place by appending a second fenced block carrying the recommend-task-backend command and a one-line MAY-language preamble; tasks WITHOUT a resolvable router policy continue to render byte-identical (no new sections appear); the wave23-02 Session Trace section ordering is preserved byte-identical; no shell-out is added"
       "router-backend-registry context (wave26-05) extends the existing Router Policy (advisory) section in place: when the registry resolves the renderer appends a read-only `check-router-backend-registry` line and (when BOTH policy + registry resolve) extends the wave25-04 recommend-task-backend command with `--backend-registry <registry-path>`; when only the policy resolves the wave25-04 command stays rendered byte-identical (backward compat); when neither the policy nor the registry resolves the section is still omitted (no surprise sections appear); no shell-out is added"
       "router-dispatch-descriptor commands (wave27-05) extend the existing Router Policy (advisory) section in place by appending a NEW dispatch-descriptor sub-section AFTER the wave26-05 recommend-task-backend block, but ONLY when BOTH policy + registry resolve. When only the policy resolves the wave25-04 + wave26-05 surface stays rendered byte-identical (no new sub-section appears). No new task-contract field is added; descriptors are generated on demand by the wave27-02 CLI from existing inputs — task + policy + registry. The sub-section's literal 'no execution' phrase joins the existing 'advisory' / 'dry-run only' / 'MUST NOT switch backend' literals; renderer never shells out — every command stays rendered text only."
       "report-contract router-fields note (wave25-04) extends the existing Report Contract section in place by appending an additional bullet group enumerating the wave25-02 optional router report fields with MAY-language guidance; tasks without a resolvable wave id (no Report Contract section) continue to render byte-identical"
       "report-contract router-readiness note (wave26-05) extends the existing Report Contract section in place by appending an additional sub-bullet group enumerating the wave26-04 optional readiness fields with MAY-language guidance; tasks without a resolvable wave id (no Report Contract section) continue to render byte-identical"
       "report-contract router-dispatch-descriptor note (wave27-05) extends the existing Report Contract section in place by appending an additional sub-bullet group enumerating the wave27-04 optional dispatch-descriptor fields with MAY-language guidance; tasks without a resolvable wave id (no Report Contract section) continue to render byte-identical"]
    :non-goal "renderer does not invent scope, acceptance, or commit policy; missing fields are checker errors")

  (verifier-contract
    :input ".missiond/tasks/**/*.lisp + git commit (default HEAD)"
    :flags [--commit --json --dry-fixture]
    :checks
      ["commit hash resolves and is reported"
       "commit subject (first line) equals contract :commit :message when present"
       "every changed file ⊆ :write-scope when :commit :scope-check is write-scope-only"
       "no changed file overlaps :must-not-touch (always enforced regardless of :scope-check)"]
    :read-only
      ["never runs git add/commit/reset/checkout/stash/push/merge/rebase"
       "only invokes git rev-parse, git log, git show with --pretty=format and --name-only"]
    :glob-semantics "shared with checker via scripts/lib/missiond_lisp.mjs (pathMatchesPattern + pathMatchesAny)")

  (scope-guard-contract
    :purpose "Pre-commit defense against the Wave 19 git-index pollution failure where parallel sessions cross-staged files outside their task scope."
    :input ".missiond/tasks/**/*.lisp + git index (staged files) | git commit"
    :flags [--task --mode --commit --json --dry-fixture]
    :modes
      [(staged
         "reads `git diff --cached --name-only -z` and fails if any staged path is outside :write-scope (when :scope-check is write-scope-only) or matches :must-not-touch (always enforced)"
         "intended to fire from a pre-commit hook so the commit is blocked before the index is locked in")
       (commit
         "delegates to verify-task-contract semantics (subject + scope + forbidden) by reusing readCommit + verifyContract from scripts/verify-task-contract.mjs"
         "accepts --commit <hash> to verify any commit, defaults to HEAD")]
    :shared-logic
      ["loadContract / loadContractFromSource / verifyContract / readCommit imported from scripts/verify-task-contract.mjs"
       "pathMatchesAny imported from scripts/lib/missiond_lisp.mjs"]
    :read-only
      ["never runs git add/commit/reset/checkout/stash/push/merge/rebase"
       "staged-mode only invokes git diff --cached --name-only --no-renames -z"
       "commit-mode only invokes git rev-parse, git log, git show with --pretty=format and --name-only"]
    :pre-commit-hook
      (:path ".githooks/pre-commit"
       :activation "MISSIOND_TASK_CONTRACT env var must point at a task.lisp; otherwise hook exits 0"
       :enable "node scripts/install-missiond-hooks.mjs --install (preferred) or git config core.hooksPath .githooks (equivalent, per-clone opt-in)"
       :delegates-to "scripts/task-scope-guard.mjs --mode staged"
       :read-only true))

  (hooks-installer-contract
    :purpose "Replace tribal-knowledge `git config core.hooksPath .githooks` with an explicit, repo-local installer + default-on read-only doctor flow so every clone can opt into the task-scope-guard pre-commit deterministically. v2 (wave22-01) promotes the doctor to a default-on preflight: drift is reported as a `preflight-drift` problem with a concrete install command, but git config is NEVER mutated by the doctor or the renderer; only `--install` flips it."
    :scripts
      (:installer "scripts/install-missiond-hooks.mjs"
       :doctor "scripts/check-missiond-hooks.mjs")
    :flags [--check --install --json --dry-fixture --strict]
    :default-mode "--check (default-on doctor when no mode flag is supplied to the installer; check-missiond-hooks.mjs always runs --check)"
    :modes
      [(check
         "read-only doctor: prints whether git core.hooksPath equals .githooks and whether .githooks/pre-commit exists"
         "JSON payload includes :severity (ok | preflight-drift), :reason (aligned | hooks-path-unset | hooks-path-wrong | hook-file-missing), :advice, and :install_command for non-ok states"
         "exits 0 by default even on drift; pair with --strict to make drift a hard non-zero exit"
         "DEFAULT mode when install-missiond-hooks.mjs is invoked with no mode flag (default-on doctor v2)")
       (install
         "performs exactly one mutation: `git config --local core.hooksPath .githooks`"
         "refuses (ok=false changed=false) when .githooks/pre-commit is missing — does not silently arm a no-op hooksPath"
         "no-op + exit 0 when already aligned; never touches --global or --system git config")
       (dry-fixture
         "self-contained fixtures (no git invoked, no disk writes) covering the four required doctor states (installed / unset / wrong-path / missing-hook-file) plus the install state machine, the install-refuses-on-missing-hook-file guard, the adapter --local enforcement, and the adviceFor() install-command surface")]
    :doctor-states
      [(aligned "core.hooksPath==.githooks AND .githooks/pre-commit present")
       (hooks-path-unset "git config core.hooksPath returned no value (severity preflight-drift)")
       (hooks-path-wrong "git config core.hooksPath != .githooks (severity preflight-drift)")
       (hook-file-missing ".githooks/pre-commit absent in working tree (severity preflight-drift; install refuses until restored)")]
    :doctor-alias
      (:script "scripts/check-missiond-hooks.mjs"
       :delegates-to "scripts/install-missiond-hooks.mjs --check"
       :rejects-mutating-flags ["--install" "--dry-fixture"]
       :default-on true
       :reason "Keeps the agent-facing doctor predictable and default-on: doctor never mutates, ever, and is the canonical preflight surface rendered into commit-required task briefs.")
    :renderer-integration
      (:section "Commit"
       :placement "above the existing git-add / staged-guard / git-commit fenced block, only when :commit :required is true"
       :emits ["node scripts/check-missiond-hooks.mjs --json"
               "node scripts/install-missiond-hooks.mjs --install"]
       :mutating-boundary "renderer never invokes git config; the install line is rendered as an explicit operator/agent action gated on doctor drift"
       :rationale "Surfaces core.hooksPath as a default preflight expectation in every commit-required brief; agents see the doctor command up front and decide whether to opt this clone in.")
    :scope
      ["repo-local only: never enables hooks globally"
       "single mutation in --install mode: `git config --local core.hooksPath .githooks`"
       "no other writes: never touches files outside .git/config; never runs git add/commit/reset/checkout/stash/push/merge/rebase"
       "doctor + renderer never mutate git config under any circumstance"]
    :acceptance
      ["node scripts/install-missiond-hooks.mjs --dry-fixture exits 0 with all 11 fixtures green and reports doctor_states_covered = [installed unset wrong-path missing-hook-file]"
       "node scripts/install-missiond-hooks.mjs (no mode flag) is equivalent to --check (default-on doctor v2)"
       "node scripts/install-missiond-hooks.mjs --check --json prints the current core.hooksPath + hook-file presence + severity + reason + install_command"
       "node scripts/check-missiond-hooks.mjs --json equals --check delegation output and includes the v2 severity + reason fields"
       "rendered commit-required briefs include the hooks-doctor preflight block above the staged-guard fenced block"]))

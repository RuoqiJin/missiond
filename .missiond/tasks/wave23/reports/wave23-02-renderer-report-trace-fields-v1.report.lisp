;; Wave 23 / Task 02 — Renderer + report trace fields v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave23/wave23-02-renderer-report-trace-fields-v1.lisp

(report wave23-02-renderer-report-trace-fields-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave23-02-renderer-report-trace-fields-v1"
  :status done
  :commit_hash "a005a45bbc32"
  :files_changed
    ["scripts/render-claudecode-task.mjs"
     "scripts/check-task-report.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp"
     ".missiond/claudecode/wave23-01-session-trace-schema-v0.md"]

  :acceptance_results
    [(:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "16 fixtures OK (10 pre-existing + 6 new wave23-02 fixtures: valid-all-explain-fields, time_sinks-not-vector, major_decisions-missing-:decision, unexpected_work-missing-:summary, blockers-empty-string, trace_refs-absolute-path).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "57 tasks OK. Schema task-contract-v1.lisp updates (status note + :session-trace-writable doc + renderer-contract :machine-context-rendered Session Trace entry) parse and validate against checker requirements; new optional contract field on wave23-01 (:session-trace-writable true) parses cleanly.")
     (:command "node scripts/render-claudecode-task.mjs --force .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "Re-rendered .missiond/claudecode/wave23-01-session-trace-schema-v0.md as golden example: Machine Contract now includes session_trace and session_trace_writable bullets; new 'Session Trace' section appears between Report Contract and Commit, exercising the :session-trace-writable true branch (instructs worker MAY append trace-event entries).")
     (:command "git diff --check -- scripts/render-claudecode-task.mjs scripts/check-task-report.mjs .missiond/tasks/schema/task-contract-v1.lisp .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/wave23/wave23-01-session-trace-schema-v0.lisp .missiond/claudecode/wave23-01-session-trace-schema-v0.md"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 6 in-scope paths; no whitespace errors introduced.")]

  :scope_deviations []

  :time_sinks
    [(:label "reading background files (renderer + checker + 2 schemas + wave23-01 + sample wave22 report)" :duration_ms 600000)
     (:label "designing report explanation field shape (string-or-property-list union)" :duration_ms 300000)
     (:label "writing 6 new fixtures + structural validators" :duration_ms 900000)]

  :major_decisions
    [(:decision "Use top-level optional :session-trace-writable boolean (default false) on the task contract"
      :rationale "Matches existing kebab-case convention (:write-scope :must-not-touch :scope-check); stays out of :commit / :requirements which already have well-defined shapes; explicit opt-in keeps trace ledger MissionD-owned by default.")
     (:decision "Each explanation field accepts a string OR a property list with one declared key (:label / :decision / :summary)"
      :rationale "Lets workers write quick prose bullets when no structure is needed, while still allowing structured entries with optional :duration_ms / :rationale / :trace_ref / :resolved when the worker wants to link back to factual telemetry.")
     (:decision "Renderer auto-detects sibling .missiond/tasks/<wave>/session-trace.lisp; no contract field required for read-only awareness"
      :rationale "Keeps the contract surface clean — every task in a wave that has a trace ledger sees the section without contract churn; only the writable flag is contract-controlled.")
     (:decision "Re-rendered wave23-01 brief uses :session-trace-writable true"
      :rationale "Wave23-01 created the ledger so it semantically owns trace writability; demonstrates the more interesting renderer branch as the golden example.")]

  :unexpected_work
    [(:summary "Re-read shared-memory.lisp tail before each Edit because parallel agents (wave23-04, wave23-06) appended entries between my reads")]

  :blockers []

  :trace_refs
    [".missiond/tasks/wave23/session-trace.lisp"
     "wave23-trace-bootstrap-001"]

  :notes
    "wave23-02 surfaces session-trace as a first-class concern in rendered briefs and adds prose-only worker-explanation fields to the report contract — without ever making report prose the SSOT for facts. Commits 7c6ea9ad9569 (functional change, 6 files, +511/-8) and a005a45bbc32 (lint clean-up, 1 file, +6/-6 — removes 3 unused `task` parameters from renderSharedMemory/renderReportContract/renderVerifyTaskContract; re-rendered wave23-01 brief is byte-identical).

Renderer (scripts/render-claudecode-task.mjs):
- New helper resolveSessionTracePath(taskId): derives wave id, checks for sibling .missiond/tasks/<wave>/session-trace.lisp, returns the relative path if it exists or null.
- loadSingleTask() now reads :session-trace-writable as a boolean (default false).
- Machine Contract block adds two bullets when the trace ledger exists: `session_trace` (path) and `session_trace_writable` (true|false).
- New renderSessionTrace() emits a 'Session Trace' section between 'Report Contract' and 'Commit' when the trace file exists. Always reminds the worker that the ledger is the SSOT for facts and prose explanation belongs in the report. Conditionally adds 'MAY append (trace-event ...) entries' instructions when :session-trace-writable true; otherwise emits an explicit 'MUST NOT write to session-trace.lisp' line.
- renderReportContract() extended to enumerate the 5 new optional explanation fields (:time_sinks / :major_decisions / :unexpected_work / :blockers / :trace_refs) with their accepted shapes.

Report checker (scripts/check-task-report.mjs):
- New shared validateExplanationField(file, report, node, fieldName, requiredKey, diagnostics) — each entry is either a non-empty string or a property list whose declared key (:label / :decision / :summary) is present and non-empty.
- New dedicated validateTraceRefs() — vector of strings, no empty strings, no absolute paths, no '~' paths (mirrors the existing :files_changed / :scope_deviations conventions).
- 6 new fixtures: all-explain-fields-pass; time_sinks-not-vector; major_decisions-missing-:decision; unexpected_work-property-list-missing-:summary; blockers-empty-string-rejected; trace_refs-absolute-path-rejected. Total fixtures grew from 10 to 16.

Schemas:
- .missiond/tasks/schema/report-contract-v1.lisp: optional-report-fields gained :time_sinks :major_decisions :unexpected_work :blockers :trace_refs (with explanatory comment that they are prose-only and facts live in session-trace.lisp); field-contract gained per-field doc strings; checker-contract :rejects extended with the 3 new structural rejection rules; status note bumped to call out wave23-02.
- .missiond/tasks/schema/task-contract-v1.lisp: field-contract gained :session-trace-writable optional doc; renderer-contract :machine-context-rendered gained an entry describing the Session Trace section auto-detection + the writable-flag conditional rendering; status note bumped.

Contract surface chosen for 'trace writable' tasks:
- Top-level optional :session-trace-writable boolean. Default false (workers MUST NOT write to session-trace.lisp). Contract opts in via `:session-trace-writable true`.
- Reasoning: matches existing kebab-case top-level fields; semantics line up with the existing default-deny / explicit-opt-in convention used elsewhere (e.g. :must-not-touch is explicit, :scope-check is enumerated). Stays out of :commit (which is a property list with shape :required/:message/:scope-check) and :requirements (which is a string vector). One clean atom on the contract surface.

Re-rendered wave23-01 brief is the golden example: it now demonstrates the writable branch (Session Trace section says 'you MAY append trace-event entries') because wave23-01 is the task that created the trace ledger. The contract gained one line (:session-trace-writable true) — no other change.

Wave 23 protocol followed:
- Append claim entry (wave23-02-claim-001, seq 6) to .missiond/tasks/wave23/shared-memory.lisp BEFORE staging — re-validated 1 ledger / 6 entries OK.
- Edit-only on every pre-existing file (no Write); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 6 staged files, all inside :write-scope.
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation. Hooks aligned (core.hooksPath==.githooks), so the .githooks/pre-commit hook also fired the staged guard.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 7c6ea9ad9569 against the contract — message + scope all green.
- Append completion entry (wave23-02-completion-001, seq 10) to shared-memory ledger; re-validated 1 ledger / 10 entries OK.

Constraints honored:
- Did NOT touch crates/** (a parallel modification to crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs from wave23-04 was observed in the worktree but left untouched and unstaged).
- Did NOT touch .missiond/v2/*.lisp.
- Did NOT touch scripts/check-session-trace.mjs / scripts/analyze-session-trace.mjs / scripts/lib/missiond_lisp.mjs (parallel-agent territory wave23-01 / wave23-04 / wave23-06).
- Did NOT touch .missiond/tasks/wave22/**.
- Did NOT git add . / git stash / git reset / git checkout / --no-verify / --amend / --force.")

;; Wave 24 / Task 05 — Renderer router context v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp

(report wave24-05-renderer-router-context-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave24-05-renderer-router-context-v0"
  :status done
  :commit_hash "294a92a18318db0dc4a89bc3bf1225f7aede3069"
  :files_changed
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :acceptance_results
    [(:command "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp >/tmp/wave24-router-render.md"
      :exit_code 0
      :ok true
      :notes "Renderer exited 0 and emitted markdown to /tmp/wave24-router-render.md (process never started a child process; fs reads only). Manual grep of the output for the two contract literals: line 102 contains '## Router Policy (advisory)' (header literal 'advisory'); line 106 contains '- This section is **advisory** and **dry-run only**...' (bullet contains BOTH literals 'advisory' and 'dry-run only' verbatim). The bullet also explicitly states 'runtime dispatch is unchanged — ClaudeCode remains the live backend for this task' so the section cannot be misread as a backend-switch instruction.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (66 tasks). The schema additions (:router-policy-path field declaration + renderer-contract :machine-context-rendered entry + :backward-compatibility entry + status string update) all pass the existing reader-syntax + structural validators. No existing contracts referenced :router-policy-path so no false-positive regression.")
     (:command "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across both write-scope files post-commit. No trailing whitespace, no whitespace-vs-tab issues, no merge conflict markers.")
     (:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (16). Wave23-02 fixtures for :time_sinks / :major_decisions / :unexpected_work / :blockers / :trace_refs all stay green. The renderer change does not touch check-task-report.mjs so the report-contract validator surface is byte-identical.")
     (:command "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
      :exit_code 0
      :ok true
      :notes "router-policy check OK (1 policy, 3 rules). Sanity check that the seed policy referenced by the renderer's auto-detect path is still valid; the renderer never executes the policy file, only points at it.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-05-renderer-router-context-v0 (2 staged file(s)). Both staged paths (scripts/render-claudecode-task.mjs and .missiond/tasks/schema/task-contract-v1.lisp) are inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/wave23-*.md). The pre-commit hook re-ran the same guard via MISSIOND_TASK_CONTRACT and reported the same OK line.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-05-renderer-router-context-v0 against 294a92a18318 — commit hash exists; commit subject equals contract :commit.message exactly ('feat(tasks): render router policy context'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave24-05-trace-start-001 wave24-05-trace-commit-001 wave24-05-trace-complete-001]

  :time_sinks
    [(:label "Cross-check renderer pattern against wave23-02 Session Trace section"
      :notes "Read scripts/render-claudecode-task.mjs end-to-end (355 lines) before editing — the wave23-02 Session Trace section is the model: a resolveXPath helper, an auto-detect against the file existing on disk, a renderXSection helper invoked between two existing sections. Mirrored that pattern exactly for resolveRouterPolicyPath / renderRouterPolicy. Spent extra time confirming the section sits between Session Trace and Commit (not inside Commit) so the existing hooks-doctor preflight, git-add, task-scope-guard, and git-commit lines stay grouped.")
     (:label "Choose between :router-policy-path and :router-context-path"
      :notes "The brief allowed either kebab-case symbol. Picked :router-policy-path because (a) it matches the file naming convention (router-policy-v1.lisp) and (b) it makes the field's referent unambiguous — :router-context-path could imply a broader 'router context' bundle that does not yet exist. Documented the choice + rationale in the field-contract entry alongside :session-trace-writable.")
     (:label "Verify wave23-02 Session Trace section is byte-identical post-edit"
      :notes "Compared lines 87-99 of the previously-rendered .missiond/claudecode/wave24-01-router-policy-schema-v1.md against lines 87-99 of a fresh stdout render of the same task contract via the patched renderer. Both blocks match byte-for-byte from '## Session Trace' through the closing fenced ```bash ... check-session-trace.mjs ... ``` block. The new Router Policy section appears AFTER (line 102+) without disturbing the Session Trace block.")]

  :major_decisions
    [(:decision "Place Router Policy section between Session Trace and Commit"
      :rationale "The section is informational metadata about dispatch context; it pairs naturally with the Session Trace ledger (which is the factual evidence the policy was distilled from) and stays out of the Commit block (which is operational). Placing it inside Commit would have created visual noise around the hooks-doctor + git-add + task-scope-guard + git-commit fenced commands. Documented the placement in the schema's :machine-context-rendered and :backward-compatibility entries.")
     (:decision "Auto-detect precedence: contract field FIRST, then default seed path"
      :rationale "Mirrors how MissionD treats other optional contract fields — explicit always wins over auto-detect. If a future wave wants to point a specific task at an alternate policy file, :router-policy-path makes that surgical without changing the renderer. The default fallback to .missiond/router/router-policy-v1.lisp keeps the section visible across the existing wave22/23/24 contract corpus without re-rendering any of them.")
     (:decision "Renderer does NOT shell out to scripts/recommend-task-backend.mjs"
      :rationale "The brief explicitly forbids hidden expensive work in the renderer. The recommendation CLI is an opt-in tool for humans and tooling — the renderer just emits a Markdown link to the policy file and an inspection command (check-router-policy.mjs, the read-only checker). Audit confirmed: the only mention of recommend-task-backend.mjs in render-claudecode-task.mjs is in the helper's comment that explicitly forbids the shell-out (line 311). No child_process, spawn, execSync, fork, or fetch imports added.")
     (:decision "Section text contains the exact contract literals 'advisory' and 'dry-run only'"
      :rationale "The brief required these two strings verbatim. Placed both in the same bullet (line 106) so a single grep can verify both: '- This section is **advisory** and **dry-run only**.' The header literal 'advisory' also appears in '## Router Policy (advisory)' so a future renderer-validation pass can grep for either occurrence.")]

  :unexpected_work []

  :notes
    "Renderer change shape (additive only):
1. New field on the parsed task object: routerPolicyPath = keywordPropText(props, ':router-policy-path') ?? null. Set inside loadSingleTask alongside sessionTraceWritable. Default null when the field is absent — the existing 66 task contracts in the repo do not declare it and remain byte-identical when re-rendered.
2. New resolver: resolveRouterPolicyPath(task) — checks task.routerPolicyPath first (if set), then falls back to .missiond/router/router-policy-v1.lisp. Returns the first path that exists on disk via fs.existsSync, or null. Mirrors resolveSessionTracePath exactly (auto-detect by file existence, never silent failure when explicit field points at a missing file — a future wave can elect to make that strict, but for v0 we match the wave23-02 pattern).
3. New helper: renderRouterPolicy(lines, task, routerPolicyPath) — emits a 4-bullet section + a single fenced bash block. The function mutates `lines` in place to match the existing helpers (renderSharedMemory / renderReportContract / renderSessionTrace). Section header is '## Router Policy (advisory)'. The first bullet contains both contract literals ('advisory' and 'dry-run only') and explicitly states 'runtime dispatch is unchanged — ClaudeCode remains the live backend for this task'. The second bullet clarifies the section is purely informational and never instructs the worker to switch backend. The third bullet records whether the source was explicit (:router-policy-path) or auto-detected (default seed) — small operator-debug aid. The fenced block is `node scripts/check-router-policy.mjs <path>` (the wave24-01 read-only checker), NOT scripts/recommend-task-backend.mjs.
4. Wired in renderTask after renderSessionTrace and BEFORE the Commit section, gated on `if (routerPolicyPath)` so absent files cleanly omit the section.

Schema change shape (additive only):
1. New field declaration in field-contract: (:router-policy-path 'OPTIONAL ... default absent ... MUST contain literal advisory and dry-run only ... MUST NOT instruct worker to change backend ... renderer never shells out to the recommendation CLI'). Sits alongside :session-trace-writable; required-task-fields list is unchanged because the field is OPTIONAL.
2. New entry in renderer-contract :machine-context-rendered describing the section behavior (placement between Session Trace and Commit; auto-detect precedence; literal-text guarantees; no-shell-out invariant).
3. New entry in renderer-contract :backward-compatibility describing the additive-only nature.
4. Updated top-level :status string to mention wave24-05 alongside wave22-01 and wave23-02.

Hard rules honored:
- Used Edit (not Write) for both files; the wave23-02 Session Trace section in render-claudecode-task.mjs is byte-identical post-edit (verified by sed-comparing lines 87-99 of the stored brief vs a fresh stdout render).
- DID NOT touch any wave23 brief (.missiond/claudecode/wave23-*.md). Re-rendering them is not in scope.
- DID NOT regenerate any wave24 brief or contract. The acceptance command renders to /tmp/ exactly to avoid this.
- DID NOT add unused imports / parameters. The new helper signature is renderRouterPolicy(lines, task, routerPolicyPath) — `task` is used to surface the explicit-vs-auto-detected distinction in the third bullet, so all three params are referenced.
- DID NOT shell out to recommend-task-backend.mjs. Audit: grep -nE 'child_process|spawn|execSync|fork|recommend-task-backend' scripts/render-claudecode-task.mjs returns ONE hit at line 311, which is a comment explicitly forbidding shell-out.
- A parallel agent (wave24-04) is editing crates/missiond-daemon/src/handlers/knowledge/plan.rs and crates/missiond-mcp/src/tools/knowledge/plan.rs. No overlap — my write-scope is scripts/render-claudecode-task.mjs and .missiond/tasks/schema/task-contract-v1.lisp.
- Shared-memory + session-trace appended-to before each ledger op (re-read after the wave24-04 agent's start event interleaved with my seq 14 entry; appended my commit/complete events with seq 16/17 AFTER their seq 15 start event without disturbing it).
- DID NOT push, --no-verify, --amend, --force, git add -A. Staged the two write-scope paths explicitly by name.

Why Markdown remains non-load-bearing:
The Lisp task contract (.missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp) is the SSOT — it carries :write-scope, :must-not-touch, :acceptance, :commit, and the new optional :router-policy-path. The rendered Markdown is a human-friendly view of that contract; nothing in MissionD reads the Markdown to make a decision. The new Router Policy (advisory) section explicitly states 'runtime dispatch is unchanged' and the renderer never shells out to the recommendation CLI — so even if a worker reads only the Markdown, they will not interpret it as a dispatch instruction. The contract field name (:router-policy-path) and its constraints (literal 'advisory' + literal 'dry-run only' + no backend-switch instruction + no shell-out from renderer) are documented in the schema, not in the rendered output.

Constraints honored: did NOT touch crates/**; did NOT touch .missiond/v2/**; did NOT touch .missiond/tasks/wave23/**; did NOT touch other wave24 contracts (.missiond/tasks/wave24/wave24-*.lisp); did NOT touch .missiond/claudecode/wave23-*.md. The wave24 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked for the next wave's archive task per the established protocol.")

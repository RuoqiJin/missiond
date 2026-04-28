;; Wave 25 / Task 04 — Renderer router recommendation command v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp

(report wave25-04-renderer-router-recommendation-command-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave25-04-renderer-router-recommendation-command-v1"
  :status done
  :commit_hash "e1fdbe4a68d0"
  :files_changed
    [".missiond/tasks/schema/task-contract-v1.lisp"
     "scripts/render-claudecode-task.mjs"]

  :acceptance_results
    [(:command "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp > /tmp/wave25-render-check.md"
      :exit_code 0
      :ok true
      :notes "Renderer wrote the brief to /tmp/wave25-render-check.md without error. The file does not overwrite the committed wave25-01 brief under .missiond/claudecode/ (out of write-scope by contract).")
     (:command "rg -n \"Router Policy|advisory|dry-run only|recommend-task-backend|check-router-policy\" /tmp/wave25-render-check.md"
      :exit_code 0
      :ok true
      :notes "All 5 patterns present. Hits: Router Policy at line 114 (section heading); advisory at lines 84/114/118/128 (Report Contract router-fields preamble + section heading + opening bullet + recommend-CLI preamble); dry-run only at lines 84/118/128; recommend-task-backend at lines 29 (Must Not Touch) and 131 (the new rendered command); check-router-policy at lines 31 (Must Not Touch), 52 (Acceptance Commands the wave25-01 contract itself declares), and 125 (the existing wave24-05 rendered command).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (74 tasks). The schema additions to .missiond/tasks/schema/task-contract-v1.lisp parse cleanly; no other contract perturbed.")
     (:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (22). All 22 wave25-02 fixtures stay byte-identically green — wave25-04 did not touch scripts/check-task-report.mjs, the report-contract schema, or any fixture.")
     (:command "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "Clean. No trailing whitespace, mixed tabs/spaces, or conflict markers in either staged file.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set; .githooks/pre-commit exists and is executable; no install needed.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-04-renderer-router-recommendation-command-v1 (2 staged file(s)). Both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** scripts/check-task-report.mjs scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs .missiond/tasks/schema/report-contract-v1.lisp .missiond/v2/** .missiond/tasks/wave24/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against e1fdbe4a68d0 — commit hash exists; commit subject exactly matches contract :commit.message ('feat(tasks): render router recommendation commands'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave25-04-trace-start-001 wave25-04-trace-commit-001 wave25-04-trace-complete-001]

  :recommended_backend "claudecode"
  :router_confidence "low"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons ["dispatch-strategy:fresh-code-alignment"
                   "owner:claudecode"
                   "kind:code-alignment"
                   "fallback:insufficient_trace_history"]

  :major_decisions
    [(:decision "Threaded relSource as a 4th argument to renderRouterPolicy() rather than re-deriving the task source path inside the helper"
      :rationale "renderTask() already computes relSource = path.relative(process.cwd(), sourcePath) at the top of the function; passing it down avoids a second path computation and keeps the helper pure (all inputs explicit). The helper now takes (lines, task, routerPolicyPath, relSource) — same caller, additive parameter, no other callers exist."
      :trace_ref "wave25-04-trace-start-001")
     (:decision "Did NOT add a new optional contract field for the recommendation command surface"
      :rationale "Everything the renderer needs is already on the contract: :router-policy-path resolves the policy, and the task source path is what the renderer is already processing. Adding a new field would expand the schema for zero behaviour — strictly worse. Documented this choice in the task-contract-v1.lisp :router-policy-path entry: 'wave25-04: ... derived from existing :router-policy-path and the task source path'."
      :trace_ref "wave25-04-trace-start-001")
     (:decision "Appended the new fenced block AFTER the existing check-router-policy block (not before)"
      :rationale "Reading order matches the task brief's recommendation: check the policy file FIRST (cheap, validates schema), then run the recommendation CLI (needs the policy + a task contract path). Putting the recommend command second also preserves the wave24-05 byte-shape of the first half of the section (anyone diffing against the prior committed brief sees the prior content unchanged, with new content strictly appended)."
      :trace_ref "wave25-04-trace-commit-001")
     (:decision "Used MAY-language ('You **may** also inspect ...') rather than MUST or SHOULD for the recommend-task-backend command preamble"
      :rationale "The recommendation is advisory; making the command MUST-level would contradict the section's 'advisory' / 'dry-run only' guarantees. The preamble explicitly re-asserts 'advisory' + 'dry-run only' + 'MUST NOT switch backend on the strength of its output' so a worker reading the brief cannot misread the command as a dispatch instruction."
      :trace_ref "wave25-04-trace-commit-001")
     (:decision "Added the report-contract router-fields note as a single bullet group with 7 nested bullets (one per field) rather than a paragraph"
      :rationale "Mirrors the existing optional-fields bullet group (:time_sinks / :major_decisions / :unexpected_work / :blockers / :trace_refs) right above it — same visual rhythm, easy to scan. The group preamble is the MAY-language guard: 'populate ONLY when you observe a dry-run recommendation; the recommendation is advisory and dry-run only, never authoritative for dispatch'. Each nested bullet carries the field's type/enum constraint so a worker drafting a report can validate locally without opening the schema file."
      :trace_ref "wave25-04-trace-commit-001")]

  :unexpected_work
    [(:summary "Confirmed no shell-out was introduced. After the edits, ran `grep -nE 'child_process|spawn|exec|execSync|spawnSync' scripts/render-claudecode-task.mjs` and got only literal text-string matches inside the new advisory bullets ('the renderer never executes it', 'shell out to the recommendation CLI', etc.). No `import { spawn } from 'child_process'` or similar — the renderer's import block remains pure (fs + path + ./lib/missiond_lisp.mjs)."
      :trace_ref "wave25-04-trace-commit-001")
     (:summary "Verified Session Trace section byte-identity by diffing the freshly rendered /tmp output's Session Trace section against the previously committed wave25-01 brief. `diff <(sed -n '/^## Session Trace/,/^## Router Policy/p' .missiond/claudecode/wave25-01-router-policy-corpus-evaluator-v0.md) <(sed -n '/^## Session Trace/,/^## Router Policy/p' /tmp/wave25-render-check.md)` exits 0 — the wave23-02 Session Trace block is unchanged."
      :trace_ref "wave25-04-trace-commit-001")]

  :notes
    "Renderer changes (scripts/render-claudecode-task.mjs):

1. renderRouterPolicy() signature grew from (lines, task, routerPolicyPath) to (lines, task, routerPolicyPath, relSource). The single call site in renderTask() was updated to pass relSource, which is already computed at the top of renderTask as `path.relative(process.cwd(), sourcePath)`. No other call sites exist — the helper is module-private.

2. Inside renderRouterPolicy(), the existing check-router-policy fenced block stays byte-identical (same preamble, same command). After the closing ``` and the trailing empty line, the helper now appends:
   - A one-line MAY-language preamble: 'You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:'
   - A second fenced block: `node scripts/recommend-task-backend.mjs --task <relSource> --policy <routerPolicyPath> --json` — verbatim, parameterized by the relSource and routerPolicyPath the renderer already has in scope.
   - A trailing empty line (matches the section's existing trailing-newline convention).

3. renderReportContract() got a new bullet group appended AFTER the existing :time_sinks / :major_decisions / :unexpected_work / :blockers / :trace_refs bullet group and BEFORE the 'Validate with:' line. The group is:
   - '- Optional router-recommendation fields (wave25-02 — populate ONLY when you observe a dry-run recommendation; the recommendation is **advisory** and **dry-run only**, never authoritative for dispatch):'
   - 7 nested bullets, one per wave25-02 optional field, with their type/enum constraint:
     :recommended_backend (string enum: claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker)
     :router_confidence (string enum: high | medium | low)
     :router_policy_path (repo-relative path)
     :router_dry_run_only (literal true, cross-wave invariant)
     :router_applied (literal false, cross-wave invariant — runtime replacement is rejected)
     :router_reasons (vector of non-empty strings)
     :router_trace_index_path (repo-relative path)

Verbatim sample of the 2 commands as rendered when the renderer processes wave25-01:
  node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp
  node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp --policy .missiond/router/router-policy-v1.lisp --json

Schema changes (.missiond/tasks/schema/task-contract-v1.lisp):

1. Top-level :status string: appended a wave25-04 sentence describing the new commands + report-fields note + the 'recommendation stays advisory + renderer never shells out' invariant.
2. field-contract :router-policy-path entry: appended a wave25-04 sentence describing the two new commands rendered as text and the 'parameterized by the resolved policy path AND the task source path' detail.
3. renderer-contract :machine-context-rendered list: added 2 entries:
   - 'router-policy commands (wave25-04): ...' describing the two new commands.
   - 'report-contract router-fields note (wave25-04): ...' describing the MAY-language note and the 7 field names.
4. renderer-contract :backward-compatibility list: added 2 entries:
   - 'router-policy commands (wave25-04) extend the existing Router Policy (advisory) section in place ...'
   - 'report-contract router-fields note (wave25-04) extends the existing Report Contract section in place ...'

NO new contract field added. The renderer derives the recommend-task-backend command's --task argument from sourcePath (which the renderer is already processing) and the --policy argument from the same routerPolicyPath the wave24-05 path resolution already computes. Adding a new field would have expanded the schema for zero observable benefit.

Backward compatibility verification:

- wave23-02 Session Trace section is byte-identical: `diff <(sed -n '/^## Session Trace/,/^## Router Policy/p' .missiond/claudecode/wave25-01-router-policy-corpus-evaluator-v0.md) <(sed -n '/^## Session Trace/,/^## Router Policy/p' /tmp/wave25-render-check.md)` exits 0.
- wave24-05 Router Policy section: the literal words 'advisory' and 'dry-run only' are preserved at their existing positions (line 118 in the rendered output, plus three new occurrences at lines 84 (Report Contract router-fields preamble) and 128 (recommend-CLI preamble)).
- Tasks WITHOUT a resolvable router policy continue to render byte-identical: the renderRouterPolicy() helper is gated on `if (routerPolicyPath)` in renderTask() — when no policy resolves, neither the wave24-05 section NOR the wave25-04 commands appear, byte-identically to wave23 output. (In practice the seed at .missiond/router/router-policy-v1.lisp always exists, so the section currently always renders; the gate is the architectural backward-compat guarantee, not a runtime branch.)
- Tasks WITHOUT a derivable wave id (no Report Contract section emitted): the new renderReportContract bullet group is local to that helper, so when reportContractPath is null the helper isn't called at all and no new content appears.
- The 22 wave25-02 dry-fixtures stay byte-identically green because wave25-04 did not touch scripts/check-task-report.mjs, the report-contract schema, or any fixture.

No shell-out introduced (verified):

- `grep -nE 'child_process|spawn|exec|execSync|spawnSync' scripts/render-claudecode-task.mjs` returns only literal text-string matches inside the advisory bullets ('the renderer never executes it', 'shell out to the recommendation CLI', etc.). No actual import or invocation.
- The renderer's import block remains: fs, path, ./lib/missiond_lisp.mjs. No new imports of node:child_process.
- The recommend-task-backend command line is emitted as Markdown text inside a ```bash fenced block; the rendering function pushes it to the lines array, never executes it.

Constraints honored: did NOT touch crates/** (wave25-03 owns the daemon side); did NOT touch scripts/check-task-report.mjs (wave25-02 just landed there at 770903136f00); did NOT touch scripts/recommend-task-backend.mjs (wave24-03/06); did NOT touch scripts/evaluate-router-policy-corpus.mjs (wave25-01 just landed at 8dbe85fa1a0c); did NOT touch .missiond/tasks/schema/report-contract-v1.lisp (wave25-02 owns it); did NOT touch .missiond/v2/** (intent-event-bus.lisp is locked); did NOT touch .missiond/tasks/wave24/** or .missiond/claudecode/** (wave23/24 briefs are out of scope; the test render targets /tmp); did NOT regenerate any wave25 brief (the test render targets /tmp). NO shell-out / spawn / git invocation from the renderer. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")

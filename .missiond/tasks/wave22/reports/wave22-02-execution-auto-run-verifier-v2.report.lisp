;; Wave 22 / Task 02 — Execution auto task-run verifier v2.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-02-execution-auto-run-verifier-v2.lisp

(report wave22-02-execution-auto-run-verifier-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave22-02-execution-auto-run-verifier-v2"
  :status done
  :commit_hash "02ac6278e886"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "116 passed; 0 failed; 0 ignored. Baseline before this task: 108. +8 new wave22-02 tests covering auto_run_task_run_verifier happy path, every new structured-error code (SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK), shared-memory projector field extraction, vocabulary reuse with the wave21-03 gate (TASK_REPORT_TASK_ID_MISMATCH still surfaces), and short<->long sha prefix overlap. Every wave21-03 verified_rejects_* / verified_accepts_* / smoke_wave21_* test still green — backward compat for the directly-driven enforce_verified_completion helper preserved.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1462 passed; 0 failed; 0 ignored. Baseline before this task: 1454. +8 new wave22-02 tests; no other tests touched.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; 0 ignored. MCP schema description for `verified` field updated to advertise the wave22-02 auto-verifier behavior + downgrade-to-legacy-claim semantics; the test_mission_execution_action_enum / test_all_tools_count / test_directive_actions_match_lisp invariants stay green because the action enum and tool count are unchanged.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 85 pre-existing warnings unchanged (struct ConversationService never constructed, field minimax never read, function get_task_jsonl_path never used, et al.). No new warnings introduced by the wave22-02 changes.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract; this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across both in-scope files; no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-02 lifts the wave21-03 caller-supplied verified=true escape hatch into a daemon-side in-process auto task-run verifier. Commit 02ac6278e886 (2 files, +982/-37). Post-commit verify-task-contract.mjs OK.

Daemon-side surface (crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs):

1. auto_run_task_run_verifier(project_root, task_contract_path, task_report_path, shared_memory_path, commit_hash) — new in-process verifier that reuses workstation_dispatch::load_task_contract for contract loading, the wave21-03 read_report_summary projector for the report fields, and a brand-new read_shared_memory_ledger projector for the ledger. It performs eight cross-checks in order: (a) task contract loadable + head id extractable, (b) task report loadable, (c) report :schema = missiond.report-contract.v1, (d) report :task_id == contract head id, (e) report :commit_hash matches the supplied commit_hash via full-string equality OR prefix overlap (mirrors the wave21-03 short<->long sha rule), (f) shared-memory ledger loadable, (g) ledger :schema = missiond.shared-memory.v1, (h) ledger contains a (completion :task <contract-id> ...) entry. Returns a structured Value summary on success containing :verifier_status, :task_id, :task_contract_path, :task_contract_resolved_path, :task_report_path, :task_report_resolved_path, :shared_memory_path, :shared_memory_resolved_path, :commit_hash, :checks (the eight rule names).

2. read_shared_memory_ledger(text) — new mini-projector returning SharedMemorySummary { schema, completion_tasks }. Walks the (shared-memory <wave> ...) form's children, harvests the :schema keyword value plus every :task value found inside (completion ...) sub-forms. Mirrors the wave21-02 scripts/verify-task-run.mjs loadLedger projection so the daemon and the script reach identical verdicts on the same file.

3. read_completion_task_id(node) — helper that pulls the :task slot off a (completion :id ... :task <id> ...) form. Entries without a :task slot are silently skipped because the wave21-02 script-side verifier uses the same matching rule.

4. action_complete dispatch rewrite — the wave21-03 verified-completion gate (enforce_verified_completion) is no longer called from action_complete. The new dispatch logic:
   - When all four of task_contract_path, task_report_path, shared_memory_path, and commit_hash are Some, action_complete invokes auto_run_task_run_verifier and on success sets verification_source=daemon-auto-verifier, auto_verifier_status=passed, and stores the structured summary as the verified_validation Value (renamed in the response surface to verified_scope_summary). On failure the structured ToolResult error is returned BEFORE any companion-log mutation.
   - Otherwise, when verified_flag == Some(true), action_complete builds a missing-paths list and sets verification_source=legacy-caller-claim, auto_verifier_status=unknown, and an auto_verifier_diagnostics prose blob explaining which path(s) were missing so the writer agent can migrate the next dispatch envelope. NO hard reject — backward compat with wave21-03 callers is preserved exactly as the brief required.
   - When neither path triggers, no verification metadata is added — legacy completions stay byte-identical to the wave21-03 / wave19-08 / wave12-01 shapes.

5. Response surface additions (action_complete only — no schema or wire changes elsewhere):
   - verification_source: 'daemon-auto-verifier' | 'legacy-caller-claim' (absent for legacy completions)
   - verifier_status: daemon-computed verdict ('passed' for auto-verifier; 'unknown' for legacy claim) — overrides the wave19-08 caller-supplied verifier_status in the response when set, but the caller-supplied label still persists in the companion log so audit can distinguish caller-asserted vs. daemon-computed values.
   - verifier_diagnostics: daemon-computed diagnostic prose — overrides the wave21-03 caller-supplied verifier_diagnostics in the response when set, same persist-vs-surface split as verifier_status.
   - verified_scope_summary: the structured wave22-02 auto-verifier summary (the eight :checks rules + every resolved path). Also exposed under the legacy verified_validation key so wave21-03 dashboards keep parsing.

6. Backward-compat helper preservation — enforce_verified_completion / enforce_task_contract_completion / enforce_scoped_commit_completion / read_report_summary / read_task_contract_id stay verbatim. action_complete still calls enforce_scoped_commit_completion + enforce_task_contract_completion when their respective triggers fire (enforce_scoped_commit=true on its own and paired with task_contract_path, respectively). The wave22-02 auto-verifier runs alongside those gates rather than replacing them — the three layers compose: (i) scoped-commit enforcement covers staged_files vs. claim scopes; (ii) wave19-08 task-contract enforcement covers :write-scope coverage; (iii) wave22-02 auto-verifier covers report + shared-memory + commit_hash cross-checks.

7. Read-only proof — daemon never spawns Node, never invokes shell scripts, never runs mutating git commands. The single Command::new(\"git\") site preserved (the wave18-08 status --porcelain=v1 read for action=preflight_commit) is still the only one in the file; the wave22-02 daemon_never_invokes_mutating_git test (line ~7691) keeps that invariant pinned. The wave22-02 auto-verifier uses std::fs::read_to_string for every input (contract, report, shared-memory ledger) — no spawn, no shell, no process boundary.

Three new structured-error codes (rejected BEFORE any file mutation):
  - SHARED_MEMORY_REQUIRED: shared_memory_path does not resolve to a readable file under the project root. Distinct from MALFORMED so the writer can tell 'wrong path' from 'wrong content' without re-running anything.
  - SHARED_MEMORY_MALFORMED: ledger parses but :schema is missing or != missiond.shared-memory.v1, OR the file has no (shared-memory ...) form. Suggestion points at scripts/check-task-memory.mjs.
  - SHARED_MEMORY_NO_COMPLETION_FOR_TASK: ledger is well-formed but contains no (completion :task <contract-id> ...) entry for the task. Mirrors the wave21-02 script-side check exactly so the daemon and the script verdict on the same file align.

Reused codes (single vocabulary across gates):
  - TASK_CONTRACT_REQUIRED / TASK_CONTRACT_MALFORMED (wave19-08)
  - TASK_REPORT_REQUIRED / TASK_REPORT_MALFORMED / TASK_REPORT_TASK_ID_MISMATCH / TASK_REPORT_COMMIT_HASH_MISMATCH (wave21-03)

MCP layer surface (crates/missiond-mcp/src/tools/knowledge/agent_execution.rs): the `verified` field property description rewritten to spell out the wave22-02 contract: when all four paths are supplied the daemon runs the in-tree auto-verifier and computes the verdict (verifier_status / verified_scope_summary / verification_source=daemon-auto-verifier); otherwise verified=true downgrades to verification_source=legacy-caller-claim with verifier_diagnostics listing the missing paths. The full mission_execution tool description string also updated to advertise the wave22-02 shift, the new structured-error codes, and the legacy-claim downgrade behavior. No action / schema field additions or removals — the wire shape stays compatible with wave21-03 callers.

Wave 22 protocol followed:
- Claim entry (wave22-02-claim-001, seq=6) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
- Edit-only on in-scope files (no Write); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 2 staged files (crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs + crates/missiond-mcp/src/tools/knowledge/agent_execution.rs), all inside :write-scope, zero touching :must-not-touch (crates/missiond-core/src/event/events/execution.rs / crates/missiond-daemon/src/handlers/knowledge/plan.rs / crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs / scripts/** / .missiond/v2/*.lisp).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 02ac6278e886 against the contract — message prefix, changed-file scope, must-not-touch intersection, acceptance command presence all green.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: did not touch crates/missiond-core/src/event/events/execution.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/plan.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset / git checkout / git mv / git rm / any other mutating git surface. Used Edit (and Write only for the report) — no Write to in-scope files.

Backward compat sanity (verified by running every wave21-03 test):
- verified_rejects_without_enforce_scoped_commit: still green (enforce_verified_completion called directly still rejects).
- verified_rejects_missing_task_contract_path / verified_rejects_missing_task_report_path / verified_rejects_missing_commit_hash: still green for the same reason.
- verified_rejects_missing_report_file / verified_rejects_report_schema_mismatch / verified_rejects_report_task_id_mismatch / verified_rejects_report_commit_hash_mismatch: still green.
- verified_accepts_aligned_report / verified_accepts_short_long_sha_prefix_overlap: still green.
- smoke_wave21_*: every smoke (machine_contract_autonomous_loop_verifier_accepts_aligned_triple, malformed_report_task_id_yields_structured_failure, malformed_task_contract_yields_structured_failure, missing_report_yields_structured_failure, mismatched_commit_hash_yields_structured_failure, report_and_contract_projectors_extract_required_fields) still green.

The wave21-03 enforce_verified_completion helper is preserved verbatim — only action_complete's dispatch is rewritten to bypass it in favor of the wave22-02 auto-verifier when the four paths are present, with legacy claim downgrade when they are not.")

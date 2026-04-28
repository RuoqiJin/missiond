;; Wave 29 / Task 01 — Context atlas schema v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp

(report wave29-01-context-atlas-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave29-01-context-atlas-schema-v0"
  :status done
  :commit_hash "1a5b01f9c292"
  :files_changed
    [".missiond/tasks/schema/context-atlas-v1.lisp"
     "scripts/check-context-atlas.mjs"]

  :acceptance_results
    [(:command "node scripts/check-context-atlas.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "context-atlas fixtures OK (21 cases, 13 categories): pass (3: minimal valid, mixed containers + flat entries, dispatch.v0 schema accepted), fail-schema (1), fail-missing-required (2: read-order, purpose+goal both absent), fail-empty-read-order (1), fail-path (3: absolute :read-order, traversal in (file ...) path, ~ in :first-reads), fail-duplicate (2: duplicate file path across nested+flat, duplicate task id across container+flat), fail-empty-grep (2: empty (file ...) :grep anchor, empty top-level (grep ...) pattern), fail-malformed-read-order (1: :read-order is a string), fail-multiple-atlases (1), fail-unknown-entry-head (1), fail-container-child (2: global-anchors with task child, task-focus with file child), fail-empty-atlas (1), fail-unknown-field (1). JSON variant verified separately (--json --dry-fixture) reports same totals.")
     (:command "node scripts/check-context-atlas.mjs .missiond/tasks/wave29/context-atlas.lisp"
      :exit_code 0
      :ok true
      :notes "Real-world smoke against the existing wave29 dispatch-time atlas: PASS. Schema accepts the dispatch.v0 schema string + :goal synonym + nested (global-anchors (file ...)) + (task-focus (task ...)) shape unchanged. Confirms forward-compatible adoption — future waves can migrate to v1 at their own pace without rewriting wave29 dispatch artifacts.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks) — all wave22..wave29 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions.")
     (:command "git diff --check -- .missiond/tasks/schema/context-atlas-v1.lisp scripts/check-context-atlas.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either path; trailing-newline / tab-stop hygiene clean on both NEW files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave29-01-context-atlas-schema-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** wave28/** wave29-*.lisp manifest dispatch-plan pattern-cards .missiond/claudecode/** sibling scripts).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-01-context-atlas-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave29-01-context-atlas-schema-v0 against 1a5b01f9c292 — commit hash exists; commit message matches `feat(tasks): add context atlas schema` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave29-01-trace-start-001
               wave29-01-trace-read-001
               wave29-01-trace-commit-001
               wave29-01-trace-complete-001]

  :major_decisions
    [(:decision "Schema accepts BOTH \"missiond.context-atlas.v1\" AND \"missiond.context-atlas.dispatch.v0\" via the ACCEPTED_SCHEMAS set."
      :rationale "The wave29 dispatch atlas declares the dispatch.v0 string and the brief mandates that the schema validate it without modification. Forking would either force rewriting the dispatch atlas (forbidden by must-not-touch) or accepting two slightly different shapes. Treating the strings as synonyms inside one schema keeps the data shape uniform and lets future waves migrate to v1 at their own pace.")
     (:decision "Required header is :schema + :read-order PLUS one-of {:purpose | :goal} (synonyms for the same field)."
      :rationale "The wave29 atlas uses :goal; the brief specifies :purpose. Accepting both as synonyms means the existing atlas validates clean and authors of new atlases can pick either spelling. Both are non-empty-string-validated; the projected object exposes both fields so renderers can render whichever is present.")
     (:decision "Top-level entries are either CONTAINER forms (global-anchors / task-focus) wrapping leaves OR direct LEAF forms (file / task_focus / grep / avoid)."
      :rationale "The wave29 atlas uses container shape exclusively; the brief lists flat top-level entries as the canonical shape. Supporting both keeps the schema permissive about layout while strict about leaf semantics. Container children are head-restricted: (global-anchors ...) accepts only (file ...) children; (task-focus ...) accepts only (task ...) children. This catches accidental shape drift.")
     (:decision "File path uniqueness is enforced ACROSS nested + flat shapes, not per-container."
      :rationale "Duplicate paths break byte-stable downstream output (wave29-03 prep CLI / wave29-06 ready-queue planner). Tracking a single seenFilePaths Map across (global-anchors (file ...) ...) and top-level (file ...) keeps the invariant clean even when authors mix shapes during migration.")
     (:decision "Atlas with zero entries is rejected as a structural error."
      :rationale "An empty atlas adds noise to the dispatch render with no signal. The point of an atlas is to surface anchors; if there are none, the atlas should be omitted entirely rather than declared empty.")
     (:decision "Multiple (context-atlas ...) forms in one file are rejected — atlases are single-document."
      :rationale "Mirrors the seed convention of wave24-01 / wave26-01 / wave28-01 (one schema instance per file). Keeps file-path → atlas mapping unambiguous for the prep / planner tooling that joins atlases to task contracts via :context-atlas-path.")
     (:decision "Dedicated collectEntries helper skips keyword/value pairs (including bracket-list values like :read-order [...]) when iterating list children for entries."
      :rationale "Initial implementation filtered children by isList(c), which inadvertently treated the bracket-list value of :read-order as an entry candidate (head() returned null because the first child was a string). The helper mirrors readKeywordProps's keyword-aware traversal so entry detection ignores keyword values regardless of whether they are bracket lists, paren lists, atoms, or strings.")
     (:decision "Named exports cover both projection (projectAtlas / readContextAtlasFile) AND object-side validation (validateAtlasObject), plus the SCHEMA / ATLAS_HEAD constants."
      :rationale "wave29-03 prep CLI emits atlas objects in-memory before serializing; wave29-06 ready-queue planner reads atlases from disk. A single import that runs the same enum / path / uniqueness rules as the on-disk validator eliminates duplication and keeps the checker as the single source of truth for atlas shape.")]

  :time_sinks
    [(:label "Reading background schemas + checkers (task-runner-manifest, router-backend-registry, shared-memory, session-trace)"
      :notes "Largest sink — task-runner-manifest-v1 was the closest analog (--stdin / --dry-fixture / projectX + readXFile + validateXObject pattern, gated main(), warnings vector in JSON shape). router-backend-registry showed the simpler enum-only checker shape. shared-memory + session-trace schemas were needed to author legitimate claim / completion / trace-event entries.")
     (:label "Reconciling brief shape (flat top-level file/task_focus/grep/avoid) vs wave29 atlas shape (nested global-anchors / task-focus containers, fields :goal + :read-order)"
      :notes "Brief mandates the schema validate the existing atlas unchanged. Resolved by accepting BOTH shapes (containers + flat) and BOTH header spellings (:purpose | :goal). Documented as :decision so wave29-03 / wave29-06 see the rationale.")
     (:label "Diagnosing the bracket-list-as-entry false negative"
      :notes "First dry-fixture run failed all 3 happy-path cases because (context-atlas ... :read-order [\"path\"] (file ...)) has the :read-order's bracket list with head=null in the entries-loop. Fixed by introducing collectEntries(parent, start) that mirrors readKeywordProps's keyword-aware skip. Added a fail-malformed-read-order fixture (read-order is a bare string, not a vector) to pin the failure mode in case the helper drifts.")]

  :unexpected_work
    [(:summary "Added a `task_focus` head as the top-level shorthand for one task entry; nested container uses `task` head. This matches the brief's flat-shape vocabulary while keeping the wave29 atlas's nested `(task ...)` head working unchanged. validateTaskEntry takes a sourceLabel parameter so error messages reference the actual head the author wrote.")
     (:summary "Added warnings[] channel to the JSON shape ({ ok, files, errors[], warnings[], atlases_validated }) for parity with wave28-01 even though no atlas rule currently produces a warning. Reserved for future wave29-03 / wave29-06 advisory checks (e.g. `(file ...) path not present on disk`).")
     (:summary "Added top-level form-head guard so (atlas ...) or any other non-(context-atlas ...) top-level form is rejected with `unknown top-level form head` instead of being silently skipped. Mirrors wave28-01's check-task-runner-manifest behavior.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Lisp schema + new Node.js checker, no Rust / SQL / cargo) is the canonical claudecode beat — verification_tier=local; no full workspace build required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave29-01 ships:
     - .missiond/tasks/schema/context-atlas-v1.lisp (schema id missiond.context-atlas.v1).
     - scripts/check-context-atlas.mjs (read-only checker; never shells out, never touches git / network / LLM).

     Schema head: (context-atlas <atlas-id> :schema :read-order [:purpose | :goal] [:wave :version :description :generated_at :generator] (entry ...) ...).
     Required header: :schema, :read-order. One-of required: :purpose | :goal (synonyms).
     Optional header: :wave :version :description :generated_at :generator.

     Top-level entry heads:
       global-anchors  — container; children MUST be (file ...) leaves.
       task-focus      — container; children MUST be (task ...) leaves.
       file            — flat per-file anchor entry (top-level shorthand).
       task_focus      — flat per-task entry (top-level shorthand for one task).
       grep            — top-level cross-task grep anchor.
       avoid           — top-level avoid note (no-go zone or noise warning).

     Entry contracts:
       (file <path> :purpose <string>? :grep [<string> ...]?) — path is repo-relative (no abs / ~ / ..); :purpose non-empty when present; :grep entries non-empty.
       (task / task_focus <task-id> :first-reads [<path> ...]? :avoid [<string> ...]? :purpose <string>?) — task-id matches ^[a-z0-9][a-z0-9._-]*$; :first-reads entries repo-relative; :avoid entries non-empty.
       (grep <pattern> :purpose <string>?) — pattern non-empty.
       (avoid <string> ...) — at least one non-empty string child.

     Accepted schema strings:
       missiond.context-atlas.v1            — durable per-task or per-wave atlas.
       missiond.context-atlas.dispatch.v0   — transitional dispatch-time atlas (wave29 dispatch atlas declares this string).

     Cross-atlas rules:
       1. Duplicate file path across nested + flat shapes inside one atlas = error.
       2. Duplicate task id across nested + flat shapes inside one atlas = error.
       3. Multiple (context-atlas ...) forms in one file = error.
       4. Atlas with zero entries = error.
       5. Container child whose head does not match (file for global-anchors, task for task-focus) = error.
       6. Unknown top-level entry head = error.

     Checker CLI: node scripts/check-context-atlas.mjs [--json] [--stdin] [--dry-fixture] [<file.lisp> ...].
     JSON shape: { ok, files, atlases_validated, errors[], warnings[] }.
     --stdin lets future tooling pipe an emitted atlas through the checker without a temp file.

     Dry-fixture totals: 21 cases across 13 categories — pass (3), fail-schema (1), fail-missing-required (2), fail-empty-read-order (1), fail-path (3), fail-duplicate (2), fail-empty-grep (2), fail-malformed-read-order (1), fail-multiple-atlases (1), fail-unknown-entry-head (1), fail-container-child (2), fail-empty-atlas (1), fail-unknown-field (1).

     Named exports for wave29-03 (prep CLI) / wave29-06 (ready-queue planner) import:
       SCHEMA, ACCEPTED_SCHEMAS, ATLAS_HEAD, CONTAINER_HEADS, TOP_LEAF_HEADS, ALL_TOP_HEADS,
       projectAtlas (form -> structured object with files / tasks / greps / avoids / read_order),
       readContextAtlasFile (file path -> array of projected atlases),
       validateAtlasObject (object -> array of error message strings; empty = valid).
     main() is gated on import.meta.url === `file://${process.argv[1]}` so importers do not accidentally trigger CLI side-effects.

     Real-world smoke: node scripts/check-context-atlas.mjs .missiond/tasks/wave29/context-atlas.lisp → context-atlas check OK (1 atlas). The schema validates the existing wave29 dispatch atlas WITHOUT modification, confirming forward-compatible adoption.

     Pre-commit pipeline: check-context-atlas.mjs --dry-fixture (exit=0, 21/21) → check-context-atlas.mjs against real wave29 atlas (exit=0) → check-task-contract.mjs --all (exit=0, 105 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (2 paths) → task-scope-guard.mjs --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): add context atlas schema\" (commit 1a5b01f9c292) → verify-task-contract.mjs (OK against 1a5b01f9c292). Append-only ledger updates: shared-memory wave29-01-claim-001 (seq 2) before staging + wave29-01-completion-001 (seq 5) after verify; session-trace wave29-01-trace-start-001 (seq 2) + wave29-01-trace-read-001 (seq 3, shared preamble + atlas + pattern-cards + task contract) + wave29-01-trace-commit-001 (seq 8, with commit hash) + wave29-01-trace-complete-001 (seq 9). Both ledgers re-validated after each append. Note: parallel agents (wave29-02, wave29-04) appended their own claims/traces in between; sequence numbers were re-read fresh before each append to honor the wave29 protocol.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/wave28/**, any wave29-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave29/manifest.lisp, .missiond/tasks/wave29/dispatch-plan.lisp, .missiond/tasks/wave29/pattern-cards.lisp, .missiond/claudecode/**, scripts/check-pattern-card.mjs, scripts/prepare-task-runner-wave.mjs, scripts/check-task-report.mjs, scripts/verify-task-run.mjs, scripts/verify-task-runner-batch.mjs, scripts/plan-task-runner.mjs. Did not git add . / git push / --no-verify / --amend / --force.")

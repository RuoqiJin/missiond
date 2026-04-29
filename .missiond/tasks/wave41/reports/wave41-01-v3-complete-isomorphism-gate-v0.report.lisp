;; Wave 41 task report.
;; Schema: missiond.report-contract.v1

(report wave41-01-v3-complete-isomorphism-gate-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave41-01-v3-complete-isomorphism-gate-v0"
  :status done
  :commit_hash "6e1e247506c7"
  :files_changed
    [".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-code-isomorphism-complete.mjs"
     "scripts/check-v3-intent-alignment-isomorphism.mjs"
     "scripts/check-v3-plan-execution-isomorphism.mjs"
     "scripts/check-v3-request-lisp-isomorphism.mjs"
     "scripts/check-v3-task-lifecycle-isomorphism.mjs"
     "scripts/check-v3-workflow-isomorphism.mjs"
     "scripts/check-v3-workstation-config-isomorphism.mjs"]
  :acceptance_results
    [(:command "node scripts/check-v3-code-isomorphism-complete.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "5 cases: good (all six surfaces code-aligned + aggregate command pinned), partial-status, missing-surface, missing-note, missing-aggregate-command. Each fail-case is matched by a specific diagnostic regex.")
     (:command "node scripts/check-v3-code-isomorphism-complete.mjs"
              :exit_code 0 :ok true
              :note "Live run: 6 graduated surfaces validated + 6 per-surface V3 checkers spawned and passed.")
     (:command "node scripts/check-v3-request-lisp-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture extended to include implementation-map (surface mission_request :status code-aligned :note fixture).")
     (:command "node scripts/check-v3-request-lisp-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run pins (surface mission_request) and :status code-aligned alongside the existing materialization-rule needles.")
     (:command "node scripts/check-v3-intent-alignment-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture switched to :status code-aligned for mission_directive.")
     (:command "node scripts/check-v3-intent-alignment-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run now pins :status code-aligned for mission_directive.")
     (:command "node scripts/check-v3-plan-execution-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture switched to :status code-aligned for mission_plan.")
     (:command "node scripts/check-v3-plan-execution-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run now pins :status code-aligned for mission_plan.")
     (:command "node scripts/check-v3-workflow-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture switched to :status code-aligned for mission_workflow.")
     (:command "node scripts/check-v3-workflow-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run now pins :status code-aligned for mission_workflow.")
     (:command "node scripts/check-v3-task-lifecycle-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture switched to :status code-aligned for task-runner-cli.")
     (:command "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run now pins :status code-aligned for task-runner-cli.")
     (:command "node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture extended with implementation-map (surface workstation-config :status code-aligned :note fixture).")
     (:command "node scripts/check-v3-workstation-config-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live run now pins (surface workstation-config) and :status code-aligned alongside the existing invariant needles.")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
              :exit_code 0 :ok true
              :note "v1 manifest + v3 blueprint compression contract still holds after graduating six surface :status atoms and adding the aggregate command to compression-contract :checks.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
              :exit_code 0 :ok true
              :note "blueprint architecture-lisp check OK on the updated file.")
     (:command "perl -ne 'exit 1 if /\\x00/' .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs"
              :exit_code 0 :ok true
              :note "no NUL bytes in any of the nine touched files.")
     (:command "git diff --check -- .missiond/v3/missiond-blueprint.lisp scripts/check-v3-code-isomorphism-complete.mjs scripts/check-v3-request-lisp-isomorphism.mjs scripts/check-v3-intent-alignment-isomorphism.mjs scripts/check-v3-plan-execution-isomorphism.mjs scripts/check-v3-workflow-isomorphism.mjs scripts/check-v3-task-lifecycle-isomorphism.mjs scripts/check-v3-workstation-config-isomorphism.mjs scripts/check-lisp-blueprint-compression.mjs"
              :exit_code 0 :ok true
              :note "no whitespace-error or conflict markers in the write-scope files.")]
  :notes "Closes the V3 implementation-map graduation gap left after waves 31-40. The six implementation-map surfaces (mission_request, mission_directive, mission_plan, mission_workflow, task-runner-cli, workstation-config) all had passing live per-surface isomorphism checkers but the blueprint still labelled them :status code-aligned-partial. Without an aggregate gate a future regression to partial would silently slip through.\n\nWhich implementation-map surfaces were graduated and why: all six. Each graduation was justified by the existing live per-surface checker passing on HEAD: mission_request (check-v3-request-lisp-isomorphism — directive enrichment / request-local materialization / unified-entry response-rule), mission_directive (check-v3-intent-alignment-isomorphism — directive-draft + ArtifactKind::IntentAlignment + sonnet sexp-head allowlist), mission_plan (check-v3-plan-execution-isomorphism — plan-draft scaffold + plan-id/version/board-task-id materialization + DAG execution lisp-hint forwarding), mission_workflow (check-v3-workflow-isomorphism — distill + compile_methodology v3 artifact projection through render_workflow_artifact_sexp), task-runner-cli (check-v3-task-lifecycle-isomorphism — wave39-01 task-scoped one-event files + wave40-01 sparse-projection finalizer + parent-hotfix planner + verification-receipt projection + batch verifier), workstation-config (check-v3-workstation-config-isomorphism — coding-default-opus-4-7 model projection + MISSION_IPC_ENDPOINT slot env + autopilot pty.send budget projection + dispatch-guard ownership). No surface was graduated speculatively.\n\nAggregate checker contract and dry-fixture cases: scripts/check-v3-code-isomorphism-complete.mjs is read-only, deterministic, supports --json / --dry-fixture / --blueprint / --repo. validateBlueprintSource(source) extracts every (surface ...) form from the implementation-map, asserts that each EXPECTED_SURFACES entry exists, that none carry :status code-aligned-partial, that each declares :status code-aligned + :code [...] + :note, that no other surface in the map is partial, and that compression-contract :checks includes the literal aggregate command 'node scripts/check-v3-code-isomorphism-complete.mjs'. The live mode then spawns every entry of PER_SURFACE_CHECKERS via spawnSync (no shell, 60s timeout, never invoking itself) and aggregates exit codes plus stderr/stdout tails. The --dry-fixture mode runs five fixtures: (1) good (all six code-aligned + aggregate command pinned) -> ok; (2) partial-status (task-runner-cli still partial) -> fail with /code-aligned-partial/; (3) missing-surface (task-runner-cli absent) -> fail with /missing required surface task-runner-cli/; (4) missing-note (workstation-config without :note) -> fail with /must declare :note/; (5) missing-aggregate-command (compression-contract :checks []) -> fail with /compression-contract :checks must include/. Each fail-case asserts a specific diagnostic regex so future contract drift can't be silently swallowed.\n\nPer-surface checker updates: check-v3-intent-alignment-isomorphism, check-v3-plan-execution-isomorphism, check-v3-workflow-isomorphism, check-v3-task-lifecycle-isomorphism switched both their live needle ('code-aligned-partial' -> ':status \"code-aligned\"') and their dry-fixture blueprint stub. check-v3-request-lisp-isomorphism and check-v3-workstation-config-isomorphism never pinned the surface status before; both now pin (surface mission_request) / (surface workstation-config) plus :status \"code-aligned\" via additional requireText/requireAll needles, and their dry fixtures grow an implementation-map block carrying the same surface so the dry mode still passes. .missiond/v3/missiond-blueprint.lisp's six :status atoms graduate to code-aligned and the compression-contract :checks list grows a single 'node scripts/check-v3-code-isomorphism-complete.mjs' entry.\n\nAcceptance command results: every command listed in the task contract exits 0; see :acceptance_results above for per-command notes. The aggregate gate's live mode also confirms the six per-surface checkers still pass against HEAD."
  :verification_tier local)

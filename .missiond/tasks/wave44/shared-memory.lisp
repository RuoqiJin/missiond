;; Wave 44 shared-memory ledger.

(shared-memory wave44
  :schema "missiond.shared-memory.v1"
  :wave wave44
  :created-at "2026-04-29T04:08:08Z"
  :sequence 4

  (observation
    :id wave44-bootstrap-001
    :task wave44-01-request-local-artifact-roots-v0
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-29T04:08:08Z"
    :touched [".missiond/tasks/wave44/manifest.lisp"
              ".missiond/tasks/wave44/wave44-01-request-local-artifact-roots-v0.lisp"
              ".missiond/tasks/wave44/context-atlas.lisp"
              ".missiond/tasks/wave44/pattern-cards.lisp"]
    :summary "Wave44 prepared by Codex parent: make mission_request default live artifacts request-local only, with legacy compatibility writers explicit opt-in.")

  (observation
    :id wave44-bootstrap-002
    :task wave44-bootstrap
    :agent prepare-task-runner-wave
    :seq 2
    :at "2026-04-29T04:10:38Z"
    :touched [".missiond/claudecode/wave44-shared-preamble.md"]
    :summary "wave prepared by prepare-task-runner-wave.mjs — briefs + report skeletons + preamble-read audit expectation seeded.")

  (claim
    :id wave44-01-claim-001
    :task wave44-01-request-local-artifact-roots-v0
    :agent claudecode-worker
    :seq 3
    :at "2026-04-29T04:30:00Z"
    :summary "Claim wave44-01-request-local-artifact-roots-v0: make request-local artifacts the V3 default; legacy alignment/<topic>/ + plans/<id>/ writers become explicit opt-in (compat_write_file).")

  (completion
    :id wave44-01-completion-001
    :task wave44-01-request-local-artifact-roots-v0
    :agent claudecode-worker
    :seq 4
    :at "2026-04-29T05:00:00Z"
    :commit_hash "c9dfe3b57a5e"
    :touched [".missiond/v3/missiond-blueprint.lisp"
              "crates/missiond-daemon/src/handlers/knowledge/request.rs"
              "crates/missiond-mcp/src/tools/knowledge/request.rs"
              "scripts/check-v3-request-flow-smoke.mjs"]
    :summary "Made mission_request request-local artifacts the V3 default. New (compat-writer-policy ...) sub-form in V3 blueprint declares compat_write_file as the V3 opt-in switch with legacy write_file alias. Daemon helper apply_compat_write_file_policy strips write_file/compat_write_file from forwarded args and re-injects write_file=true only when compat was explicitly requested. MCP schema gained compat_write_file property; long-form description states compat roots are opt-in. Smoke checker default --live-ipc no longer passes write_file; new compat_write_audit step asserts no .missiond/alignment/<rid>/ and no new .missiond/plans/*/PLAN.lisp containing the smoke objective. Optional --compat-write-file flag exercises the legacy opt-in path. Daemon test count 79 -> 86 (all pass); MCP surfaces test pass; aggregate v3 gate still daemon-free."))

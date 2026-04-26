# Machine Contract Pilot

This note was generated as the deliverable of task `wave19-00-machine-contract-pilot`.
Its purpose is to demonstrate that a Lisp task-contract can act as a
machine-readable single source of truth (SSOT), while the ClaudeCode brief
remains a rendered execution view derived from that contract.

## Source of Truth

- Lisp SSOT: `.missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp`
- Rendered ClaudeCode brief: `.missiond/claudecode/wave19-00-machine-contract-pilot.md`
- Renderer: `node scripts/render-claudecode-task.mjs --force <task.lisp>`
- Verifier: `node scripts/check-task-contract.mjs <task.lisp>`

If the `.md` brief and the `.lisp` contract diverge, the `.lisp` wins. The
Markdown is a derived artifact and may be regenerated at any time.

## Contract Shape (v1)

The Lisp form `(task <id> ...)` carries the following machine fields:

- `:schema` — pinned to `missiond.task-contract.v1`.
- `:title` / `:goal` — human-readable summary.
- `:kind` / `:status` / `:owner` — routing hints for the dispatcher.
- `:depends-on` — DAG predecessors (empty for this pilot).
- `:dispatch-strategy` — e.g. `fresh-code-alignment`.
- `:write-scope` — explicit allowlist of files the task may touch.
- `:must-not-touch` — hard guard rails (architecture Lisp, crates, scripts).
- `:requirements` — checklist for the executing agent.
- `:acceptance` — shell commands the verifier must pass.
- `:commit` — message and scope-check policy (`write-scope-only`).
- `:report` — structured fields the agent must return on completion.

## Why This Matters

1. The contract is parseable by tools (renderer, verifier, dispatcher) instead
   of being locked inside prose.
2. The write-scope and must-not-touch lists are machine-enforceable, which is
   the foundation for the upcoming `wave19-02-task-contract-verifier-v1`
   guard-rail work.
3. ClaudeCode sessions consume a rendered brief but can always reconcile with
   the `.lisp` if the brief drifts.

## Pilot Acceptance

This pilot is considered successful when:

- `git diff --check -- docs/machine-contract-pilot.md` reports no whitespace
  errors.
- `node scripts/check-task-contract.mjs
  .missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp` exits 0.
- The commit touches only `docs/machine-contract-pilot.md`, satisfying the
  declared `write-scope-only` policy.

That is the entire surface area of this pilot — no source code, no
architecture Lisp, no scripts. Subsequent Wave 19 tasks build on this same
contract shape to wire dispatch, verification, and reporting end-to-end.

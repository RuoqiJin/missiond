# MissionD Operator Manual

This manual is the operational entry point for running MissionD as a Lisp-driven
multi-agent work system. It summarizes the current V3 control-plane contracts so
operators and resident agents do not need to infer behavior from scattered Board
notes.

## Operating Model

MissionD has five distinct authority lanes:

- **Lisp SSOT**: `.missiond/v3/missiond-blueprint.lisp`, project blueprints, and
  workflow Lisp define architecture, runtime policy, maturity, and worker
  contracts.
- **OCaml compiler/checker**: `tools/missiond_lispc` validates typed Lisp
  semantics and emits compiled projections.
- **Rust runtime**: daemon, MCP tools, EventBus, worker orchestration, shared
  memory, and provider ingestion.
- **Board**: coordination and operator decision surface. Board notes are
  projections, not canonical worker results.
- **Evidence stores**: task-result artifacts, provider durable logs, event log,
  reviewed KB, and cold evidence files.

Do not treat raw PTY output, unreviewed KB entries, or old Board tasks as higher
authority than task-result artifacts and durable events.

## Standard Work Loop

1. Read the active BoardTask and its current status.
2. Ask what is unknown before interpreting the user's intent.
3. Resolve unknowns from the correct authority:
   - project identity and maturity: MissionD Universe
   - deployment/runtime facts: deploy-center
   - credentials: secret-store references only
   - component/pattern catalog: Forge
   - operating experience: skill evidence, then promote if verified
4. For non-trivial work, create a context pack with:
   - objective
   - unknowns
   - evidence needed
   - relevant SSOT facts
   - read scope
   - accepted shards, if implementation is ready
5. Dispatch investigator workers for broad questions.
6. Dispatch implementer workers only with an accepted exact shard and write
   scope.
7. Wait for durable final evidence, not PTY idle alone.
8. Normalize worker output into a task-result artifact.
9. Project the summary into Board notes and close tasks only after settle.

## Worker Types

- **resident master**: decides, asks questions, creates BoardTasks, dispatches
  workers, writes checkpoints. It should not directly edit code unless the task
  is explicitly master-maintenance with an exact write scope.
- **investigator worker**: reads and reports. It produces Findings, Evidence,
  Recommendations, and Verification. It does not mutate code or Board history.
- **implementer worker**: receives an accepted exact shard and writes only within
  the declared write scope.
- **deploy-ops worker**: operates through deploy-center and deployment skills.
  It must capture provenance and rollback evidence.
- **deterministic LLM/tool**: can receive precise prompts because it is not an
  autonomous agent.

## Completion Rules

A task is not complete because a PTY appears idle. Completion requires:

1. provider durable final or a high-confidence final summary after the settle
   window;
2. task-result artifact exists for worker/workflow output;
3. conversation is bound to taskId/slotId and has ended_at;
4. slot is released or marked stale/interrupted with diagnostic evidence;
5. BoardTask is closed only after the above evidence is present.

## Board Cleanup Rules

Historical BoardTasks are not closed from keyword matches alone.

Use `board-cleanup-batch-runner`:

1. validate task ids;
2. materialize context;
3. dispatch read-only fact-check workers;
4. collect task-result artifacts;
5. classify each task as one of:
   - covered-by-ssot
   - covered-by-code
   - duplicate
   - obsolete
   - needs-new-task
   - needs-human
   - keep-open
6. close only generated review tasks automatically; historical tasks require
   operator or approved maintenance action.

## Memory And KB

KB is reviewed long-term knowledge, not a transcript dump.

- Default retrieval must use active reviewed memory plus explicit evidence
  requests.
- Superseded, historical, duplicate, wrong, or delete-candidate memory is
  excluded from default reasoning.
- Raw provider logs remain cold evidence and can be queried for audits.
- Memory review must write overlay state first; physical deletion requires a
  separate manifest and review window.

## Deployment Facts

MissionD does not infer production state by stitching together GitHub, curl, and
local guesses when deploy-center has a better authority.

- deploy-center owns release provenance, runtime target, agent/executor status,
  health, rollback artifacts, and deployment event relay.
- MissionD Universe owns project identity, SSOT paths, maturity, and worker
  coordination.
- secret-store owns secrets; Lisp and Board may only hold secret references.
- skills are evidence and operating guides, not runtime truth.

## Formatting

For Rust code:

- prefer repo-owned formatting gates;
- do not run broad recursive `cargo fmt`;
- use `bash scripts/rustfmt-missiond.sh --check` for MissionD checks;
- if code is intentionally changed, format only touched files or patch manually.

The M6 standard expects formatting debt to be resolved intentionally, not hidden
by reverting generated formatting noise.

## Quick Checks

Common checks from the MissionD root:

```bash
node scripts/check-v3-final-convergence.mjs --json --static-only
node scripts/check-v3-workflow-isomorphism.mjs --engine=ocaml --json
node scripts/check-project-maturity.mjs --engine=ocaml --json --min-level M6 --project auth
node scripts/check-v3-memory-kb-isomorphism.mjs --json
bash scripts/rustfmt-missiond.sh --check
git diff --check
```

Use focused Rust tests whenever possible; avoid full workspace tests while
triaging unless the change touches shared runtime behavior.

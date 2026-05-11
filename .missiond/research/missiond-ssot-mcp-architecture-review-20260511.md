# MissionD SSOT + MCP Architecture Review (2026-05-11)

## Scope

This review follows the Board cleanup pass that closed stale MissionD tasks around PTY, workstation attribution, Stdio::null automation, INFRA_M6 failed shards, and commit backfill noise.

The goal is to answer two questions:

1. From the current V3 Lisp SSOT, what architectural logic is still unclear, duplicated, or too coarse?
2. From the current MCP surface, how should MissionD reduce tool-choice noise without losing capability?

## Board Cleanup Result

The active MissionD BoardTask count is now 0 after closing the current MissionD cleanup batch.

Closed as stale or covered:

- 5 duplicated `Stdio::null` workflow automation tasks. The runtime and checker already pin this behavior through `sysinfra-control`; the tasks were historical duplicate noise.
- 3 stale `INFRA_M6` failed implementation shards. Their parent outcomes are now superseded by current project maturity evidence.
- 4 commit backfill tasks. The relevant reports/checkers now prove coverage for `93e7630f`, `ac37a16`, and `8aeac84`.
- 5 MissionD infrastructure tasks around compact-resilient memory workers, PTY integration testing, four-layer KB, PTY anomaly recovery, and search control-plane. Each is either covered by current SSOT/runtime/checker behavior or intentionally moved out of MissionD core.

## Current Good Foundations

MissionD already has the right high-level architecture in V3:

- `public-surface-map` requires every public MCP tool to map to exactly one tool group and code-aligned surface.
- `workstation-config` owns slot model profiles, timeout policies, cwd/read-scope, exact shard requirements, and MCP recovery behavior.
- `resident-master-control` includes unknowns-first intake and intent-memory candidate creation before action.
- `delegated-boardtask-runtime` and `worker-completion-settle` rules already put durable provider evidence above PTY idle.
- `mission-shared-memory` defines shared events, artifacts, claims, cursors, context slices, and evidence lanes.
- `decision-inbox-revalidation` exists and is the right home for stale question revalidation.
- Board frontend Lisp separates Terminal as PTY/slot viewer and Exec as execution cockpit.

The direction is coherent: Lisp is authoring SSOT, OCaml is typed semantic checking/projection, Rust is runtime/event/coordination, JS is wrapper/code-anchor glue.

## Remaining Architecture Issues

### 1. Workstation Policy Is Too Dense

`workstation-config` still carries many unrelated accident-derived invariants in one block. This makes it hard for the resident master or a checker to know which rule governs a failure.

Recommended split:

- `slot-lifecycle-policy`: create, heartbeat, release, TTL, reuse, stale dynamic slot recovery.
- `delegation-contract-policy`: context pack, accepted shard, read/write scope, must-not-touch.
- `completion-authority-policy`: durable provider final, task-result-artifact, settle window, PTY fallback.
- `cross-project-dispatch-policy`: project root resolution, cwd/read_scope, target_project_ids fanout.
- `context-prefetch-policy`: KB/skill prefetch disabled by default, explicit memory-audit only.
- `mcp-recovery-policy`: ClaudeCode `/mcp` arrow-key reconnect, missing tool incidents, retry budget.

This should reduce future hotfixes where one giant invariant list accumulates unrelated rules.

### 2. Task Result Artifact Must Become the Only Worker Result Authority

V3 already says worker results should land in `task-result-artifact`, but many flows still project through Board notes, provider finals, or PTY summaries first.

Target invariant:

`provider durable final -> task-result-artifact -> Board note projection -> BoardTask close -> conversation ended_at -> slot release`

Board notes should be human-readable projections, not canonical completion records.

### 3. Decision Inbox Needs Stale-State Lifecycle

The lisp-code-sync incident showed the right pattern:

- The question was valid when created.
- Runtime facts later changed.
- The frontend still displayed old text as if it required human judgment.

`decision-inbox-revalidation` should require every operational question to carry:

- `revalidator_kind`
- `root_cause_key`
- `freshness_query`
- `stale_close_reason`
- `linked_task_ids`

If runtime evidence proves the question obsolete, MissionD should close it as `stale_evidence` or `resolved_by_runtime_fix`.

### 4. Board Search Needs Scope-Aware Query Semantics

Historical keyword matches repeatedly surfaced closed/skipped tasks as current work. Board cleanup workflows should never use broad keyword search as an active-work source.

Required behavior:

- Default query: active open work only.
- Historical query: explicit `include_historical=true`.
- Cleanup query: returns `active`, `historical`, `superseded`, and `duplicate` buckets separately.
- Frontend must show whether historical scope was applied.

### 5. Project Maturity Language Is Still Confusing

The M0-M6 compression is correct, but MissionD/Board sometimes show evidence-level and declared-level mismatches. That is useful for audit, but confusing for operators.

Recommended operator display:

- `current_maturity`: declared operational level.
- `evidence_level`: what the checker can prove today.
- `why_not_current`: only when evidence exceeds declaration or declaration exceeds evidence.
- `next_gap`: one concrete next action.

### 6. Search Control Plane Should Stay Outside MissionD Core

Search, embedding, rerank, and web search should be router/service capabilities. MissionD should own:

- capability discovery
- provider health
- intent-to-capability routing
- result artifact storage

MissionD should not become the search engine implementation.

## MCP Surface Review

MissionD currently exposes many public tools. The V3 map groups them, but the actual MCP list is still too flat for agents. The problem is not raw capability count; the problem is that agents cannot quickly decide which tool family owns an intent.

### Current Shape

The public surface map has useful groups:

- request/directive/plan/workflow/execution
- board
- workstation/resident master/compute runtime
- memory/shared-memory/project registry/skill runtime
- conversation/router/question/capability/sysinfra

But several groups are still raw runtime plumbing:

- `mission_pty_*`, `mission_task_*`, `mission_cc_*`, `mission_worker`, `mission_control`
- many Board tools instead of one `mission_board` action gateway
- separate conversation/timeline/retrospective/embedding tools
- infra, permission, power, sys logs/config/update scattered across sysinfra

### Recommended MCP Tool System

Keep all existing tools for compatibility, but expose a smaller primary tool family to agents.

Primary tools:

1. `mission_board`
   - actions: `query`, `create`, `update`, `note`, `claim`, `decompose`, `retry`, `question`
   - replaces default use of `mission_board_query/create/update/delete/claim/note_add/decompose/retry`

2. `mission_workflow`
   - actions: `plan`, `run`, `swarm`, `delegate`, `status`, `artifact`, `result`
   - owns exact shard workflow and task-result-artifact access

3. `mission_workstation`
   - actions: `slots`, `pty_status`, `pty_read`, `delegate`, `claim`, `release`, `context_slice`
   - hides most direct PTY/runtime tools unless diagnostic mode is explicit

4. `mission_context`
   - actions: `conversation`, `timeline`, `logs`, `evidence_view`, `artifact_get`
   - separates conversation read model, timeline causality, logs, and worker result artifacts

5. `mission_memory`
   - actions: `query`, `remember`, `review`, `mutate`, `distill`, `active_policy`
   - default retrieval only returns active/reviewed memory

6. `mission_universe`
   - actions: `project`, `infra`, `skill_evidence`, `reconcile`, `capabilities`
   - owns project identity, runtime targets, deploy-center/Forged catalog consistency

7. `mission_ops`
   - actions: `health`, `sys_logs`, `daemon_update`, `permissions`, `power`, `pause`
   - operator and deployment control plane

8. `mission_router`
   - actions: `chat`, `search`, `embed`, `rerank`, `health`
   - points to XJP router or other external providers; MissionD stores artifacts, not implementations

9. `mission_tool_directory`
   - actions: `lookup`, `explain`, `recommend`, `deprecated`
   - maps a natural-language operator intent to the right primary tool/action and explains why.

### Tool Governance Rules

Each public MCP tool should carry:

- `tool_family`
- `primary_action`
- `tier`: `primary | compatibility | internal | diagnostic`
- `danger_level`: `read | write | destructive | external-effect`
- `intent_examples`
- `preferred_over`
- `deprecated_by`

Final convergence should fail or warn when:

- a public tool has no family mapping
- more than a small fixed number of tools are primary
- a compatibility tool appears in default resident-master tool guidance
- a destructive/external-effect tool lacks approval policy

## Suggested Implementation Order

1. Add `mcp-tool-governance` to V3 under the communication/operations boundary.
2. Add `mission_tool_directory` as read-only MCP surface.
3. Convert existing tool groups into family metadata without deleting any tool.
4. Change resident master guidance to prefer primary tools and use raw tools only in diagnostic mode.
5. Add checker: every `ToolDefinition::new` has family/tier/danger/primary mapping.
6. Later: add gateway aliases such as `mission_board(action=...)` while keeping existing raw tools.

## Immediate Status

The current Board noise has been handled. The remaining architecture work is not another cleanup pass; it is MCP and execution-governance refinement:

- split workstation policy shards
- make task-result-artifact the only worker result authority
- add MCP tool-family governance
- add a `mission_tool_directory`
- continue moving broad runtime/search capabilities behind explicit provider-backed surfaces


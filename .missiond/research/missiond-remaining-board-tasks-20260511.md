# MissionD Board Cleanup Audit - 2026-05-11

This report records the cleanup pass over Board tasks that matched broad
MissionD/PTY/MCP/Board/Memory/Conversation/Timeline keywords.

## Result

The actual MissionD infrastructure cleanup set has been resolved:

- Execution-control-plane tasks: done or deliberately skipped.
- Memory/KB/conversation governance tasks: done.
- Search/context retrieval tasks: done or deliberately skipped.
- Jarvis/router usage contract tasks: done.
- Permission/security residual tasks: done or deliberately skipped.
- Operator/UI presentation tasks: done.
- Runtime extraction in-flight watermark task: done.
- Approval queue task: done via Decision Inbox.
- Auth M6 wrapper tasks: closed as superseded by completed child tasks and
  current auth M6 evidence.
- Frontend optimistic update rollback task: done via frontend SSOT + store
  rollback implementation.
- Memory/Kb skip diagnostic + deployment-monitor text noise classification:
  done via V3 `memory-kb` invariant, runtime classifier, checker, and focused
  Rust tests.
- MissionD project-owned active backlog after the cleanup pass is no longer
  zero by design: five still-valid infrastructure tasks were restored from
  `skipped` to `open` because they represent current gaps rather than
  historical noise.

## Restored MissionD Tasks

These rows were put back on the Board with a note requiring SSOT-first design
and checker/runtime code-isomorphism:

- `fe506dd5-0f99-40a9-89e0-0fd4d5d69600` Search control-plane: Grok AI-search
  + Tavily/Bocha deterministic adapters.
- `9e71cf4b-ae65-4fe1-99c5-f0c652477267` PTY anomaly recovery: frozen
  Thinking/ToolRunning interrupt and restart guard.
- `6a9960a5-580e-4aaf-a93f-6afbe9d42f07` PTY integration tests for upstream CLI
  updates.
- `a37f5ddc-2fe2-4453-a775-059acfda0eb7` Systematic MissionD knowledge base:
  layered KB and retrieval governance.
- `3b217aee-0b47-4ac6-bc28-4c466793c9dd` Memory worker compact resilience:
  restricted-worker role anchoring after provider compaction.

Skipped rows left skipped are mostly old survey shards, old smoke requests,
historical code-first backfills, OpenClaw research notes, or work now covered
by V3 surfaces such as typed-lisp-compiler, conversation-ingestion,
workstation-pool, and provider-aware PTY recognition.

## Remaining Keyword Hits

The remaining active keyword hits are not a single MissionD infrastructure
backlog. As of this pass, broad keyword matching returns MissionD-owned active
rows only for the five restored infrastructure tasks above. Other active rows
are now one of three categories:

1. External or application work:
   - `c16a2e2c-c7ca-4775-86bd-1ae2d509f627` xiaojinpro frontend Claude Code remote panel.
   - `d5e21000-3bcc-4ed3-a465-b9049e67bc67` private-cloud dnsmasq sync.

2. XJP project maturity work:
   - `4043434b-75e7-4102-a5c0-af9ccda935cb` xiaojinpro-backend M6.
   - `20e04cee-3eb1-4540-ae57-20ceae291f0c` payments M6.
   - `cceb15fc-ccf7-4004-9a4d-47f255a64d77` asr M6.
   - `1c1cb4a9-8c22-4cbf-9e41-680b904d16e6` timeline M6.
   - `50d80ae8-1e82-498c-97be-1c356e14e4bc` INFRA_M6 Shard A1 failed child task.
   - `2db487ff-756d-48ad-ad07-911a515bd49f` payments M6 failed child task.
   - `892390fd-48a5-4253-adff-5d01a7913985` router M6 failed child task.

3. Commit Lisp/checker backfills:
   - `a02279af-426d-421b-9786-0630b70125d6` same commit, xiaojinpro-frontend.

Historical rows that used to appear here, including old Sonnet provider drift,
Auth wrapper tasks, approval queue, runtime extraction watermark, and frontend
optimistic rollback, have been closed or normalized with notes.

## Architecture Reading

The cleanup confirms that broad Board keyword search cannot be treated as
"current MissionD work". The proper architecture is:

- `mission_board` search defaults to active statuses and exposes
  `meta.activeFilterApplied`.
- Historical cleanup tasks must opt into `includeHistorical=true`.
- Board cleanup must classify hits by current ownership:
  MissionD infrastructure, external application work, project M6 work,
  commit backfill, or historical/superseded.
- Worker output should land as a task-result artifact; Board notes are only
  the projection.

## SSOT Impact

No new top-level pillar is required. The relevant SSOT surfaces already exist:

- `mission_board`
- `board-search-noise-governance`
- `decision-inbox-revalidation`
- `execution-control-plane`
- `memory-kb`
- `conversation-ingestion`
- `router-policy`

The remaining work should not be closed by keyword. It should continue through
the specific project or backfill workflows that own those tasks.

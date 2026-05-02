# GPT Pro review package: MissionD resident Codex master

## Purpose

Review the architecture for turning MissionD into an event-driven multi-project workstation OS with a resident `codex-master-control` brain. The implementation now treats V3 Lisp as the engineering SSOT and projects worker-pool/runtime configuration into Rust/Next code.

## Current SSOT files

- `/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp`
- `/Users/jinchen/Projects/missiond/.missiond/v3/evidence/workstation-pool.lisp`
- `/Users/jinchen/Projects/missiond/.missiond/frontend/board-blueprint.lisp`
- `/Users/jinchen/Projects/jarvis-forge/.missiond/intent.lisp`
- `/Users/jinchen/Projects/jarvis-forge/.missiond/backend/forge-backend-blueprint.lisp`
- `/Users/jinchen/Projects/jarvis-forge/.missiond/frontend/forge-ui-blueprint.lisp`

## Runtime/code anchors

- V3 runtime parser: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/context/v3_blueprint_runtime.rs`
- Daemon slot registration: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/main.rs`
- Slot config type: `/Users/jinchen/Projects/missiond/crates/missiond-core/src/types/slot.rs`
- PTY spawn options: `/Users/jinchen/Projects/missiond/crates/missiond-pty/src/manager.rs`
- Provider CLI command builder: `/Users/jinchen/Projects/missiond/crates/missiond-pty/src/session.rs`
- Autopilot dispatcher: `/Users/jinchen/Projects/missiond/crates/missiond-daemon/src/engine/intent_engine/autopilot.rs`
- Board slot API: `/Users/jinchen/Projects/missiond/packages/board/src/app/api/slots/route.ts`

## Newly added/updated checks

- `node scripts/check-v3-code-isomorphism-complete.mjs`
- `node scripts/check-v3-workstation-pool-isomorphism.mjs`
- `node scripts/check-v3-master-control-isomorphism.mjs`
- `node scripts/check-v3-direct-code-drift-policy.mjs`
- `node scripts/check-frontend-board-runtime-projection.mjs`

## Implemented decisions

- `workstation-pool` now declares:
  - `claude-code-default`: normal Opus 4.7/1M coding lane, no `--model`.
  - `claude-code-fast-patch`: Sonnet only for atomized narrow patch tasks.
  - `gemini-ultra-pro`: read-only Gemini 3.1 Pro Preview investigation/review lane.
  - `gemini-fast-survey`: low-authority mechanical scan/summary lane.
  - `codex-master-control`: resident Codex GPT-5.5 xhigh orchestrator, not a normal code shard worker.
- Codex CLI spawn now projects `--model`, `-c model_reasoning_effort="xhigh"`, `--search`, `--sandbox`, and `--ask-for-approval`.
- Board slots projection includes provider/model/reasoning/search/sandbox/approval metadata and falls back to `mission_slots` latestConversation when `mission_pty_status` lacks it.
- V3 adds `resident-master-control`, `lisp-code-drift-policy`, and `hot-reload-policy`.
- Forge project registry is corrected to `.missiond/intent.lisp`.
- Old open `[Proactive]` ops test tasks were hidden, not deleted.

## Questions for GPT Pro

1. Is `resident-master-control` the right boundary, or should the brain loop be split into smaller surfaces such as `master-checkpoint`, `master-dispatch`, and `master-review`?
2. Is the evidence authority order correct: provider JSONL/sqlite/chat files first, MissionD event/Board lifecycle second, PTY snapshot third?
3. Should `codex-master-control` be a persistent PTY slot, a scheduled daemon worker that invokes Codex CLI per tick, or both?
4. Is the code-first emergency waiver/backfill policy strict enough to preserve Lisp SSOT without blocking real incident repair?
5. What is the cleanest runtime reload boundary for Lisp/prompt/pool/signature changes without unsafe Rust hot swapping?
6. How should MissionD and Forge divide responsibility once multiple projects have project-local blueprints?

## Review prompt

You are reviewing MissionD’s architecture. MissionD is intended to become an event-driven multi-project workstation OS. V3 Lisp is the only engineering SSOT; Rust/JS code should project from it and be checked for code/Lisp isomorphism. A resident Codex GPT-5.5 xhigh master-control slot should act as the brain: read Lisp/Board/KB/events, checkpoint decisions, delegate ClaudeCode/Gemini/Codex workers, and recover after daemon restarts. Please identify architecture flaws, missing invariants, unsafe assumptions, and the smallest high-leverage implementation changes needed before this design can reliably run overnight swarms.

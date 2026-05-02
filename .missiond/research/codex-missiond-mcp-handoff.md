# Codex handoff: MissionD MCP mount + worker-visibility recovery

## Current state

- Repo: `/Users/jinchen/Projects/missiond`
- Date: 2026-05-02
- Codex-side MCP config has been updated in `~/.codex/config.toml`:
  - `[mcp_servers.missiond]`
  - `command = "/Users/jinchen/.xjp-mission/mission-mcp"`
- Verified:
  - `/Users/jinchen/.xjp-mission/mission-mcp` responds to MCP `initialize`.
  - `tools/list` includes MissionD tools such as `mission_board_create`, `mission_slots`, and `mission_pty_status`.
  - `codex mcp list` shows `missiond` enabled.
- Limitation in this live Codex session:
  - The newly mounted MissionD MCP tools are not hot-loaded into the current tool palette.
  - Restart Codex or start a new Codex session so the `missiond` MCP tools become native callable tools.
  - Until restart, Board/Next API at `http://127.0.0.1:3120/api/*` can bridge to the running MissionD daemon.

## User correction to preserve

The user explicitly corrected that I must use MissionD-dispatched ClaudeCode/Gemini workers, not Codex internal subagents, because the goal is to consume the external workstation pool and make MissionD the operating surface.

Do not use Codex internal `spawn_agent` workers for the main implementation path unless the user explicitly asks for Codex-only delegation. Use MissionD BoardTask/Autopilot/slots.

## Real MissionD tasks already created

Created through Board API, not Codex subagents:

1. `abb97446-87bd-483c-a480-0e134dc48772`
   - Title: `[Forge/MissionD] Fix PTY slot visibility in Board frontend`
   - Assigned slot: `slot-claude-code-default`
   - Goal: fix Board slot/PTTY visibility so the user can see which MissionD worker is actually doing the work.
   - Intended write scope:
     - `/Users/jinchen/Projects/missiond/packages/board/src/app/api/slots/route.ts`
     - `/Users/jinchen/Projects/missiond/packages/board/src/components/AutopilotMonitor.tsx`
   - Acceptance:
     - `pnpm --dir /Users/jinchen/Projects/missiond/packages/board build`
     - `curl -sS http://127.0.0.1:3120/api/slots | jq '.[] | {id,state,running,provider,engine,taskClass,acceptsBoardTask}'`
     - `git diff --check -- packages/board/src/app/api/slots/route.ts packages/board/src/components/AutopilotMonitor.tsx`

2. `b99a3fb6-697b-419a-a12f-12baccc66fa5`
   - Title: `[Forge/MissionD] Gemini review of worker visibility and SSOT rule`
   - Assigned slot: `slot-gemini-ultra`
   - Read-only review of MissionD frontend slot visibility and Lisp SSOT rules.

After restart, first check whether these BoardTasks ran or stalled:

```sh
curl -sS 'http://127.0.0.1:3120/api/tasks?status=open' \
  | jq 'map(select(.id=="abb97446-87bd-483c-a480-0e134dc48772" or .id=="b99a3fb6-697b-419a-a12f-12baccc66fa5"))'

curl -sS http://127.0.0.1:3120/api/slots \
  | jq '.[] | {id,state,running,provider,engine,taskClass,acceptsBoardTask,lastTaskId,lastTaskTitle}'
```

## Confirmed frontend visibility bug

`/api/slots` currently trusts backend `status.running` too much. Completed/exited slots can return `state: "complete"` with `running: true`, which makes the Board think stale slots are active.

Likely files:

- `/Users/jinchen/Projects/missiond/packages/board/src/app/api/slots/route.ts`
  - Current issue pattern:
    - `const running = status?.running ?? (...)`
  - Fix direction:
    - Normalize state.
    - Treat `complete`, `completed`, `exited`, `not_running`, `stopped`, `dead`, `missing` as terminal.
    - Treat only explicit active states like `starting`, `running`, `thinking`, `responding`, `tool_running`, `confirming`, `blocked`, `waiting_for_confirmation`, `sending` as active.
    - Return `activeSession`, `provider`, `engine`, `modelProfile`, `taskClass`, `lastTaskId`, `lastTaskTitle`, `recognitionConfidence`, and a readable short label if available.

- `/Users/jinchen/Projects/missiond/packages/board/src/app/api/pty/status/route.ts`
  - Similar issue pattern:
    - `if (result && result.state && result.state !== 'exited') return { running: true, ...result }`
  - Fix direction:
    - Do not mark `complete` as running.

- `/Users/jinchen/Projects/missiond/packages/board/src/components/AutopilotMonitor.tsx`
  - Current likely issue:
    - Auto-selects first `slots.find(s => s.running)`.
  - Fix direction:
    - Prefer the slot tied to the currently running BoardTask.
    - Then active slots.
    - Then idle BoardTask-capable slots.
    - Show task id/title/provider/state/confidence in the PTY header.
    - Truncate long dynamic objective labels in slot cards.

After fixing, also encode the invariant into:

- `/Users/jinchen/Projects/missiond/.missiond/frontend/board-blueprint.lisp`
- The relevant frontend checker if a slot/terminal runtime projection checker exists.

## Forge state to resume

Repo: `/Users/jinchen/Projects/jarvis-forge`

User explicitly allowed committing Forge and then said dirty changes do not need to be avoided; handle them directly.

Forge SSOT work already done:

- `.missiond/intent.lisp` rewritten as compact L1.
- Added backend/frontend blueprints:
  - `.missiond/backend/forge-backend-blueprint.lisp`
  - `.missiond/frontend/forge-ui-blueprint.lisp`
- Added evidence:
  - `.missiond/evidence/forge-ssot-convergence.lisp`
- Added checkers:
  - `scripts/lib/forge_lisp.mjs`
  - `scripts/check-forge-lisp-schema.mjs`
  - `scripts/check-forge-backend-code-isomorphism.mjs`
  - `scripts/check-forge-frontend-code-isomorphism.mjs`
  - `scripts/check-forge-ssot-complete.mjs`
- Patched:
  - `crates/forge-core/src/universe_graph.rs`
  - `crates/nc-jarvis-gen/src/rust/patterns/connector.rs`

Recent Forge checks passed:

```sh
node scripts/check-forge-lisp-schema.mjs
node scripts/check-forge-backend-code-isomorphism.mjs
node scripts/check-forge-frontend-code-isomorphism.mjs
```

Need resume with:

```sh
cd /Users/jinchen/Projects/jarvis-forge
node scripts/check-forge-ssot-complete.mjs
cargo run -q -p forge-cli -- check /Users/jinchen/Projects/jarvis-forge
pnpm --dir packages/ui build
cargo test --workspace
git diff --check
```

Then commit the Forge changes if all checks pass.

## Next action after Codex restart

1. Confirm native MissionD MCP tools are available through tool search or direct tool list.
2. Query `mission_slots` / `mission_pty_status` natively if available.
3. Inspect the two BoardTasks above and decide whether MissionD workers completed, stalled, or need re-dispatch.
4. Fix the Board slot/PTTY visibility chain using MissionD workers first; only patch directly if MissionD cannot yet execute the scoped task.
5. Return to Forge, run the remaining checks, and commit.

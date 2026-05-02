(frontend-convergence-report
  :schema "missiond.frontend-convergence-report.v1"
  :project board
  :blueprint ".missiond/frontend/board-blueprint.lisp"
  :status code-aligned
  :generated_by codex
  :summary "Board frontend now has a project-local Lisp SSOT registered from backend V3. Current UI behavior is preserved; runtime workstation identity projects from MissionD slots/PTY state, and tabs/task taxonomy/flow phases/EventBus routes project from board-blueprint.lisp into generated TypeScript."
  :surfaces [app-shell missiond-proxy board-task-ui workstation-terminal-ui event-stream-ui timeline-log-ui knowledge-system-ui frontend-design-system]
  :runtime-projections [workstation-slots pty-recognition frontend-runtime-config eventbus-cache-invalidation board-task-contract]
  :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
           "node scripts/project-frontend-board-config.mjs --check"
           "node scripts/check-frontend-board-code-isomorphism.mjs"
           "node scripts/check-frontend-board-runtime-projection.mjs"
           "node scripts/check-v3-code-isomorphism-complete.mjs"
           "node scripts/check-v3-final-convergence.mjs"
           "pnpm --dir packages/board build"
           "cargo test --workspace"
           "git diff --check"]
  :real-dispatch-smoke
    (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --execute-real-dispatch --cleanup --json"
     :delegated_board_task_id "a3428eac-d8a8-41e5-9813-825848c8d15d"
     :status done
     :result "MissionD created a read-only delegated BoardTask, Autopilot/worker completed it, and the request-local smoke directory was cleaned up without tracked worktree pollution.")
  :next-shards
    [(shard task-api-schema-projection
       :owner claude-code-default
       :write_scope ["packages/board/src/api.ts" "packages/board/src/store.ts" "packages/board/src/app/api/tasks/route.ts"]
       :goal "Tighten BoardTask frontend type/API projection against mission_board public fields.")
     (shard timeline-slot-provider-visuals
       :owner claude-code-default
       :write_scope ["packages/board/src/components/timeline/constants.tsx" "packages/board/src/components/timeline/helpers.ts"]
       :goal "Replace historical hardcoded slot color assumptions with provider/runtime fallback rules.")
     (shard frontend-blueprint-dispatch-runner
       :owner claude-code-default
       :write_scope ["scripts/context-pack-run-wave.mjs" ".missiond/frontend/context-packs/"]
       :goal "Teach MissionD to materialize frontend-blueprint context packs into BoardTasks with disjoint write scopes.")]))

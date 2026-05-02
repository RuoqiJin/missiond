(frontend-convergence-report
  :schema "missiond.frontend-convergence-report.v1"
  :project board
  :blueprint ".missiond/frontend/board-blueprint.lisp"
  :status code-aligned
  :generated_by codex
  :summary "Board frontend now has a project-local Lisp SSOT registered from backend V3. Current UI behavior is preserved; runtime workstation identity projects from MissionD slots/PTY state, and tabs/task taxonomy/flow phases/EventBus routes project from board-blueprint.lisp into generated TypeScript."
  :surfaces [app-shell missiond-proxy board-task-ui workstation-terminal-ui event-stream-ui timeline-log-ui knowledge-system-ui frontend-design-system]
  :runtime-projections [workstation-slots pty-recognition frontend-runtime-config timeline-visuals board-task-api-contract eventbus-cache-invalidation board-task-contract]
  :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
           "node scripts/project-frontend-board-config.mjs --check"
           "node scripts/check-frontend-board-code-isomorphism.mjs"
           "node scripts/check-frontend-board-runtime-projection.mjs"
           "node scripts/check-v3-code-isomorphism-complete.mjs"
           "node scripts/check-v3-final-convergence.mjs"
           "pnpm --dir packages/board build"
           "node scripts/check-v3-context-pack-isomorphism.mjs"
           "node scripts/context-pack-run-wave.mjs --dry-fixture --json"
           "cargo test --workspace"
           "git diff --check"]
  :real-dispatch-smoke
    (:command "node scripts/check-v3-request-flow-smoke.mjs --live-ipc --execute-real-dispatch --cleanup --json"
     :delegated_board_task_id "a3428eac-d8a8-41e5-9813-825848c8d15d"
     :status done
     :result "MissionD created a read-only delegated BoardTask, Autopilot/worker completed it, and the request-local smoke directory was cleaned up without tracked worktree pollution.")
  :dispatch-runner
    (:status closed
     :implementation "scripts/context-pack-run-wave.mjs"
     :evidence ["node scripts/check-v3-context-pack-isomorphism.mjs"
                "node scripts/context-pack-run-wave.mjs --dry-fixture --json"
                "node scripts/context-pack-materialize-wave.mjs --dry-fixture --json"
                "node scripts/check-context-pack.mjs --dry-fixture --json"]
     :result "Generic context-pack runner already materializes mapped frontend or backend context packs into task-runner manifests/contracts and can submit/apply through MissionD only when explicitly requested.")
  :next-shards []))

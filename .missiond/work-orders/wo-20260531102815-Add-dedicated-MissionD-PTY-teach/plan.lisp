(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531102815-Add-dedicated-MissionD-PTY-teach"
  :intent "wo-20260531102815-Add-dedicated-MissionD-PTY-teach"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531102815-Add-dedicated-MissionD-PTY-teach-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend/board-blueprint.lisp"
                     "packages/board/src/App.tsx"
                     "packages/board/src/components/Terminal.tsx"
                     "packages/board/src/components/PtyTeachingPanel.tsx"
                     "packages/board/src/generated/board-frontend-config.ts"]
       :acceptance ["pnpm --dir packages/board typecheck"
                    "node scripts/project-frontend-board-config.mjs --check"
                    "node scripts/check-frontend-board-lisp-schema.mjs"
                    "node scripts/check-frontend-board-code-isomorphism.mjs"])))

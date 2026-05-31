(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531100156-Add-MissionD-board-PTY-teaching-"
  :intent "wo-20260531100156-Add-MissionD-board-PTY-teaching-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531100156-Add-MissionD-board-PTY-teaching--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend/board-blueprint.lisp"
                     "packages/board/src/components/Terminal.tsx"
                     "packages/board/src/app/api/pty/input/route.ts"
                     "packages/board/src/app/api/pty/status/route.ts"
                     "packages/board/src/app/api/slots/route.ts"]
       :acceptance ["pnpm --dir packages/board typecheck"
                    "node scripts/check-frontend-board-lisp-schema.mjs"
                    "node scripts/check-frontend-board-code-isomorphism.mjs"])))

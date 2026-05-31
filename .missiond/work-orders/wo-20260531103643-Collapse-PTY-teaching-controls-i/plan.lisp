(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531103643-Collapse-PTY-teaching-controls-i"
  :intent "wo-20260531103643-Collapse-PTY-teaching-controls-i"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531103643-Collapse-PTY-teaching-controls-i-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend/board-blueprint.lisp"
                     "packages/board/src/components/PtyTeachingPanel.tsx"]
       :acceptance ["pnpm --dir packages/board typecheck"
                    "node scripts/project-frontend-board-config.mjs --check"
                    "node scripts/check-frontend-board-lisp-schema.mjs"
                    "node scripts/check-frontend-board-code-isomorphism.mjs"])))

;; Wave 48 dispatch-time pattern cards.

(pattern-cards wave48-context-pack-parallel-investigation-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave48

  (card append-only-context-pack
    :use-for [wave48-01-context-autopilot-restart-recovery-v0
              wave48-02-context-dispatch-shard-plan-v0]
    :summary "Use the append helper; do not hand-edit context-pack sequence numbers."
    :recipe ["Append with node scripts/context-pack-append.mjs --pack .missiond/tasks/wave48/context-pack.lisp ..."
             "Use observation entries for evidence and shard-proposal entries for implementation-ready slices."
             "Run node scripts/check-context-pack.mjs .missiond/tasks/wave48/context-pack.lisp after appending."
             "If another agent appended first, rerun the helper; it allocates the next seq under a sibling lock."]
    :known-good ["scripts/context-pack-append.mjs"
                 "scripts/check-context-pack.mjs"])

  (card investigation-not-implementation
    :use-for [wave48-01-context-autopilot-restart-recovery-v0
              wave48-02-context-dispatch-shard-plan-v0]
    :summary "These tasks produce context and shard proposals only."
    :recipe ["Read implementation files, but do not edit them."
             "Keep report prose short; put executable shard details in context-pack.lisp."
             "A later integrator will append integration-plan and dispatch code workers."
             "If a proposed shard touches the same hotspot as another likely shard, append a conflict entry."]
    :known-good [".missiond/tasks/wave48/context-pack.lisp"])
)

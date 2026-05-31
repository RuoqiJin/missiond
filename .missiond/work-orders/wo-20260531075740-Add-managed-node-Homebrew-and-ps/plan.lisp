(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531075740-Add-managed-node-Homebrew-and-ps"
  :intent "wo-20260531075740-Add-managed-node-Homebrew-and-ps"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531075740-Add-managed-node-Homebrew-and-ps-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260531075740-Add-managed-node-Homebrew-and-ps/intent.lisp"
                     ".missiond/work-orders/wo-20260531075740-Add-managed-node-Homebrew-and-ps/plan.lisp"
                     ".missiond/v3/shards/universe/infrastructure.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/v3/shards/implementation/ops-surfaces.lisp"
                     "scripts/bootstrap-managed-mac-node.sh"
                     "scripts/deploy-daemon.sh"
                     "scripts/check-v3-infrastructure-universe-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-infrastructure-universe-isomorphism.mjs --json"
                    "node scripts/check-typed-lisp-compiler.mjs --json"
                    "git diff --check"])))

(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528190946-final-bootstrap-xjpcode-evidence"
  :intent "wo-20260528190946-final-bootstrap-xjpcode-evidence"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528190946-final-bootstrap-xjpcode-evidence-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/universe/behavior-closure.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "scripts/check-v3-final-convergence.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-20260528190946-final-bootstrap-xjpcode-evidence/intent.lisp"
                     ".missiond/work-orders/wo-20260528190946-final-bootstrap-xjpcode-evidence/plan.lisp"
                     ".missiond/work-orders/wo-20260528190946-final-bootstrap-xjpcode-evidence/audit.lisp"]
       :acceptance ["node scripts/project-v3-contracts.mjs --check --json"
                    "node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "bash scripts/rustfmt-missiond.sh --check"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))

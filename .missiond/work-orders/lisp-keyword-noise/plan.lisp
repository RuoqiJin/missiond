(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "lisp-keyword-noise"
  :intent "lisp-keyword-noise"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "lisp-keyword-noise-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/keyword_pairs.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/request/respond/routing.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/request/tests.rs"]
       :acceptance ["cargo test -p missiond-daemon extract_lisp_keyword_ignores_strings_and_comments"
                   "cargo test -p missiond-daemon scan_keyword_pairs_ignores_strings_and_comments"
                   "cargo check -p missiond-daemon"
                   "node scripts/check-v3-final-convergence.mjs --json --static-only"
                   "git diff --check"])))

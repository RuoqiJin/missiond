(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :work_order_id wo-20260513-aliyun-global-accesskey-rotation
  :source_intent "intent.lisp"
  :accepted_shards
    ((shard parse-csv-secret
       :lane codex-local
       :read_scope ["/Users/jinchen/Downloads/AccessKey.csv"]
       :write_scope []
       :acceptance "Confirm key id shape without printing secret.")
     (shard secret-store-write
       :lane codex-local
       :read_scope ["secret-store CLI xjp" "secret-store service endpoint"]
       :write_scope ["secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID" "secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET"]
       :acceptance "Namespace aliyun-global contains both keys and no value is printed.")
     (shard dns-read-probe
       :lane codex-local
       :read_scope ["Aliyun DNS DescribeDomainRecords changtu.pro"]
       :write_scope []
       :acceptance "Read-only DNS query returns successfully.")
     (shard ssot-evidence-update
       :lane codex-local
       :read_scope ["/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp"
                    "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs/.missiond/intent.lisp"]
       :write_scope ["/Users/jinchen/Projects/missiond/.missiond/v3/missiond-blueprint.lisp"
                     "/Users/jinchen/Projects/missiond/.missiond/research/aliyun-global-access-key-rotation-20260513.md"
                     "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs/.missiond/intent.lisp"
                     "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs/.missiond/evidence/aliyun-global-access-key-rotation-20260513.md"]
       :acceptance "MissionD infrastructure universe and secret-store SSOT point at aliyun-global account-level credential refs.")
     (shard work-order-backfill
       :lane codex-local
       :read_scope [".missiond/workflows/work-order-lifecycle.lisp"]
       :write_scope [".missiond/work-orders/20260513-aliyun-global-accesskey-rotation/**"]
       :acceptance "The operation is replayable as intent.lisp -> plan.lisp -> audit.lisp."))
  :risk_gates [no-secret-values no-dns-mutation no-inline-credential]
  :completion_authority [task-result-artifact audit.lisp checker])

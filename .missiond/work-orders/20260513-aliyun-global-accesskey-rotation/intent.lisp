(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :work_order_id wo-20260513-aliyun-global-accesskey-rotation
  :source (operator-request
            :origin "Codex desktop conversation"
            :received_at "2026-05-13T00:00:00+08:00"
            :summary "Rotate global Aliyun AccessKey into secret-store, verify DNS read capability, and update MissionD infrastructure governance.")
  :board_anchor (:status not-created :rule "BoardTask and intent.lisp sources share the same work-order lifecycle; future repeats should create a visible BoardTask first.")
  :objective "Store the new global Aliyun cloud AccessKey in secret-store, remove the mistaken DNS-only key framing, verify read-only DNS access for changtu.pro, and persist reusable deploy-ops evidence."
  :constraints
    ((constraint no-secret-values :rule "Never write or print AccessKey secret values in Lisp, Board notes, logs, or evidence.")
     (constraint account-scope :rule "This is a global Aliyun account credential, not a DNS-only credential.")
     (constraint mutation-guard :rule "DNS, ECS, RAM, and OSS mutations require explicit work-order approval.")
     (constraint worker-lane :rule "Future cloud operations should be delegated to claude-code-deploy-ops with a redacted context pack."))
  :expected_artifacts [plan.lisp audit.lisp aliyun-global-access-key-rotation-20260513.md]
  :external_reuse "External applications may submit the same work-order shape for translation, code-refactor, deploy-ops, or other worker jobs; MissionD must still bind BoardTask, plan, task-result-artifact, and audit.")

(codex-boot-context
  :schema "missiond.codex-boot-context.v1"
  :version "2026-05-16"
  :purpose "Small startup capsule for resident Codex, Codex worker, and external Codex handoff. This is the shared collaboration protocol, not a dump of historical chat."
  :size-budget-kb 8
  :layers
    ((layer L0-always-on
       :rules
         ["Lisp is the SSOT: change Lisp first, then make code/checkers/runtime isomorphic."
          "Standard path: user input -> intent.lisp -> plan.lisp -> BoardTask/work-order -> accepted shard -> worker -> task-result-artifact -> checker/test -> commit."
          "Before judging intent, ask what information is unknown and use mission_context_gather for bounded SSOT/project/skill/infra/active-memory evidence."
          "Do not preload broad KB, old conversation logs, provider durable logs, or runtime evidence. Open cold evidence only for audit/debug by explicit request."
          "For deployed MissionD, runtime artifacts live under MISSIOND_RUNTIME_DIR and MISSIOND_COMPILED_RUNTIME_DIR; repo .missiond/v3/runtime/** is dev/cold evidence only and must not be used to declare deployed compiled projections missing."
          "Implementation workers require accepted_shard_id, context_pack_path, write_scope, and write lease; broad objectives are investigation/design work, not code shards."
          "Skill edits route to ClaudeCode skill-maintainer/deploy-ops lanes; Codex owns CLI, architecture, verification, and small control-plane patches."
          "Deployment facts live in deploy-center, secrets in secret-store, and project identity/maturity in MissionD Universe."
          "Remote node updates, including Mac mini MissionD, sync source through GitHub or XJP codebase/deploy-center lanes, then build and deploy on the target node; ad-hoc rsync/scp from an operator laptop is break-glass only and must not be the normal workflow."
          "PTY is diagnostic only; durable provider logs, EventBus, task-result-artifact, and Board lifecycle are completion authority."])
     (layer L1-current-task
       :source "mission_context_boot + mission_master_status + active BoardTask/work-order"
       :fields [active_objective active_boardtask_id work_order_id project_id context_pack_path accepted_shard_id latest_checkpoint])
     (layer L2-grounded-facts
       :source "mission_context_gather"
       :fields [ssot_summary project_registry skill_evidence active_memory infra_deploy_facts tool_directory runtime_environment])
     (layer L3-cold-evidence
       :source "explicit audit/debug requests only"
       :fields [raw_conversations historical_board_tasks provider_logs runtime_reports]))
  :required-tools [mission_context_boot mission_context_gather mission_board mission_workflow mission_task_delegate mission_shared_memory mission_tool_directory]
  :forbidden-content [secret-values raw-provider-logs bulk-chat-history unreviewed-kb-preload]
  :handoff-card
    "Use MissionD as the control plane. If you need missing facts, call mission_context_gather. For deployed runtime facts, trust runtime_environment and its canonical Jarvis monitor endpoints before repo-local runtime files. Do not guess monitor ports. If you need to change code, ensure intent.lisp/plan.lisp/accepted_shard/write_scope exist and commit through the work-order gate. Never treat PTY text as completion authority.")

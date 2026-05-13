(workflow intent-intake-grounding
  :schema "missiond.workflow.intent-intake-grounding.v1"
  :workflow_id intent-intake-grounding
  :status active
  :owner resident-master-control
  :purpose "Turn a raw user or external-app request into a grounded, reviewable intent artifact before plan or worker dispatch."
  :authority [mission_request agent-interaction-policy work-order-lifecycle project-universe memory-access-plane skill-infra-evidence-index]
  :source_plans [mission_request resident-master-control work-order-lifecycle conversation-memory-distillation]
  :match_rules
    ((trigger :kind user-message :surface JarvisChat :when "raw user message enters resident master gateway")
     (trigger :kind boardtask :promptTemplate resident-master-control :when "BoardTask asks for interpreting a user objective")
     (trigger :kind external-app :shape "external intent envelope or work-order intent")
     (trigger :kind manual :tool mission_request :when "caller requests request-local intent review packet"))
  :inputs [raw_user_message source_channel project_hint? image_refs? active_boardtask_id? prior_context_pack?]
  :context_policy
    (:default-inputs [raw_user_message source_channel active_boardtask_id project_hint]
     :primary-grounding-tool mission_context_gather
     :allowed-grounding-tools [mission_context_gather mission_intent mission_kb_query mission_skill_context mission_infra_query mission_project mission_board_query mission_tool_directory]
     :bounded-query-rule "Every grounding query must be derived from an explicit unknown; broad KB, Board backlog, provider-log, or historical conversation preload is forbidden."
     :privacy-rule "Image/file attachments are referenced by durable artifact id or source pointer; do not paste binary payloads into intent.lisp.")
  :workers
    ((codex-master :role intent-integrator :write [context-pack request-local-intent BoardTask-note] :code-write false)
     (claude-opus :role optional-intent-investigator :write context-pack-only :code-write false)
     (gemini :role optional-wide-summary :write context-pack-only :code-write false))
  :steps
    ((step s1 :id capture-request
       :question "用户原话是什么？它来自 Jarvis、Board、外部应用还是 mission_request？是否已有 project_hint、图片、文件或父 BoardTask？"
       :core "Normalize the input into request_id/source_channel/objective/source_refs and create or reuse the BoardTask anchor.")
     (step s2 :id intent-understanding
       :question "第一轮：用户现在想做什么？这个诉求是任务执行、架构设计、部署运维、记忆整理、项目治理，还是需要先澄清？"
       :core "Write inferred_user_intent with confidence, assumptions, non_goals, likely_project_ids, and immediate next review question.")
     (step s3 :id unknowns-inventory
       :question "关于这个诉求，我还有哪些不知道的信息？每个未知信息应该向哪个权威源查询？"
       :core "Classify unknowns by authority: project SSOT, memory, skill operational facts, deployment facts, code surface, EventBus evidence, checker output, or user decision.")
     (step s4 :id scoped-grounding-search
       :question "第二轮：用最小查询查一遍记忆库、项目注册、SSOT Lisp、skill 证据和必要代码锚点，确认用户说的对象到底是什么。"
       :core "Run mission_context_gather as the primary aggregate over KB, project registry, SSOT Lisp, skill evidence, and infra evidence. Fall back to individual mission_kb_query/mission_intent/mission_skill_context/mission_infra_query only when the aggregate reports a source-specific diagnostic. Every query must be derived from s3 unknowns.")
     (step s5 :id evidence-synthesis
       :question "这些证据是否足以支持意图判断？有没有过期事实、同名项目、路径不明、服务器未知或用户前后偏好冲突？"
       :core "Merge evidence into grounded_intent_summary, evidence_refs, confidence, stale_fact_notes, unresolved_unknowns, and decision_needed flags.")
     (step s6 :id optional-investigation-worker
       :question "如果证据不足，是否需要一个只读工位帮忙查证？它应该查哪些 read_scope，返回哪些 Findings/Evidence/Recommendations/Verification？"
       :core "Dispatch optional read-only investigator only for unresolved_unknowns that cannot be answered by bounded local lookup.")
     (step s7 :id draft-intent-lisp
       :question "第三轮：把已查证的理解整理成用户/主控可确认的 intent.lisp。"
       :core "Draft request-local intent-alignment.lisp or work-order intent with objective, scope, assumptions, non_goals, evidence_refs, acceptance, risks, and approval state awaiting_intent_approval.")
     (step s8 :id review-packet
       :question "这个 intent 是否需要用户确认？如果是可信 Agent 快路径，哪些 policy gate 允许折叠确认？"
       :core "Return mission_request review_packet or Board note projection that points to the request-local intent artifact. Do not compile plan or dispatch workers before confirmation unless trusted-agent policy explicitly allows folded intent.")
     (step s9 :id plan-authoring-worker
       :question "intent.lisp 已确认后，是否需要生成 plan.lisp？如果没有现成 workflow.lisp，应由哪个计划工位读取工具目录、资源和调度能力，把 intent 编译成可验收的 accepted shards？"
       :core "After intent approval, assign a read/write-scoped plan-authoring worker or mission_request respond approve_intent path to compile plan.lisp from confirmed intent, mission_context_gather evidence, mission_tool_directory resource/tool inventory, available workstation pool, and acceptance policy. If a workflow.lisp already matches the request, bind the workflow directly and skip intent/plan authoring.")
     (step s10 :id intent-memory-candidate
       :question "这次判断里是否出现了可复用的用户长期偏好、项目常量或工具边界？"
       :core "Emit intent_memory_candidate with evidence_refs/confidence/supersession_scope; high-confidence stable preferences may be stored as memory:decision, otherwise remain candidate artifact."))
  :context-pack-artifacts [raw_user_message inferred_user_intent unknowns evidence_needed grounding_results grounded_intent_summary intent_lisp_path review_packet plan_authoring_context plan_lisp_path accepted_shards intent_memory_candidate]
  :egress [request.lisp intent-alignment.lisp review_packet plan.lisp accepted_shards context-pack BoardTask-note intent_memory_candidate]
  :risk-gates
    ((gate g1 :rule "No plan.lisp or worker dispatch before grounded intent exists and review policy is satisfied; after intent confirmation, plan.lisp authoring is a worker/runtime stage unless an existing workflow.lisp explicitly matches.")
     (gate g2 :rule "Memory search is required for ambiguous user references but must be scoped by unknowns; KB preload is forbidden.")
     (gate g3 :rule "Project identity must come from MissionD Universe or project SSOT; guessed paths are evidence gaps, not facts.")
     (gate g4 :rule "Skill operational facts are evidence until promoted through infra/project registry; they cannot silently override SSOT.")
     (gate g5 :rule "If user intent conflicts with prior active memory, surface supersession_scope instead of keeping both active.")
     (gate g6 :rule "Intent investigator workers are read-only and produce structured artifacts; they do not create implementation tasks."))
  :completion
    ((criterion c1 :rule "inferred_user_intent, unknowns, evidence_needed, and evidence_refs are present.")
     (criterion c2 :rule "request-local intent-alignment.lisp or equivalent work-order intent exists and is referenced by review_packet.")
     (criterion c3 :rule "unresolved unknowns are either assigned to a read-only investigation task or surfaced as user decision items.")
     (criterion c4 :rule "No plan/worker dispatch occurred before intent review or trusted-agent policy gate.")
     (criterion c5 :rule "If intent is approved and no matching workflow.lisp exists, plan-authoring-worker or mission_request approve_intent compiled plan.lisp with tool/resource inventory and accepted shards.")))

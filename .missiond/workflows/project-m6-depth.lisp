;; MissionD workflow: project M6 depth.
;;
;; Purpose: prove Auth-grade maturity inside the single M0-M6 model. M6 means
;; the project is not merely registered, mapped, and worker-operational; its
;; domain model, policies, flows, events, runtime projection, implementation
;; map, compatibility ledger, hot paths, and regression matrix are explicit and
;; code-aligned.

(workflow project-m6-depth
  :schema "missiond.workflow.project-m6-depth.v1"
  :workflow_id project-m6-depth
  :status active
  :owner resident-master-control
  :authority [project-ssot-convergence project-maturity-model commit-lisp-convergence]
  :source_plans [auth-m6-depth project-ssot-convergence v3-runtime-ssot]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective requests M6, Auth-grade maturity, production-ready architecture, or hot-path wiring")
     (trigger :kind boardtask :title-prefix "Run project M6 depth")
     (trigger :kind maturity :after M5 :when "project is core infrastructure or a long-lived application dependency"))
  :inputs [project-id project-root project-blueprints current-code-mapping compatibility-ledger? runtime-config event-contracts tests dirty-baseline context-pack-path?]
  :workers
    ((codex-master :role architect-integrator :write [board checkpoint context-pack decision-inbox] :code-write false)
     (claude-opus :role implementation :write exact-shard-only :code-write true)
     (claude-sonnet :role narrow-patch :write exact-file-region-only :code-write true)
     (gemini :role readonly-wide-scan :write context-pack-only :code-write false))
  :steps
    ((step s1 :id unknowns-first-intake
       :logic "Before judging intent or architecture, ask: what do I still not know about this user request? Map each unknown to the correct authority source: project SSOT, skill operational facts, current code, deployment facts, EventBus evidence, checker output, or a user decision. Gather bounded evidence before proceeding.")
     (step s2 :id intent-inference
       :logic "Infer the user's immediate objective, durable preference, and governance principle from the active request plus collected evidence. If the inference is uncertain, mark confidence and evidence gaps rather than guessing.")
     (step s3 :id intent-memory-capture
       :logic "Write an intent_memory_candidate into the context-pack with evidence_refs, confidence, supersession_scope, and suggested active/needs-review state. If the inferred intent is stable and high-confidence, persist it as memory:decision through mission_kb_remember; otherwise keep it as needs-review/candidate. This is for MissionD consciousness evolution, not broad KB preloading.")
     (step s4 :id review-question
       :logic "Start with a heuristic architecture question: please review this project's SSOT Lisp, decide whether the granularity is fine enough, identify design smells, and state what evidence or workers are needed. Do not jump to implementation unless exact-shard-ready=true is explicitly present.")
     (step s5 :id evidence-plan
       :logic "Write context-pack questions, hypotheses, evidence_needed, read_scope, and proposed investigation lanes. The plan must name the Lisp shards, code surfaces, tests, and compatibility evidence needed before design acceptance.")
     (step s6 :id investigation
       :logic "Dispatch read-only investigator workers or perform bounded local reads. Investigator prompts ask them to审视/比较/找缺口/给建议 and return Findings / Evidence / Recommendations / Verification.")
     (step s7 :id synthesis
       :logic "Merge investigator findings into a single context-pack synthesis: confirmed facts, contradictions, missing anchors, design risks, and evidence confidence.")
     (step s8 :id design-proposal
       :logic "Produce design_options and select the accepted architecture direction before code shards. Accepted design must preserve entry/core ordered steps/egress/surfaces and declare compatibility implications.")
     (step s9 :id domain-model-audit
       :logic "Extract durable domain nouns, ownership chain, identity boundaries, permission boundaries, state machines, and external dependencies from Lisp plus code. Reject naming that lets protocol clients, runtime configs, or bridge tables masquerade as domain objects.")
     (step s10 :id target-architecture-draft
       :logic "Write or refine fine-grained domain/policy/flow/token/event/runtime/implementation Lisp shards. Every function must use entry/core ordered steps/egress/surfaces/runtime-projection; long prose moves to evidence.")
     (step s11 :id authority-chain-check
       :logic "State the canonical authority chain in one place and map every public route, API, job, event, and DB mutation to one authority node. Auth example: tenant -> application -> product -> product_user -> product_user_group.")
     (step s12 :id compatibility-ledger
       :logic "For each legacy bridge, record owner, allowed reads, allowed writes, compatibility reason, diagnostic event, and exit condition. New semantic ownership in bridge fields is forbidden.")
     (step s13 :id runtime-registration-check
       :logic "Prove that registering new business objects is runtime data through DB/API/audit/event surfaces, not env/config/rebuild/redeploy. If registration still needs deploy, create implementation shards before claiming M6.")
     (step s14 :id event-contract-check
       :logic "Split producer, local durable outbox/audit, adapter owner, sink payload, ack, retry, failure status, privacy class, severity, trace id, dedupe key, and non-blocking policy. The primary user path must not depend on downstream event delivery.")
     (step s15 :id hot-path-wiring-check
       :logic "For every architecture contract and policy service, grep-map the real runtime callers. A contract that is only in Lisp, tests, or dead service code remains design-only and cannot satisfy M6.")
     (step s16 :id regression-matrix
       :logic "Build focused tests for existing production flows, old compatibility inputs, new target model paths, event failure behavior, authorization boundaries, and backward compatibility.")
     (step s17 :id exact-shards
       :logic "Compile accepted_shards with file/region ownership from the accepted design. Claude Opus implements Lisp/checker/runtime code; Sonnet handles only narrow patches; Gemini remains read-only.")
     (step s18 :id implementation
       :logic "Implement only accepted shards with exact write_scope. Worker prompts begin with context-prepared questions, but scope and acceptance are enforced by BoardTask metadata and runtime guards.")
     (step s19 :id verification
       :logic "Run focused regression matrix, project checks, and M6 maturity gate; write final report and residual gap list before closing the objective."))
  :context-pack-artifacts [unknowns inferred_user_intent intent_memory_candidate questions hypotheses evidence_needed findings design_options accepted_shards]
  :egress [unknowns inferred_user_intent intent_memory_candidate questions hypotheses evidence_needed findings design_options accepted_shards final-m6-report refined-blueprints compatibility-ledger checker-hard-gates code-shards regression-results decision-items]
  :risk-gates
    ((gate g1 :rule "Lisp-first: target architecture and checker gates are updated before runtime code changes.")
     (gate g2 :rule "No destructive DB migration in an M6 pass; use additive-first migrations plus compatibility ledger exit criteria.")
     (gate g3 :rule "No production deploy, DNS mutation, or secret mutation unless a separate approved deploy BoardTask owns it.")
     (gate g4 :rule "Critical contracts must be hot-path wired; tests alone do not prove runtime behavior.")
     (gate g5 :rule "Existing production flows must have regression coverage before refactoring shared login/token/admin/event paths.")
     (gate g6 :rule "Event delivery must be durable and retryable, but primary request paths must remain non-blocking.")
     (gate g7 :rule "Every user decision is written to Decision Inbox with evidence and recommended options; worker prompts must not bury decisions in prose.")
     (gate g8 :rule "No recursive cargo fmt or broad formatter runs; format/check only touched files unless a dedicated formatting task owns the repo.")
     (gate g9 :rule "Initial objectives must pass through unknowns-first-intake, intent-inference, intent-memory-capture, review-question, and evidence-plan before investigation or implementation, unless exact-shard-ready=true is explicit."))
  :completion
    ((criterion c1 :rule "Domain model and authority chain are explicit and not conflated with protocol/runtime bridge objects.")
     (criterion c2 :rule "Compatibility ledger covers every legacy bridge with owner, read/write boundary, diagnostics, and exit condition.")
     (criterion c3 :rule "Runtime registration of new business objects does not require rebuild or redeploy.")
     (criterion c4 :rule "Event contracts cover producer, outbox/audit, adapter, sink, ack, retry, failure, privacy, severity, trace, and dedupe.")
     (criterion c5 :rule "Architecture contracts are wired into real runtime hot paths and pinned by checker tokens.")
     (criterion c6 :rule "Regression matrix passes for old flows, new model paths, compatibility inputs, and event failure behavior.")
     (criterion c7 :rule "Final report identifies remaining design-only or user-decision items without inflating maturity claims.")))

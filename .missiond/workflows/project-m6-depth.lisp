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
    ((step s1 :id domain-model-audit
       :logic "Extract durable domain nouns, ownership chain, identity boundaries, permission boundaries, state machines, and external dependencies from Lisp plus code. Reject naming that lets protocol clients, runtime configs, or bridge tables masquerade as domain objects.")
     (step s2 :id target-architecture-draft
       :logic "Write or refine fine-grained domain/policy/flow/token/event/runtime/implementation Lisp shards. Every function must use entry/core ordered steps/egress/surfaces/runtime-projection; long prose moves to evidence.")
     (step s3 :id authority-chain-check
       :logic "State the canonical authority chain in one place and map every public route, API, job, event, and DB mutation to one authority node. Auth example: tenant -> application -> product -> product_user -> product_user_group.")
     (step s4 :id compatibility-ledger
       :logic "For each legacy bridge, record owner, allowed reads, allowed writes, compatibility reason, diagnostic event, and exit condition. New semantic ownership in bridge fields is forbidden.")
     (step s5 :id runtime-registration-check
       :logic "Prove that registering new business objects is runtime data through DB/API/audit/event surfaces, not env/config/rebuild/redeploy. If registration still needs deploy, create implementation shards before claiming M6.")
     (step s6 :id event-contract-check
       :logic "Split producer, local durable outbox/audit, adapter owner, sink payload, ack, retry, failure status, privacy class, severity, trace id, dedupe key, and non-blocking policy. The primary user path must not depend on downstream event delivery.")
     (step s7 :id hot-path-wiring-check
       :logic "For every architecture contract and policy service, grep-map the real runtime callers. A contract that is only in Lisp, tests, or dead service code remains design-only and cannot satisfy M6.")
     (step s8 :id regression-matrix
       :logic "Build focused tests for existing production flows, old compatibility inputs, new target model paths, event failure behavior, authorization boundaries, and backward compatibility.")
     (step s9 :id exact-code-shards
       :logic "Compile context-pack accepted shards with file/region ownership. Claude Opus implements Lisp/checker/runtime code; Sonnet handles only narrow patches; Gemini remains read-only.")
     (step s10 :id final-m6-report
       :logic "Write a report that classifies each function as code-aligned, runtime-projected, hot-path-wired, design-only, blocked, or requires-user-decision. Do not call M6 while any critical hot path is design-only."))
  :egress [final-m6-report refined-blueprints compatibility-ledger checker-hard-gates code-shards regression-results decision-items]
  :risk-gates
    ((gate g1 :rule "Lisp-first: target architecture and checker gates are updated before runtime code changes.")
     (gate g2 :rule "No destructive DB migration in an M6 pass; use additive-first migrations plus compatibility ledger exit criteria.")
     (gate g3 :rule "No production deploy, DNS mutation, or secret mutation unless a separate approved deploy BoardTask owns it.")
     (gate g4 :rule "Critical contracts must be hot-path wired; tests alone do not prove runtime behavior.")
     (gate g5 :rule "Existing production flows must have regression coverage before refactoring shared login/token/admin/event paths.")
     (gate g6 :rule "Event delivery must be durable and retryable, but primary request paths must remain non-blocking.")
     (gate g7 :rule "Every user decision is written to Decision Inbox with evidence and recommended options; worker prompts must not bury decisions in prose.")
     (gate g8 :rule "No recursive cargo fmt or broad formatter runs; format/check only touched files unless a dedicated formatting task owns the repo."))
  :completion
    ((criterion c1 :rule "Domain model and authority chain are explicit and not conflated with protocol/runtime bridge objects.")
     (criterion c2 :rule "Compatibility ledger covers every legacy bridge with owner, read/write boundary, diagnostics, and exit condition.")
     (criterion c3 :rule "Runtime registration of new business objects does not require rebuild or redeploy.")
     (criterion c4 :rule "Event contracts cover producer, outbox/audit, adapter, sink, ack, retry, failure, privacy, severity, trace, and dedupe.")
     (criterion c5 :rule "Architecture contracts are wired into real runtime hot paths and pinned by checker tokens.")
     (criterion c6 :rule "Regression matrix passes for old flows, new model paths, compatibility inputs, and event failure behavior.")
     (criterion c7 :rule "Final report identifies remaining design-only or user-decision items without inflating maturity claims.")))

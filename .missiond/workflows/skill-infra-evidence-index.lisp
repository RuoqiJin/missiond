(workflow skill-infra-evidence-index
  :schema "missiond.workflow.v1"
  :workflow_id skill-infra-evidence-index
  :status active
  :source_plans [infrastructure-universe project-identity-contract registry-authority-map service-runtime-universe]
  :match_rules
    ((trigger :kind manual :tool "mission_infra_query(action=skill_evidence)")
     (trigger :kind manual :tool "mission_infra_query(action=credential_refs)")
     (trigger :kind manual :surface operator-request :when "operator asks about server/host/login/runtime fact whose authority is unclear")
     (dedupe-key "skill-infra-evidence-index:<skill-or-scope>"))
  :owner resident-master-control
  :purpose "Turn scattered Claude/Codex skill operational notes into structured infrastructure evidence without promoting them to runtime truth."
  :inputs [operator-request mission_infra_query.skill_evidence mission_infra_query.credential_refs project_registry_reconcile]
  :entry [resident-master-control mission_infra_query.skill_evidence project_registry_reconcile operator-request]
  :steps
    ((step s1 :name identify-unknowns
       :logic "Ask: for this user request, what infrastructure/server/deployment facts do we not yet know, and which skills might contain evidence?")
     (step s2 :name query-skill-evidence
       :logic "Query mission_infra_query(action=skill_evidence) and mission_infra_query(action=credential_refs) before guessing host, login, model runner, or deploy-agent paths.")
     (step s3 :name classify-facts
       :logic "Classify each skill fact as runtime-target-candidate, deploy-center-executor-candidate, router/model evidence, credential-inline-risk, or stale/historical guidance.")
     (step s4 :name redact-and-migrate-credentials
       :logic "Redact credential-like excerpts and write only secret_ref migration candidates; never place credential values into Board, Lisp, worker prompt, or context pack.")
     (step s5 :name compare-with-authority
       :logic "Compare skill facts with deploy-center runtime inventory, MissionD Universe, and Forge catalog; emit runtime_fact_missing, credential_inline_risk, stale_skill_fact, and root_mismatch drift.")
     (step s6 :name route-conflicts
       :logic "Create Decision Inbox items only for unresolved authority conflicts or credential migration questions; safe skill-only evidence remains low-authority until promoted by deploy-center/secret-store."))
  :egress [skill-infra-evidence-artifact infrastructure-reconcile-report credential-migration-report decision-inbox]
  :risk-gates
    ((gate g1 :rule "No credential values may be written to Lisp, Board, context pack, or worker prompt.")
     (gate g2 :rule "No production mutation is performed by evidence indexing.")
     (gate g3 :rule "Skill-only facts remain unverified until reconciled with deploy-center/secret-store."))
  :completion
    ((criterion c1 :rule "Every extracted fact is classified as reconciled, unverified evidence, credential migration candidate, stale skill fact, or Decision Inbox item.")
     (criterion c2 :rule "Credential-like excerpts are redacted and represented only as secret_ref migration candidates.")
     (criterion c3 :rule "MissionD can answer unknown-server questions by querying infra evidence before guessing.")))

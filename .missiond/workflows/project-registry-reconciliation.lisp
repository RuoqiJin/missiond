(workflow project-registry-reconciliation
  :schema "missiond.workflow.v1"
  :workflow_id project-registry-reconciliation
  :status active
  :source_plans [project-identity-contract registry-authority-map project-registry-policy]
  :match_rules
    ((trigger :kind manual :tool "mission_project(action=reconcile)")
     (trigger :kind schedule :policy registry-reconciliation)
     (dedupe-key "project-registry-reconcile:<project_id-or-all>"))
  :owner missiond
  :purpose "Reconcile MissionD Universe project identity, deploy-center deployment facts, and Forge code-reality catalog without silently overwriting authority boundaries."
  :inputs [mission_project.registry service-runtime-universe deploy-center.provenance forge.universe-catalog]
  :entry ["mission_project(action=reconcile)" "resident-master-control"]
  :steps
    ((step s1 :name collect-missiond-registry
       :logic "Read MissionD project_id, canonical_root, SSOT paths, maturity, aliases, Board/workstation metadata.")
     (step s2 :name collect-deploy-center-facts
       :logic "Read deploy-center project/provenance availability and service runtime fact ownership; if unavailable, mark deploy_fact_missing rather than synthesizing facts.")
     (step s3 :name collect-forge-catalog
       :logic "Read Forge catalog/reality mirror availability; Forge-only projects remain catalog references until MissionD registers them.")
     (step s4 :name compare-authorities
       :logic "Emit consistent, missing_in_*, root_mismatch, alias_conflict, and deploy_fact_missing drift items.")
     (step s5 :name route-drift
       :logic "Auto-fix only safe inactive aliases; route root conflicts, deployment fact gaps, or authority disagreements to Decision Inbox or BoardTask."))
  :risk-gates
    ((gate g1 :rule "Root, alias, and project-id conflicts are reported as drift; this workflow never silently overwrites canonical identities.")
     (gate g2 :rule "Forge-only catalog entries do not become deployable or schedulable projects until MissionD registers them.")
     (gate g3 :rule "deploy-center provenance remains the deployment fact authority; MissionD Universe stores identity and orchestration metadata only.")
     (gate g4 :rule "Cloud/service facts missing from deploy-center are recorded as deploy_fact_missing instead of guessed from curl, GitHub, or local code."))
  :completion
    ((criterion c1 :rule "Reconciliation returns consistent, missing_in_*, root_mismatch, alias_conflict, and deploy_fact_missing items.")
     (criterion c2 :rule "Safe inactive alias cleanup can be proposed, but conflicting identities create Decision Inbox or BoardTask items.")
     (criterion c3 :rule "MissionD, deploy-center, and Forge authority boundaries remain explicit in the report.")))

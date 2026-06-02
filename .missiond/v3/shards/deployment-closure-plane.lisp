(deployment-closure-plane
    :schema "missiond.deployment-closure-plane.v1"
    :purpose "Unify MissionD self-deploy, XJP service deploys, and new product deploy templates behind typed release evidence and a fail-closed closure verdict."
    :authorities
      ((missiond :owns [project-identity deployment-policy maturity work-order approval eventbridge display-cache])
       (deploy-center :owns [runtime-target-inventory release-closure-authority release-evidence closure-verdict release-lease runtime-observation])
       (deploy-agent :owns [controlled-runtime-actions read-only-runtime-inspection evidence-reporting])
       (secret-store :owns [credential-values credential-availability])
       (deploy-ops-agent :owns [preflight-report evidence-review rollback-plan postmortem]))
    :release-state-machine
      (:states [classify_change preflight build_candidate acquire_release_lease deploy runtime_observe deep_smoke closure_verdict release_or_rollback]
      :rules ["build_succeeded means only that an artifact was produced."
              "deploy_succeeded means only that the executor completed its deploy action."
              "runtime_observed requires observed production process/container evidence such as running digest, entrypoint, binary marker, compose files, and container id."
              "smoke_succeeded requires service-declared deep readiness, not only a shallow liveness endpoint."
              "closed is legal only when ClosureVerdict.verdict=success."]
      :terminal-verdicts [success failed blocked stale provenance_partial])
    :records
      ((record DeploymentIntent
         :fields [project service runtime_target change_class work_order initiator desired_commit deployment_policy_hash])
       (record ReleaseCandidate
         :fields [git_sha image_digest builder artifact_lane manifest_hash compiled_abi_hash migration_plan rollback_artifact])
       (record ReleaseLease
         :fields [service runtime_target owner expected_active_root expected_running_digest generation expires_at conflict_policy])
       (record RuntimeObservation
         :fields [running_image_digest container_id compose_files entrypoint binary_marker env_ref_names db_migration_state caddy_dns_binding health_result])
       (record ReleaseEvidence
         :fields [deployment_intent release_candidate release_lease runtime_observation smoke migration_adoption secret_availability rollback_artifact_refs])
       (record ClosureVerdict
         :fields [verdict typed_diagnostics next_action confidence evidence_ref]
         :verdicts [success failed blocked stale provenance_partial]))
    :compiled-policy
      (:fields [manifest_required immutable_image_required runtime_digest_required smoke_required db_adoption_required release_lease_required artifact_lane target_side_build_allowed approval_policy diagnostic_profiles]
       :strict-rule "Production, M5, and M6 services default to fail-closed gates: manifest, immutable image, runtime digest, smoke, release lease, and explicit artifact lane are required."
       :db-rule "Payments-like services require explicit production DB legacy-schema adoption before closure.")
    :invariants
      ["MissionD MUST NOT infer production closure from GitHub success, curl probes, local git state, or PTY logs when Deploy Center closure evidence exists."
       "GitHub workflow success and deploy-center notify HTTP 200 only move a task to wait_for_provenance."
       "Every production deploy writes or references a ReleaseLease before mutating an active symlink, compose target, or service runtime target."
       "Mutable image tags such as :latest are deployment intent only; runtime closure compares immutable target digest with observed running digest."
       "A missing service.manifest.toml, runtime target, deploy-center slug, Secret Store ref, or DB adoption plan is a production blocker, not a warning."
       "MissionD self-deploy writes release-evidence.json and closure-verdict.json beside release-manifest.json; active symlink changes are lease-owned, and launchd runs from the release-local source snapshot rather than a mutable operator worktree."
       "MissionD self-deploy MUST capture the active release generation before build and fail closed before active switch if another deploy changed active, unless an explicit active-release-race override is present."
       "deploy-ops agents may audit, plan, and collect approved read-only diagnostics by default, but production deploy, rollback, DNS mutation, secret mutation, SSH, and break-glass actions require deploy-center policy or explicit Board/user approval."
       "Production resolver/readiness MUST surface abi_freshness_mismatch when binary/runtime compiled hashes disagree; silent candidate fallback is forbidden for closure decisions."]
    :diagnostics [reported_digest_missing runtime_digest_mismatch provenance_partial db_adoption_required abi_freshness_mismatch release_lease_conflict deployment_lane_mismatch deploy_blocked_by_secret_store target_digest_missing final_runtime_digest_missing final_source_commit_missing source_commit_mismatch]
    :surfaces [deployment-event-ingest m6-deployment-confirmation deployment-evidence-preflight deploy-agent-self-update-governance mission_project mission_timeline.wait compiled-deployment-policy scripts/deploy-daemon.sh deploy-center-release-closure-ledger]
    :checker "node scripts/check-v3-deployment-closure-plane.mjs")

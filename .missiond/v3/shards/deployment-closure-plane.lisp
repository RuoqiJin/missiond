(deployment-closure-plane
    :schema "missiond.deployment-closure-plane.v1"
    :purpose "Unify MissionD self-deploy, XJP service deploys, and new product deploy templates behind typed release evidence and a fail-closed closure verdict."
    :authorities
      ((missiond :owns [project-identity deployment-policy maturity work-order approval eventbridge display-cache])
       (deploy-center :owns [runtime-target-inventory release-closure-authority release-evidence closure-verdict release-lease runtime-observation])
       (deploy-agent :owns [controlled-runtime-actions read-only-runtime-inspection evidence-reporting])
       (secret-store :owns [credential-values credential-availability])
       (deploy-ops-agent :owns [preflight-report release-evidence-review closure-verdict-review rollback-plan postmortem]))
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
       (record ReleasePlan
         :fields [project service env strict runner_bindings artifact_policy secret_requirements diagnostics source_refs]
         :invariant "Production deploy trigger paths MUST compile a ReleasePlan before creating deploy logs, native workflow jobs, runtime mutation tasks, or frontend/domain release tasks. ReleasePlan blockers are fail-closed and must not be bypassed by executor fallback.")
       (record RunnerBinding
         :fields [role runner_name runner_labels builder_id required_capabilities readiness_snapshot allowed_agent_ids]
         :roles [build_runner runtime_runner frontend_runner domain_runner self_update_runner]
         :invariant "RunnerBinding.role is authoritative: gcp-agent is runtime_runner only, privatecloud 10900kf/12900kf lanes are build_runner lanes, and macmini is limited to self_update_runner plus explicitly declared Darwin/Vercel lanes.")
       (record SecretRequirement
         :fields [env_name secret_ref scope required availability_result evidence_ref]
         :invariant "Secret Store owns values; Deploy Center preflight records only availability and projected runner_required_env metadata.")
       (record ReleaseCandidate
         :fields [project executor_project git_sha image_digest builder artifact_lane manifest_hash compiled_abi_hash migration_plan rollback_artifact])
       (record ReleaseLease
         :fields [project executor_project service runtime_target owner expected_active_root expected_running_digest generation expires_at conflict_policy])
       (record RuntimeObservation
         :fields [release_id deploy_log_id project executor_project running_image_digest container_id compose_files entrypoint binary_marker env_ref_names db_migration_state caddy_dns_binding health_result])
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
       "Deploy-chain continuation MUST be evidence-gated in order: Deploy Center/MissionD self-update lane first, then pay, search, legal, project-universe, and domain-route closure. scripts/audit-deploy-chain-closure.mjs is the read-only operator audit for this sequence and must separate public route health from Deploy Center provenance closure."
       "GitHub workflow success and deploy-center notify HTTP 200 only move a task to wait_for_provenance."
       "Every production deploy writes or references a ReleaseLease before mutating an active symlink, compose target, or service runtime target."
       "ReleaseCandidate.project, ReleaseLease.project, RuntimeObservation.project, and ClosureVerdict.project MUST be the canonical Deploy Center pipeline/stage project slug. Agent-local executor_project is execution address/config identity only and MUST NOT be used as the release owner or registry project."
       "Pull-mode deploy agents MUST preserve both identities from Deploy Center claims: XJP_DEPLOY_PROJECT/project_slug for release ownership, XJP_EXECUTOR_PROJECT/executor_project for local locks, work dirs, and executor routing. Compatibility fallbacks may use the local project only when the claim lacks explicit deployment identity."
       "Release evidence for leased runtime mutations MUST be written through the release-scoped ABI POST /api/deploy/releases/:release_id/evidence before POST /api/deploy/releases/:release_id/close. Task-log runtime evidence endpoints are compatibility-only and MUST NOT be the sole evidence path for closure decisions."
       "Every deploy-center runtime deploy task creation path, including GitHub Actions CI handoff and native_workflow build-to-deploy chaining, MUST record manifest_result and secret_availability_result as release evidence before assigning the runtime executor. Runtime agents report observed runtime facts; deploy-center owns control-plane preflight evidence."
       "Every production trigger path MUST be ReleasePlan-driven. A missing ReleasePlan, ReleasePlan diagnostics with severity=blocker, missing runner_required_env projection, missing Secret Store availability evidence, or missing runner readiness snapshot is a production blocker."
       "gcp-agent MUST NOT claim or execute build_runner jobs. If privatecloud build runners are offline or stale, the release is blocked with build_runner_unavailable; Deploy Center MUST NOT fallback to GCP docker build, source build, docker compose build, or other target-side build."
       "macmini MUST NOT claim generic Linux/Docker build work. macmini lanes are limited to MissionD self-update, Darwin-arm64 local build, and explicitly declared Vercel/native frontend tooling lanes."
       "Deploy-chain self-update preflight MUST expose credential presence and Secret Store refs only; read-token refs may default to secret-store://missiond/production/MISSIOND_DEPLOY_CENTER_READ_TOKEN, but write-token values must never be emitted."
       "Deploy-chain closure audit MUST validate compiled Project Universe service config before provenance continuation: canonical service id, Deploy Center slug, build_runner/runtime_runner channel split, GCP build prohibition, target-side build prohibition, domain_management DNS records, and Caddy proxy intent are static config closure requirements."
       "Mutable image tags such as :latest are deployment intent only; runtime closure compares immutable target digest with observed running digest."
       "A missing service.manifest.toml, runtime target, deploy-center slug, Secret Store ref, or DB adoption plan is a production blocker, not a warning."
       "MissionD self-deploy writes release-evidence.json and closure-verdict.json beside release-manifest.json; active symlink changes are lease-owned, and launchd runs from the release-local source snapshot rather than a mutable operator worktree."
       "MissionD self-deploy MUST capture the active release generation before build and fail closed before active switch if another deploy changed active, unless an explicit active-release-race override is present."
       "MissionD self-deploy MUST compare the candidate git_full_sha with the current active release commit and fail closed when the candidate is behind or divergent, unless an explicit commit-regression override is present."
       "MissionD self-deploy MUST treat compiled runtime projections as release artifacts: compile them under the candidate release and switch MISSIOND_COMPILED_RUNTIME_DIR only with the active release, so failed deploys cannot leave source/ABI freshness mismatches."
       "deploy-ops agents may audit, plan, and collect approved read-only diagnostics by default, but production deploy, rollback, DNS mutation, secret mutation, SSH, and break-glass actions require deploy-center policy or explicit Board/user approval."
       "deploy-ops agent outputs MUST be exactly one of preflight-report, release-evidence-review, closure-verdict-review, rollback-plan, or postmortem; generic Findings/Evidence summaries are not closure artifacts for deploy-ops tasks."
       "Production resolver/readiness MUST surface abi_freshness_mismatch when binary/runtime compiled hashes disagree; silent candidate fallback is forbidden for closure decisions."]
    :diagnostics [reported_digest_missing runtime_digest_mismatch provenance_partial db_adoption_required abi_freshness_mismatch release_lease_conflict deployment_lane_mismatch release_owner_executor_project_mismatch release_scoped_evidence_missing runtime_preflight_evidence_missing deploy_blocked_by_secret_store target_digest_missing final_runtime_digest_missing final_source_commit_missing source_commit_mismatch release_plan_missing release_plan_blocked build_runner_unavailable gcp_build_forbidden target_side_build_forbidden macmini_lane_forbidden runner_required_env_missing secret_availability_missing compiled_project_universe_unavailable service_runtime_config_missing deploy_center_slug_mismatch build_runner_role_missing runtime_runner_role_missing target_side_build_not_prohibited caddy_proxy_intent_missing domain_management_binding_missing dns_record_missing]
    :surfaces [deployment-event-ingest m6-deployment-confirmation deployment-evidence-preflight deploy-chain-closure-audit project-universe-domain-route-config deploy-agent-self-update-governance mission_project mission_timeline.wait compiled-deployment-policy scripts/deploy-daemon.sh deploy-center-release-closure-ledger]
    :checker "node scripts/check-v3-deployment-closure-plane.mjs")

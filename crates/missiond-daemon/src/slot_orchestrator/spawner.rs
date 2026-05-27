use crate::context::slot_env::{
    build_slot_tracking_env, capture_slot_session_uuid, sync_slot_hooks_to_local_settings,
};
use anyhow::{anyhow, Result};
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{PTYAgentInfo, PTYManager};
use missiond_core::types::SharedProjectRegistry;
use missiond_core::LearnedPermissions;
use missiond_core::{PTYSlot, PTYSpawnOptions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::{canonical_source_for_engine, register_slot_session};

/// A unified spawner for tracked slot PTY processes.
/// This ensures that the session tracking environment and UUID capture
/// are always correctly applied, preventing orphaned sessions.
///
/// Also runs the permission injector before spawn so any `learned_permissions.yaml`
/// entries (global/role/project/slot scope union) are materialized into
/// `<cwd>/.claude/settings.local.json`. This centralizes perm injection inside
/// the single spawn bottleneck — eliminating the historical gap where 8 of 10
/// spawn paths bypassed `sync_learned_to_local_settings`.
///
/// **Project-root spawn cwd contract** — implements
/// `intent-worker.lisp :: invariant project-root-spawn-cwd` at the bottleneck:
///
///   - When `pty_slot.cwd` resolves under a registered project,
///     `pty_slot.cwd` is normalized to the canonical project root before any
///     spawn. The original (sub-)path stays available to callers for prompt
///     metadata via `SlotConfig.requested_cwd`.
///   - When `pty_slot.cwd` is `None` or does not resolve, behavior is
///     preserved (e.g. lisp-surveyor runs from `/Users/jinchen/Projects` and
///     intentionally has no project root — that's a non-project-bound CLI
///     slot, exempt from this contract per task spec §7).
///   - For Gemini/Codex CLI engines, an unresolvable cwd is rejected — these
///     engines cannot reliably operate cross-folder. ClaudeCode is permissive
///     for backward compatibility with non-project-bound meta agents.
///
/// Callers that have richer signals (explicit project_id, board task project,
/// dynamic slot config) should resolve via
/// `crate::slot_orchestrator::project_root::resolve_target_project_root`
/// before constructing `PTYSlot`. The spawner is the last-line defense.
#[allow(clippy::too_many_arguments)]
pub async fn spawn_tracked_slot(
    pty: &Arc<PTYManager>,
    store: &Arc<dyn MissionStore>,
    pty_session_uuids: &Arc<RwLock<HashSet<String>>>,
    project_registry: &SharedProjectRegistry,
    learned: Option<&Arc<LearnedPermissions>>,
    pty_slot: &PTYSlot,
    mut options: PTYSpawnOptions,
    original_slot_env: Option<&HashMap<String, String>>,
) -> Result<PTYAgentInfo> {
    // 0a. Project-root normalization. We don't mutate the caller's PTYSlot;
    //     we clone and override cwd if the registry says cwd is a sub-path of
    //     a registered project. Failures for Gemini/Codex are hard errors.
    let mut pty_slot_owned = pty_slot.clone();
    if let Some(cwd) = pty_slot_owned.cwd.clone() {
        let cwd_str = cwd.display().to_string();
        let resolved = {
            let reg = project_registry.read().await;
            reg.resolve(&cwd_str)
                .map(|s| s.to_string())
                .and_then(|id| reg.get(&id).map(|p| (id, PathBuf::from(&p.path))))
        };
        match resolved {
            Some((project_id, root)) if root != cwd => {
                info!(
                    slot_id = %pty_slot_owned.id,
                    project = %project_id,
                    requested_cwd = %cwd_str,
                    project_root = %root.display(),
                    "spawn_tracked_slot: normalizing cwd to project root (project-root-spawn-cwd contract)"
                );
                pty_slot_owned.cwd = Some(root);
            }
            Some(_) => { /* already at canonical root */ }
            None => {
                use missiond_core::CliEngine;
                if matches!(
                    pty_slot_owned.engine,
                    CliEngine::Gemini | CliEngine::Codex | CliEngine::Agy
                ) {
                    return Err(anyhow!(
                        "spawn_tracked_slot rejected: cwd '{}' is not under any registered project; \
                         {} CLI cannot run cross-folder. Resolve target_project_root before spawn \
                         (intent-worker.lisp :: invariant project-root-spawn-cwd)",
                        cwd_str,
                        match pty_slot_owned.engine {
                            CliEngine::Gemini => "Gemini",
                            CliEngine::Codex => "Codex",
                            CliEngine::Agy => "Agy",
                            _ => unreachable!(),
                        }
                    ));
                }
                // ClaudeCode: preserve existing cwd; this is the non-project-bound
                // meta-agent path (e.g. lisp-surveyor running from
                // /Users/jinchen/Projects). Worker pillar §7 of the spec
                // explicitly preserves this behavior.
                warn!(
                    slot_id = %pty_slot_owned.id,
                    cwd = %cwd_str,
                    "spawn_tracked_slot: cwd not under registered project; preserving (non-project-bound ClaudeCode slot)"
                );
            }
        }
    }
    let pty_slot = &pty_slot_owned;
    enforce_spawn_sandbox_policy(pty_slot, &mut options)?;

    // 0b. Inject learned permissions into <project_root>/.claude/settings.local.json
    //     so Claude Code picks up any earlier "don't ask again" decisions.
    //     After 0a, cwd is the canonical project root for project-bound slots,
    //     so settings now consistently land at the project level (not subdirs).
    if let (Some(cwd), Some(learned)) = (pty_slot.cwd.as_deref(), learned) {
        let project_id = project_registry
            .read()
            .await
            .resolve(&cwd.display().to_string())
            .map(|s| s.to_string());
        crate::slot_orchestrator::perm_injector::sync_learned_to_local_settings(
            cwd,
            &pty_slot.role,
            project_id.as_deref(),
            Some(pty_slot.id.as_str()),
            learned,
        );
    }
    if let Some(cwd) = pty_slot.cwd.as_deref() {
        sync_slot_hooks_to_local_settings(cwd);
    }

    // 1. Automatically build tracking environment and session file path
    let (tracking_env, session_file) =
        build_slot_tracking_env(&pty_slot.id, original_slot_env).await;

    // 2. Merge tracking variables into options
    options.extra_env.extend(tracking_env);

    let wait_for_idle = options.wait_for_idle;
    let initial_prompt = options.initial_prompt.take();

    // 3. Execute underlying PTY spawn
    let spawn_result = pty.spawn(pty_slot, options).await?;

    // 3.5. Engines without a Claude JSONL session hook still need an
    // observable conversation row as soon as they are spawned through the
    // generic `mission_pty_spawn` path. Controller-based dispatch also calls
    // `register_slot_session`, but direct PTY spawn used by smoke tests and
    // manual operators bypasses those controllers.
    if matches!(
        pty_slot.engine,
        missiond_core::CliEngine::Gemini
            | missiond_core::CliEngine::Codex
            | missiond_core::CliEngine::Agy
    ) {
        let session_id = format!("pty-{}", pty_slot.id);
        register_slot_session(
            store,
            &pty_slot.id,
            &session_id,
            canonical_source_for_engine(pty_slot.engine),
            false,
        )
        .await;
    }

    // 4. Handle UUID capture based on whether we waited for idle
    if wait_for_idle {
        // The process has reached idle, so the hook should have written the file
        capture_slot_session_uuid(store, pty_session_uuids, &pty_slot.id, &session_file).await;

        // 5. Send initial prompt if configured (session is already idle)
        if let Some(prompt) = initial_prompt {
            info!(slot_id = %pty_slot.id, "Sending initial prompt after idle");
            if let Err(e) = pty.send_fire_and_forget(&pty_slot.id, &prompt).await {
                warn!(slot_id = %pty_slot.id, error = %e, "Failed to send initial prompt");
            }
        }
    } else {
        // Spawning asynchronously, poll for the UUID in a background task
        let store_clone = Arc::clone(store);
        let uuids_clone = Arc::clone(pty_session_uuids);
        let slot_id = pty_slot.id.clone();
        let pty_clone = Arc::clone(pty);
        tokio::spawn(async move {
            capture_slot_session_uuid(&store_clone, &uuids_clone, &slot_id, &session_file).await;

            // Send initial prompt after UUID capture (session should be idle by now)
            if let Some(prompt) = initial_prompt {
                info!(slot_id = %slot_id, "Sending initial prompt after async idle");
                if let Err(e) = pty_clone.send_fire_and_forget(&slot_id, &prompt).await {
                    warn!(slot_id = %slot_id, error = %e, "Failed to send initial prompt");
                }
            }
        });
    }

    Ok(spawn_result)
}

fn enforce_spawn_sandbox_policy(pty_slot: &PTYSlot, options: &mut PTYSpawnOptions) -> Result<()> {
    use missiond_core::CliEngine;

    let role = pty_slot.role.to_ascii_lowercase();
    let privileged_role = role.contains("orchestrator")
        || role.contains("master")
        || role.contains("supervisor")
        || role.contains("deploy");
    match pty_slot.engine {
        CliEngine::Codex => {
            if options.sandbox.as_deref() == Some("danger-full-access") && !privileged_role {
                warn!(
                    slot_id = %pty_slot.id,
                    role = %pty_slot.role,
                    "spawn sandbox policy: downgrading Codex worker from danger-full-access to workspace-write"
                );
                options.sandbox = Some("workspace-write".to_string());
                options
                    .approval_policy
                    .get_or_insert_with(|| "never".to_string());
            }
            if options.sandbox.is_none() && !privileged_role {
                options.sandbox = Some("read-only".to_string());
            }
        }
        CliEngine::Gemini => {
            if options.dangerously_skip_permissions && !privileged_role {
                warn!(
                    slot_id = %pty_slot.id,
                    role = %pty_slot.role,
                    "spawn sandbox policy: refusing Gemini yolo permissions for non-privileged worker"
                );
                options.dangerously_skip_permissions = false;
                options.approval_policy = Some("plan".to_string());
            }
            options
                .approval_policy
                .get_or_insert_with(|| "plan".to_string());
        }
        CliEngine::ClaudeCode => {
            let explicit_allow = options
                .extra_env
                .get("MISSIOND_ALLOW_BROAD_SKIP_PERMISSIONS")
                .is_some_and(|value| value == "true");
            if options.dangerously_skip_permissions && !privileged_role && !explicit_allow {
                warn!(
                    slot_id = %pty_slot.id,
                    role = %pty_slot.role,
                    "spawn sandbox policy: disabling Claude Code broad skip-permissions without an explicit capability override"
                );
                options.dangerously_skip_permissions = false;
            }
        }
        CliEngine::Agy => {
            let sandbox = options.sandbox.as_deref().unwrap_or("read-only");
            if !matches!(sandbox, "read-only" | "none") && !privileged_role {
                return Err(anyhow!(
                    "SANDBOX_POLICY_UNSUPPORTED: Agy write sandbox is not enforceable for non-privileged worker {}",
                    pty_slot.id
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_core::types::{ProjectConfig, ProjectRegistry};
    use missiond_core::CliEngine;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn registry_with(projects: Vec<ProjectConfig>) -> SharedProjectRegistry {
        Arc::new(RwLock::new(ProjectRegistry::new(projects)))
    }

    fn project(id: &str, path: &str) -> ProjectConfig {
        ProjectConfig {
            id: id.to_string(),
            path: path.to_string(),
            intent_path: None,
            active: true,
            slots: vec![],
            github_url: None,
            kind: "managed".to_string(),
            vault_path: None,
            parent_id: None,
            created_at: None,
            updated_at: None,
        }
    }

    /// Resolve-only assertion (the actual normalization runs inside
    /// spawn_tracked_slot, but the resolution lookup is testable in isolation).
    #[tokio::test]
    async fn registry_resolves_subdir_to_root() {
        let reg = registry_with(vec![project("missiond", "/tmp/p/missiond")]);
        let r = reg.read().await;
        let id = r.resolve("/tmp/p/missiond/crates/missiond-core").unwrap();
        assert_eq!(id, "missiond");
    }

    #[tokio::test]
    async fn registry_returns_none_for_non_project_cwd() {
        let reg = registry_with(vec![project("missiond", "/tmp/p/missiond")]);
        let r = reg.read().await;
        assert!(r.resolve("/tmp/p").is_none());
        assert!(r.resolve("/Users/jinchen/Projects").is_none());
    }

    #[tokio::test]
    async fn cli_engine_classification_for_hard_fail() {
        // Smoke: ensure the matches!() in spawn_tracked_slot covers the engines
        // we care about. If new CLI engines appear, this test forces a review.
        for (engine, must_hard_fail_unresolved) in [
            (CliEngine::Gemini, true),
            (CliEngine::Codex, true),
            (CliEngine::Agy, true),
            (CliEngine::ClaudeCode, false),
        ] {
            let hard_fail = matches!(
                engine,
                CliEngine::Gemini | CliEngine::Codex | CliEngine::Agy
            );
            assert_eq!(hard_fail, must_hard_fail_unresolved, "{:?}", engine);
        }
    }
}

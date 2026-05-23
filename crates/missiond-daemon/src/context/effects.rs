use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Result};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOperation {
    Write,
    Append,
    Delete,
}

impl FileOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Append => "append",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectContext {
    pub(crate) feature_id: &'static str,
    pub(crate) effect_id: &'static str,
}

impl EffectContext {
    pub(crate) const fn new(feature_id: &'static str, effect_id: &'static str) -> Self {
        Self {
            feature_id,
            effect_id,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectContract {
    effect_id: &'static str,
    feature_id: &'static str,
    operation: FileOperation,
    path_pattern: &'static str,
    default_enabled: bool,
    kill_switch: Option<&'static str>,
}

const EFFECT_CONTRACTS: &[EffectContract] = &[
    EffectContract {
        effect_id: "global-claude-md-managed-section",
        feature_id: "global-claude-md-sync",
        operation: FileOperation::Write,
        path_pattern: "~/.claude/CLAUDE.md",
        default_enabled: false,
        kill_switch: Some("MISSIOND_CLAUDE_MD_SYNC"),
    },
    EffectContract {
        effect_id: "mission-global-instruction-write",
        feature_id: "mission_global_instruction",
        operation: FileOperation::Write,
        path_pattern: "~/.claude/CLAUDE.md",
        default_enabled: true,
        kill_switch: None,
    },
    EffectContract {
        effect_id: "xjpcode-briefing-write",
        feature_id: "xjpcode-briefing-worker",
        operation: FileOperation::Write,
        path_pattern: "~/.xjpcode/xjpcode.md",
        default_enabled: true,
        kill_switch: None,
    },
    EffectContract {
        effect_id: "project-vault-sync-write",
        feature_id: "project-vault-sync",
        operation: FileOperation::Write,
        path_pattern: "~/.missiond/vault/**",
        default_enabled: true,
        kill_switch: None,
    },
    EffectContract {
        effect_id: "gemini-shadow-settings-write",
        feature_id: "gemini-cli-auth-shadow-home",
        operation: FileOperation::Write,
        path_pattern: "$MISSIOND_HOME/gemini-*-home/.gemini/settings.json",
        default_enabled: true,
        kill_switch: None,
    },
];

pub(crate) fn write_text(ctx: EffectContext, path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    validate_file_effect(ctx, FileOperation::Write, path, EFFECT_CONTRACTS)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    audit(ctx, FileOperation::Write, path);
    Ok(())
}

pub(crate) fn atomic_write_text(
    ctx: EffectContext,
    path: &Path,
    content: impl AsRef<[u8]>,
) -> Result<()> {
    validate_file_effect(ctx, FileOperation::Write, path, EFFECT_CONTRACTS)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("effect target path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%fZ").to_string();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "effect-write".to_string());
    let tmp = parent.join(format!(".{}.tmp.{}", name, stamp));
    if let Err(err) = fs::write(&tmp, content) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    audit(ctx, FileOperation::Write, path);
    Ok(())
}

pub(crate) fn append_text(
    ctx: EffectContext,
    path: &Path,
    content: impl AsRef<[u8]>,
) -> Result<()> {
    validate_file_effect(ctx, FileOperation::Append, path, EFFECT_CONTRACTS)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(content.as_ref())?;
    audit(ctx, FileOperation::Append, path);
    Ok(())
}

pub(crate) fn remove_file(ctx: EffectContext, path: &Path) -> Result<()> {
    validate_file_effect(ctx, FileOperation::Delete, path, EFFECT_CONTRACTS)?;
    fs::remove_file(path)?;
    audit(ctx, FileOperation::Delete, path);
    Ok(())
}

fn validate_file_effect(
    ctx: EffectContext,
    operation: FileOperation,
    path: &Path,
    contracts: &[EffectContract],
) -> Result<()> {
    let contract = contracts
        .iter()
        .find(|contract| contract.effect_id == ctx.effect_id)
        .ok_or_else(|| anyhow!("undeclared effect_id {}", ctx.effect_id))?;
    if contract.feature_id != ctx.feature_id {
        return Err(anyhow!(
            "effect {} belongs to feature {}, got {}",
            ctx.effect_id,
            contract.feature_id,
            ctx.feature_id
        ));
    }
    if contract.operation != operation {
        return Err(anyhow!(
            "effect {} operation mismatch: declared {}, got {}",
            ctx.effect_id,
            contract.operation.as_str(),
            operation.as_str()
        ));
    }
    if !contract.default_enabled && !contract.kill_switch.map(env_truthy).unwrap_or(false) {
        return Err(anyhow!(
            "effect {} is disabled by default; set {}=1 to enable",
            ctx.effect_id,
            contract.kill_switch.unwrap_or("<no-kill-switch>")
        ));
    }
    if !path_matches_pattern(path, contract.path_pattern) {
        return Err(anyhow!(
            "effect {} path {} outside declared pattern {}",
            ctx.effect_id,
            path.display(),
            contract.path_pattern
        ));
    }
    Ok(())
}

fn path_matches_pattern(path: &Path, pattern: &str) -> bool {
    let normalized = normalize_path(path);
    let expected = expand_pattern(pattern);
    glob_match(&normalized, &expected)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn expand_pattern(pattern: &str) -> String {
    let mut out = pattern.to_string();
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy();
        out = out.replace('~', home.as_ref());
    }
    let mission_home = missiond_core::default_mission_home();
    let mission_home = mission_home.to_string_lossy();
    out = out.replace("$MISSIOND_HOME", mission_home.as_ref());
    out.replace('\\', "/")
}

fn glob_match(value: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return value == pattern;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0usize;
    if let Some(first) = parts.first() {
        if !value.starts_with(first) {
            return false;
        }
        cursor = first.len();
    }
    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(found) = value[cursor..].find(part) else {
            return false;
        };
        cursor += found + part.len();
    }
    if let Some(last) = parts.last() {
        if !last.is_empty() && !value.ends_with(last) {
            return false;
        }
    }
    true
}

fn env_truthy(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn audit(ctx: EffectContext, operation: FileOperation, path: &Path) {
    info!(
        feature_id = ctx.feature_id,
        effect_id = ctx.effect_id,
        operation = operation.as_str(),
        path = %path.display(),
        "MissionD effect guard allowed filesystem effect"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CONTRACTS: &[EffectContract] = &[
        EffectContract {
            effect_id: "test-write",
            feature_id: "test-feature",
            operation: FileOperation::Write,
            path_pattern: "/tmp/missiond-effect-test/**",
            default_enabled: true,
            kill_switch: None,
        },
        EffectContract {
            effect_id: "test-disabled",
            feature_id: "test-feature",
            operation: FileOperation::Write,
            path_pattern: "/tmp/missiond-effect-test/**",
            default_enabled: false,
            kill_switch: Some("MISSIOND_EFFECT_TEST_ENABLE"),
        },
    ];

    #[test]
    fn allows_matching_declared_effect() {
        let ctx = EffectContext::new("test-feature", "test-write");
        validate_file_effect(
            ctx,
            FileOperation::Write,
            Path::new("/tmp/missiond-effect-test/a.txt"),
            TEST_CONTRACTS,
        )
        .unwrap();
    }

    #[test]
    fn rejects_unknown_effect_id() {
        let ctx = EffectContext::new("test-feature", "missing");
        let err = validate_file_effect(
            ctx,
            FileOperation::Write,
            Path::new("/tmp/missiond-effect-test/a.txt"),
            TEST_CONTRACTS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("undeclared effect_id"));
    }

    #[test]
    fn rejects_wrong_operation() {
        let ctx = EffectContext::new("test-feature", "test-write");
        let err = validate_file_effect(
            ctx,
            FileOperation::Append,
            Path::new("/tmp/missiond-effect-test/a.txt"),
            TEST_CONTRACTS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("operation mismatch"));
    }

    #[test]
    fn rejects_path_outside_pattern() {
        let ctx = EffectContext::new("test-feature", "test-write");
        let err = validate_file_effect(
            ctx,
            FileOperation::Write,
            Path::new("/tmp/not-missiond-effect-test/a.txt"),
            TEST_CONTRACTS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("outside declared pattern"));
    }

    #[test]
    fn rejects_disabled_effect_without_kill_switch() {
        std::env::remove_var("MISSIOND_EFFECT_TEST_ENABLE");
        let ctx = EffectContext::new("test-feature", "test-disabled");
        let err = validate_file_effect(
            ctx,
            FileOperation::Write,
            Path::new("/tmp/missiond-effect-test/a.txt"),
            TEST_CONTRACTS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("disabled by default"));
    }

    #[test]
    fn allows_disabled_effect_with_kill_switch() {
        const ENABLED_CONTRACTS: &[EffectContract] = &[EffectContract {
            effect_id: "test-disabled-enabled",
            feature_id: "test-feature",
            operation: FileOperation::Write,
            path_pattern: "/tmp/missiond-effect-test/**",
            default_enabled: false,
            kill_switch: Some("MISSIOND_EFFECT_TEST_ENABLE_ALLOW"),
        }];
        std::env::set_var("MISSIOND_EFFECT_TEST_ENABLE_ALLOW", "1");
        let ctx = EffectContext::new("test-feature", "test-disabled-enabled");
        validate_file_effect(
            ctx,
            FileOperation::Write,
            Path::new("/tmp/missiond-effect-test/a.txt"),
            ENABLED_CONTRACTS,
        )
        .unwrap();
        std::env::remove_var("MISSIOND_EFFECT_TEST_ENABLE_ALLOW");
    }
}

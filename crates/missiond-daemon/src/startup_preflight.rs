use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupPreflightReport {
    pub(crate) ok: bool,
    pub(crate) fatal_count: usize,
    pub(crate) warning_count: usize,
    pub(crate) checks: Vec<StartupPreflightCheck>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartupPreflightCheck {
    pub(crate) name: String,
    pub(crate) status: StartupPreflightStatus,
    pub(crate) severity: StartupPreflightSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartupPreflightStatus {
    Ok,
    Warning,
    Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartupPreflightSeverity {
    Info,
    Warning,
    Fatal,
}

#[derive(Debug, Clone)]
pub(crate) struct StartupPreflightInput {
    pub(crate) project_root: PathBuf,
    pub(crate) slots_config: PathBuf,
    pub(crate) permission_config: PathBuf,
    pub(crate) learned_permissions: PathBuf,
    pub(crate) logs_dir: PathBuf,
    pub(crate) blueprint_path: Option<PathBuf>,
    pub(crate) mission_pg_url: Option<String>,
    pub(crate) codex_local_index: Option<PathBuf>,
}

pub(crate) fn run_startup_preflight(input: &StartupPreflightInput) -> StartupPreflightReport {
    let mut report = StartupPreflightReport {
        ok: true,
        fatal_count: 0,
        warning_count: 0,
        checks: Vec::new(),
    };

    check_mission_pg_url(&mut report, input.mission_pg_url.as_deref());
    check_required_file(&mut report, "slots_config", &input.slots_config);
    check_optional_file(&mut report, "permission_config", &input.permission_config);
    check_parent_dir(
        &mut report,
        "learned_permissions",
        &input.learned_permissions,
    );
    check_directory(&mut report, "logs_dir", &input.logs_dir);
    check_blueprint(
        &mut report,
        &input.project_root,
        input.blueprint_path.as_deref(),
    );
    check_codex_local_index(&mut report, input.codex_local_index.as_deref());

    report.ok = report.fatal_count == 0;
    report
}

pub(crate) fn default_codex_local_index_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex/state_5.sqlite"))
}

pub(crate) fn fatal_messages(report: &StartupPreflightReport) -> Vec<String> {
    report
        .checks
        .iter()
        .filter(|check| check.status == StartupPreflightStatus::Fatal)
        .map(|check| {
            if let Some(path) = check.path.as_deref() {
                format!("{}: {} ({})", check.name, check.message, path)
            } else {
                format!("{}: {}", check.name, check.message)
            }
        })
        .collect()
}

fn check_mission_pg_url(report: &mut StartupPreflightReport, mission_pg_url: Option<&str>) {
    match mission_pg_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(_) => push_ok(
            report,
            "mission_pg_url",
            None,
            "MISSION_PG_URL is configured",
        ),
        None => push_fatal(
            report,
            "mission_pg_url",
            None,
            "MISSION_PG_URL is required; PostgreSQL is the only MissionD runtime database",
        ),
    }
}

fn check_required_file(report: &mut StartupPreflightReport, name: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => match std::fs::File::open(path) {
            Ok(_) => push_ok(report, name, Some(path), "file exists and is readable"),
            Err(err) => push_fatal(
                report,
                name,
                Some(path),
                &format!("file exists but cannot be read: {err}"),
            ),
        },
        Ok(_) => push_fatal(report, name, Some(path), "path exists but is not a file"),
        Err(err) => push_fatal(
            report,
            name,
            Some(path),
            &format!("required file is missing or unreadable: {err}"),
        ),
    }
}

fn check_optional_file(report: &mut StartupPreflightReport, name: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => push_ok(report, name, Some(path), "file exists"),
        Ok(_) => push_warning(report, name, Some(path), "path exists but is not a file"),
        Err(_) => push_warning(report, name, Some(path), "optional file is absent"),
    }
}

fn check_parent_dir(report: &mut StartupPreflightReport, name: &str, path: &Path) {
    match path
        .parent()
        .and_then(|parent| std::fs::metadata(parent).ok())
    {
        Some(meta) if meta.is_dir() => push_ok(report, name, Some(path), "parent directory exists"),
        _ => push_warning(report, name, Some(path), "parent directory is absent"),
    }
}

fn check_directory(report: &mut StartupPreflightReport, name: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => push_ok(report, name, Some(path), "directory exists"),
        Ok(_) => push_warning(
            report,
            name,
            Some(path),
            "path exists but is not a directory",
        ),
        Err(err) => push_warning(
            report,
            name,
            Some(path),
            &format!("directory is missing or unreadable: {err}"),
        ),
    }
}

fn check_blueprint(report: &mut StartupPreflightReport, project_root: &Path, path: Option<&Path>) {
    if let Some(path) = path {
        check_optional_file(report, "v3_blueprint", path);
        return;
    }
    let expected = project_root
        .join(".missiond")
        .join("v3")
        .join("missiond-blueprint.lisp");
    push_warning(
        report,
        "v3_blueprint",
        Some(&expected),
        "missiond blueprint was not discovered from the current project root",
    );
}

fn check_codex_local_index(report: &mut StartupPreflightReport, path: Option<&Path>) {
    let Some(path) = path else {
        push_warning(
            report,
            "codex_provider_index",
            None,
            "home directory was not available; Codex provider-local index check skipped",
        );
        return;
    };

    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => push_ok(
            report,
            "codex_provider_index",
            Some(path),
            "provider-local Codex index exists; it is treated as read-only external input",
        ),
        Ok(_) => push_warning(
            report,
            "codex_provider_index",
            Some(path),
            "provider-local Codex index path is not a file",
        ),
        Err(_) => push_warning(
            report,
            "codex_provider_index",
            Some(path),
            "provider-local Codex index is absent; Codex ingestion will remain idle",
        ),
    }
}

fn push_ok(report: &mut StartupPreflightReport, name: &str, path: Option<&Path>, message: &str) {
    report.checks.push(check(
        name,
        StartupPreflightStatus::Ok,
        StartupPreflightSeverity::Info,
        path,
        message,
    ));
}

fn push_warning(
    report: &mut StartupPreflightReport,
    name: &str,
    path: Option<&Path>,
    message: &str,
) {
    report.warning_count += 1;
    report.checks.push(check(
        name,
        StartupPreflightStatus::Warning,
        StartupPreflightSeverity::Warning,
        path,
        message,
    ));
}

fn push_fatal(report: &mut StartupPreflightReport, name: &str, path: Option<&Path>, message: &str) {
    report.fatal_count += 1;
    report.checks.push(check(
        name,
        StartupPreflightStatus::Fatal,
        StartupPreflightSeverity::Fatal,
        path,
        message,
    ));
}

fn check(
    name: &str,
    status: StartupPreflightStatus,
    severity: StartupPreflightSeverity,
    path: Option<&Path>,
    message: &str,
) -> StartupPreflightCheck {
    StartupPreflightCheck {
        name: name.to_string(),
        status,
        severity,
        path: path.map(|path| path.display().to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "missiond-startup-preflight-{}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn startup_preflight_fails_when_pg_url_or_slots_config_missing() {
        let root = temp_path("missing-root");
        let report = run_startup_preflight(&StartupPreflightInput {
            project_root: root.clone(),
            slots_config: root.join("slots.yaml"),
            permission_config: root.join("config").join("permissions.yaml"),
            learned_permissions: root.join("config").join("learned_permissions.yaml"),
            logs_dir: root.join("logs"),
            blueprint_path: None,
            mission_pg_url: None,
            codex_local_index: Some(root.join(".codex").join("state_5.sqlite")),
        });

        assert!(!report.ok);
        assert!(fatal_messages(&report)
            .iter()
            .any(|message| message.contains("MISSION_PG_URL")));
        assert!(fatal_messages(&report)
            .iter()
            .any(|message| message.contains("slots_config")));
    }

    #[test]
    fn startup_preflight_treats_missing_codex_index_as_warning() {
        let root = temp_path("warning-root");
        let config = root.join("config");
        let logs = root.join("logs");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(root.join(".missiond").join("v3")).unwrap();
        let slots = root.join("slots.yaml");
        let blueprint = root
            .join(".missiond")
            .join("v3")
            .join("missiond-blueprint.lisp");
        std::fs::write(&slots, "slots: []\n").unwrap();
        std::fs::write(&blueprint, "(missiond)\n").unwrap();

        let report = run_startup_preflight(&StartupPreflightInput {
            project_root: root.clone(),
            slots_config: slots,
            permission_config: config.join("permissions.yaml"),
            learned_permissions: config.join("learned_permissions.yaml"),
            logs_dir: logs,
            blueprint_path: Some(blueprint),
            mission_pg_url: Some("postgres://localhost/missiond".to_string()),
            codex_local_index: Some(root.join(".codex").join("state_5.sqlite")),
        });

        assert!(report.ok);
        assert!(report.warning_count > 0);
        assert!(report
            .checks
            .iter()
            .any(|check| check.name == "codex_provider_index"
                && check.status == StartupPreflightStatus::Warning));

        let _ = std::fs::remove_dir_all(root);
    }
}

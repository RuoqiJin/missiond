use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use crate::state::AppState;

use super::args::KBDiscoverArgs;

pub(super) async fn handle_kb_discover(state: &AppState, args: Value) -> Result<ToolResult> {
    let KBDiscoverArgs {
        host,
        port,
        password,
    } = serde_json::from_value(args)?;

    let (ssh_user, ssh_host, ssh_port, ssh_pass) = if !host.contains('@') && !host.contains('.') {
        let server = state.infra.read().unwrap().get(&host).cloned();
        let ip_owned = server
            .and_then(|s| s.host.clone())
            .unwrap_or_else(|| host.clone());
        let ip = ip_owned.as_str();
        let cred_pass = state
            .store
            .kb_search(&format!("{} password", host), Some("credential"))
            .await
            .ok()
            .and_then(|entries| entries.into_iter().next())
            .and_then(|e| {
                e.detail
                    .as_ref()
                    .and_then(|d| d.get("password").and_then(|v| v.as_str().map(String::from)))
            });
        (
            "root".to_string(),
            ip.to_string(),
            port.unwrap_or(22),
            password.or(cred_pass),
        )
    } else if host.contains('@') {
        let parts: Vec<&str> = host.splitn(2, '@').collect();
        (
            parts[0].to_string(),
            parts[1].to_string(),
            port.unwrap_or(22),
            password,
        )
    } else {
        (
            "root".to_string(),
            host.clone(),
            port.unwrap_or(22),
            password,
        )
    };

    let probe_script = concat!(
        "echo \"HOSTNAME=$(hostname)\"\n",
        "echo \"UNAME=$(uname -a)\"\n",
        "echo \"OS=$(. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\" || echo unknown)\"\n",
        "echo \"CPU=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo unknown)\"\n",
        "MEM=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{print $2}'); echo \"MEM=${MEM:-unknown}\"\n",
        "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{print $2}'); echo \"DISK=${DISK:-unknown}\"\n",
        "echo \"UPTIME=$(uptime -p 2>/dev/null || uptime || echo unknown)\"\n",
        "DOCKER=$(docker ps --format '{{.Names}}:{{.Image}}' 2>/dev/null | tr '\\n' ','); echo \"DOCKER=${DOCKER:-none}\"\n",
        "LISTEN=$(LANG=C ss -tlnp 2>/dev/null | awk 'NR>1{print $4}' | tr '\\n' ','); echo \"LISTEN=${LISTEN:-unknown}\"\n",
    );

    let mut ssh_args: Vec<String> = Vec::new();
    if let Some(ref pass) = ssh_pass {
        ssh_args.extend(["sshpass".into(), "-p".into(), pass.clone(), "ssh".into()]);
    } else {
        ssh_args.push("ssh".into());
        ssh_args.extend(["-o".into(), "BatchMode=yes".into()]);
    }
    ssh_args.extend([
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-p".into(),
        ssh_port.to_string(),
        format!("{}@{}", ssh_user, ssh_host),
        "bash".into(),
    ]);

    let program = ssh_args.remove(0);
    let mut cmd = tokio::process::Command::new(&program);
    cmd.args(&ssh_args);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn SSH: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(probe_script.as_bytes()).await.ok();
        drop(stdin);
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| anyhow!("SSH failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(ToolResult::error(format!(
            "SSH probe failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut detail = serde_json::Map::new();
    for line in stdout.lines() {
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_lowercase();
            let val = v.trim().to_string();
            if !val.is_empty() && val != "unknown" && val != "none" {
                detail.insert(key, serde_json::Value::String(val));
            }
        }
    }

    detail.insert(
        "ssh_user".to_string(),
        serde_json::Value::String(ssh_user.clone()),
    );
    detail.insert(
        "ssh_host".to_string(),
        serde_json::Value::String(ssh_host.clone()),
    );
    if ssh_port != 22 {
        detail.insert(
            "ssh_port".to_string(),
            serde_json::Value::Number(ssh_port.into()),
        );
    }

    let hostname = detail
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let os = detail
        .get("os")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let cpu = detail.get("cpu").and_then(|v| v.as_str()).unwrap_or("?");
    let mem = detail.get("mem").and_then(|v| v.as_str()).unwrap_or("?");
    let summary = format!("{} — {} ({}C, {})", hostname, os, cpu, mem);
    let key = hostname.to_lowercase().replace(' ', "-");

    let input = missiond_core::types::KBRememberInput {
        category: "infra".to_string(),
        key: key.clone(),
        summary: summary.clone(),
        detail: Some(serde_json::Value::Object(detail.clone())),
        source: Some("discovery".to_string()),
        confidence: Some(1.0),
        project_id: None,
    };
    state
        .store
        .kb_remember(&input)
        .await
        .map_err(|e| anyhow!("DB error: {}", e))?;

    Ok(ToolResult::json(&serde_json::json!({
        "status": "discovered",
        "key": key,
        "summary": summary,
        "detail": detail,
    })))
}

use anyhow::{anyhow, Result};
use missiond_core::evidence_redactor;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

const WINDOWS_12900KF_SKILL: &str = "windows-runner";
const WINDOWS_12900KF_INFRA_ID: &str = "windows-12900kf";

#[derive(Deserialize)]
struct InfraListArgs {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Deserialize)]
struct InfraGetArgs {
    id: String,
}

#[derive(Deserialize)]
struct InfraEvidenceArgs {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default, alias = "project", alias = "projectId")]
    project_id: Option<String>,
    #[serde(default = "default_evidence_limit")]
    limit: usize,
}

#[derive(Deserialize)]
struct InfraDiagnosticProfileArgs {
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

#[derive(Deserialize)]
struct ReachabilityArgs {
    target: String,
    #[serde(default)]
    channels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct OsDiagnoseArgs {
    target: String,
    #[serde(default)]
    checks: Option<Vec<String>>,
}

pub(crate) async fn handle(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    // Consolidated tools
    if name == "mission_infra_query" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        return match action {
            "list" => handle_inner(state, "mission_infra_list", args).await,
            "get" => handle_inner(state, "mission_infra_get", args).await,
            "health" => handle_inner(state, "mission_infra_health", args).await,
            "reconcile" => handle_inner(state, "mission_infra_reconcile", args).await,
            "skill_evidence" => handle_inner(state, "mission_infra_skill_evidence", args).await,
            "credential_refs" => handle_inner(state, "mission_infra_credential_refs", args).await,
            "diagnostic_profiles" => {
                handle_inner(state, "mission_infra_diagnostic_profiles", args).await
            }
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    if name == "mission_infra_ops" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("health");
        return match action {
            "health" => crate::handlers::misc::handle(state, "mission_health", args).await,
            "reachability" => handle_inner(state, "mission_reachability", args).await,
            "diagnose" => handle_inner(state, "mission_os_diagnose", args).await,
            _ => Ok(ToolResult::error(format!("Unknown action: {}", action))),
        };
    }
    handle_inner(state, name, args).await
}

async fn handle_inner(state: &AppState, name: &str, args: Value) -> Result<ToolResult> {
    match name {
        // ===== Infrastructure Registry =====
        "mission_infra_list" => {
            let InfraListArgs { role, provider } =
                serde_json::from_value(args).unwrap_or(InfraListArgs {
                    role: None,
                    provider: None,
                });
            let servers = list_infra_servers(state, role.as_deref(), provider.as_deref());
            Ok(ToolResult::json_pretty(&servers))
        }
        "mission_infra_get" => {
            let InfraGetArgs { id } = serde_json::from_value(args)?;
            match get_infra_server(state, &id) {
                Some(server) => Ok(ToolResult::json_pretty(&server)),
                None => Ok(ToolResult::error(format!("Server not found: {}", id))),
            }
        }
        "mission_infra_health" => {
            let servers = list_infra_servers(state, None, None);
            let skill_evidence = collect_skill_evidence(
                state,
                InfraEvidenceFilter {
                    target_id: None,
                    skill: None,
                    query: None,
                    project_id: None,
                    limit: 200,
                },
            );
            let credential_risks = skill_evidence
                .iter()
                .filter(|item| {
                    item.get("credentialInlineRisk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                })
                .count();
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.infrastructure-health.v1",
                "runtimeTargets": servers.len(),
                "skillEvidenceItems": skill_evidence.len(),
                "credentialInlineRisks": credential_risks,
                "authority": {
                    "missiond": "project identity, Universe summary, dispatch, EventBridge",
                    "deployCenter": "deployment runtime facts, executors, release provenance",
                    "secretStore": "credential values",
                    "skills": "operational evidence and procedures"
                }
            })))
        }
        "mission_infra_skill_evidence" => {
            let args: InfraEvidenceArgs =
                serde_json::from_value(args).unwrap_or(InfraEvidenceArgs {
                    target_id: None,
                    skill: None,
                    query: None,
                    project_id: None,
                    limit: default_evidence_limit(),
                });
            let evidence = collect_skill_evidence(
                state,
                InfraEvidenceFilter {
                    target_id: args.target_id,
                    skill: args.skill,
                    query: args.query,
                    project_id: args.project_id,
                    limit: args.limit.min(500),
                },
            );
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.skill-infra-evidence.v1",
                "authority": "skills are evidence, not runtime truth",
                "redaction": "credential-like substrings are redacted; values must live in secret-store",
                "items": evidence
            })))
        }
        "mission_infra_credential_refs" => {
            let args: InfraEvidenceArgs =
                serde_json::from_value(args).unwrap_or(InfraEvidenceArgs {
                    target_id: None,
                    skill: None,
                    query: None,
                    project_id: None,
                    limit: default_evidence_limit(),
                });
            let refs = credential_refs_filtered(
                args.target_id.as_deref(),
                args.query.as_deref(),
                args.project_id.as_deref(),
            );
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.credential-ref-inventory.v1",
                "rule": "Only secret refs are returned. MissionD never returns credential values from Lisp, Board, or skills.",
                "credentialRefs": refs
            })))
        }
        "mission_infra_diagnostic_profiles" => {
            let args: InfraDiagnosticProfileArgs =
                serde_json::from_value(args).unwrap_or(InfraDiagnosticProfileArgs {
                    target_id: None,
                    service_id: None,
                    profile: None,
                });
            let profiles = diagnostic_profiles(
                args.target_id.as_deref(),
                args.service_id.as_deref(),
                args.profile.as_deref(),
            );
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.remote-diagnostic-profile.v1",
                "authority": "deploy-center owns remote runtime execution; MissionD only exposes profile requirements and consumes diagnostic artifacts",
                "executionRule": "Use deploy-center read-only diagnostic profiles. Do not guess deploy-agent API keys or run raw agent exec from MissionD.",
                "credentialAvailability": "unknown",
                "profiles": profiles
            })))
        }
        "mission_infra_reconcile" => {
            let servers = list_infra_servers(state, None, None);
            let skill_evidence = collect_skill_evidence(
                state,
                InfraEvidenceFilter {
                    target_id: None,
                    skill: None,
                    query: None,
                    project_id: None,
                    limit: 500,
                },
            );
            let credential_inline_risks: Vec<_> = skill_evidence
                .iter()
                .filter(|item| {
                    item.get("credentialInlineRisk")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            let runtime_fact_missing: Vec<_> = servers
                .iter()
                .filter(|server| {
                    server.provider == "skill-derived"
                        || server.tags.iter().any(|tag| tag == "unverified")
                })
                .map(|server| {
                    json!({
                        "targetId": server.id,
                        "kind": "runtime_fact_missing",
                        "message": "target is present as skill evidence or unverified runtime fact; deploy-center should own the verified runtime record",
                        "promoteTo": "deploy-center.runtime-target-inventory"
                    })
                })
                .collect();
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.infrastructure-reconcile.v1",
                "consistent": credential_inline_risks.is_empty() && runtime_fact_missing.is_empty(),
                "sources": {
                    "missiond": "Universe summary and worker dispatch",
                    "deployCenter": "runtime targets, executors, service deploy locations",
                    "secretStore": "credential values and rotations",
                    "skills": "evidence only"
                },
                "runtimeTargets": servers.len(),
                "skillEvidenceItems": skill_evidence.len(),
                "drift": {
                    "runtime_fact_missing": runtime_fact_missing,
                    "credential_inline_risk": credential_inline_risks
                }
            })))
        }

        "mission_reachability" => {
            let ReachabilityArgs { target, channels } = serde_json::from_value(args)?;

            let server: Option<missiond_core::InfraServer> = get_infra_server(state, &target);
            let public_ip = server.as_ref().and_then(|s| s.host.clone());
            let lan_ip = server.as_ref().and_then(|s| s.lan.clone());
            let server_name = server
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| target.clone());

            // Parse Tailscale IP from description (e.g. "ssh user@100.x.x.x")
            let ts_ip = server.as_ref().and_then(|s| {
                let targets = s.parse_ssh_targets();
                targets
                    .iter()
                    .find(|t| t.via == "tailscale")
                    .map(|t| t.host.clone())
            });

            let should_probe = |ch: &str| -> bool {
                channels
                    .as_ref()
                    .map_or(true, |chs| chs.iter().any(|c| c == ch))
            };

            // Probe 1: LAN ping
            let lan_ip_owned = lan_ip.map(String::from);
            let do_lan = should_probe("lan_ping") && lan_ip_owned.is_some();
            let lan_ping_fut = async {
                if !do_lan {
                    return None;
                }
                let ip = lan_ip_owned.as_ref().unwrap();
                let output = tokio::process::Command::new("ping")
                    .args(["-c", "3", "-W", "2", ip])
                    .output()
                    .await
                    .ok()?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let latency = stdout
                    .lines()
                    .find(|l| l.contains("avg"))
                    .and_then(|l| l.split('=').nth(1))
                    .and_then(|v| v.split('/').nth(1))
                    .and_then(|v| v.trim().parse::<f64>().ok());
                Some(serde_json::json!({
                    "reachable": output.status.success(),
                    "ip": ip,
                    "latency_ms": latency,
                    "error": if !output.status.success() { Some(stderr.trim().to_string()) } else { None::<String> }
                }))
            };

            // Probe 2: Public ping
            let public_ip_owned = public_ip.map(String::from);
            let do_pub = should_probe("public_ping") && public_ip_owned.is_some();
            let public_ping_fut = async {
                if !do_pub {
                    return None;
                }
                let ip = public_ip_owned.as_ref().unwrap();
                let output = tokio::process::Command::new("ping")
                    .args(["-c", "3", "-W", "2", ip])
                    .output()
                    .await
                    .ok()?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let latency = stdout
                    .lines()
                    .find(|l| l.contains("avg"))
                    .and_then(|l| l.split('=').nth(1))
                    .and_then(|v| v.split('/').nth(1))
                    .and_then(|v| v.trim().parse::<f64>().ok());
                Some(serde_json::json!({
                    "reachable": output.status.success(),
                    "ip": ip,
                    "latency_ms": latency,
                    "error": if !output.status.success() { Some(stderr.trim().to_string()) } else { None::<String> }
                }))
            };

            // Probe 3: Tailscale status
            let ts_ip_owned = ts_ip.clone();
            let do_ts = should_probe("tailscale");
            let tailscale_fut = async {
                if !do_ts {
                    return None;
                }
                let output = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    tokio::process::Command::new("tailscale")
                        .args(["status", "--json"])
                        .output(),
                )
                .await
                .ok()?
                .ok()?;
                if !output.status.success() {
                    return None;
                }
                let status_json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
                let peers = status_json.get("Peer")?.as_object()?;
                for (_key, peer) in peers {
                    let ips = peer.get("TailscaleIPs")?.as_array()?;
                    let ip_strs: Vec<&str> = ips.iter().filter_map(|v| v.as_str()).collect();
                    // Match by Tailscale IP from description
                    let matched = ts_ip_owned
                        .as_ref()
                        .map_or(false, |tip| ip_strs.contains(&tip.as_str()));
                    if matched {
                        let online = peer
                            .get("Online")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let last_seen = peer
                            .get("LastSeen")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let dns_name = peer.get("DNSName").and_then(|v| v.as_str()).unwrap_or("");
                        let hostname = dns_name.split('.').next().unwrap_or(
                            peer.get("HostName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown"),
                        );
                        let ip = ip_strs.first().unwrap_or(&"unknown");
                        return Some(serde_json::json!({
                            "status": if online { "online" } else { "offline" },
                            "hostname": hostname,
                            "ip": ip,
                            "last_seen": if online { "now".to_string() } else { last_seen.to_string() },
                            "os": peer.get("OS").and_then(|v| v.as_str()),
                        }));
                    }
                }
                Some(
                    serde_json::json!({ "status": "not_found", "error": "Node not found in Tailscale peers" }),
                )
            };

            // Probe 4: SSH TCP port
            let ssh_targets_owned: Vec<(String, u16, String)> = server
                .as_ref()
                .map(|s| {
                    s.parse_ssh_targets()
                        .into_iter()
                        .map(|t| (t.host, t.port, t.via))
                        .collect()
                })
                .unwrap_or_default();
            let do_ssh = should_probe("ssh") && !ssh_targets_owned.is_empty();
            let ssh_fut = async {
                if !do_ssh {
                    return None;
                }
                for (host, port, via) in &ssh_targets_owned {
                    let addr = format!("{}:{}", host, port);
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        tokio::net::TcpStream::connect(&addr),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {
                            return Some(serde_json::json!({
                                "reachable": true, "ip": host, "port": port, "via": via,
                            }));
                        }
                        Ok(Err(e)) => {
                            return Some(serde_json::json!({
                                "reachable": false, "ip": host, "port": port, "via": via,
                                "error": e.to_string(),
                            }));
                        }
                        Err(_) => continue, // timeout, try next
                    }
                }
                Some(
                    serde_json::json!({ "reachable": false, "error": "All SSH targets timed out" }),
                )
            };

            // Probe 5: Deploy agent HTTP (reads health_endpoint from servers.yaml)
            let health_url = server.as_ref().and_then(|s| s.health_endpoint.clone());
            let do_agent = should_probe("deploy_agent") && health_url.is_some();
            let agent_fut = async {
                if !do_agent {
                    return None;
                }
                let url = health_url.as_ref().unwrap();
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .danger_accept_invalid_certs(true)
                    .build()
                    .ok()?;
                match client.get(url).send().await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        Some(serde_json::json!({
                            "reachable": status == 200,
                            "http_status": status,
                        }))
                    }
                    Err(e) => Some(serde_json::json!({
                        "reachable": false,
                        "error": e.to_string(),
                    })),
                }
            };

            // Run all in parallel
            let (lan_ping, public_ping, tailscale, ssh, agent) = tokio::join!(
                lan_ping_fut,
                public_ping_fut,
                tailscale_fut,
                ssh_fut,
                agent_fut
            );

            let mut channels_result = serde_json::Map::new();
            if let Some(v) = lan_ping {
                channels_result.insert("lan_ping".into(), v);
            }
            if let Some(v) = public_ping {
                channels_result.insert("public_ping".into(), v);
            }
            if let Some(v) = tailscale {
                channels_result.insert("tailscale".into(), v);
            }
            if let Some(v) = ssh {
                channels_result.insert("ssh".into(), v);
            }
            if let Some(v) = agent {
                channels_result.insert("deploy_agent".into(), v);
            }

            let total = channels_result.len();
            let reachable = channels_result
                .values()
                .filter(|v| {
                    v.get("reachable")
                        .and_then(|r| r.as_bool())
                        .unwrap_or(false)
                        || v.get("status").and_then(|s| s.as_str()) == Some("online")
                })
                .count();

            let severity = if reachable == 0 {
                "red"
            } else if reachable < total {
                "yellow"
            } else {
                "green"
            };

            Ok(ToolResult::json_pretty(&serde_json::json!({
                "target": target,
                "name": server_name,
                "channels": channels_result,
                "summary": format!("{}/{} channels reachable", reachable, total),
                "severity": severity,
            })))
        }

        "mission_os_diagnose" => {
            let OsDiagnoseArgs { target, checks } = serde_json::from_value(args)?;

            let server: Option<missiond_core::InfraServer> = get_infra_server(state, &target);
            if server.is_none() && !target.contains('@') && !target.contains('.') {
                return Ok(ToolResult::error(format!(
                    "Server not found in infra registry: {}",
                    target
                )));
            }

            // Resolve SSH targets
            let ssh_targets = if let Some(srv) = server {
                let mut targets = srv.parse_ssh_targets();
                if targets.is_empty() {
                    if let Some(ip) = srv.host.as_deref() {
                        targets.push(missiond_core::types::SshTarget {
                            user: "root".to_string(),
                            host: ip.to_string(),
                            port: 22,
                            password: None,
                            via: "public".to_string(),
                        });
                    }
                }
                targets
            } else if target.contains('@') {
                let parts: Vec<&str> = target.splitn(2, '@').collect();
                vec![missiond_core::types::SshTarget {
                    user: parts[0].to_string(),
                    host: parts[1].to_string(),
                    port: 22,
                    password: None,
                    via: "direct".to_string(),
                }]
            } else {
                vec![missiond_core::types::SshTarget {
                    user: "root".to_string(),
                    host: target.clone(),
                    port: 22,
                    password: None,
                    via: "direct".to_string(),
                }]
            };

            if ssh_targets.is_empty() {
                return Ok(ToolResult::error(format!("No SSH targets for {}", target)));
            }

            // KB credential fallback
            let kb_pass = state
                .store
                .kb_search(&format!("{} password", target), Some("credential"))
                .await
                .ok()
                .and_then(|entries| entries.into_iter().next())
                .and_then(|e| {
                    e.detail
                        .as_ref()
                        .and_then(|d| d.get("password").and_then(|v| v.as_str().map(String::from)))
                });

            // Build probe script based on checks filter
            let all_checks = [
                "system",
                "crashes",
                "top_cpu",
                "temperatures",
                "journal_errors",
                "docker",
                "network",
                "gpu",
            ];
            let active: Vec<&str> = if let Some(ref chs) = checks {
                chs.iter()
                    .map(|s| s.as_str())
                    .filter(|c| all_checks.contains(c))
                    .collect()
            } else {
                all_checks.to_vec()
            };

            let mut script = String::from("#!/bin/bash\n");

            if active.contains(&"system") {
                script.push_str(concat!(
                    "echo 'SECTION=system'\n",
                    "echo \"HOSTNAME=$(hostname)\"\n",
                    "echo \"KERNEL=$(uname -r)\"\n",
                    "echo \"UPTIME=$(uptime)\"\n",
                    "LOAD=$(cat /proc/loadavg 2>/dev/null || echo unknown); echo \"LOAD=$LOAD\"\n",
                    "FREE=$(LANG=C free -h 2>/dev/null | awk '/Mem:/{printf \"%s/%s\", $3, $2}'); echo \"MEMORY=${FREE:-unknown}\"\n",
                    "DISK=$(LANG=C df -h / 2>/dev/null | awk 'NR==2{printf \"%s/%s (%s)\", $3, $2, $5}'); echo \"DISK=${DISK:-unknown}\"\n",
                    "DISK_PCT=$(LANG=C df / 2>/dev/null | awk 'NR==2{print $5}' | tr -d '%'); echo \"DISK_PCT=${DISK_PCT:-0}\"\n",
                    "echo \"NPROC=$(nproc 2>/dev/null || echo 1)\"\n",
                ));
            }

            if active.contains(&"crashes") {
                script.push_str(concat!(
                    "echo 'SECTION=crashes'\n",
                    "if [ -d /var/crash ]; then\n",
                    "  for f in /var/crash/*.crash; do\n",
                    "    [ -f \"$f\" ] || continue\n",
                    "    echo \"CRASH_FILE=$f\"\n",
                    "    grep -E '^(ProblemType|Date|ExecutablePath|Signal|SignalName|Package)' \"$f\" 2>/dev/null || true\n",
                    "    echo '---'\n",
                    "  done\n",
                    "else\n",
                    "  echo 'NO_CRASH_DIR'\n",
                    "fi\n",
                ));
            }

            if active.contains(&"top_cpu") {
                script.push_str(concat!(
                    "echo 'SECTION=top_cpu'\n",
                    "ps aux --sort=-%cpu 2>/dev/null | head -11 || echo 'PS_FAILED'\n",
                ));
            }

            if active.contains(&"temperatures") {
                script.push_str(concat!(
                    "echo 'SECTION=temperatures'\n",
                    "sensors 2>/dev/null || echo 'NO_SENSORS'\n",
                ));
            }

            if active.contains(&"journal_errors") {
                script.push_str(concat!(
                    "echo 'SECTION=journal_errors'\n",
                    "journalctl --since '1 hour ago' -p err -n 20 --no-pager 2>/dev/null || echo 'NO_JOURNALCTL'\n",
                ));
            }

            if active.contains(&"docker") {
                script.push_str(concat!(
                    "echo 'SECTION=docker'\n",
                    "docker ps --format 'table {{.Names}}\\t{{.Image}}\\t{{.Status}}' 2>/dev/null || echo 'NO_DOCKER'\n",
                ));
            }

            if active.contains(&"network") {
                script.push_str(concat!(
                    "echo 'SECTION=network'\n",
                    "ss -tlnp 2>/dev/null | head -30 || echo 'NO_SS'\n",
                ));
            }

            if active.contains(&"gpu") {
                script.push_str(concat!(
                    "echo 'SECTION=gpu'\n",
                    "nvidia-smi --query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu --format=csv,noheader 2>/dev/null || ",
                    "cat /sys/class/drm/card*/device/gpu_busy_percent 2>/dev/null || ",
                    "echo 'NO_GPU'\n",
                ));
            }

            // Try SSH targets in order
            let mut last_error = String::new();
            let mut connected_via = String::new();
            let mut raw_output = String::new();

            for st in &ssh_targets {
                let pass = st.password.as_ref().or(kb_pass.as_ref());
                let mut ssh_args: Vec<String> = Vec::new();
                if let Some(p) = pass {
                    ssh_args.extend(["sshpass".into(), "-p".into(), p.clone(), "ssh".into()]);
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
                    st.port.to_string(),
                    format!("{}@{}", st.user, st.host),
                    "bash".into(),
                ]);

                let program = ssh_args.remove(0);
                let mut cmd = tokio::process::Command::new(&program);
                cmd.args(&ssh_args);
                cmd.stdin(std::process::Stdio::piped());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            use tokio::io::AsyncWriteExt;
                            stdin.write_all(script.as_bytes()).await.ok();
                            drop(stdin);
                        }
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            child.wait_with_output(),
                        )
                        .await
                        {
                            Ok(Ok(output)) if output.status.success() => {
                                connected_via = format!("{} ({}:{})", st.via, st.host, st.port);
                                raw_output = String::from_utf8_lossy(&output.stdout).to_string();
                                break;
                            }
                            Ok(Ok(output)) => {
                                // Non-zero exit but may still have partial output
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                if !stdout.trim().is_empty() {
                                    connected_via = format!("{} ({}:{})", st.via, st.host, st.port);
                                    raw_output = stdout.to_string();
                                    break;
                                }
                                last_error =
                                    String::from_utf8_lossy(&output.stderr).trim().to_string();
                            }
                            Ok(Err(e)) => {
                                last_error = e.to_string();
                            }
                            Err(_) => {
                                last_error = format!(
                                    "SSH timed out (30s) via {} {}:{}",
                                    st.via, st.host, st.port
                                );
                            }
                        }
                    }
                    Err(e) => {
                        last_error = e.to_string();
                    }
                }
            }

            if raw_output.is_empty() {
                return Ok(ToolResult::error(format!(
                    "All SSH channels failed for '{}'. Last error: {}",
                    target, last_error
                )));
            }

            // Parse SECTION-based output
            let mut result = serde_json::Map::new();
            result.insert("target".into(), serde_json::json!(target));
            result.insert("connected_via".into(), serde_json::json!(connected_via));

            let mut current_section = String::new();
            let mut section_lines: Vec<String> = Vec::new();

            let parse_section = |name: &str, lines: &[String]| -> serde_json::Value {
                match name {
                    "system" => {
                        let mut obj = serde_json::Map::new();
                        for line in lines {
                            if let Some((k, v)) = line.split_once('=') {
                                let key = k.trim().to_lowercase();
                                let val = v.trim().to_string();
                                if !val.is_empty() && val != "unknown" {
                                    obj.insert(key, serde_json::Value::String(val));
                                }
                            }
                        }
                        serde_json::Value::Object(obj)
                    }
                    "crashes" => {
                        let mut crashes = Vec::new();
                        let mut current: serde_json::Map<String, serde_json::Value> =
                            serde_json::Map::new();
                        for line in lines {
                            if line == "NO_CRASH_DIR" {
                                return serde_json::json!([]);
                            }
                            if line.starts_with("CRASH_FILE=") {
                                if !current.is_empty() {
                                    crashes.push(serde_json::Value::Object(current.clone()));
                                }
                                current = serde_json::Map::new();
                                current.insert(
                                    "file".into(),
                                    serde_json::json!(line
                                        .strip_prefix("CRASH_FILE=")
                                        .unwrap_or("")),
                                );
                            } else if line == "---" {
                                if !current.is_empty() {
                                    crashes.push(serde_json::Value::Object(current.clone()));
                                }
                                current = serde_json::Map::new();
                            } else if let Some((k, v)) = line.split_once(": ") {
                                current.insert(
                                    k.to_lowercase().replace(' ', "_"),
                                    serde_json::json!(v.trim()),
                                );
                            }
                        }
                        serde_json::json!(crashes)
                    }
                    _ => {
                        // Raw text for top_cpu, temperatures, journal_errors, docker, network, gpu
                        let text = lines.join("\n");
                        serde_json::Value::String(text)
                    }
                }
            };

            for line in raw_output.lines() {
                if let Some(section_name) = line.strip_prefix("SECTION=") {
                    if !current_section.is_empty() {
                        result.insert(
                            current_section.clone(),
                            parse_section(&current_section, &section_lines),
                        );
                    }
                    current_section = section_name.to_string();
                    section_lines.clear();
                } else {
                    section_lines.push(line.to_string());
                }
            }
            if !current_section.is_empty() {
                result.insert(
                    current_section.clone(),
                    parse_section(&current_section, &section_lines),
                );
            }

            // Compute severity
            let mut severity = "green";
            if let Some(sys) = result.get("system").and_then(|v| v.as_object()) {
                // Check disk usage
                if let Some(pct) = sys
                    .get("disk_pct")
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<u32>().ok())
                {
                    if pct > 90 {
                        severity = "red";
                    } else if pct > 80 {
                        severity = "yellow";
                    }
                }
                // Check load vs nproc
                if let Some(load_str) = sys.get("load").and_then(|v| v.as_str()) {
                    if let Some(nproc) = sys
                        .get("nproc")
                        .and_then(|v| v.as_str())
                        .and_then(|v| v.parse::<f64>().ok())
                    {
                        if let Some(load1) = load_str
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse::<f64>().ok())
                        {
                            if load1 > nproc {
                                severity = "red";
                            } else if load1 > nproc * 0.8 && severity != "red" {
                                severity = "yellow";
                            }
                        }
                    }
                }
            }
            if let Some(crashes) = result.get("crashes").and_then(|v| v.as_array()) {
                if !crashes.is_empty() && severity != "red" {
                    severity = "yellow";
                }
            }

            result.insert("severity".into(), serde_json::json!(severity));

            Ok(ToolResult::json_pretty(&serde_json::Value::Object(result)))
        }

        _ => Err(anyhow!("Unknown infra tool: {name}")),
    }
}

fn default_evidence_limit() -> usize {
    100
}

struct InfraEvidenceFilter {
    target_id: Option<String>,
    skill: Option<String>,
    query: Option<String>,
    project_id: Option<String>,
    limit: usize,
}

fn collect_skill_evidence(state: &AppState, filter: InfraEvidenceFilter) -> Vec<Value> {
    let target = filter.target_id.as_deref().map(str::to_ascii_lowercase);
    let mut candidates: Vec<(i64, usize, Value)> = Vec::new();
    let mut ordinal = 0usize;
    for skill in state.skills.list() {
        let skill_name = skill.name.as_str();
        if filter
            .skill
            .as_deref()
            .map_or(false, |name| name != skill_name)
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&skill.path) else {
            continue;
        };
        let skill_has_target_context = skill_content_has_query_target(&content, &filter);
        for (idx, line) in content.lines().enumerate() {
            if !is_infra_evidence_line(line) {
                continue;
            }
            if let Some(target) = target.as_deref() {
                let lower = line.to_ascii_lowercase();
                if !lower.contains(target) && !skill_name.to_ascii_lowercase().contains(target) {
                    continue;
                }
            }
            let line_scoped = evidence_matches_scope(
                skill_name,
                &skill.path.display().to_string(),
                line,
                &filter,
            );
            let skill_context_scoped = !line_scoped
                && skill_has_target_context
                && skill_target_context_allows_deploy_closure_line(
                    skill_name, line, &content, &filter,
                );
            if !line_scoped && !skill_context_scoped {
                continue;
            }
            let mut score =
                evidence_scope_score(skill_name, &skill.path.display().to_string(), line, &filter);
            if skill_context_scoped {
                score += 6;
            }
            let (excerpt, credential_risk) = redact_skill_evidence_line(line);
            candidates.push((
                score,
                ordinal,
                json!({
                    "sourceSkill": skill_name,
                    "sourcePath": skill.path.display().to_string(),
                    "sourceLine": idx + 1,
                    "confidence": evidence_confidence(line),
                    "promoteTo": evidence_promotion_target(line),
                    "credentialInlineRisk": credential_risk,
                    "scopeMatch": if skill_context_scoped { "skill-target-context" } else { "line" },
                    "excerpt": excerpt
                }),
            ));
            ordinal += 1;
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    candidates
        .into_iter()
        .take(filter.limit)
        .map(|(_, _, item)| item)
        .collect()
}

fn evidence_matches_scope(
    skill_name: &str,
    skill_path: &str,
    line: &str,
    filter: &InfraEvidenceFilter,
) -> bool {
    if filter
        .target_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || filter
            .skill
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return true;
    }
    if query_has_specific_file_token_without_match(skill_name, skill_path, line, filter) {
        return false;
    }
    if line_mentions_unrequested_foreign_project(line, filter) {
        return false;
    }
    if lacks_line_anchor_for_broad_deploy_query(line, filter) {
        return false;
    }
    if lacks_project_anchor_and_query_term_density(skill_name, skill_path, line, filter) {
        return false;
    }
    let score = evidence_scope_score(skill_name, skill_path, line, filter);
    if filter
        .query
        .as_deref()
        .map(evidence_query_tokens)
        .is_none_or(|tokens| tokens.is_empty())
        && normalized_evidence_token(filter.project_id.as_deref()).is_none()
    {
        return score > 0;
    }
    score >= 4
}

fn line_mentions_unrequested_foreign_project(line: &str, filter: &InfraEvidenceFilter) -> bool {
    let Some(project) = normalized_evidence_token(filter.project_id.as_deref()) else {
        return false;
    };
    let query_tokens = filter
        .query
        .as_deref()
        .map(evidence_query_tokens)
        .unwrap_or_default();
    let line_haystack = line.to_ascii_lowercase();
    known_project_evidence_tokens()
        .iter()
        .filter(|token| **token != project)
        .filter(|token| !query_tokens.iter().any(|query_token| query_token == *token))
        .any(|token| contains_evidence_token(&line_haystack, token))
}

fn lacks_line_anchor_for_broad_deploy_query(line: &str, filter: &InfraEvidenceFilter) -> bool {
    let Some(query) = filter.query.as_deref() else {
        return false;
    };
    let query_tokens = evidence_query_tokens(query);
    if !query_tokens
        .iter()
        .any(|token| is_deploy_drift_anchor_token(token))
    {
        return false;
    }

    let line_haystack = line.to_ascii_lowercase();
    query_tokens
        .iter()
        .filter(|token| is_deploy_drift_anchor_token(token))
        .all(|token| !contains_evidence_token(&line_haystack, token))
}

fn lacks_project_anchor_and_query_term_density(
    skill_name: &str,
    skill_path: &str,
    line: &str,
    filter: &InfraEvidenceFilter,
) -> bool {
    let Some(query) = filter.query.as_deref() else {
        return false;
    };
    let tokens = evidence_query_tokens(query);
    if tokens.is_empty() {
        return false;
    }

    let line_haystack = line.to_ascii_lowercase();
    let project_token = normalized_evidence_token(filter.project_id.as_deref());
    if project_token.as_deref().is_some_and(|project| {
        project != "missiond" && contains_evidence_token(&line_haystack, project)
    }) {
        return false;
    }

    let match_weight = query_match_weight(skill_name, skill_path, line, filter);
    match_weight < 3
}

fn query_match_weight(
    skill_name: &str,
    skill_path: &str,
    line: &str,
    filter: &InfraEvidenceFilter,
) -> usize {
    let Some(query) = filter.query.as_deref() else {
        return 0;
    };
    let line_haystack = line.to_ascii_lowercase();
    let skill_haystack = format!("{skill_name}\n{skill_path}").to_ascii_lowercase();
    let project_token = normalized_evidence_token(filter.project_id.as_deref());
    evidence_query_tokens(query)
        .iter()
        .filter(|token| project_token.as_deref() != Some(token.as_str()))
        .filter(|token| {
            contains_evidence_token(&line_haystack, token)
                || contains_evidence_token(&skill_haystack, token)
        })
        .map(|token| {
            if is_weak_target_evidence_token(token) {
                1
            } else {
                2
            }
        })
        .sum()
}

fn query_has_specific_file_token_without_match(
    skill_name: &str,
    skill_path: &str,
    line: &str,
    filter: &InfraEvidenceFilter,
) -> bool {
    let Some(query) = filter.query.as_deref() else {
        return false;
    };
    let query_tokens = evidence_query_tokens(query);
    let specific_tokens: Vec<&str> = query_tokens
        .iter()
        .map(String::as_str)
        .filter(|token| token.contains('.') || token.contains('/'))
        .collect();
    if specific_tokens.is_empty() {
        return false;
    }

    let haystack = format!("{skill_name}\n{skill_path}\n{line}").to_ascii_lowercase();
    if specific_tokens
        .iter()
        .any(|token| contains_evidence_token(&haystack, token))
    {
        return false;
    }
    if line_matches_deploy_closure_sibling_evidence(line, &query_tokens) {
        return false;
    }

    let line_haystack = line.to_ascii_lowercase();
    let Some(project) = normalized_evidence_token(filter.project_id.as_deref()) else {
        return true;
    };
    if project == "missiond" || !contains_evidence_token(&line_haystack, &project) {
        return true;
    }

    let project = project.as_str();
    !evidence_query_tokens(query)
        .into_iter()
        .filter(|token| !token.contains('.') && !token.contains('/'))
        .filter(|token| token != project)
        .any(|token| contains_evidence_token(&line_haystack, &token))
}

fn line_matches_deploy_closure_sibling_evidence(line: &str, query_tokens: &[String]) -> bool {
    let line_haystack = line.to_ascii_lowercase();
    let target_tokens: Vec<&str> = query_tokens
        .iter()
        .map(String::as_str)
        .filter(|token| is_known_project_evidence_token(token))
        .collect();
    if !target_tokens.is_empty()
        && target_tokens
            .iter()
            .all(|token| !contains_evidence_token(&line_haystack, token))
    {
        return false;
    }

    let matched_anchor_count = query_tokens
        .iter()
        .filter(|token| is_deploy_closure_sibling_anchor_token(token))
        .filter(|token| deploy_closure_sibling_anchor_matches(&line_haystack, token))
        .count();
    matched_anchor_count >= 2
}

fn skill_target_context_allows_deploy_closure_line(
    skill_name: &str,
    line: &str,
    skill_content: &str,
    filter: &InfraEvidenceFilter,
) -> bool {
    if filter
        .target_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || filter
            .skill
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return false;
    }
    if !is_deploy_skill_context_source(skill_name) {
        return false;
    }
    if !skill_content_has_query_target(skill_content, filter) {
        return false;
    }
    if line_mentions_unrequested_foreign_project(line, filter) {
        return false;
    }
    let Some(query) = filter.query.as_deref() else {
        return false;
    };
    if !deployment_closure_phrase_overlap(query, line) {
        return false;
    }
    if !line_has_strong_deployment_closure_anchor(line) {
        return false;
    }
    if is_known_project_evidence_token(skill_name) {
        let skill_name_haystack = skill_name.to_ascii_lowercase();
        let query_tokens = evidence_query_tokens(query);
        if query_tokens
            .iter()
            .filter(|token| is_known_project_evidence_token(token))
            .all(|token| !contains_evidence_token(&skill_name_haystack, token))
        {
            return false;
        }
    }
    true
}

fn line_has_strong_deployment_closure_anchor(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "service.manifest.toml",
        "manifest gate",
        "db adoption",
        "_sqlx_migrations",
        "sqlx migrate",
        "relation",
        "old binary",
        "binary marker",
        "image marker",
        "entrypoint",
        "volume override",
        "releaselease",
        "runtimeobservation",
        "releaseevidence",
        "closureverdict",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_deploy_skill_context_source(skill_name: &str) -> bool {
    matches!(
        skill_name,
        "deploy-ops"
            | "backend-deploy"
            | "deployment-troubleshoot"
            | "xjp-deploy-agent"
            | "xjp-deploy-center"
            | "deploy-center"
            | "xiaojinpro-backend"
            | "sqlx-cache"
            | "payments"
    )
}

fn skill_content_has_query_target(content: &str, filter: &InfraEvidenceFilter) -> bool {
    let Some(query) = filter.query.as_deref() else {
        return false;
    };
    let target_tokens: Vec<String> = evidence_query_tokens(query)
        .into_iter()
        .filter(|token| is_known_project_evidence_token(token))
        .collect();
    if target_tokens.is_empty() {
        return false;
    }
    let content_haystack = content.to_ascii_lowercase();
    target_tokens
        .iter()
        .any(|token| contains_evidence_token(&content_haystack, token))
}

fn deployment_closure_phrase_overlap(query: &str, line: &str) -> bool {
    let query_lower = query.to_ascii_lowercase();
    let line_lower = line.to_ascii_lowercase();
    [
        &["service.manifest.toml", "manifest gate", "manifest"] as &[&str],
        &["canary", "smoke", "healthcheck", "health check"],
        &["migration", "sqlx migrate", "relation"],
        &[
            "old binary",
            "binary",
            "image marker",
            "entrypoint",
            "volume override",
        ],
    ]
    .iter()
    .any(|phrases| {
        phrases.iter().any(|phrase| query_lower.contains(phrase))
            && phrases.iter().any(|phrase| line_lower.contains(phrase))
    })
}

fn is_deploy_closure_sibling_anchor_token(token: &str) -> bool {
    matches!(
        token,
        "migration"
            | "relation"
            | "compose"
            | "entrypoint"
            | "binary"
            | "marker"
            | "volume"
            | "volumes"
    )
}

fn deploy_closure_sibling_anchor_matches(line_haystack: &str, token: &str) -> bool {
    match token {
        "migration" => {
            contains_evidence_token(line_haystack, "migration")
                || contains_evidence_token(line_haystack, "migrate")
                || line_haystack.contains("sqlx migrate")
        }
        "volume" | "volumes" => {
            contains_evidence_token(line_haystack, "volume")
                || contains_evidence_token(line_haystack, "volumes")
        }
        other => contains_evidence_token(line_haystack, other),
    }
}

fn evidence_scope_score(
    skill_name: &str,
    skill_path: &str,
    line: &str,
    filter: &InfraEvidenceFilter,
) -> i64 {
    if filter
        .target_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || filter
            .skill
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return 100;
    }
    let haystack = format!("{skill_name}\n{skill_path}\n{line}").to_ascii_lowercase();
    let line_haystack = line.to_ascii_lowercase();
    let skill_haystack = format!("{skill_name}\n{skill_path}").to_ascii_lowercase();
    let mut score = 0i64;

    let project_token = normalized_evidence_token(filter.project_id.as_deref());
    if let Some(project) = project_token.as_deref() {
        if project != "missiond" && contains_evidence_token(&haystack, project) {
            score += if contains_evidence_token(&line_haystack, project) {
                4
            } else {
                2
            };
        }
    }
    for term in filter
        .query
        .as_deref()
        .map(evidence_query_tokens)
        .unwrap_or_default()
    {
        if project_token.as_deref() == Some(term.as_str()) {
            continue;
        }
        let line_score = if is_weak_target_evidence_token(&term) {
            1
        } else {
            8
        };
        let skill_score = if is_weak_target_evidence_token(&term) {
            1
        } else {
            4
        };
        if contains_evidence_token(&line_haystack, &term) {
            score += line_score;
        } else if contains_evidence_token(&skill_haystack, &term) {
            score += skill_score;
        }
    }
    let query_weight = query_match_weight(skill_name, skill_path, line, filter);
    if query_weight >= 3 {
        score += query_weight as i64;
    }
    if score == 0
        && normalized_evidence_token(filter.project_id.as_deref()).is_none()
        && filter
            .query
            .as_deref()
            .map(evidence_query_tokens)
            .unwrap_or_default()
            .is_empty()
    {
        return 1;
    }
    score
}

fn contains_evidence_token(haystack: &str, token: &str) -> bool {
    if token.chars().any(|ch| matches!(ch, '-' | '_' | '.' | '/')) {
        return haystack.contains(token);
    }

    let bytes = haystack.as_bytes();
    let needle = token.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
        return false;
    }
    for idx in 0..=bytes.len() - needle.len() {
        if &bytes[idx..idx + needle.len()] != needle {
            continue;
        }
        let before = idx.checked_sub(1).and_then(|pos| bytes.get(pos)).copied();
        let after = bytes.get(idx + needle.len()).copied();
        if before.is_none_or(|ch| !is_evidence_word_byte(ch))
            && after.is_none_or(|ch| !is_evidence_word_byte(ch))
        {
            return true;
        }
        if token.len() > 3
            && token.bytes().all(|ch| ch.is_ascii_alphabetic())
            && after == Some(b's')
        {
            let plural_after = bytes.get(idx + needle.len() + 1).copied();
            if before.is_none_or(|ch| !is_evidence_word_byte(ch))
                && plural_after.is_none_or(|ch| !is_evidence_word_byte(ch))
            {
                return true;
            }
        }
    }
    false
}

fn is_evidence_word_byte(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn is_infra_evidence_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "12900kf",
        "hostvds",
        "deploy-agent",
        "agent_url",
        "router",
        "embedding",
        "rerank",
        "pcea",
        "ecs",
        "gcp",
        "bwg",
        "vps",
        "192.168.1.20",
        "192.168.1.19",
        "104.194.81.38",
        "45.156.24.163",
        "106.15.2.17",
        "tailscale",
        "harbor",
        "secret-store",
        "service.manifest.toml",
        "manifest gate",
        "deploy center provenance",
        "deploy-center provenance",
        "canary",
        "smoke",
        "docker-compose",
        "compose",
        "entrypoint",
        "old binary",
        "binary",
        "image marker",
        "migration",
        "sqlx migrate",
        "relation",
        "volume override",
        "volumes",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn evidence_confidence(line: &str) -> &'static str {
    let lower = line.to_ascii_lowercase();
    if lower.contains("verified") || lower.contains("已验证") || lower.contains("smoke") {
        "medium"
    } else if lower.contains("todo") || lower.contains("maybe") || lower.contains("候选") {
        "low"
    } else {
        "evidence-only"
    }
}

fn evidence_promotion_target(line: &str) -> &'static str {
    let lower = line.to_ascii_lowercase();
    if lower.contains("secret") || lower.contains("password") || lower.contains("密码") {
        "secret-store.credential-ref"
    } else if lower.contains("deploy") || lower.contains("agent") || lower.contains("ecs") {
        "deploy-center.runtime-target-inventory"
    } else {
        "missiond.infrastructure-universe.evidence"
    }
}

fn redact_skill_evidence_line(line: &str) -> (String, bool) {
    let lower = line.to_ascii_lowercase();
    let risk = lower.contains("sshpass")
        || lower.contains("password")
        || lower.contains("密码")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("api key")
        || lower.contains("secret");
    if !risk {
        return (line.trim().to_string(), false);
    }
    (evidence_redactor::redact_text(line.trim()).text, true)
}

fn diagnostic_profiles(
    target_id: Option<&str>,
    service_id: Option<&str>,
    profile: Option<&str>,
) -> Vec<Value> {
    let target_filter = target_id.map(str::to_ascii_lowercase);
    let profile_filter = profile.map(str::to_ascii_lowercase);
    let service = service_id.unwrap_or("unspecified");
    let targets = [
        (
            "ecs-pcea",
            "ecs",
            "http://104.194.81.38:9876/tunnel/proxy/ecs",
            vec!["pcea", "pcea-api", "pcea-video-vault"],
        ),
        (
            "gcp-runtime",
            "gcp",
            "http://34.104.147.118:9876",
            vec![
                "auth",
                "router",
                "deploy-center",
                "secret-store",
                "pcea-global",
            ],
        ),
        (
            "windows-12900kf",
            "windows",
            "http://104.194.81.38:9876/tunnel/proxy/windows",
            vec!["router", "embedding", "rerank"],
        ),
        (
            "privatecloud-10900kf",
            "privatecloud",
            "http://104.194.81.38:9876/tunnel/proxy/privatecloud",
            vec!["cn-builder", "harbor-cache", "deploy-jump"],
        ),
        (
            "synology-astrill-gw",
            "manual-jump",
            "credential-ref-required",
            vec!["domestic-jump", "network-gateway"],
        ),
        (
            "bwg-vps",
            "bwg",
            "http://104.194.81.38:9876",
            vec!["router-relay", "model-relay"],
        ),
    ];
    let profile_specs = [
        json!({
            "profileId": "deploy_provenance_snapshot",
            "allowedOperations": ["deploy-center.project-info", "deploy-center.provenance", "deploy-center.health"],
            "forbiddenOperations": ["raw-agent-exec", "secret-read", "container-env"],
            "artifactKind": "deploy-provenance-diagnostic",
            "requiresAgentCredential": false
        }),
        json!({
            "profileId": "container_inventory",
            "allowedOperations": ["agent.container-list", "docker ps --format name,image,status"],
            "forbiddenOperations": ["docker inspect env", "printenv", "cat /proc/*/environ", "mutating docker commands"],
            "artifactKind": "container-inventory-diagnostic",
            "requiresAgentCredential": true
        }),
        json!({
            "profileId": "dependency_manifest_scan",
            "allowedOperations": ["read package.json", "read package-lock.json", "read pnpm-lock.yaml", "read yarn.lock", "read pyproject.toml", "read requirements*.txt"],
            "forbiddenOperations": ["npm install", "pnpm install", "yarn install", "pip install", "python import", "node import", "lifecycle scripts"],
            "artifactKind": "dependency-manifest-diagnostic",
            "requiresAgentCredential": true
        }),
        json!({
            "profileId": "supply_chain_ioc_scan",
            "allowedOperations": ["grep known IoC strings in already-present files", "hash known suspicious setup/router files"],
            "forbiddenOperations": ["package install", "network fetch", "import-time execution", "credential rotation"],
            "artifactKind": "supply-chain-ioc-diagnostic",
            "requiresAgentCredential": true
        }),
    ];

    let mut items = Vec::new();
    for (target, lane, endpoint, service_ids) in targets {
        if target_filter
            .as_deref()
            .map_or(false, |filter| target != filter)
        {
            continue;
        }
        for spec in &profile_specs {
            let profile_id = spec
                .get("profileId")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if profile_filter
                .as_deref()
                .map_or(false, |filter| profile_id != filter)
            {
                continue;
            }
            items.push(json!({
                "targetId": target,
                "serviceId": service,
                "knownServiceIds": service_ids,
                "deployCenterLane": lane,
                "agentEndpoint": endpoint,
                "profileId": profile_id,
                "authority": "deploy-center",
                "readOnly": true,
                "requiredExecutor": "deploy-center-readonly-diagnostic-profile",
                "credentialRefs": credential_refs(Some(target)),
                "credentialAvailability": "unknown",
                "canExecuteFromMissionD": false,
                "eventSink": "SystemEvent::ExternalServiceEvent",
                "artifactSink": "task-result-artifact",
                "spec": spec,
                "policy": {
                    "noRawAgentExecFromMissionD": true,
                    "noCredentialValues": true,
                    "noInstallOrImport": true,
                    "noContainerEnvRead": true
                }
            }));
        }
    }
    items
}

fn credential_refs(target_id: Option<&str>) -> Vec<Value> {
    let refs = vec![
        json!({
            "targetId": "ecs-pcea",
            "namespace": "deploy-agent",
            "keyName": "DEPLOY_AGENT_ECS_API_KEY",
            "secretRef": "secret-store://deploy-agent/ecs/DEPLOY_AGENT_API_KEY",
            "purpose": "ECS/PCEA deploy-agent read-only diagnostics and deploy operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "gcp-runtime",
            "namespace": "deploy-agent",
            "keyName": "DEPLOY_AGENT_GCP_API_KEY",
            "secretRef": "secret-store://deploy-agent/gcp/DEPLOY_AGENT_API_KEY",
            "purpose": "GCP deploy-agent read-only diagnostics and deploy operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "windows-12900kf",
            "namespace": "deploy-agent",
            "keyName": "windows-agent-token",
            "secretRef": "secret-store://deploy-agent/windows-12900kf/agent-token",
            "purpose": "Windows deploy-agent / model runner operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "privatecloud-hostvds",
            "namespace": "ssh",
            "keyName": "hostvds-ssh",
            "secretRef": "secret-store://infra/privatecloud-hostvds/ssh",
            "purpose": "privatecloud/HostVDS operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "privatecloud-10900kf",
            "namespace": "deploy-agent",
            "keyName": "DEPLOY_AGENT_API_KEY",
            "secretRef": "secret-store://deploy-agent/DEPLOY_AGENT_API_KEY",
            "purpose": "privatecloud CN build/cache/jump operations on xjp-zibo-lan",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "synology-astrill-gw",
            "namespace": "ssh",
            "keyName": "synology-astrill-gw-ssh",
            "secretRef": "secret-store://infra/synology-astrill-gw/ssh",
            "purpose": "Synology VM domestic jump/network gateway operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "bwg-vps",
            "namespace": "tunnel",
            "keyName": "bwg-tunnel-ssh",
            "secretRef": "secret-store://infra/bwg-vps/tunnel-ssh",
            "purpose": "BWG tunnel and router/model relay operations",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "gcp-runtime",
            "namespace": "cloud",
            "keyName": "gcp-deploy-center-runtime",
            "secretRef": "secret-store://cloud/gcp/deploy-center-runtime",
            "purpose": "GCP production runtime and deploy-center agent access",
            "requiredCapability": "deploy-ops",
            "availability": "unknown"
        }),
        json!({
            "targetId": "gcp-runtime",
            "namespace": "secret-store",
            "keyName": "cloudflare/CLOUDFLARE_DNS_TOKEN",
            "secretRef": "secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN",
            "purpose": "Secret Store DNS migration / certificate recovery bootstrap reference",
            "requiredCapability": "dns-ops",
            "availability": "unknown"
        }),
    ];
    target_id.map_or(refs.clone(), |target| {
        refs.into_iter()
            .filter(|item| item.get("targetId").and_then(|v| v.as_str()) == Some(target))
            .collect()
    })
}

fn credential_refs_filtered(
    target_id: Option<&str>,
    query: Option<&str>,
    _project_id: Option<&str>,
) -> Vec<Value> {
    let refs = credential_refs(target_id);
    if target_id.is_some_and(|target| !target.trim().is_empty()) {
        return refs;
    }
    let terms = credential_query_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let required_capability = credential_required_capability(query);
    if !credential_query_mentions_secret_intent(query) && !credential_query_mentions_target(query) {
        return Vec::new();
    }
    if required_capability.is_none() && !credential_query_mentions_target(query) {
        return Vec::new();
    }
    let target_terms = credential_target_terms(query);
    refs.into_iter()
        .filter(|item| {
            if let Some(required_capability) = required_capability {
                if item
                    .get("requiredCapability")
                    .and_then(|value| value.as_str())
                    != Some(required_capability)
                {
                    return false;
                }
            }
            let haystack = item.to_string().to_ascii_lowercase();
            if !target_terms.is_empty() && !target_terms.iter().any(|term| haystack.contains(term))
            {
                return false;
            }
            terms.iter().any(|term| haystack.contains(term))
        })
        .collect()
}

fn credential_query_terms(query: Option<&str>) -> Vec<String> {
    query.map(evidence_query_tokens).unwrap_or_default()
}

fn credential_required_capability(query: Option<&str>) -> Option<&'static str> {
    let query = query?.to_ascii_lowercase();
    if query.contains("cloudflare")
        || query.contains("dns")
        || query.contains("certificate")
        || query.contains("cert")
    {
        return Some("dns-ops");
    }
    if query.contains("deploy")
        || query.contains("agent")
        || query.contains("canary")
        || query.contains("diagnostic")
        || query.contains("diagnostics")
    {
        return Some("deploy-ops");
    }
    None
}

fn credential_query_mentions_target(query: Option<&str>) -> bool {
    !credential_target_terms(query).is_empty()
}

fn credential_query_mentions_secret_intent(query: Option<&str>) -> bool {
    let Some(query) = query.map(str::to_ascii_lowercase) else {
        return false;
    };
    [
        "key",
        "keys",
        "secret",
        "credential",
        "credentials",
        "token",
        "access",
    ]
    .into_iter()
    .any(|token| query.contains(token))
}

fn credential_target_terms(query: Option<&str>) -> Vec<&'static str> {
    let Some(query) = query.map(str::to_ascii_lowercase) else {
        return Vec::new();
    };
    [
        "gcp",
        "ecs",
        "windows",
        "privatecloud",
        "hostvds",
        "synology",
        "bwg",
        "cloudflare",
        "dns",
    ]
    .into_iter()
    .filter(|token| query.contains(token))
    .collect()
}

fn evidence_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let lower = query.to_ascii_lowercase();
    for compound in [
        ("deploy agent", "deploy-agent"),
        ("deploy-agent", "deploy-agent"),
        ("deploy center", "deploy-center"),
        ("deploy-center", "deploy-center"),
    ] {
        if lower.contains(compound.0) {
            tokens.push(compound.1.to_string());
        }
    }
    for raw in
        query.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'))
    {
        let Some(token) = normalized_evidence_token(Some(raw)) else {
            continue;
        };
        if is_generic_evidence_token(&token) {
            continue;
        }
        if !tokens.iter().any(|existing| existing == &token) {
            tokens.push(token);
        }
    }
    tokens
}

fn normalized_evidence_token(value: Option<&str>) -> Option<String> {
    let token = value?.trim().trim_matches(|ch: char| {
        !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    });
    if token.is_empty() {
        return None;
    }
    let token = token.to_ascii_lowercase();
    if token.len() < 3 && !token.chars().any(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(token)
}

fn is_generic_evidence_token(token: &str) -> bool {
    matches!(
        token,
        "deploy"
            | "agent"
            | "runtime"
            | "service"
            | "project"
            | "status"
            | "state"
            | "success"
            | "failure"
            | "failed"
            | "workflow"
            | "github"
            | "center"
            | "production"
            | "key"
            | "keys"
            | "secret"
            | "secrets"
            | "credential"
            | "credentials"
            | "diagnostic"
            | "diagnostics"
            | "canary"
            | "access"
    )
}

fn known_project_evidence_tokens() -> &'static [&'static str] {
    &[
        "asr",
        "speechscribe",
        "payments",
        "xjp-payments",
        "xjp_payments",
        "xjp-router",
        "xjp_router",
        "xjp_auth",
        "xjp-auth",
        "xjp-backend",
        "xjp_backend",
        "pcea",
        "pcea-video-vault",
        "tiermate",
        "astrill",
        "openclaw",
        "aliyun",
    ]
}

fn is_known_project_evidence_token(token: &str) -> bool {
    known_project_evidence_tokens()
        .iter()
        .any(|known| *known == token)
}

fn is_deploy_drift_anchor_token(token: &str) -> bool {
    matches!(
        token,
        "service.manifest.toml"
            | "manifest"
            | "migration"
            | "relation"
            | "compose"
            | "entrypoint"
            | "binary"
            | "marker"
            | "volume"
            | "volumes"
    )
}

fn is_weak_target_evidence_token(token: &str) -> bool {
    matches!(
        token,
        "gcp"
            | "ecs"
            | "bwg"
            | "vps"
            | "windows"
            | "aliyun"
            | "cloud"
            | "image"
            | "images"
            | "compose"
            | "docker"
            | "build"
            | "ci"
            | "volume"
            | "volumes"
            | "deploy-center"
    )
}

fn list_infra_servers(
    state: &AppState,
    role: Option<&str>,
    provider: Option<&str>,
) -> Vec<missiond_core::InfraServer> {
    let mut servers = {
        let infra = state.infra.read().unwrap();
        infra.servers.clone()
    };
    append_skill_derived_infra_servers(&mut servers);
    servers
        .into_iter()
        .filter(|server| {
            role.map_or(true, |role| server.roles.iter().any(|r| r == role))
                && provider.map_or(true, |provider| server.provider == provider)
        })
        .collect()
}

fn get_infra_server(state: &AppState, id: &str) -> Option<missiond_core::InfraServer> {
    list_infra_servers(state, None, None)
        .into_iter()
        .find(|server| infra_id_matches(server, id))
}

fn infra_id_matches(server: &missiond_core::InfraServer, id: &str) -> bool {
    let needle = id.to_ascii_lowercase();
    server.id.eq_ignore_ascii_case(id)
        || server.name.to_ascii_lowercase().contains(&needle)
        || server
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(id) || tag.to_ascii_lowercase().contains(&needle))
}

fn append_skill_derived_infra_servers(servers: &mut Vec<missiond_core::InfraServer>) {
    maybe_push_skill_target(
        servers,
        WINDOWS_12900KF_INFRA_ID,
        missiond_core::InfraServer {
            id: WINDOWS_12900KF_INFRA_ID.to_string(),
            name: "Windows 12900KF / RTX 3090 Ti".to_string(),
            provider: "skill-derived".to_string(),
            host: Some("100.73.97.46".to_string()),
            lan: Some("192.168.1.19".to_string()),
            location: Some("local-lan/tailscale".to_string()),
            roles: vec![
                "windows-runner".to_string(),
                "github-runner".to_string(),
                "gpu".to_string(),
                "embedding".to_string(),
                "rerank-candidate".to_string(),
                "deploy-agent".to_string(),
            ],
            tags: vec![
                "12900kf".to_string(),
                "3090ti".to_string(),
                "windows".to_string(),
                "qwen3-embedding".to_string(),
                "ollama".to_string(),
                "agent_url=windows".to_string(),
                "unverified".to_string(),
                format!("skill:{}", WINDOWS_12900KF_SKILL),
            ],
            description: Some(
                "Skill evidence: deploy-agent `xjp_agent_exec(agent_url=\"windows\")`; host ssh target via Tailscale or LAN; Ollama/Qwen embedding/rerank candidate via BWG tunnel. Promote to deploy-center/secret-store before treating as verified runtime truth."
                    .to_string(),
            ),
            health_endpoint: Some("http://104.194.81.38:19434/api/tags".to_string()),
        },
    );
    maybe_push_skill_target(
        servers,
        "privatecloud-hostvds",
        missiond_core::InfraServer {
            id: "privatecloud-hostvds".to_string(),
            name: "privatecloud / HostVDS runtime".to_string(),
            provider: "skill-derived".to_string(),
            host: Some("45.156.24.163".to_string()),
            lan: None,
            location: Some("HostVDS/privatecloud".to_string()),
            roles: vec!["deploy".to_string(), "tunnel".to_string(), "runtime".to_string()],
            tags: vec![
                "privatecloud".to_string(),
                "hostvds".to_string(),
                "deploy-center-evidence".to_string(),
                "unverified".to_string(),
            ],
            description: Some(
                "Skill/evidence-derived HostVDS/privatecloud runtime target. Login details must be resolved through secret-store refs and deploy-center provenance, not copied from skills."
                    .to_string(),
            ),
            health_endpoint: None,
        },
    );
    maybe_push_skill_target(
        servers,
        "ecs-pcea",
        missiond_core::InfraServer {
            id: "ecs-pcea".to_string(),
            name: "PCEA ECS runtime".to_string(),
            provider: "skill-derived".to_string(),
            host: Some("106.15.2.17".to_string()),
            lan: None,
            location: Some("Aliyun ECS/PCEA".to_string()),
            roles: vec!["pcea".to_string(), "runtime".to_string(), "deploy-agent".to_string()],
            tags: vec!["pcea".to_string(), "ecs".to_string(), "unverified".to_string()],
            description: Some(
                "Skill-derived PCEA ECS runtime target; deploy-center should own current service deploy location and rollback artifacts."
                    .to_string(),
            ),
            health_endpoint: None,
        },
    );
    maybe_push_skill_target(
        servers,
        "bwg-vps",
        missiond_core::InfraServer {
            id: "bwg-vps".to_string(),
            name: "BWG/VPS tunnel runtime".to_string(),
            provider: "skill-derived".to_string(),
            host: Some("104.194.81.38".to_string()),
            lan: None,
            location: Some("BWG/VPS".to_string()),
            roles: vec!["tunnel".to_string(), "router-relay".to_string(), "model-relay".to_string()],
            tags: vec!["bwg".to_string(), "vps".to_string(), "unverified".to_string()],
            description: Some(
                "Skill-derived BWG tunnel target used as model/router relay evidence. Secrets and tunnel lifecycle belong in secret-store and deploy-center."
                    .to_string(),
            ),
            health_endpoint: None,
        },
    );
    maybe_push_skill_target(
        servers,
        "privatecloud-lan-192-168-1-20",
        missiond_core::InfraServer {
            id: "privatecloud-lan-192-168-1-20".to_string(),
            name: "Private LAN infra node".to_string(),
            provider: "skill-derived".to_string(),
            host: None,
            lan: Some("192.168.1.20".to_string()),
            location: Some("local-lan/private-infra".to_string()),
            roles: vec!["infra".to_string(), "cache".to_string(), "harbor".to_string(), "dns".to_string()],
            tags: vec!["private-lan".to_string(), "192.168.1.20".to_string(), "unverified".to_string()],
            description: Some(
                "Skill-derived private LAN infra target. Treat as unverified until deploy-center inventory or an operator-approved probe confirms the runtime facts."
                    .to_string(),
            ),
            health_endpoint: None,
        },
    );
    maybe_push_skill_target(
        servers,
        "gcp-runtime",
        missiond_core::InfraServer {
            id: "gcp-runtime".to_string(),
            name: "GCP production runtime".to_string(),
            provider: "skill-derived".to_string(),
            host: Some("34.104.147.118".to_string()),
            lan: None,
            location: Some("GCP production / xjp-backend VM".to_string()),
            roles: vec![
                "production".to_string(),
                "auth".to_string(),
                "router".to_string(),
                "deploy-center".to_string(),
                "secret-store".to_string(),
                "credential-vault".to_string(),
            ],
            tags: vec![
                "gcp".to_string(),
                "production".to_string(),
                "verified-2026-05-11".to_string(),
                "ss.xiaojinpro.top".to_string(),
            ],
            description: Some(
                "Universe summary for GCP-hosted production services. Secret Store moved here on 2026-05-11 (ss.xiaojinpro.top -> 34.104.147.118, Caddy to local secret-store container, DB in xjp-postgres/secret_store). deploy-center provenance remains the runtime authority; MissionD keeps only the identity summary."
                    .to_string(),
            ),
            health_endpoint: Some("https://ss.xiaojinpro.top/livez".to_string()),
        },
    );
}

fn maybe_push_skill_target(
    servers: &mut Vec<missiond_core::InfraServer>,
    id: &str,
    server: missiond_core::InfraServer,
) {
    if !servers
        .iter()
        .any(|existing| infra_id_matches(existing, id))
    {
        servers.push(server);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        credential_refs_filtered, evidence_matches_scope, evidence_scope_score,
        is_infra_evidence_line, skill_target_context_allows_deploy_closure_line,
        InfraEvidenceFilter,
    };

    #[test]
    fn evidence_scope_rejects_unrelated_project_skill_lines() {
        let filter = InfraEvidenceFilter {
            target_id: None,
            skill: None,
            query: Some("Payments service.manifest.toml missing manifest gate".to_string()),
            project_id: Some("payments".to_string()),
            limit: 10,
        };

        assert!(!evidence_matches_scope(
            "tiermate",
            "/Users/jinchen/.claude/skills/tiermate/SKILL.md",
            "GCP deploy-agent endpoint and secret-store references",
            &filter,
        ));
        assert!(!evidence_matches_scope(
            "astrill-gateway",
            "/Users/jinchen/.claude/skills/astrill-gateway/SKILL.md",
            "Router/Gateway 192.168.80.254 ARP flux and gateway-iptables.sh diagnostics",
            &filter,
        ));
        assert!(!evidence_matches_scope(
            "openclaw",
            "/Users/jinchen/.claude/skills/openclaw/SKILL.md",
            "OpenAI-compatible API model routing backend notes",
            &filter,
        ));
        assert!(!evidence_matches_scope(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            "media type application/vnd.docker.distribution.manifest.v1+prettyjws is no longer supported",
            &filter,
        ));
        assert!(!evidence_matches_scope(
            "missiond",
            "/Users/jinchen/.claude/skills/missiond/SKILL.md",
            "Feature gate: embeddings feature, MUSL build disables ONNX Runtime",
            &filter,
        ));
        assert!(!evidence_matches_scope(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            "/opt/xiaojinpro/docker-compose.yml — monolith, router, payments, investor-panel",
            &filter,
        ));
        assert!(evidence_matches_scope(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            "Payments deploy-agent canary evidence and manifest gate notes",
            &filter,
        ));
        assert!(is_infra_evidence_line(
            "Payments deploy-agent canary evidence and manifest gate notes"
        ));

        let manifest_filter = InfraEvidenceFilter {
            target_id: None,
            skill: None,
            query: Some(
                "payments Deploy Center canary service.manifest.toml migration relation payments already exists compose old binary image marker"
                    .to_string(),
            ),
            project_id: Some("missiond".to_string()),
            limit: 10,
        };
        let manifest_line = "Payments service.manifest.toml Manifest Gate canary smoke provenance";
        let compose_line = "Payments compose volume override kept the old binary image marker running after canary";
        let migration_line = "sqlx migrate relation payments already exists during canary";
        assert!(is_infra_evidence_line(manifest_line));
        assert!(is_infra_evidence_line(compose_line));
        assert!(is_infra_evidence_line(migration_line));
        assert!(evidence_matches_scope(
            "xjp-deploy-center",
            "/Users/jinchen/.claude/skills/xjp-deploy-center/SKILL.md",
            manifest_line,
            &manifest_filter,
        ));
        assert!(evidence_matches_scope(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            compose_line,
            &manifest_filter,
        ));
        assert!(evidence_matches_scope(
            "sqlx-cache",
            "/Users/jinchen/.claude/skills/sqlx-cache/SKILL.md",
            migration_line,
            &manifest_filter,
        ));
        assert!(!evidence_matches_scope(
            "palm-era",
            "/Users/jinchen/.claude/skills/palm-era/SKILL.md",
            "sqlx migrate relation already exists during canary",
            &manifest_filter,
        ));
        assert!(!evidence_matches_scope(
            "xjp-deploy-agent",
            "/Users/jinchen/.claude/skills/xjp-deploy-agent/SKILL.md",
            "deploy.sh runs migrations before docker compose up",
            &manifest_filter,
        ));
        assert!(skill_target_context_allows_deploy_closure_line(
            "deploy-ops",
            "service.manifest.toml Manifest Gate is required before Deploy Center canary smoke can be trusted",
            "Payments has an independent Cargo.lock and is deployed through Deploy Center.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "deploy-ops",
            "CI green only means the image built; Deploy Center canary and smoke decide CD truth",
            "Payments has an independent Cargo.lock and is deployed through Deploy Center.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "deploy-ops",
            "Docker healthcheck can use curl or wget during Deploy Center canary",
            "Payments has an independent Cargo.lock and is deployed through Deploy Center.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "deploy-ops",
            "/opt/xiaojinpro/docker-compose.yml -- monolith, router, payments, investor-panel",
            "Payments has an independent Cargo.lock and is deployed through Deploy Center.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "independent-app-bootstrap",
            "Keep ALTER migrations idempotent: ALTER TABLE ... ADD COLUMN IF NOT EXISTS.",
            "Payments appears here only as generic app bootstrap documentation.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "palm-era",
            "sqlx migrate relation already exists during canary",
            "Palm Era deploy notes without the requested service target.",
            &manifest_filter,
        ));
        assert!(!skill_target_context_allows_deploy_closure_line(
            "deploy-ops",
            "xjp-router canary wait can fail while the service is already listening",
            "Payments has an independent Cargo.lock and is deployed through Deploy Center.",
            &manifest_filter,
        ));

        let deploy_runtime_filter = InfraEvidenceFilter {
            target_id: None,
            skill: None,
            query: Some(
                "Payments CI image marker but Deploy Center canary old binary compose entrypoint volume override"
                    .to_string(),
            ),
            project_id: Some("payments".to_string()),
            limit: 10,
        };
        assert!(evidence_matches_scope(
            "xjp-deploy-center",
            "/Users/jinchen/.claude/skills/xjp-deploy-center/SKILL.md",
            "compose volume override kept the old binary image running after canary",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "pcea",
            "/Users/jinchen/.claude/skills/pcea/SKILL.md",
            "Postgres volume: `pcea_postgres_data` (fixed compose project storage)",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "xjp-deploy-center",
            "/Users/jinchen/.claude/skills/xjp-deploy-center/SKILL.md",
            "| 镜像传输 | OSS: `rickyjim/deploy-images/pcea-video-vault/pcea-{sha}.tar.gz` |",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "xjp-deploy-center",
            "/Users/jinchen/.claude/skills/xjp-deploy-center/SKILL.md",
            "**OSS 中转策略**: GA CI 构建 docker image → docker save → Deploy Center 触发 → ECS Agent 下载。Build stage 必须 DISABLED。",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "xjp-pg-prod",
            "/Users/jinchen/.claude/skills/xjp-pg-prod/SKILL.md",
            "| xjp-monolith-app | `postgres:<R>@10.146.0.4:6432/log_center`(+ router/timeline/payments/deploy_center/knowledge env 同) |",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "xiaojinpro-backend",
            "/Users/jinchen/.claude/skills/xiaojinpro-backend/SKILL.md",
            "Router → Payments: `POST /payments/internal/credits/spend`",
            &deploy_runtime_filter,
        ));
        let volume_override_score = evidence_scope_score(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            "| xjp-router docker-compose.yml `volumes: ./config:/app/config:ro` 挂载覆盖镜像内 config | push → CI 构建新镜像 → 部署 → 容器里还是旧 config（挂载优先级高于 image layer） |",
            &deploy_runtime_filter,
        );
        let generic_payments_compose_score = evidence_scope_score(
            "deploy-ops",
            "/Users/jinchen/.claude/skills/deploy-ops/SKILL.md",
            "/opt/xiaojinpro/docker-compose.yml — monolith, router, payments, investor-panel 等",
            &deploy_runtime_filter,
        );
        assert!(
            volume_override_score > generic_payments_compose_score,
            "volume override evidence should outrank generic payments compose inventory"
        );
        assert!(!evidence_matches_scope(
            "tiermate",
            "/Users/jinchen/.claude/skills/tiermate/SKILL.md",
            "GCP deploy-agent endpoint and secret-store references",
            &deploy_runtime_filter,
        ));
        assert!(!evidence_matches_scope(
            "aliyun",
            "/Users/jinchen/.claude/skills/aliyun/SKILL.md",
            "| Secret Store CN | secret-store-cn-app | 8091 (127.0.0.1) | Docker Compose, OSS image transfer |",
            &deploy_runtime_filter,
        ));

        let gcp_agent_filter = InfraEvidenceFilter {
            target_id: None,
            skill: None,
            query: Some("GCP deploy agent key for payments canary diagnostics".to_string()),
            project_id: Some("payments".to_string()),
            limit: 10,
        };
        assert!(evidence_matches_scope(
            "xjp-deploy-agent",
            "/Users/jinchen/.claude/skills/xjp-deploy-agent/SKILL.md",
            "GCP tunnel API and deploy-agent diagnostics",
            &gcp_agent_filter,
        ));
        assert!(!evidence_matches_scope(
            "wepub",
            "/Users/jinchen/.claude/skills/wepub/SKILL.md",
            "Backend GCP /opt/wepub deployment notes",
            &gcp_agent_filter,
        ));
    }

    #[test]
    fn credential_refs_require_explicit_target_or_query_relevance() {
        assert!(!credential_refs_filtered(Some("gcp-runtime"), None, None).is_empty());
        let gcp_refs = credential_refs_filtered(
            None,
            Some("GCP deploy agent key for payments canary diagnostics"),
            Some("payments"),
        );
        assert!(!gcp_refs.is_empty());
        assert!(gcp_refs.iter().all(|item| {
            item.get("targetId").and_then(|value| value.as_str()) == Some("gcp-runtime")
        }));
        assert!(gcp_refs.iter().all(|item| {
            item.get("requiredCapability")
                .and_then(|value| value.as_str())
                == Some("deploy-ops")
        }));
        assert!(gcp_refs.iter().all(|item| {
            item.get("keyName")
                .and_then(|value| value.as_str())
                .is_none_or(|key| !key.contains("CLOUDFLARE_DNS_TOKEN"))
        }));
        let dns_refs =
            credential_refs_filtered(None, Some("Cloudflare DNS token on GCP runtime"), None);
        assert!(dns_refs.iter().any(|item| {
            item.get("keyName").and_then(|value| value.as_str())
                == Some("cloudflare/CLOUDFLARE_DNS_TOKEN")
        }));
        assert!(credential_refs_filtered(
            None,
            Some("Payments service.manifest.toml missing manifest gate"),
            Some("payments"),
        )
        .is_empty());
        assert!(
            credential_refs_filtered(
                None,
                Some(
                    "Payments CI image marker but Deploy Center canary old binary compose entrypoint volume override",
                ),
                Some("payments"),
            )
            .is_empty()
        );
    }
}

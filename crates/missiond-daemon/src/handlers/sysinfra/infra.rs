use anyhow::{anyhow, Result};
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
    #[serde(default = "default_evidence_limit")]
    limit: usize,
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
                    limit: default_evidence_limit(),
                });
            let evidence = collect_skill_evidence(
                state,
                InfraEvidenceFilter {
                    target_id: args.target_id,
                    skill: args.skill,
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
                    limit: default_evidence_limit(),
                });
            let refs = credential_refs(args.target_id.as_deref());
            Ok(ToolResult::json_pretty(&json!({
                "schema": "missiond.credential-ref-inventory.v1",
                "rule": "Only secret refs are returned. MissionD never returns credential values from Lisp, Board, or skills.",
                "credentialRefs": refs
            })))
        }
        "mission_infra_reconcile" => {
            let servers = list_infra_servers(state, None, None);
            let skill_evidence = collect_skill_evidence(
                state,
                InfraEvidenceFilter {
                    target_id: None,
                    skill: None,
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
    limit: usize,
}

fn collect_skill_evidence(state: &AppState, filter: InfraEvidenceFilter) -> Vec<Value> {
    let target = filter.target_id.as_deref().map(str::to_ascii_lowercase);
    let mut items = Vec::new();
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
        for (idx, line) in content.lines().enumerate() {
            if items.len() >= filter.limit {
                return items;
            }
            if !is_infra_evidence_line(line) {
                continue;
            }
            if let Some(target) = target.as_deref() {
                let lower = line.to_ascii_lowercase();
                if !lower.contains(target) && !skill_name.to_ascii_lowercase().contains(target) {
                    continue;
                }
            }
            let (excerpt, credential_risk) = redact_skill_evidence_line(line);
            items.push(json!({
                "sourceSkill": skill_name,
                "sourcePath": skill.path.display().to_string(),
                "sourceLine": idx + 1,
                "confidence": evidence_confidence(line),
                "promoteTo": evidence_promotion_target(line),
                "credentialInlineRisk": credential_risk,
                "excerpt": excerpt
            }));
        }
    }
    items
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

    let mut redacted = line.trim().to_string();
    for marker in [
        "sshpass -p",
        "password",
        "Password",
        "密码",
        "token",
        "TOKEN",
        "api_key",
        "API_KEY",
    ] {
        if let Some(idx) = redacted.find(marker) {
            redacted.truncate(idx + marker.len());
            redacted.push_str(" <redacted>");
            break;
        }
    }
    (redacted, true)
}

fn credential_refs(target_id: Option<&str>) -> Vec<Value> {
    let refs = vec![
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

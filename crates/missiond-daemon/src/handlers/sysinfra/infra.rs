use anyhow::{anyhow, Result};
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::Value;

use crate::state::AppState;

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
            let infra = state.infra.read().unwrap();
            let servers: Vec<missiond_core::InfraServer> = if let Some(role) = role {
                infra.by_role(&role).into_iter().cloned().collect()
            } else if let Some(provider) = provider {
                infra.by_provider(&provider).into_iter().cloned().collect()
            } else {
                infra.servers.clone()
            };
            drop(infra);
            Ok(ToolResult::json_pretty(&servers))
        }
        "mission_infra_get" => {
            let InfraGetArgs { id } = serde_json::from_value(args)?;
            let server = state.infra.read().unwrap().get(&id).cloned();
            match server {
                Some(server) => Ok(ToolResult::json_pretty(&server)),
                None => Ok(ToolResult::error(format!("Server not found: {}", id))),
            }
        }

        "mission_reachability" => {
            let ReachabilityArgs { target, channels } = serde_json::from_value(args)?;

            let server: Option<missiond_core::InfraServer> =
                state.infra.read().unwrap().get(&target).cloned();
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
                            }))
                        }
                        Ok(Err(e)) => {
                            return Some(serde_json::json!({
                                "reachable": false, "ip": host, "port": port, "via": via,
                                "error": e.to_string(),
                            }))
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

            let server: Option<missiond_core::InfraServer> =
                state.infra.read().unwrap().get(&target).cloned();
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

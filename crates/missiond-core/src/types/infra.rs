use serde::{Deserialize, Serialize};

// ============ Infrastructure Server Registry ============

/// A server in the infrastructure registry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfraServer {
    pub id: String,
    pub name: String,
    pub provider: String, // gcp, aliyun, self-hosted, bandwagon
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>, // build, deploy, gpu, vpn, production
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// HTTP(S) endpoint for deploy-agent health checks (Probe 5 in reachability).
    /// If set, reachability tool uses this URL instead of hardcoded map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_endpoint: Option<String>,
}

/// Parsed SSH connection target from InfraServer description
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub password: Option<String>,
    /// "lan", "tailscale", or "public"
    pub via: String,
}

impl InfraServer {
    /// Parse SSH targets from description field, ordered by priority (LAN > Tailscale > public).
    ///
    /// Matches patterns like:
    /// - `ssh user@192.168.1.100 (密码 pass`
    /// - `ssh -p 2222 user@198.51.100.1 (密码 pass`
    /// - `ssh user@100.64.0.1`
    pub fn parse_ssh_targets(&self) -> Vec<SshTarget> {
        let desc = match &self.description {
            Some(d) => d,
            None => return Vec::new(),
        };

        let re = regex::Regex::new(
            r"ssh\s+(?:-p\s+(\d+)\s+)?(\w[\w.-]*)@([\d.]+)(?:\s+\([^)]*(?:密码|password)\s+([^\s,)]+))?"
        ).unwrap();

        let mut targets = Vec::new();
        for cap in re.captures_iter(desc) {
            let port = cap.get(1)
                .and_then(|m| m.as_str().parse::<u16>().ok())
                .unwrap_or(22);
            let user = cap[2].to_string();
            let host = cap[3].to_string();
            let password = cap.get(4).map(|m| m.as_str().to_string());

            let via = if self.lan.as_deref() == Some(host.as_str()) {
                "lan"
            } else if host.starts_with("100.") {
                "tailscale"
            } else if host.starts_with("192.168.") || host.starts_with("10.") || host.starts_with("172.") {
                "lan"
            } else {
                "public"
            };

            targets.push(SshTarget {
                user,
                host,
                port,
                password,
                via: via.to_string(),
            });
        }

        // Sort: lan first, tailscale second, public last
        targets.sort_by_key(|t| match t.via.as_str() {
            "lan" => 0,
            "tailscale" => 1,
            _ => 2,
        });

        targets
    }
}

/// Infrastructure configuration (loaded from servers.yaml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraConfig {
    #[serde(default)]
    pub servers: Vec<InfraServer>,
}

impl InfraConfig {
    /// Load from YAML file, returns empty config if file doesn't exist
    pub fn load(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self { servers: Vec::new() };
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_yaml::from_str(&content).unwrap_or(Self { servers: Vec::new() }),
            Err(_) => Self { servers: Vec::new() },
        }
    }

    /// Get server by ID
    pub fn get(&self, id: &str) -> Option<&InfraServer> {
        self.servers.iter().find(|s| s.id == id)
    }

    /// Filter servers by role
    pub fn by_role(&self, role: &str) -> Vec<&InfraServer> {
        self.servers.iter().filter(|s| s.roles.iter().any(|r| r == role)).collect()
    }

    /// Filter servers by provider
    pub fn by_provider(&self, provider: &str) -> Vec<&InfraServer> {
        self.servers.iter().filter(|s| s.provider == provider).collect()
    }
}

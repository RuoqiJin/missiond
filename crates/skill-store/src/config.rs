use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    /// HTTP server bind address
    #[serde(default = "default_bind")]
    pub bind: String,

    /// SQLite database file path
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,

    /// JWT secret key
    pub jwt_secret: String,

    /// LLM upstream providers
    pub llm: LlmConfig,

    /// Defense layer config
    #[serde(default)]
    pub defense: DefenseConfig,

    /// Platform revenue share ratio (0.0 - 1.0, platform takes this %)
    #[serde(default = "default_platform_cut")]
    pub platform_cut: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    /// Clewdr endpoint for defense detection
    #[serde(default)]
    pub clewdr_endpoint: Option<String>,

    /// Default LLM provider for skill execution
    pub providers: Vec<LlmProvider>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmProvider {
    pub name: String,
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    /// Cost per 1K input tokens
    #[serde(default)]
    pub input_cost_per_1k: f64,
    /// Cost per 1K output tokens
    #[serde(default)]
    pub output_cost_per_1k: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefenseConfig {
    /// Enable input injection detection
    #[serde(default = "default_true")]
    pub input_detection: bool,

    /// Enable output leak detection
    #[serde(default = "default_true")]
    pub output_detection: bool,

    /// Output similarity threshold (0.0 - 1.0) to trigger leak block
    #[serde(default = "default_leak_threshold")]
    pub leak_threshold: f64,
}

impl Default for DefenseConfig {
    fn default() -> Self {
        Self {
            input_detection: true,
            output_detection: true,
            leak_threshold: 0.3,
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:8900".to_string()
}

fn default_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".skill-store")
        .join("store.db")
}

fn default_platform_cut() -> f64 {
    0.30
}

fn default_true() -> bool {
    true
}

fn default_leak_threshold() -> f64 {
    0.3
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".skill-store")
            .join("config.yaml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            Ok(serde_yaml::from_str(&content)?)
        } else {
            // Minimal default for development
            Ok(Self {
                bind: default_bind(),
                db_path: default_db_path(),
                jwt_secret: "dev-secret-change-in-production".to_string(),
                llm: LlmConfig {
                    clewdr_endpoint: None,
                    providers: vec![],
                },
                defense: DefenseConfig::default(),
                platform_cut: default_platform_cut(),
            })
        }
    }
}

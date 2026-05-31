use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub creator_id: String,
    pub name: String,
    pub description: String,
    /// Hidden from API responses — the core asset
    #[serde(skip_serializing)]
    pub prompt_template: String,
    /// JSON schema defining required input variables
    pub input_schema: serde_json::Value,
    /// 1=Free/Basic, 2=Pro, 3=Max
    pub tier: i32,
    /// Price per invocation when user exceeds quota
    pub price_per_use: f64,
    pub is_active: bool,
    pub invoke_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Public view of a skill (no prompt_template)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPublic {
    pub id: String,
    pub creator_id: String,
    pub creator_name: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub tier: i32,
    pub price_per_use: f64,
    pub invoke_count: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSkillRequest {
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    pub input_schema: serde_json::Value,
    #[serde(default = "default_tier")]
    pub tier: i32,
    #[serde(default)]
    pub price_per_use: f64,
}

fn default_tier() -> i32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSkillRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub prompt_template: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub tier: Option<i32>,
    pub price_per_use: Option<f64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvokeSkillRequest {
    pub inputs: serde_json::Value,
}

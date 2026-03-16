use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub name: String,
    /// Monthly invocation quota
    pub monthly_quota: i64,
    /// Max skill tier accessible (1=Basic, 2=Pro, 3=Max)
    pub max_skill_tier: i32,
    /// Monthly price
    pub price: f64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub user_id: String,
    pub plan_id: String,
    pub status: SubStatus,
    pub current_period_start: String,
    pub current_period_end: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SubStatus {
    Active,
    Expired,
    Cancelled,
}

impl SubStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "cancelled" => Self::Cancelled,
            _ => Self::Expired,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeRequest {
    pub plan_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUpRequest {
    /// Number of extra invocations to purchase
    pub amount: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    pub plan_name: String,
    pub monthly_quota: i64,
    pub used_this_period: i64,
    pub remaining: i64,
    pub extra_purchased: i64,
    pub period_end: String,
}

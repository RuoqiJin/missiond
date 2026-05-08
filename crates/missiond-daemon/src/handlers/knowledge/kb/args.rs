use serde::Deserialize;
use serde_json::Value;

use crate::lenient;

#[derive(Deserialize)]
pub(super) struct KBRememberArgs {
    pub(super) category: String,
    pub(super) key: String,
    pub(super) summary: String,
    #[serde(default)]
    pub(super) detail: Option<Value>,
    #[serde(default)]
    pub(super) source: Option<String>,
    #[serde(default)]
    pub(super) confidence: Option<f64>,
    #[serde(default)]
    pub(super) project: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct KBKeyArgs {
    pub(super) key: String,
    #[serde(default)]
    pub(super) include_archived: bool,
}

#[derive(Deserialize)]
pub(super) struct KBUpdateArgs {
    pub(super) key: String,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) detail: Option<Value>,
    #[serde(default)]
    pub(super) confidence: Option<f64>,
    #[serde(default)]
    pub(super) linked_task_id: Option<String>,
    #[serde(default)]
    pub(super) project_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct KBSearchArgs {
    #[serde(default)]
    pub(super) query: Option<String>,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) offset: Option<usize>,
    #[serde(default)]
    pub(super) search_mode: Option<String>,
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default)]
    pub(super) include_archived: bool,
    #[serde(default)]
    pub(super) state_filter: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct KBListArgs {
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default = "default_list_limit")]
    pub(super) limit: u32,
    #[serde(default)]
    pub(super) offset: u32,
    #[serde(default)]
    pub(super) compact: bool,
    #[serde(default)]
    pub(super) include_archived: bool,
    #[serde(default)]
    pub(super) state_filter: Option<String>,
}

fn default_list_limit() -> u32 {
    50
}

#[derive(Deserialize)]
pub(super) struct KBImportArgs {
    pub(super) format: String,
    #[serde(default)]
    pub(super) path: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct KBDiscoverArgs {
    pub(super) host: String,
    #[serde(default)]
    pub(super) port: Option<u16>,
    #[serde(default)]
    pub(super) password: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct KBGCArgs {
    pub(super) action: String,
    #[serde(default, deserialize_with = "lenient::option_i64")]
    pub(super) days: Option<i64>,
}

#[derive(Deserialize)]
pub(super) struct KBReviewArgs {
    pub(super) action: String,
    #[serde(default)]
    pub(super) key: Option<String>,
    #[serde(default)]
    pub(super) knowledge_id: Option<String>,
    #[serde(default)]
    pub(super) state: Option<String>,
    #[serde(default)]
    pub(super) batch_id: Option<String>,
    #[serde(default)]
    pub(super) reviewer: Option<String>,
    #[serde(default)]
    pub(super) rationale: Option<String>,
    #[serde(default)]
    pub(super) evidence_refs: Option<Value>,
    #[serde(default)]
    pub(super) superseded_by: Option<String>,
    #[serde(default)]
    pub(super) confidence: Option<f64>,
    #[serde(default)]
    pub(super) applied_at: Option<String>,
}

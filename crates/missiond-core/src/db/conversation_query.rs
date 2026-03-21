use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ConversationTypeFilter {
    User,
    Meta,
    Worker,
    Jarvis,
    System, // 组合：Meta + Worker (不含 Jarvis)
    Subagent,
    Compaction,
    All,
    Custom(String),
}

impl ConversationTypeFilter {
    pub fn to_sql_clause(&self, alias: &str) -> String {
        let prefix = if alias.is_empty() { String::new() } else { format!("{}.", alias) };
        match self {
            Self::All => String::new(),
            Self::User => format!(" AND {}conversation_type = 'user'", prefix),
            Self::Meta => format!(" AND {}conversation_type = 'meta'", prefix),
            Self::Worker => format!(" AND {}conversation_type = 'worker'", prefix),
            Self::Jarvis => format!(" AND {}conversation_type = 'jarvis'", prefix),
            Self::System => format!(" AND {}conversation_type IN ('meta', 'worker')", prefix),
            Self::Subagent => format!(" AND {}conversation_type = 'subagent'", prefix),
            Self::Compaction => format!(" AND {}conversation_type = 'compaction'", prefix),
            Self::Custom(s) => format!(" AND {}conversation_type = '{}'", prefix, s.replace('\'', "''")),
        }
    }
    
    pub fn from_str(s: &str) -> Self {
        match s {
            "all" => Self::All,
            "user" => Self::User,
            "meta" => Self::Meta,
            "worker" => Self::Worker,
            "jarvis" => Self::Jarvis,
            "system" => Self::System, // <-- 系统大盘视图：只有 meta 和 worker
            "subagent" => Self::Subagent,
            "compaction" => Self::Compaction,
            _ => Self::Custom(s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConversationQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub conv_type: Option<ConversationTypeFilter>,
    pub task_id: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub source: Option<String>,
    pub project: Option<String>,
    pub parent_session_id: Option<String>,
}

impl ConversationQuery {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }
    
    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
    
    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
    
    pub fn conv_type(mut self, conv_type: ConversationTypeFilter) -> Self {
        self.conv_type = Some(conv_type);
        self
    }
    
    pub fn task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }
    
    pub fn since(mut self, since: impl Into<String>) -> Self {
        self.since = Some(since.into());
        self
    }
    
    pub fn until(mut self, until: impl Into<String>) -> Self {
        self.until = Some(until.into());
        self
    }
    
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

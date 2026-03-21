use crate::db::traits::MissionStore;
use crate::db::conversation_query::{ConversationQuery, ConversationTypeFilter};
use crate::types::Conversation;
use anyhow::Result;
use std::sync::Arc;

pub struct ConversationManager {
    store: Arc<dyn MissionStore>,
}

impl ConversationManager {
    pub fn new(store: Arc<dyn MissionStore>) -> Self {
        Self { store }
    }

    /// 执行会话查询。将语义化的 Query 转换为底层的 DB 参数。
    pub async fn list(&self, query: ConversationQuery) -> Result<Vec<Conversation>> {
        // 这里我们把强类型的 ConversationTypeFilter 转换回底层接口需要的字符串
        // TODO: 最终应该让底层接口也接受 ConversationQuery 对象
        let conv_type_str = match query.conv_type {
            Some(ConversationTypeFilter::System) => Some("system".to_string()),
            Some(ConversationTypeFilter::Jarvis) => Some("jarvis".to_string()),
            Some(ConversationTypeFilter::Worker) => Some("worker".to_string()),
            Some(ConversationTypeFilter::Meta) => Some("meta".to_string()),
            Some(ConversationTypeFilter::User) => Some("user".to_string()),
            Some(ConversationTypeFilter::Subagent) => Some("subagent".to_string()),
            Some(ConversationTypeFilter::Compaction) => Some("compaction".to_string()),
            Some(ConversationTypeFilter::All) => Some("all".to_string()),
            Some(ConversationTypeFilter::Custom(s)) => Some(s),
            None => None,
        };

        let convs = self.store.list_conversations(
            query.status.as_deref(),
            query.limit.unwrap_or(20),
            conv_type_str.as_deref(),
            query.task_id.as_deref(),
            query.since.as_deref(),
            query.until.as_deref(),
            query.source.as_deref(),
        ).await?;
        
        Ok(convs)
    }

    // 可以添加更多基于业务角色的查询：
    
    /// 专门获取前端 Jarvis 面板的会话（不包含后台工作流）
    pub async fn list_jarvis_sessions(&self, limit: i64) -> Result<Vec<Conversation>> {
        let query = ConversationQuery::new()
            .conv_type(ConversationTypeFilter::Jarvis)
            .limit(limit);
        self.list(query).await
    }
    
    /// 获取后台运维大盘数据（Meta + Worker），不包含人类和前台助理
    pub async fn list_system_workflows(&self, limit: i64) -> Result<Vec<Conversation>> {
        let query = ConversationQuery::new()
            .conv_type(ConversationTypeFilter::System)
            .limit(limit);
        self.list(query).await
    }
}

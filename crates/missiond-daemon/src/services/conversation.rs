use crate::state::AppState;
use missiond_core::services::ConversationManager;
use std::sync::Arc;

pub struct ConversationService {
    manager: ConversationManager,
}

impl ConversationService {
    pub fn new(state: &AppState) -> Self {
        Self {
            manager: ConversationManager::new(Arc::clone(&state.store)),
        }
    }

    // 我们可以在这个代理服务层添加更多的带上下文的安全/权限控制逻辑，
    // 例如只允许某个工位读取自己的或者分配给它的任务，以及封装一些常用的前端查询
}

// TODO: 把 handlers 里面的一些通用查询逻辑搬到这里

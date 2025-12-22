//! 消息查询处理器
//!
//! 职责：处理消息相关的读操作

use std::sync::Arc;
use crate::domain::repository::ReadStore;
use crate::application::queries::*;

/// 消息查询处理器
pub struct MessageQueryHandler {
    read_store: Arc<dyn ReadStore>,
}

impl MessageQueryHandler {
    pub fn new(read_store: Arc<dyn ReadStore>) -> Self {
        Self { read_store }
    }
    
    /// 处理查询消息列表
    pub async fn handle_list(&self, query: ListMessagesQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        use crate::domain::repository::{Query, QueryResult};
        let q = Query::MessageList {
            conversation_id: query.conversation_id,
            limit: query.limit,
            cursor: query.cursor,
        };
        
        match self.read_store.query(q).await? {
            QueryResult::MessageList { items, .. } => Ok(items),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
    
    /// 处理查询消息详情
    pub async fn handle_get(&self, query: GetMessageQuery) -> anyhow::Result<serde_json::Value> {
        use crate::domain::repository::{Query, QueryResult};
        let q = Query::MessageDetail {
            message_id: query.message_id,
        };
        
        match self.read_store.query(q).await? {
            QueryResult::MessageDetail { item } => Ok(item),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
    
    /// 处理搜索消息
    pub async fn handle_search(&self, query: SearchMessagesQuery) -> anyhow::Result<Vec<serde_json::Value>> {
        use crate::domain::repository::{Query, QueryResult};
        let q = Query::SearchMessages {
            conversation_id: query.conversation_id,
            keyword: query.keyword,
            limit: query.limit,
        };
        
        match self.read_store.query(q).await? {
            QueryResult::SearchMessages { items } => Ok(items),
            _ => Err(anyhow::anyhow!("Unexpected query result type")),
        }
    }
}

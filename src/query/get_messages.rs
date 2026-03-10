use crate::error::Result;
use crate::model::message::Message;
use crate::store::MessageStore;

/// 查询消息列表（本地存储）
pub struct GetMessagesQuery {
    pub conversation_id: String,
    pub before_seq: u64,
    pub limit: u32,
}

impl GetMessagesQuery {
    pub async fn execute(&self, store: &dyn MessageStore) -> Result<Vec<Message>> {
        store.get_by_conversation(&self.conversation_id, self.before_seq, self.limit).await
    }
}

/// 搜索消息
pub struct SearchMessagesQuery {
    pub keyword: String,
    pub limit: u32,
}

impl SearchMessagesQuery {
    pub async fn execute(&self, store: &dyn MessageStore) -> Result<Vec<Message>> {
        store.search(&self.keyword, self.limit).await
    }
}

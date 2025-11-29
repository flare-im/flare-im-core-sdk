# Flare IM Core SDK 重构架构设计

## 设计原则

1. **核心数据来自 flare-proto**：消息和会话的基础结构直接使用 `flare-proto`
2. **SDK 扩展字段**：在 SDK 层添加客户端特有的扩展字段（头像、名称、本地状态等）
3. **可扩展机制**：通过扩展系统支持后续添加用户、好友、群等模块
4. **代码简洁**：最小化抽象，保持代码清晰易读

## 架构设计

### 1. 消息模型（Message）

```rust
// 直接使用 flare-proto::Message 作为基础
// 通过扩展字段添加 SDK 特有的信息

pub use flare_proto::Message;

/// 消息扩展信息（SDK 层特有）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MessageExtension {
    /// 发送者头像 URL（客户端缓存）
    pub sender_avatar: Option<String>,
    
    /// 发送者显示名称（客户端缓存）
    pub sender_name: Option<String>,
    
    /// 消息本地状态（已读、已删除等）
    pub local_state: Option<MessageLocalState>,
    
    /// 是否已下载（媒体消息）
    pub is_downloaded: Option<bool>,
    
    /// 下载进度（0-100）
    pub download_progress: Option<u8>,
    
    /// 自定义扩展字段
    pub custom: std::collections::HashMap<String, String>,
}

/// 消息本地状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MessageLocalState {
    /// 发送中
    Sending,
    /// 发送成功
    Sent,
    /// 发送失败
    Failed,
    /// 已读（本地标记）
    Read,
    /// 已删除（本地标记）
    Deleted,
}

/// 带扩展的消息（SDK 使用）
#[derive(Debug, Clone)]
pub struct ExtendedMessage {
    /// 基础消息（来自 flare-proto）
    pub message: Message,
    
    /// SDK 扩展信息
    pub extension: MessageExtension,
}

impl ExtendedMessage {
    /// 从 Message 创建，扩展信息为空
    pub fn from_message(message: Message) -> Self {
        Self {
            message,
            extension: MessageExtension::default(),
        }
    }
    
    /// 获取发送者头像（优先使用扩展字段）
    pub fn sender_avatar(&self) -> Option<&str> {
        self.extension.sender_avatar.as_deref()
            .or_else(|| self.message.sender_avatar_url.as_deref())
    }
    
    /// 获取发送者名称（优先使用扩展字段）
    pub fn sender_name(&self) -> Option<&str> {
        self.extension.sender_name.as_deref()
            .or_else(|| self.message.sender_nickname.as_deref())
    }
}
```

### 2. 会话模型（Session）

```rust
/// 会话扩展信息（SDK 层特有）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SessionExtension {
    /// 会话头像 URL（群聊/频道）
    pub avatar: Option<String>,
    
    /// 会话显示名称（客户端缓存）
    pub display_name: Option<String>,
    
    /// 是否置顶
    pub is_pinned: bool,
    
    /// 是否免打扰
    pub is_muted: bool,
    
    /// 最后查看时间（本地）
    pub last_viewed_at: Option<i64>,
    
    /// 自定义扩展字段
    pub custom: std::collections::HashMap<String, String>,
}

/// 带扩展的会话摘要（SDK 使用）
#[derive(Debug, Clone)]
pub struct ExtendedSessionSummary {
    /// 基础会话摘要（来自 flare-proto）
    pub session: SessionSummary,
    
    /// SDK 扩展信息
    pub extension: SessionExtension,
}

impl ExtendedSessionSummary {
    /// 获取显示名称（优先使用扩展字段）
    pub fn display_name(&self) -> Option<&str> {
        self.extension.display_name.as_deref()
            .or_else(|| self.session.display_name.as_deref())
    }
    
    /// 获取头像 URL
    pub fn avatar(&self) -> Option<&str> {
        self.extension.avatar.as_deref()
    }
}
```

### 3. 扩展提供者（Extension Provider）

```rust
/// 扩展信息提供者
/// 
/// 用于从各种来源（服务端、本地缓存、用户模块等）获取扩展信息
#[async_trait]
pub trait ExtensionProvider: Send + Sync {
    /// 获取用户扩展信息
    async fn get_user_extension(
        &self,
        user_id: &str,
    ) -> Result<Option<UserExtension>>;
    
    /// 获取会话扩展信息
    async fn get_session_extension(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionExtension>>;
    
    /// 批量获取用户扩展信息
    async fn batch_get_user_extensions(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>>;
}

/// 用户扩展信息
#[derive(Debug, Clone, Default)]
pub struct UserExtension {
    /// 用户头像 URL
    pub avatar: Option<String>,
    
    /// 用户显示名称
    pub name: Option<String>,
    
    /// 用户在线状态
    pub online_status: Option<String>,
    
    /// 自定义字段
    pub custom: std::collections::HashMap<String, String>,
}

/// 默认扩展提供者（从消息/会话的 metadata 中提取）
pub struct DefaultExtensionProvider;

#[async_trait]
impl ExtensionProvider for DefaultExtensionProvider {
    async fn get_user_extension(&self, user_id: &str) -> Result<Option<UserExtension>> {
        // 默认实现：返回 None，由扩展模块填充
        Ok(None)
    }
    
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>> {
        Ok(None)
    }
    
    async fn batch_get_user_extensions(
        &self,
        _user_ids: &[String],
    ) -> Result<Vec<(String, UserExtension)>> {
        Ok(vec![])
    }
}
```

### 4. 扩展管理器（Extension Manager）

```rust
/// 扩展管理器
/// 
/// 负责管理和填充消息、会话的扩展信息
pub struct ExtensionManager {
    /// 扩展提供者列表
    providers: Vec<Arc<dyn ExtensionProvider>>,
    
    /// 本地缓存（可选）
    cache: Option<Arc<dyn ExtensionCache>>,
}

impl ExtensionManager {
    /// 创建新的扩展管理器
    pub fn new() -> Self {
        Self {
            providers: vec![],
            cache: None,
        }
    }
    
    /// 添加扩展提供者
    pub fn add_provider(&mut self, provider: Arc<dyn ExtensionProvider>) {
        self.providers.push(provider);
    }
    
    /// 填充消息扩展信息
    pub async fn enrich_message(&self, message: &mut ExtendedMessage) -> Result<()> {
        // 1. 从缓存获取
        if let Some(cache) = &self.cache {
            if let Some(ext) = cache.get_user_extension(&message.message.sender_id).await? {
                message.extension.sender_avatar = ext.avatar;
                message.extension.sender_name = ext.name;
            }
        }
        
        // 2. 从提供者获取
        for provider in &self.providers {
            if let Some(ext) = provider.get_user_extension(&message.message.sender_id).await? {
                message.extension.sender_avatar = ext.avatar.or(message.extension.sender_avatar);
                message.extension.sender_name = ext.name.or(message.extension.sender_name);
                
                // 更新缓存
                if let Some(cache) = &self.cache {
                    cache.save_user_extension(&message.message.sender_id, &ext).await?;
                }
                
                break; // 使用第一个提供者的结果
            }
        }
        
        Ok(())
    }
    
    /// 填充会话扩展信息
    pub async fn enrich_session(&self, session: &mut ExtendedSessionSummary) -> Result<()> {
        // 类似逻辑...
        Ok(())
    }
    
    /// 批量填充消息扩展信息
    pub async fn batch_enrich_messages(
        &self,
        messages: &mut [ExtendedMessage],
    ) -> Result<()> {
        // 收集所有需要查询的 user_id
        let user_ids: Vec<String> = messages
            .iter()
            .map(|m| m.message.sender_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        
        // 批量获取
        for provider in &self.providers {
            let extensions = provider.batch_get_user_extensions(&user_ids).await?;
            
            // 填充到消息中
            for (user_id, ext) in extensions {
                for message in messages.iter_mut() {
                    if message.message.sender_id == user_id {
                        message.extension.sender_avatar = ext.avatar.or(message.extension.sender_avatar.clone());
                        message.extension.sender_name = ext.name.or(message.extension.sender_name.clone());
                    }
                }
            }
            
            if !extensions.is_empty() {
                break; // 使用第一个提供者的结果
            }
        }
        
        Ok(())
    }
}
```

### 5. 模块结构

```
src/
├── model/
│   ├── mod.rs
│   ├── message.rs              # Message + MessageExtension + ExtendedMessage
│   ├── session.rs              # SessionSummary + SessionExtension + ExtendedSessionSummary
│   ├── extension.rs            # ExtensionProvider, UserExtension, SessionExtension
│   └── sync.rs                 # SyncCursor（保持不变）
│
├── extension/
│   ├── mod.rs
│   ├── manager.rs              # ExtensionManager
│   ├── provider.rs             # ExtensionProvider trait
│   └── cache.rs                # ExtensionCache（可选）
│
├── service/
│   ├── message/
│   │   └── service.rs          # 使用 ExtendedMessage
│   └── session/
│       └── service.rs           # 使用 ExtendedSessionSummary
│
└── storage/
    └── storage_trait.rs         # 存储 ExtendedMessage 和 ExtendedSessionSummary
```

## 实施步骤

### 阶段 1：重构 Model 模块

1. 创建 `MessageExtension` 和 `ExtendedMessage`
2. 创建 `SessionExtension` 和 `ExtendedSessionSummary`
3. 创建 `ExtensionProvider` trait 和基础实现

### 阶段 2：实现扩展管理器

1. 实现 `ExtensionManager`
2. 实现扩展缓存（可选）
3. 集成到 Service 层

### 阶段 3：更新 Service 和 Storage

1. 更新 `MessageService` 使用 `ExtendedMessage`
2. 更新 `SessionService` 使用 `ExtendedSessionSummary`
3. 更新 `StorageBackend` 存储扩展信息

### 阶段 4：集成到 Client

1. 在 `FlareIMClient` 中集成 `ExtensionManager`
2. 自动填充扩展信息
3. 提供扩展提供者注册接口


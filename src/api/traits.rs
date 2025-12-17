//! API Trait 定义
//!
//! 定义各个功能域的 API trait，用于模块化组织

use crate::application::vo::{MessageVO, SessionListVO, SessionVO};
use crate::domain::message::Message as DomainMessage;
use crate::infrastructure::event::EventBus;
use crate::infrastructure::storage::SessionFilter;
use crate::shared::observer::ArcMessageObserver;
use anyhow::Result;
use flare_proto::MessageContent;
use std::collections::HashMap;
use std::sync::Arc;

// 为了向后兼容，保留 Message 类型别名
pub type Message = DomainMessage;

/// 连接管理 API
pub trait ConnectionApi: Send + Sync {
    /// 登录到服务器
    async fn login(&self, user_id: &str, token: &str) -> Result<crate::api::LoginResult>;

    /// 登出
    async fn logout(&self) -> Result<()>;

    /// 获取连接状态
    async fn connection_state(&self) -> crate::infrastructure::connection::ConnectionState;

    /// 设置 AES-256 加密
    async fn set_crypto_aes256(&self, key: &[u8]) -> Result<()>;

    /// 设置自定义加密服务
    async fn set_crypto(&self, crypto: Arc<dyn crate::application::CryptoService>) -> Result<()>;
}

/// 会话管理 API
///
/// 提供完整的会话管理功能，参考微信、Telegram、飞书、Discord 等主流 IM 的设计。
///
/// ## 设计原则
/// - **本地优先**：所有查询方法优先返回本地缓存，保证快速响应
/// - **自动同步**：本地为空时自动触发后台同步（不阻塞）
/// - **乐观更新**：写操作立即更新本地，异步同步到服务器
/// - **统一命名**：使用 `session` 作为统一术语（而非 conversation）
pub trait SessionApi: Send + Sync {
    // ========== 会话查询 ==========

    /// 获取所有会话列表
    ///
    /// # 参数
    /// - `filter`: 会话过滤器（可选）
    ///
    /// # 返回
    /// - `Result<Vec<SessionVO>>`: 会话列表（按更新时间倒序）
    ///
    /// # 设计说明
    /// - 优先返回本地缓存，保证快速响应
    /// - 如果本地为空且已连接，自动触发后台同步（不阻塞）
    /// - 支持过滤条件：会话类型、业务类型、标签、是否隐藏等
    ///
    /// # 示例
    /// ```rust,no_run
    /// let sessions = client.get_sessions(SessionFilter::default()).await?;
    /// ```
    async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionVO>>;

    /// 分页获取会话列表（游标分页）
    ///
    /// # 参数
    /// - `limit`: 每页数量（建议 20-50）
    /// - `cursor`: 游标（可选，用于下一页）
    /// - `filter`: 会话过滤器（可选）
    ///
    /// # 返回
    /// - `Result<(Vec<SessionVO>, Option<String>)>`: (会话列表, 下一页游标)
    ///
    /// # 设计说明
    /// - 使用游标分页，避免偏移量过大时的性能问题
    /// - 游标格式：`timestamp:<timestamp>:<session_id>`
    /// - 适用于会话数量较多的场景（如企业 IM）
    ///
    /// # 示例
    /// ```rust,no_run
    /// let (sessions, next_cursor) = client.get_sessions_paginated(20, None, None).await?;
    /// if let Some(cursor) = next_cursor {
    ///     let (next_page, _) = client.get_sessions_paginated(20, Some(cursor), None).await?;
    /// }
    /// ```
    async fn get_sessions_paginated(
        &self,
        limit: usize,
        cursor: Option<String>,
        filter: Option<SessionFilter>,
    ) -> Result<(Vec<SessionVO>, Option<String>)>;

    /// 获取单个会话详情
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - `Result<Option<SessionVO>>`: 会话详情（如果不存在返回 None）
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionVO>>;

    /// 批量获取会话详情
    ///
    /// # 参数
    /// - `session_ids`: 会话 ID 列表
    ///
    /// # 返回
    /// - `Result<Vec<SessionVO>>`: 会话列表（不存在的会话会被跳过）
    ///
    /// # 性能优化
    /// - 批量查询，减少数据库往返
    async fn get_sessions_batch(&self, session_ids: Vec<String>) -> Result<Vec<SessionVO>>;

    /// 根据业务信息查找会话 ID
    ///
    /// # 参数
    /// - `session_type`: 会话类型（single/group/channel）
    /// - `business_type`: 业务类型
    /// - `target_id`: 目标 ID（单聊时为对方用户 ID，群聊时为群 ID）
    ///
    /// # 返回
    /// - `Result<Option<String>>`: 会话 ID（如果不存在返回 None）
    ///
    /// # 设计说明
    /// - 会话 ID 生成规则：`{session_type}:{business_type}:{target_id}`
    async fn find_session_id(
        &self,
        session_type: &str,
        business_type: &str,
        target_id: &str,
    ) -> Result<Option<String>>;

    /// 获取会话列表（带扩展信息）
    #[cfg(feature = "extensions")]
    async fn get_sessions_extended(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<crate::domain::session::ExtendedSessionSummary>>;

    /// 获取会话详情（带扩展信息）
    #[cfg(feature = "extensions")]
    async fn get_session_extended(
        &self,
        session_id: &str,
    ) -> Result<crate::domain::session::ExtendedSessionSummary>;

    // ========== 会话操作 ==========

    /// 创建会话
    ///
    /// # 参数
    /// - `session_id`: 会话 ID（可选，如果不提供则自动生成）
    /// - `session_type`: 会话类型（single/group/channel）
    /// - `business_type`: 业务类型
    /// - `display_name`: 显示名称（可选）
    /// - `participants`: 参与者列表（可选）
    ///
    /// # 返回
    /// - `Result<String>`: 创建的会话 ID
    async fn create_session(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Option<Vec<String>>,
    ) -> Result<String>;

    /// 更新会话信息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `updates`: 要更新的字段（display_name, avatar_url, metadata 等）
    ///
    /// # 返回
    /// - `Result<()>`: 更新结果
    async fn update_session(
        &self,
        session_id: &str,
        updates: std::collections::HashMap<String, String>,
    ) -> Result<()>;

    /// 删除会话（及所有消息）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `delete_messages`: 是否同时删除消息（默认 true）
    ///
    /// # 返回
    /// - `Result<usize>`: 删除的消息数量（如果 delete_messages=true）
    async fn delete_session(&self, session_id: &str, delete_messages: bool) -> Result<usize>;

    /// 隐藏会话（从列表中移除，但不删除）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    async fn hide_session(&self, session_id: &str) -> Result<()>;

    /// 显示会话（恢复隐藏的会话）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    async fn show_session(&self, session_id: &str) -> Result<()>;

    // ========== 未读数管理 ==========

    /// 获取总未读数（所有会话的未读数之和）
    ///
    /// # 返回
    /// - `Result<u32>`: 总未读数
    ///
    /// # 设计说明
    /// - 用于显示应用角标（如 iOS/Android 的 badge）
    /// - 性能优化：从本地缓存读取
    async fn get_total_unread_count(&self) -> Result<u32>;

    /// 标记会话已读
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `message_seq`: 已读到的消息序列号（可选，如果不提供则标记所有消息为已读）
    ///
    /// # 返回
    /// - `Result<()>`: 操作结果
    ///
    /// # 设计说明
    /// - 本地立即更新未读数（乐观更新）
    /// - 异步同步到服务器（后台任务）
    /// - 支持部分已读（指定 message_seq）
    async fn mark_read(&self, session_id: &str, message_seq: Option<i64>) -> Result<()>;

    /// 批量标记会话已读
    ///
    /// # 参数
    /// - `session_ids`: 会话 ID 列表
    ///
    /// # 返回
    /// - `Result<usize>`: 成功标记的会话数量
    async fn mark_read_batch(&self, session_ids: Vec<String>) -> Result<usize>;

    // ========== 会话草稿 ==========

    /// 设置会话草稿
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `draft`: 草稿内容（如果为 None 则清空草稿）
    ///
    /// # 设计说明
    /// - 草稿仅保存在本地，不同步到服务器
    async fn set_draft(&self, session_id: &str, draft: Option<String>) -> Result<()>;

    /// 获取会话草稿
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - `Result<Option<String>>`: 草稿内容（如果没有草稿返回 None）
    async fn get_draft(&self, session_id: &str) -> Result<Option<String>>;

    // ========== 输入状态（Typing Indicator）==========

    /// 发送输入状态（正在输入/停止输入）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `is_typing`: 是否正在输入
    ///
    /// # 设计说明
    /// - 仅支持单聊（一对一聊天）
    /// - 自动管理状态：发送后 3 秒自动停止
    async fn send_typing(&self, session_id: &str, is_typing: bool) -> Result<()>;

    /// 获取对方的输入状态
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - `Result<bool>`: 是否正在输入
    ///
    /// # 设计说明
    /// - 状态有效期：3 秒
    /// - 仅支持单聊
    async fn get_typing_status(&self, session_id: &str) -> Result<bool>;
}

/// 消息管理 API
///
/// 提供完整的消息创建、发送、查询、操作功能，参考微信、Telegram、飞书、Discord 等主流 IM 的设计。
///
/// ## 设计原则
/// - **职责分离**：创建和发送分离，符合单一职责原则
/// - **统一模式**：所有消息都通过 `create_*` 创建，`send_message()` 统一发送
/// - **灵活性优先**：支持预览、草稿、定时发送等高级功能
/// - **本地优先**：查询方法优先返回本地缓存
///
/// ## 使用方式
///
/// ### 标准流程：创建 + 发送
/// ```rust,no_run
/// // 1. 创建消息
/// let msg = client.create_text_message("session_123", "Hello", None)?;
///
/// // 2. 可选：预览、修改、保存草稿
/// // println!("预览: {}", msg.content);
/// // client.set_draft("session_123", Some("Hello")).await?;
///
/// // 3. 发送消息
/// let msg_id = client.send_message(msg).await?;
/// ```
///
/// ### 简单场景：一行完成
/// ```rust,no_run
/// // 创建后立即发送
/// let msg_id = client.send_message(
///     client.create_text_message("session_123", "Hello", None)?
/// ).await?;
/// ```
pub trait MessageApi: Send + Sync {
    // ========== 消息创建方法（核心方法，推荐使用）==========

    /// 创建文本消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `text`: 文本内容
    /// - `mentions`: @提及的用户 ID 列表（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象（尚未发送）
    ///
    /// # 使用场景
    /// - 需要预览消息
    /// - 需要保存草稿
    /// - 需要定时发送
    /// - 需要批量构建后统一发送
    fn create_text_message(
        &self,
        session_id: &str,
        text: &str,
        mentions: Option<Vec<String>>,
    ) -> Result<Message>;

    /// 创建@消息（带@提及的文本消息）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `text`: 文本内容
    /// - `user_ids`: @提及的用户 ID 列表
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_text_at_message(
        &self,
        session_id: &str,
        text: &str,
        user_ids: Vec<String>,
    ) -> Result<Message>;

    /// 创建引用回复消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `quoted_message_id`: 被引用的消息 ID
    /// - `text`: 回复的文本内容
    /// - `preview_text`: 引用内容预览（可选，如果不提供则自动从被引用消息提取）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_quote_message(
        &self,
        session_id: &str,
        quoted_message_id: &str,
        text: &str,
        preview_text: Option<String>,
    ) -> Result<Message>;

    /// 创建位置消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `latitude`: 纬度
    /// - `longitude`: 经度
    /// - `address`: 地址描述（可选）
    /// - `description`: 位置描述（可选，用于位置说明文字）
    /// - `poi_id`: POI ID（可选，关联的地点ID）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_location_message(
        &self,
        session_id: &str,
        latitude: f64,
        longitude: f64,
        address: Option<String>,
        description: Option<String>,
        poi_id: Option<String>,
    ) -> Result<Message>;

    /// 创建卡片消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `title`: 卡片标题
    /// - `description`: 卡片描述（可选）
    /// - `image_url`: 卡片图片 URL（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_card_message(
        &self,
        session_id: &str,
        title: &str,
        description: Option<String>,
        image_url: Option<String>,
    ) -> Result<Message>;

    /// 创建表情消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `emoji`: 表情符号或表情 ID
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_face_message(&self, session_id: &str, emoji: &str) -> Result<Message>;

    /// 创建自定义消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `data`: 自定义数据（字节数组）
    /// - `mime_type`: MIME 类型
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_custom_message(
        &self,
        session_id: &str,
        data: Vec<u8>,
        mime_type: &str,
    ) -> Result<Message>;

    // ========== 图片消息创建（多种方式）==========

    /// 根据文件绝对路径创建图片消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `image_path`: 图片文件绝对路径
    /// - `description`: 图片描述（可选，用于图片说明文字）
    /// - `options`: 上传选项（压缩、缩略图等）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象（包含上传后的 URL）
    ///
    /// # 注意
    /// 此方法会立即上传文件，返回的消息已包含上传后的 URL
    async fn create_image_message_from_full_path(
        &self,
        session_id: &str,
        image_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    /// 根据 URL 创建图片消息（文件已上传）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `image_url`: 图片 URL
    /// - `width`: 图片宽度（可选）
    /// - `height`: 图片高度（可选）
    /// - `description`: 图片描述（可选，用于图片说明文字）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_image_message_by_url(
        &self,
        session_id: &str,
        image_url: String,
        width: Option<i32>,
        height: Option<i32>,
        description: Option<String>,
    ) -> Result<Message>;

    /// 根据文件对象创建图片消息（Web 平台）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file`: 文件对象（Web File API）
    /// - `description`: 图片描述（可选，用于图片说明文字）
    /// - `options`: 上传选项
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    #[cfg(target_arch = "wasm32")]
    async fn create_image_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    // ========== 音频消息创建（多种方式）==========

    /// 根据文件绝对路径创建音频消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `audio_path`: 音频文件绝对路径
    /// - `description`: 音频描述（可选，用于音频说明文字）
    /// - `options`: 上传选项
    async fn create_sound_message_from_full_path(
        &self,
        session_id: &str,
        audio_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    /// 根据 URL 创建音频消息（文件已上传）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `audio_url`: 音频 URL
    /// - `duration`: 音频时长（秒，可选）
    /// - `description`: 音频描述（可选，用于音频说明文字）
    fn create_sound_message_by_url(
        &self,
        session_id: &str,
        audio_url: String,
        duration: Option<i32>,
        description: Option<String>,
    ) -> Result<Message>;

    /// 根据文件对象创建音频消息（Web 平台）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file`: 文件对象（Web File API）
    /// - `description`: 音频描述（可选，用于音频说明文字）
    /// - `options`: 上传选项
    #[cfg(target_arch = "wasm32")]
    async fn create_sound_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    // ========== 视频消息创建（多种方式）==========

    /// 根据文件绝对路径创建视频消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `video_path`: 视频文件绝对路径
    /// - `description`: 视频描述（可选，用于视频说明文字）
    /// - `options`: 上传选项（压缩、封面图等）
    async fn create_video_message_from_full_path(
        &self,
        session_id: &str,
        video_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    /// 根据 URL 创建视频消息（文件已上传）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `video_url`: 视频 URL
    /// - `duration`: 视频时长（秒，可选）
    /// - `width`: 视频宽度（可选）
    /// - `height`: 视频高度（可选）
    /// - `description`: 视频描述（可选，用于视频说明文字）
    fn create_video_message_by_url(
        &self,
        session_id: &str,
        video_url: String,
        duration: Option<i32>,
        width: Option<i32>,
        height: Option<i32>,
        description: Option<String>,
    ) -> Result<Message>;

    /// 根据文件对象创建视频消息（Web 平台）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file`: 文件对象（Web File API）
    /// - `description`: 视频描述（可选，用于视频说明文字）
    /// - `options`: 上传选项
    #[cfg(target_arch = "wasm32")]
    async fn create_video_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    // ========== 文件消息创建（多种方式）==========

    /// 根据文件绝对路径创建文件消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file_path`: 文件绝对路径
    /// - `description`: 文件描述（可选，用于文件说明文字）
    /// - `options`: 上传选项
    async fn create_file_message_from_full_path(
        &self,
        session_id: &str,
        file_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    /// 根据 URL 创建文件消息（文件已上传）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file_url`: 文件 URL
    /// - `file_name`: 文件名
    /// - `file_size`: 文件大小（字节）
    /// - `description`: 文件描述（可选，用于文件说明文字）
    fn create_file_message_by_url(
        &self,
        session_id: &str,
        file_url: String,
        file_name: String,
        file_size: i64,
        description: Option<String>,
    ) -> Result<Message>;

    /// 根据文件对象创建文件消息（Web 平台）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `file`: 文件对象（Web File API）
    /// - `description`: 文件描述（可选，用于文件说明文字）
    /// - `options`: 上传选项
    #[cfg(target_arch = "wasm32")]
    async fn create_file_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<Message>;

    // ========== 转发和合并消息创建 ==========

    /// 创建转发消息
    ///
    /// # 参数
    /// - `session_id`: 目标会话 ID
    /// - `message_ids`: 要转发的消息 ID 列表
    /// - `merge`: 是否合并转发（默认 false）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_forward_message(
        &self,
        session_id: &str,
        message_ids: Vec<String>,
        merge: bool,
    ) -> Result<Message>;

    /// 创建合并消息（多条消息合并为一条）
    ///
    /// # 参数
    /// - `session_id`: 目标会话 ID
    /// - `message_ids`: 要合并的消息 ID 列表
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_merge_message(&self, session_id: &str, message_ids: Vec<String>) -> Result<Message>;

    // ========== 扩展消息类型创建 ==========

    /// 创建链接卡片消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `url`: 链接地址
    /// - `title`: 标题
    /// - `description`: 描述（可选）
    /// - `thumbnail_url`: 缩略图 URL（可选）
    /// - `site_name`: 网站名称（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_link_card_message(
        &self,
        session_id: &str,
        url: String,
        title: String,
        description: Option<String>,
        thumbnail_url: Option<String>,
        site_name: Option<String>,
    ) -> Result<Message>;

    /// 创建小程序消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `app_id`: 小程序 ID
    /// - `page_path`: 页面路径
    /// - `title`: 标题
    /// - `description`: 描述（可选）
    /// - `thumbnail_url`: 缩略图 URL（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_mini_program_message(
        &self,
        session_id: &str,
        app_id: String,
        page_path: String,
        title: String,
        description: Option<String>,
        thumbnail_url: Option<String>,
    ) -> Result<Message>;

    /// 创建投票消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `question`: 问题/标题
    /// - `options`: 选项列表
    /// - `allow_multiple`: 是否允许多选（默认 false）
    /// - `expire_at`: 过期时间（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_vote_message(
        &self,
        session_id: &str,
        question: String,
        options: Vec<String>,
        allow_multiple: bool,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<Message>;

    /// 创建任务消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `title`: 任务标题
    /// - `description`: 任务描述（可选）
    /// - `assignee_id`: 负责人 ID（可选）
    /// - `due_date`: 截止日期（可选）
    /// - `priority`: 优先级（可选，0=低，1=中，2=高）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_task_message(
        &self,
        session_id: &str,
        title: String,
        description: Option<String>,
        assignee_id: Option<String>,
        due_date: Option<prost_types::Timestamp>,
        priority: Option<i32>,
    ) -> Result<Message>;

    /// 创建日程消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `title`: 日程标题
    /// - `description`: 日程描述（可选）
    /// - `start_time`: 开始时间
    /// - `end_time`: 结束时间
    /// - `location`: 地点（可选）
    /// - `attendees`: 参与者 ID 列表（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_schedule_message(
        &self,
        session_id: &str,
        title: String,
        description: Option<String>,
        start_time: prost_types::Timestamp,
        end_time: prost_types::Timestamp,
        location: Option<String>,
        attendees: Option<Vec<String>>,
    ) -> Result<Message>;

    /// 创建群公告消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID（群聊）
    /// - `title`: 公告标题
    /// - `content`: 公告内容
    /// - `pinned`: 是否置顶（默认 true）
    /// - `expire_at`: 过期时间（可选）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_announcement_message(
        &self,
        session_id: &str,
        title: String,
        content: String,
        pinned: bool,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<Message>;

    /// 创建通知消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `notification_type`: 通知类型（业务系统定义，如 "friend_request", "group_joined"）
    /// - `title`: 通知标题
    /// - `body`: 通知内容
    /// - `data`: 通知数据（可选，键值对）
    /// - `target_user_ids`: 目标用户 ID 列表（可选，用于定向通知）
    ///
    /// # 返回
    /// - `Result<Message>`: 消息对象
    fn create_notification_message(
        &self,
        session_id: &str,
        notification_type: String,
        title: String,
        body: String,
        data: Option<std::collections::HashMap<String, String>>,
        target_user_ids: Option<Vec<String>>,
    ) -> Result<Message>;

    // ========== 统一发送方法（核心方法）==========

    /// 发送消息（统一发送接口）
    ///
    /// # 参数
    /// - `message`: 消息对象（由 create_* 方法创建）
    /// - `receiver_id`: 接收者 ID（单聊时必需，群聊时为空）
    /// - `channel_id`: 通道 ID（群聊/频道时使用，单聊时为空）
    ///
    /// # 返回
    /// - `Result<String>`: 消息 ID
    ///
    /// # 路由规则
    /// - **单聊**：必须提供 `receiver_id`，`channel_id` 应为 `None`
    /// - **群聊/频道**：必须提供 `channel_id`，`receiver_id` 应为 `None`
    /// - `receiver_id` 和 `channel_id` 必须至少提供一个（不能同时为 `None`）
    /// - 如果 `message` 对象中已包含 `receiver_id` 或 `channel_id`，参数会覆盖消息对象中的值
    ///
    /// # 使用场景
    /// - 发送已创建的消息
    /// - 支持预览后发送
    /// - 支持草稿发送
    /// - 支持定时发送（需要配合任务调度器）
    ///
    /// # 示例
    /// ```rust,no_run
    /// // 单聊：创建并发送消息
    /// let msg = client.create_text_message("session_123", "Hello", None)?;
    /// let msg_id = client.send_message(msg, Some("user_456".to_string()), None).await?;
    ///
    /// // 群聊：创建并发送消息
    /// let msg = client.create_text_message("group_123", "Hello", None)?;
    /// let msg_id = client.send_message(msg, None, Some("group_123".to_string())).await?;
    /// ```
    async fn send_message(
        &self,
        message: Message,
        receiver_id: Option<String>,
        channel_id: Option<String>,
    ) -> Result<String>;

    // ========== 消息操作 ==========

    /// 撤回消息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    ///
    /// # 设计说明
    /// - 撤回时间限制：通常为 2 分钟（由服务器控制）
    /// - 撤回后消息内容会被替换为"已撤回"提示
    async fn recall_message(&self, message_id: &str) -> Result<()>;

    /// 批量撤回消息
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    ///
    /// # 返回
    /// - `Result<Vec<(String, Result<()>)>>`: 撤回结果列表（消息ID -> 结果）
    async fn recall_messages_batch(
        &self,
        message_ids: Vec<String>,
    ) -> Result<Vec<(String, Result<()>)>>;

    /// 编辑消息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `new_content`: 新的消息内容
    ///
    /// # 设计说明
    /// - 仅支持文本消息编辑
    /// - 编辑后会在消息上显示"已编辑"标记
    async fn edit_message(&self, message_id: &str, new_content: &str) -> Result<()>;

    /// 删除消息（服务端删除）
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `delete_type`: 删除类型（0=软删除，1=硬删除）
    /// - `notify_others`: 是否通知其他用户
    ///
    /// # 设计说明
    /// - 软删除：标记为已删除，仍可恢复
    /// - 硬删除：彻底删除，不可恢复
    async fn delete_message(
        &self,
        message_id: &str,
        delete_type: i32,
        notify_others: bool,
    ) -> Result<()>;

    /// 批量删除消息（服务端删除）
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    /// - `delete_type`: 删除类型
    ///
    /// # 返回
    /// - `Result<Vec<(String, Result<()>)>>`: 删除结果列表
    async fn delete_messages_batch(
        &self,
        message_ids: Vec<String>,
        delete_type: i32,
    ) -> Result<Vec<(String, Result<()>)>>;

    /// 删除本地消息（仅本地，不通知服务器）
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    async fn delete_local(&self, message_id: &str) -> Result<()>;

    /// 批量删除本地消息
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    async fn delete_local_batch(&self, message_ids: Vec<String>) -> Result<()>;

    /// 清空会话消息（仅本地）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - `Result<usize>`: 删除的消息数量
    async fn clear_local(&self, session_id: &str) -> Result<usize>;

    /// 清空会话消息（本地和服务端）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - `Result<usize>`: 删除的消息数量
    async fn clear(&self, session_id: &str) -> Result<usize>;

    // ========== 消息反应 ==========

    /// 添加消息反应（表情）
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `emoji`: 表情符号
    async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()>;

    /// 移除消息反应
    ///   
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `emoji`: 表情符号
    async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()>;

    // ========== 消息转发 ==========

    /// 转发消息
    ///
    /// # 参数
    /// - `message_ids`: 要转发的消息 ID 列表
    /// - `target_session_id`: 目标会话 ID
    /// - `merge`: 是否合并转发（默认 false）
    ///
    /// # 返回
    /// - `Result<Vec<String>>`: 转发的消息 ID 列表
    async fn forward_messages(
        &self,
        message_ids: Vec<String>,
        target_session_id: &str,
        merge: bool,
    ) -> Result<Vec<String>>;

    /// 批量转发消息（转发到多个会话）
    ///
    /// # 参数
    /// - `message_ids`: 要转发的消息 ID 列表
    /// - `target_session_ids`: 目标会话 ID 列表
    /// - `merge`: 是否合并转发
    ///
    /// # 返回
    /// - `Result<HashMap<String, Vec<String>>>`: 转发结果（目标会话ID -> 消息ID列表）
    async fn forward_messages_batch(
        &self,
        message_ids: Vec<String>,
        target_session_ids: Vec<String>,
        merge: bool,
    ) -> Result<std::collections::HashMap<String, Vec<String>>>;

    // ========== 消息查询 ==========

    /// 获取会话消息列表
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 每页数量
    /// - `cursor`: 游标（可选，用于分页）
    ///
    /// # 返回
    /// - `Result<Vec<Message>>`: 消息列表（按时间倒序，最新的在前）
    ///
    /// # 设计说明
    /// - 优先返回本地缓存
    /// - 如果本地消息不足，自动触发后台同步
    async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<MessageVO>>;

    /// 获取消息（根据消息 ID）
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    ///
    /// # 返回
    /// - `Result<Option<MessageVO>>`: 消息（如果不存在返回 None）
    async fn get_message(&self, message_id: &str) -> Result<Option<MessageVO>>;

    /// 批量获取消息
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    ///
    /// # 返回
    /// - `Result<Vec<MessageVO>>`: 消息列表（不存在的消息会被跳过）
    async fn get_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageVO>>;

    /// 搜索消息
    ///
    /// # 参数
    /// - `query`: 搜索关键词
    /// - `session_id`: 会话 ID（可选，如果提供则只搜索该会话）
    /// - `limit`: 最大返回数量
    ///
    /// # 返回
    /// - `Result<Vec<MessageVO>>`: 匹配的消息列表
    async fn search(
        &self,
        query: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MessageVO>>;

    /// 获取消息列表（带扩展信息）
    #[cfg(feature = "extensions")]
    async fn get_messages_extended(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<crate::domain::message::ExtendedMessage>>;

    // ========== 消息重试 ==========

    /// 重试发送消息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    async fn retry(&self, message_id: &str) -> Result<()>;

    /// 取消消息重试
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    async fn cancel_retry(&self, message_id: &str) -> Result<()>;

    /// 获取重试中的消息列表
    ///
    /// # 返回
    /// - `Vec<String>`: 消息 ID 列表
    async fn get_retrying(&self) -> Vec<String>;

    // ========== 消息扩展功能 ==========

    /// 置顶消息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `expire_at`: 置顶到期时间（可选）
    async fn pin(&self, message_id: &str, expire_at: Option<prost_types::Timestamp>) -> Result<()>;

    /// 取消置顶
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    async fn unpin(&self, message_id: &str) -> Result<()>;

    /// 收藏消息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `tags`: 收藏标签（可选）
    /// - `note`: 收藏备注（可选）
    async fn favorite(
        &self,
        message_id: &str,
        tags: Option<Vec<String>>,
        note: Option<String>,
    ) -> Result<()>;

    /// 取消收藏
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    async fn unfavorite(&self, message_id: &str) -> Result<()>;

    /// 设置消息扩展信息（本地）
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `extension`: 扩展信息（键值对）
    async fn set_extension(
        &self,
        message_id: &str,
        extension: HashMap<String, String>,
    ) -> Result<()>;
}

/// 事件通知 API
pub trait EventApi: Send + Sync {
    /// 获取事件总线（用于订阅事件）
    fn event_bus(&self) -> Arc<EventBus>;

    /// 注册消息观察者（统一的消息处理接口）
    async fn register_message_observer(&self, observer: ArcMessageObserver);
}

/// 数据同步 API
pub trait SyncApi: Send + Sync {
    /// 同步消息（增量）
    async fn sync_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<crate::domain::sync::SyncResult>;

    /// 同步会话（增量/全量）
    async fn sync_sessions(
        &self,
        cursor: Option<String>,
    ) -> Result<crate::application::vo::session::SessionSyncResultVO>;
}

/// 扩展功能 API
#[cfg(feature = "extensions")]
pub trait ExtensionApi: Send + Sync {
    /// 注册扩展提供者
    async fn register_extension_provider(
        &self,
        provider: Arc<dyn crate::domain::ExtensionProvider>,
    ) -> Result<()>;

    /// 设置扩展缓存
    async fn set_extension_cache(
        &self,
        cache: Arc<dyn crate::domain::ExtensionCache>,
    ) -> Result<()>;

    /// 注册用户业务扩展点
    ///
    /// # 参数
    /// - `extension`: 用户业务扩展点实例
    ///
    /// # 示例
    /// ```rust,no_run
    /// use flare_im_core_sdk::shared::extension::business::UserBusinessExtension;
    ///
    /// struct MyUserExtension;
    /// impl UserBusinessExtension for MyUserExtension { ... }
    ///
    /// client.register_user_business_extension(Arc::new(MyUserExtension)).await?;
    /// ```
    async fn register_user_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::UserBusinessExtension>,
    ) -> Result<()>;

    /// 注册群组业务扩展点
    ///
    /// # 参数
    /// - `extension`: 群组业务扩展点实例
    async fn register_group_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::GroupBusinessExtension>,
    ) -> Result<()>;

    /// 注册频道业务扩展点
    ///
    /// # 参数
    /// - `extension`: 频道业务扩展点实例
    async fn register_channel_business_extension(
        &self,
        extension: Arc<dyn crate::shared::extension::business::ChannelBusinessExtension>,
    ) -> Result<()>;

    /// 获取业务扩展注册中心（用于高级用法）
    fn business_extension_registry(
        &self,
    ) -> Arc<crate::shared::extension::BusinessExtensionRegistry>;
}

/// 工具方法 API
pub trait UtilityApi: Send + Sync {
    /// 获取当前用户 ID
    async fn user_id(&self) -> Result<String>;

    /// 获取性能指标快照
    fn metrics_snapshot(&self) -> crate::shared::metrics::MetricsSnapshot;

    /// 重置性能指标
    fn reset_metrics(&self);

    /// 获取任务管理器（用于高级用法）
    fn task_manager(&self) -> Arc<crate::infrastructure::task::TaskManager>;

    /// 获取任务调度器（用于注册和调度自定义任务）
    fn task_scheduler(&self) -> Arc<crate::infrastructure::task::TaskScheduler>;

    /// 获取存储后端（用于高级用法）
    fn storage(&self) -> Arc<dyn crate::infrastructure::storage::StorageBackend>;

    /// 获取消息服务（用于高级用法）
    fn message_command_handler(&self) -> Arc<crate::application::MessageCommandHandler>;
    fn message_query_handler(&self) -> Arc<crate::application::MessageQueryHandler>;

    /// 获取内存泄漏检测器（仅在 debug 模式）
    #[cfg(debug_assertions)]
    fn leak_detector(&self) -> Arc<crate::shared::memory_leak_detector::MemoryLeakDetector>;
}

/// 任务调度 API
///
/// 提供任务注册、调度、状态查询等功能
pub trait TaskApi: Send + Sync {
    /// 注册自定义任务执行器
    ///
    /// # 参数
    /// - `executor`: 任务执行器
    ///
    /// # 示例
    /// ```rust,no_run
    /// use flare_im_core_sdk::infrastructure::task::executor::SyncTaskExecutor;
    ///
    /// struct MyTask;
    /// impl SyncTaskExecutor for MyTask { ... }
    ///
    /// client.register_task(Arc::new(MyTask)).await;
    /// ```
    async fn register_task(
        &self,
        executor: Arc<dyn crate::infrastructure::task::executor::SyncTaskExecutor>,
    );

    /// 取消注册任务执行器
    async fn unregister_task(&self, name: &str) -> bool;

    /// 获取所有已注册的任务名称
    async fn get_registered_tasks(&self) -> Vec<String>;

    /// 调度任务（通过任务名称）
    ///
    /// # 参数
    /// - `task_name`: 任务名称（必须是已注册的任务）
    /// - `task_id`: 任务 ID（可选，如果不提供则自动生成）
    ///
    /// # 返回
    /// - `Result<String>`: 任务 ID
    async fn schedule_task_by_name(
        &self,
        task_name: &str,
        task_id: Option<String>,
    ) -> Result<String>;

    /// 获取任务状态
    async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Option<crate::infrastructure::task::standard::TaskStatus>;

    /// 取消任务
    async fn cancel_task(&self, task_id: &str) -> bool;

    /// 获取任务调度器统计信息
    async fn get_task_scheduler_stats(&self) -> crate::infrastructure::task::TaskSchedulerStats;

    /// 获取任务调度器性能快照（用于性能监控）
    async fn get_task_scheduler_performance(
        &self,
    ) -> crate::infrastructure::task::TaskSchedulerPerformanceSnapshot;
}

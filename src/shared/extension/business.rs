//! 业务扩展点接口定义
//!
//! 提供用户、群组、频道等业务模块的扩展接口
//!
//! ## 设计理念
//!
//! 参考微信、Telegram、Discord 等顶级 IM SDK 的设计：
//! - **接口隔离**: 每个业务领域有独立的扩展接口
//! - **依赖倒置**: 业务层依赖抽象，不依赖具体实现
//! - **可组合性**: 多个扩展点可以组合使用
//! - **优先级机制**: 支持扩展点优先级，高优先级覆盖低优先级

use crate::api::FlareIMClient;
use crate::shared::extension::point::ExtensionPoint;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 业务领域类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BusinessDomain {
    /// 用户业务
    User,
    /// 群组业务
    Group,
    /// 频道业务
    Channel,
    /// 自定义业务
    Custom(String),
}

impl std::fmt::Display for BusinessDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BusinessDomain::User => write!(f, "user"),
            BusinessDomain::Group => write!(f, "group"),
            BusinessDomain::Channel => write!(f, "channel"),
            BusinessDomain::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

/// 业务扩展点基接口
///
/// 所有业务扩展点必须实现此接口
#[async_trait]
pub trait BusinessExtensionPoint: ExtensionPoint + Send + Sync {
    /// 业务领域
    fn business_domain(&self) -> BusinessDomain;

    /// 扩展优先级（数字越小优先级越高，0-255）
    /// 默认优先级为 100
    fn priority(&self) -> u8 {
        100
    }

    /// 扩展点依赖的其他扩展点（按业务领域）
    /// 返回依赖的业务领域列表
    fn dependencies(&self) -> Vec<BusinessDomain> {
        vec![]
    }

    /// 健康检查
    /// 返回扩展点是否正常工作
    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// 用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// 用户 ID
    pub user_id: String,
    /// 用户名
    pub name: String,
    /// 用户头像 URL
    pub avatar: Option<String>,
    /// 在线状态
    pub online_status: OnlineStatus,
    /// 用户签名/简介
    pub bio: Option<String>,
    /// 自定义字段
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

/// 在线状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnlineStatus {
    /// 在线
    Online,
    /// 离线
    Offline,
    /// 忙碌
    Busy,
    /// 离开
    Away,
    /// 隐身
    Invisible,
}

/// 用户变更回调
pub type UserChangeCallback = Arc<dyn Fn(UserChangeEvent) + Send + Sync>;

/// 用户变更事件
#[derive(Debug, Clone)]
pub struct UserChangeEvent {
    /// 用户 ID
    pub user_id: String,
    /// 变更类型
    pub change_type: UserChangeType,
    /// 变更后的用户信息（如果有）
    pub user_info: Option<UserInfo>,
}

/// 用户变更类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserChangeType {
    /// 信息更新
    Updated,
    /// 在线状态变更
    StatusChanged,
    /// 用户删除
    Deleted,
}

/// 用户业务扩展点
///
/// 实现此接口以提供用户相关的业务功能
#[async_trait]
pub trait UserBusinessExtension: BusinessExtensionPoint {
    /// 获取用户信息
    ///
    /// # 参数
    /// - `user_id`: 用户 ID
    ///
    /// # 返回
    /// - `Ok(Some(UserInfo))`: 用户信息
    /// - `Ok(None)`: 用户不存在
    /// - `Err`: 获取失败
    async fn get_user_info(&self, user_id: &str) -> Result<Option<UserInfo>>;

    /// 批量获取用户信息
    ///
    /// # 参数
    /// - `user_ids`: 用户 ID 列表
    ///
    /// # 返回
    /// - 用户信息列表（可能包含部分失败的结果）
    async fn batch_get_user_info(&self, user_ids: &[String]) -> Result<Vec<UserInfo>> {
        // 默认实现：串行调用 get_user_info
        let mut results = Vec::with_capacity(user_ids.len());
        for user_id in user_ids {
            if let Ok(Some(info)) = self.get_user_info(user_id).await {
                results.push(info);
            }
        }
        Ok(results)
    }

    /// 监听用户信息变更
    ///
    /// # 参数
    /// - `callback`: 变更回调函数
    ///
    /// # 注意
    /// - 如果扩展点不支持监听，可以返回 Ok(()) 但不实际监听
    async fn subscribe_user_changes(&self, _callback: UserChangeCallback) -> Result<()> {
        // 默认实现：不支持监听
        Ok(())
    }

    /// 搜索用户
    ///
    /// # 参数
    /// - `keyword`: 搜索关键词（用户名、ID 等）
    /// - `limit`: 返回结果数量限制
    ///
    /// # 返回
    /// - 匹配的用户列表
    async fn search_users(&self, _keyword: &str, _limit: usize) -> Result<Vec<UserInfo>> {
        // 默认实现：不支持搜索
        Ok(vec![])
    }
}

/// 群组信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    /// 群组 ID
    pub group_id: String,
    /// 群组名称
    pub name: String,
    /// 群组头像 URL
    pub avatar: Option<String>,
    /// 群组描述
    pub description: Option<String>,
    /// 群主 ID
    pub owner_id: String,
    /// 成员数量
    pub member_count: u32,
    /// 最大成员数（None 表示无限制）
    pub max_members: Option<u32>,
    /// 是否公开群组
    pub is_public: bool,
    /// 自定义字段
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

/// 群组成员信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    /// 用户 ID
    pub user_id: String,
    /// 群内昵称
    pub nickname: Option<String>,
    /// 角色（owner/admin/member）
    pub role: GroupMemberRole,
    /// 加入时间
    pub joined_at: Option<i64>,
    /// 自定义字段
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

/// 群组成员角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupMemberRole {
    /// 群主
    Owner,
    /// 管理员
    Admin,
    /// 普通成员
    Member,
}

/// 群组成员查询结果
#[derive(Debug, Clone)]
pub struct GroupMembersResult {
    /// 成员列表
    pub members: Vec<GroupMember>,
    /// 下一页游标（如果有）
    pub next_cursor: Option<String>,
    /// 是否还有更多
    pub has_more: bool,
}

/// 群组变更回调
pub type GroupChangeCallback = Arc<dyn Fn(GroupChangeEvent) + Send + Sync>;

/// 群组变更事件
#[derive(Debug, Clone)]
pub struct GroupChangeEvent {
    /// 群组 ID
    pub group_id: String,
    /// 变更类型
    pub change_type: GroupChangeType,
    /// 变更后的群组信息（如果有）
    pub group_info: Option<GroupInfo>,
}

/// 群组变更类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupChangeType {
    /// 信息更新
    Updated,
    /// 成员加入
    MemberJoined,
    /// 成员离开
    MemberLeft,
    /// 成员被踢出
    MemberKicked,
    /// 群组解散
    Dissolved,
}

/// 群组业务扩展点
///
/// 实现此接口以提供群组相关的业务功能
#[async_trait]
pub trait GroupBusinessExtension: BusinessExtensionPoint {
    /// 获取群组信息
    ///
    /// # 参数
    /// - `group_id`: 群组 ID
    ///
    /// # 返回
    /// - `Ok(Some(GroupInfo))`: 群组信息
    /// - `Ok(None)`: 群组不存在
    /// - `Err`: 获取失败
    async fn get_group_info(&self, group_id: &str) -> Result<Option<GroupInfo>>;

    /// 获取群成员列表
    ///
    /// # 参数
    /// - `group_id`: 群组 ID
    /// - `limit`: 返回数量限制
    /// - `cursor`: 分页游标
    ///
    /// # 返回
    /// - 群成员列表和分页信息
    async fn get_group_members(
        &self,
        group_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<GroupMembersResult> {
        // 默认实现：不支持成员查询
        Ok(GroupMembersResult {
            members: vec![],
            next_cursor: None,
            has_more: false,
        })
    }

    /// 监听群组变更
    ///
    /// # 参数
    /// - `callback`: 变更回调函数
    async fn subscribe_group_changes(&self, _callback: GroupChangeCallback) -> Result<()> {
        // 默认实现：不支持监听
        Ok(())
    }

    /// 搜索群组
    ///
    /// # 参数
    /// - `keyword`: 搜索关键词
    /// - `limit`: 返回结果数量限制
    ///
    /// # 返回
    /// - 匹配的群组列表
    async fn search_groups(&self, _keyword: &str, _limit: usize) -> Result<Vec<GroupInfo>> {
        // 默认实现：不支持搜索
        Ok(vec![])
    }
}

/// 频道信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// 频道 ID
    pub channel_id: String,
    /// 频道名称
    pub name: String,
    /// 频道描述
    pub description: Option<String>,
    /// 频道类型（text/voice/video）
    pub channel_type: ChannelType,
    /// 成员数量
    pub member_count: u32,
    /// 自定义字段
    #[serde(default)]
    pub custom: HashMap<String, String>,
}

/// 频道类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    /// 文本频道
    Text,
    /// 语音频道
    Voice,
    /// 视频频道
    Video,
}

/// 频道成员查询结果
#[derive(Debug, Clone)]
pub struct ChannelMembersResult {
    /// 成员列表
    pub members: Vec<String>, // 用户 ID 列表
    /// 下一页游标
    pub next_cursor: Option<String>,
    /// 是否还有更多
    pub has_more: bool,
}

/// 频道业务扩展点
///
/// 实现此接口以提供频道相关的业务功能
#[async_trait]
pub trait ChannelBusinessExtension: BusinessExtensionPoint {
    /// 获取频道信息
    ///
    /// # 参数
    /// - `channel_id`: 频道 ID
    ///
    /// # 返回
    /// - `Ok(Some(ChannelInfo))`: 频道信息
    /// - `Ok(None)`: 频道不存在
    /// - `Err`: 获取失败
    async fn get_channel_info(&self, channel_id: &str) -> Result<Option<ChannelInfo>>;

    /// 获取频道成员列表
    ///
    /// # 参数
    /// - `channel_id`: 频道 ID
    /// - `limit`: 返回数量限制
    /// - `cursor`: 分页游标
    ///
    /// # 返回
    /// - 频道成员列表和分页信息
    async fn get_channel_members(
        &self,
        channel_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<ChannelMembersResult> {
        // 默认实现：不支持成员查询
        Ok(ChannelMembersResult {
            members: vec![],
            next_cursor: None,
            has_more: false,
        })
    }
}

/// 自定义业务扩展点
///
/// 用于实现特定业务场景的扩展
#[async_trait]
pub trait CustomBusinessExtension: BusinessExtensionPoint {
    /// 处理自定义业务请求
    ///
    /// # 参数
    /// - `action`: 业务动作
    /// - `params`: 业务参数
    ///
    /// # 返回
    /// - 业务结果（JSON 格式）
    async fn handle_custom_request(
        &self,
        action: &str,
        params: &HashMap<String, String>,
    ) -> Result<serde_json::Value>;
}

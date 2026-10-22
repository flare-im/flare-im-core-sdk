//! 插件实体共享状态（`EVENT_MESSAGE_PLUGIN_STATE`）的应用规则：实时事件与同步重放共用这一处。
//!
//! 共享层来自 Core，可信：Core 只对已绑定的实体发布，并已校验版本严格递增。客户端这里只做
//! 三件事：按协议边界校验快照；确认目标消息是同一插件的卡片；按版本单调写入（更新的才写，
//! 相同版本是重复、更旧的是乱序）。消息还不在本地时交给调用方回滚去重键等重放，和表情回应一样。

use flare_plugin_protocol::state::SharedState;
use flare_proto::common::PluginStateEvent;

use crate::domain::{MessageStore, OperationApplyResult};
use crate::shared::error::Result;

/// 一次共享层应用的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginSharedStateOutcome {
    /// 写入了更新的版本：发布 `MessageEvent::PluginStateChanged`。
    Applied {
        conversation_id: String,
        /// 本地消息 ID（server_id 优先）。
        message_id: String,
        version: u64,
    },
    /// 重复或乱序：已被更新的版本覆盖，不重绘。
    Ignored,
    /// 消息还不在本地：回滚去重键，等消息到达后重放。
    NotFound,
    /// 事件不合法，或目标不是这个插件的卡片：丢弃，重放也不会变好。
    Rejected,
}

/// 把一条 `PluginStateEvent` 写成目标消息的共享层（`pluginSharedStateJson`、
/// `pluginSharedStateVersion`、`pluginSharedEntity`）。
pub(crate) async fn apply_plugin_shared_state(
    messages: &dyn MessageStore,
    event: &PluginStateEvent,
) -> Result<PluginSharedStateOutcome> {
    let server_msg_id = event.server_msg_id.trim();
    let state = match SharedState::from_parts(&event.entity, event.version, &event.values_json) {
        Ok(state) if !server_msg_id.is_empty() => state,
        _ => {
            tracing::warn!(
                target: "flare_sdk.plugin",
                server_msg_id = %event.server_msg_id,
                plugin_id = %event.plugin_id,
                version = event.version,
                "plugin state event rejected: malformed shared state"
            );
            return Ok(PluginSharedStateOutcome::Rejected);
        }
    };
    let Some(stored) = messages.get(server_msg_id).await? else {
        return Ok(PluginSharedStateOutcome::NotFound);
    };
    if stored
        .plugin_message()
        .is_none_or(|card| card.plugin_id != event.plugin_id)
    {
        tracing::warn!(
            target: "flare_sdk.plugin",
            server_msg_id = %event.server_msg_id,
            plugin_id = %event.plugin_id,
            "plugin state event rejected: the message is not a card of this plugin"
        );
        return Ok(PluginSharedStateOutcome::Rejected);
    }
    Ok(
        match messages
            .apply_plugin_shared_state(server_msg_id, &state)
            .await?
        {
            OperationApplyResult::Applied => PluginSharedStateOutcome::Applied {
                conversation_id: stored.conversation_id.clone(),
                message_id: if stored.server_id.trim().is_empty() {
                    stored.client_msg_id.clone()
                } else {
                    stored.server_id.clone()
                },
                version: state.version,
            },
            OperationApplyResult::IgnoredStale => PluginSharedStateOutcome::Ignored,
            OperationApplyResult::NotFound => PluginSharedStateOutcome::NotFound,
        },
    )
}

//! 消息动作可用性：一条消息此刻能做什么。
//!
//! "谁能撤回""什么能编辑""撤回后还能不能复制"——这些是**产品中立的规则**，
//! 同一条消息在任何端都必须给出同一个答案，所以只能有一份，放在核心
//! （flare-im-spec 约束 4）。
//!
//! 此前这份规则散在各端：iOS 有完整判定（`MessageMenuModel`），
//! Android 的菜单则是一个**无条件全显的静态列表** —— `Pin` 与 `Unpin` 同时出现、
//! 别人的消息上也显示 `Recall`、图片上也显示 `Edit`。用户点了必然失败。
//!
//! 规则取自 iOS 那份（最完整、且已在真机上跑过）。

use crate::model::IMMessage;

/// 判定所需的上下文——消息本身给不出的那些。
#[derive(Debug, Clone, Default)]
pub struct MessageActionContext {
    pub current_user_id: String,
    pub is_connected: bool,
    /// 处于多选模式：单条动作（回复/编辑/撤回）此时应让位。
    pub multi_select_mode: bool,
    /// 本地待发（尚未拿到服务端回执）。
    pub is_pending: bool,
    /// 发送失败。
    pub is_failed: bool,
    pub is_pinned: bool,
}

/// 一条消息此刻可用的动作。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageActionAvailability {
    pub can_reply: bool,
    pub can_forward: bool,
    pub can_copy: bool,
    pub can_edit: bool,
    pub can_delete: bool,
    pub can_recall: bool,
    pub can_pin: bool,
    pub can_unpin: bool,
    pub can_react: bool,
    pub can_multi_select: bool,
    pub can_save: bool,
    pub can_resend: bool,
}

const STATUS_FAILED: i32 = 4;
const STATUS_RECALLED: i32 = 5;
const STATUS_DELETED: i32 = 6;

const TYPE_TEXT: i32 = 1;
const TYPE_RICH_TEXT: i32 = 15;
const MEDIA_TYPES: [i32; 5] = [
    2,  // IMAGE
    3,  // VIDEO
    4,  // AUDIO
    5,  // FILE
    16, // IMAGE_GROUP
];

/// 计算一条消息此刻可用的动作。
pub fn message_action_availability(
    message: &IMMessage,
    ctx: &MessageActionContext,
) -> MessageActionAvailability {
    let recalled = message.status == STATUS_RECALLED;
    let deleted = message.status == STATUS_DELETED;
    // 「活着」= 既没撤回也没删除。撤回/删除是终态，绝大多数动作到此为止。
    let active = !recalled && !deleted;

    let is_failed = ctx.is_failed || message.status == STATUS_FAILED;
    let self_sent = !ctx.current_user_id.is_empty() && message.sender_id == ctx.current_user_id;
    let editable_type = message.message_type == TYPE_TEXT || message.message_type == TYPE_RICH_TEXT;
    let media_type = MEDIA_TYPES.contains(&message.message_type);
    let single = !ctx.multi_select_mode;

    MessageActionAvailability {
        can_reply: single && active,
        can_forward: active && !ctx.is_pending,
        // 复制看的是"有没有可复制的正文"，而不是消息类型：
        // 一条没有文字的图片消息，复制什么都不会发生，不该给这个入口。
        can_copy: active
            && message
                .text_for_storage()
                .is_some_and(|text| !text.trim().is_empty()),
        can_edit: single && self_sent && active && !ctx.is_pending && !is_failed && editable_type,
        can_delete: active,
        can_recall: single && self_sent && active && !is_failed,
        can_pin: active && !ctx.is_pending && !ctx.is_pinned,
        can_unpin: active && !ctx.is_pending && ctx.is_pinned,
        can_react: active && !ctx.is_pending,
        can_multi_select: active,
        can_save: media_type && active && !ctx.is_pending,
        // 只有自己发的、确实失败的、且此刻连着的，才谈得上重发。
        can_resend: is_failed && self_sent && ctx.is_connected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sender: &str, message_type: i32, status: i32) -> IMMessage {
        let mut m = IMMessage::new(flare_proto::common::Message {
            server_id: "m1".to_string(),
            sender_id: sender.to_string(),
            message_type,
            status,
            ..Default::default()
        });
        m.message_type = message_type;
        m.status = status;
        m.sender_id = sender.to_string();
        m
    }

    fn ctx() -> MessageActionContext {
        MessageActionContext {
            current_user_id: "me".to_string(),
            is_connected: true,
            ..Default::default()
        }
    }

    #[test]
    fn pin_and_unpin_are_never_offered_together() {
        // Android 的静态菜单同时常显 Pin 与 Unpin —— 其中一个必然是错的。
        let m = message("me", TYPE_TEXT, 3);
        let unpinned = message_action_availability(&m, &ctx());
        assert!(unpinned.can_pin && !unpinned.can_unpin);

        let pinned_ctx = MessageActionContext { is_pinned: true, ..ctx() };
        let pinned = message_action_availability(&m, &pinned_ctx);
        assert!(!pinned.can_pin && pinned.can_unpin);
    }

    #[test]
    fn only_own_messages_can_be_recalled_or_edited() {
        let mine = message_action_availability(&message("me", TYPE_TEXT, 3), &ctx());
        assert!(mine.can_recall && mine.can_edit);

        let theirs = message_action_availability(&message("other", TYPE_TEXT, 3), &ctx());
        assert!(
            !theirs.can_recall && !theirs.can_edit,
            "别人的消息不能撤回/编辑——点了必然失败"
        );
    }

    #[test]
    fn only_text_like_messages_are_editable() {
        for t in [TYPE_TEXT, TYPE_RICH_TEXT] {
            assert!(message_action_availability(&message("me", t, 3), &ctx()).can_edit);
        }
        for t in MEDIA_TYPES {
            assert!(
                !message_action_availability(&message("me", t, 3), &ctx()).can_edit,
                "类型 {t} 不该显示编辑"
            );
        }
    }

    #[test]
    fn recalled_and_deleted_messages_are_terminal() {
        for status in [STATUS_RECALLED, STATUS_DELETED] {
            let a = message_action_availability(&message("me", TYPE_TEXT, status), &ctx());
            assert_eq!(
                a,
                MessageActionAvailability::default(),
                "status={status} 是终态，不该再有任何可用动作"
            );
        }
    }

    #[test]
    fn media_can_be_saved_but_text_cannot() {
        assert!(message_action_availability(&message("me", 2, 3), &ctx()).can_save);
        assert!(!message_action_availability(&message("me", TYPE_TEXT, 3), &ctx()).can_save);
    }

    #[test]
    fn resend_requires_own_failed_message_and_a_connection() {
        let failed = message("me", TYPE_TEXT, STATUS_FAILED);
        assert!(message_action_availability(&failed, &ctx()).can_resend);

        let offline = MessageActionContext { is_connected: false, ..ctx() };
        assert!(
            !message_action_availability(&failed, &offline).can_resend,
            "断线时重发只会再失败一次"
        );
        assert!(
            !message_action_availability(&message("other", TYPE_TEXT, STATUS_FAILED), &ctx())
                .can_resend,
            "别人的失败消息轮不到我重发"
        );
    }

    #[test]
    fn pending_message_blocks_actions_that_need_a_server_id() {
        let pending = MessageActionContext { is_pending: true, ..ctx() };
        let a = message_action_availability(&message("me", TYPE_TEXT, 1), &pending);
        assert!(!a.can_react && !a.can_pin && !a.can_forward && !a.can_edit);
        assert!(a.can_delete, "本地待发的消息仍应可以删掉");
    }

    #[test]
    fn multi_select_mode_hides_single_message_actions() {
        let multi = MessageActionContext { multi_select_mode: true, ..ctx() };
        let a = message_action_availability(&message("me", TYPE_TEXT, 3), &multi);
        assert!(!a.can_reply && !a.can_edit && !a.can_recall);
        assert!(a.can_forward, "多选下转发仍然成立（批量转发）");
    }
}

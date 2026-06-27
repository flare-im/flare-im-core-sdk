#[derive(Debug, Clone)]
pub enum MessageTransportAction {
    Recall {
        conversation_id: String,
        server_msg_id: String,
    },
    Edit {
        conversation_id: String,
        server_msg_id: String,
        new_content: Vec<u8>,
        edit_version: i32,
        reason: String,
        show_edited_mark: bool,
    },
    Delete {
        conversation_id: String,
        server_msg_id: String,
        delete_type: i32,
        scope: i32,
        reason: Option<String>,
        notify_others: bool,
        target_user_id: Option<String>,
    },
    Reaction {
        conversation_id: String,
        server_msg_id: String,
        user_id: String,
        emoji: String,
        action: i32,
    },
    Pin {
        conversation_id: String,
        server_msg_id: String,
        pinned_by: String,
        scope: i32,
    },
    Unpin {
        conversation_id: String,
        server_msg_id: String,
        unpinned_by: String,
        scope: i32,
    },
    Mark {
        conversation_id: String,
        server_msg_id: String,
        user_id: String,
        mark_type: i32,
        color: String,
    },
    Unmark {
        conversation_id: String,
        server_msg_id: String,
        user_id: String,
        mark_type: i32,
    },
}

impl MessageTransportAction {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::Recall {
                conversation_id, ..
            }
            | Self::Edit {
                conversation_id, ..
            }
            | Self::Delete {
                conversation_id, ..
            }
            | Self::Reaction {
                conversation_id, ..
            }
            | Self::Pin {
                conversation_id, ..
            }
            | Self::Unpin {
                conversation_id, ..
            }
            | Self::Mark {
                conversation_id, ..
            }
            | Self::Unmark {
                conversation_id, ..
            } => conversation_id,
        }
    }

    pub fn server_msg_id(&self) -> Option<&str> {
        match self {
            Self::Recall { server_msg_id, .. }
            | Self::Edit { server_msg_id, .. }
            | Self::Delete { server_msg_id, .. }
            | Self::Reaction { server_msg_id, .. }
            | Self::Pin { server_msg_id, .. }
            | Self::Unpin { server_msg_id, .. }
            | Self::Mark { server_msg_id, .. }
            | Self::Unmark { server_msg_id, .. } => Some(server_msg_id),
        }
    }
}

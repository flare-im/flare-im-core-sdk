// ── 核心消息类型 ────────────────────────────────────────
pub use flare_proto::common::Message;
pub use flare_proto::common::MessageStatus;
pub use flare_proto::common::MessageType;
pub use flare_proto::common::MessageSource;
pub use flare_proto::common::ConversationType;
pub use flare_proto::common::OfflinePushInfo;
pub use flare_proto::common::MessageTimeline;
pub use flare_proto::common::MessageReadRecord;
pub use flare_proto::common::SendAck;
pub use flare_proto::common::OperationResponse;

// ── 消息内容 ────────────────────────────────────────────
pub use flare_proto::common::MessageContent;
pub use flare_proto::common::message_content;
pub use flare_proto::common::MessagePreview;

// ── 内容类型 (1-15 基础) ────────────────────────────────
pub use flare_proto::common::TextContent;
pub use flare_proto::common::Mention;
pub use flare_proto::common::MentionType;
pub use flare_proto::common::ImageContent;
pub use flare_proto::common::ImageInfo;
pub use flare_proto::common::VideoContent;
pub use flare_proto::common::VideoInfo;
pub use flare_proto::common::AudioContent;
pub use flare_proto::common::AudioInfo;
pub use flare_proto::common::FileContent;
pub use flare_proto::common::LocationContent;
pub use flare_proto::common::CardContent;
pub use flare_proto::common::StickerContent;
pub use flare_proto::common::EmojiContent;
pub use flare_proto::common::GifContent;
pub use flare_proto::common::QuoteContent;
pub use flare_proto::common::LinkCardContent;
pub use flare_proto::common::ForwardContent;
pub use flare_proto::common::ThreadContent;
pub use flare_proto::common::MiniProgramContent;

// ── 内容类型 (30-32 富媒体) ─────────────────────────────
pub use flare_proto::common::RichTextContent;
pub use flare_proto::common::MarkdownContent;
pub use flare_proto::common::ImageGroupContent;

// ── 内容类型 (60-61 系统/通知) ──────────────────────────
pub use flare_proto::common::SystemContent;
pub use flare_proto::common::NotificationContent;

// ── 内容类型 (80-83 平台推荐业务) ───────────────────────
pub use flare_proto::common::VoteContent;
pub use flare_proto::common::TaskContent;
pub use flare_proto::common::ScheduleContent;
pub use flare_proto::common::AnnouncementContent;

// ── 内容类型 (100 自定义) ───────────────────────────────
pub use flare_proto::common::CustomContent;

// ── 内容类型 (101-115 平台能力) ─────────────────────────
pub use flare_proto::common::PlaceholderContent;

// ── 事件操作类型 ────────────────────────────────────────
pub use flare_proto::common::MessageRecallEvent;
pub use flare_proto::common::MessageEditEvent;
pub use flare_proto::common::MessageDeleteEvent;
pub use flare_proto::common::ReadReceiptEvent;
pub use flare_proto::common::TypingEvent;
pub use flare_proto::common::ReactionEvent;
pub use flare_proto::common::PinEvent;
pub use flare_proto::common::UnpinEvent;
pub use flare_proto::common::MarkEvent;
pub use flare_proto::common::UnmarkEvent;
pub use flare_proto::common::CallSignalEvent;
pub use flare_proto::common::CustomEvent;
pub use flare_proto::common::ConversationUpdateEvent;
pub use flare_proto::common::ConversationDeleteEvent;
pub use flare_proto::common::PresenceEvent;

// ── 枚举 ────────────────────────────────────────────────
pub use flare_proto::common::MarkType;
pub use flare_proto::common::ReactionAction;
pub use flare_proto::common::DeleteType;
pub use flare_proto::common::DeleteScope;
pub use flare_proto::common::DiffusionStrategy;

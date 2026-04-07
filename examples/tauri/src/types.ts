/**
 * SDK 类型定义
 *
 * 与 Rust SDK 的 IMMessage、Elem、Conversation 的 serde 序列化结果一致（camelCase）。
 * 事件 im://message 与 sdk_get_messages 均返回 IMMessage 结构。
 */

// ============================================================================
// 消息内容 Elem（与 message_elem.rs Elem 对应，serde tag = "contentType", rename_all = "camelCase"）
// ============================================================================

export interface MentionElem {
  type: number;
  userId: string;
  userIds: string[];
  roleId: string;
  start: number;
  length: number;
}

export interface ImageInfoElem {
  uuid: string;
  /** 媒体存储稳定 id（proto ImageInfo.image_id），展示时走 GetFileUrl */
  imageId?: string;
  url: string;
  mimeType: string;
  size: number;
  width: number;
  height: number;
}

export interface VideoInfoElem {
  uuid: string;
  url: string;
  mimeType: string;
  size: number;
  durationMs: number;
  width: number;
  height: number;
}

export interface AudioInfoElem {
  uuid: string;
  url: string;
  mimeType: string;
  size: number;
  durationMs: number;
}

export interface MessagePreviewElem {
  messageId: string;
  senderId: string;
  type: number;
  text: string;
  /** 毫秒时间戳 */
  time: number;
}

/** 解码后的消息内容联合类型（根据 contentType 取对应结构） */
export type ContentElem =
  | { contentType: 'text'; text: { text: string; mentions: MentionElem[] } }
  | {
      contentType: 'image';
      image: { source?: ImageInfoElem; thumbnail?: ImageInfoElem; description: string };
    }
  | { contentType: 'video'; video: { videoId: string; source?: VideoInfoElem; cover?: ImageInfoElem; description: string } }
  | { contentType: 'audio'; audio: { audioId: string; source?: AudioInfoElem; description: string } }
  | { contentType: 'file'; file: { fileId: string; fileName: string; mimeType: string; fileSize: number; url: string; description: string } }
  | { contentType: 'location'; location: { longitude: number; latitude: number; address: string; description: string; poiId: string } }
  | { contentType: 'card'; card: { userId: string; nickname: string; avatarUrl: string; description: string; extra?: Record<string, string> } }
  | { contentType: 'sticker'; sticker: { stickerId: string; url: string; width: number; height: number; extra?: Record<string, string> } }
  | { contentType: 'emoji'; emoji: { emoji: string; description: string; extra?: Record<string, string> } }
  | { contentType: 'gif'; gif: { gifId: string; url: string; thumbnail?: ImageInfoElem; durationMs: number; width: number; height: number } }
  | { contentType: 'quote'; quote: { quotedMessageId: string; quotedSenderId: string; quotedTextPreview: string; quotedContent?: ContentElem; currentContent?: ContentElem } }
  | { contentType: 'linkCard'; linkCard: { url: string; title: string; description: string; thumbnailUrl: string; siteName: string } }
  | { contentType: 'forward'; forward: { messageIds: string[]; forwardReason: string; forwardedPreviews: MessagePreviewElem[] } }
  | { contentType: 'thread'; thread: { threadId: string; threadTitle: string; rootContent?: ContentElem; metadata?: Record<string, string> } }
  | { contentType: 'miniProgram'; miniProgram: { appId: string; title: string; pagePath: string; thumbnailUrl: string; extra?: Record<string, string> } }
  | { contentType: 'richText'; richText: { content: string; format: string; mentions: MentionElem[]; metadata?: Record<string, string> } }
  | { contentType: 'markdown'; markdown: { text: string; mentions: MentionElem[]; metadata?: Record<string, string> } }
  | { contentType: 'imageGroup'; imageGroup: { images: ImageInfoElem[]; description: string; metadata?: Record<string, string> } }
  | { contentType: 'system'; system: { eventKind: string; body: string; data?: Record<string, string> } }
  | { contentType: 'notification'; notification: { title: string; body: string; notificationType: string; data?: Record<string, string>; targetUserIds?: string[] } }
  | { contentType: 'vote'; vote: { voteId: string; title: string; options: string[]; metadata?: Record<string, string> } }
  | { contentType: 'task'; task: { taskId: string; title: string; status: string; metadata?: Record<string, string> } }
  | { contentType: 'schedule'; schedule: { scheduleId: string; title: string; startTime: number; endTime: number; metadata?: Record<string, string> } }
  | { contentType: 'announcement'; announcement: { title: string; body: string; pinned: boolean; metadata?: Record<string, string> } }
  | { contentType: 'custom'; custom: { type: string; description: string; metadata?: Record<string, string> } }
  | { contentType: 'placeholder'; placeholder: { reason: string; fallbackText: string; metadata?: Record<string, string> } };

// ============================================================================
// IMMessage（与 message.rs IMMessage 序列化一致，camelCase；content_bytes / local_state / offline_push_info 已 skip）
// ============================================================================

/**
 * Tauri 绑定返回的消息形状（与 Rust IMMessage 序列化一致）。
 * content 为解码后的 Elem（Option<Elem>），序列化后即本结构中的 content。
 */
export interface IMMessage {
  serverId: string;
  clientMsgId: string;
  conversationId: string;
  conversationType: number;
  channelId?: string | null;
  senderId: string;
  receiverId?: string | null;
  source: number;
  seq: number;
  /** 服务端时间（毫秒） */
  timestamp: number;
  /** 客户端时间（毫秒） */
  clientTimestamp: number;
  messageType: number;
  /** 解码后的消息体（Elem），按 contentType 取对应结构 */
  content: ContentElem | null;
  senderName: string;
  senderAvatar: string;
  senderDisplayName: string;
  replyTo?: string | null;
  quotePreview?: string | null;
  status: number;
  isRead: boolean;
  isRecalled: boolean;
  isEdited: boolean;
  mentionUsers: string[];
  mentionAll: boolean;
  extra: Record<string, string>;
  extensions: Record<string, number[]>;
  version: number;
  updatedAt: number;
}

/** 表情反应（扩展字段，可由事件更新后挂载到消息对象） */
export interface ReactionEntry {
  emoji: string;
  userIds: string[];
  count: number;
  lastUpdated?: string;
  createdAt?: string;
}

export interface LocalUploadState {
  mediaKind: 'image' | 'video' | 'audio' | 'file';
  filePath: string;
  fileName: string;
  phase: 'Preparing' | 'Uploading' | 'Completing' | 'Finished' | 'Failed' | string;
  progressPercent?: number | null;
  uploadedBytes?: number;
  totalBytes?: number;
}

/**
 * 消息类型（UI 层统一使用）
 * 与 IMMessage 一致；可含扩展字段 attributes、reactions 等（由 UI/事件挂载）。
 */
export type Message = IMMessage & {
  attributes?: Record<string, string>;
  reactions?: ReactionEntry[];
  localUpload?: LocalUploadState;
};

// ============================================================================
// 会话 Conversation（与 conversation.rs 序列化一致，camelCase）
// ============================================================================

/** 会话类型（与 Rust ConversationType 对应；含服务端/历史兼容别名） */
export type ConversationTypeStr =
  | 'unspecified'
  | 'single'
  | 'private'
  | 'Single'
  | 'group'
  | 'channel';

/**
 * 会话列表项（与 Rust Conversation 一致，camelCase）
 * lastMessage.time、updatedAt、createdAt 为毫秒时间戳（number）。
 * 兼容后端返回的 snake_case 字段（如 last_message、updated_at）。
 */
export interface Conversation {
  conversationId: string;
  conversationType: ConversationTypeStr;
  businessType?: string;
  ownerId?: string | null;
  membersCount?: number;
  displayName: string;
  avatarUrl: string;
  remark?: string | null;
  description?: string | null;
  lastMessageId?: string | null;
  lastSenderId?: string | null;
  lastMessageAt?: number | null;
  lastMessagePreview?: string | null;
  lastMessage?: MessagePreviewElem | null;
  lastSenderNickname?: string;
  lastSenderAvatarUrl?: string;
  unreadCount: number;
  lastReadSeq: number;
  maxSeq: number;
  isPinned: boolean;
  isMuted: boolean;
  isArchived?: boolean;
  version?: number;
  /** 毫秒时间戳 */
  updatedAt: number;
  /** 毫秒时间戳 */
  createdAt: number;
  updatedAtTs?: number | null;
  peerId?: string | null;
  ext?: Record<string, string>;
  draft?: string | null;
  mentionCount?: number;
  mentionMe?: boolean;
  badge?: string | null;
  role?: string | null;
  // 兼容 snake_case 序列化
  display_name?: string;
  avatar_url?: string;
  last_message?: MessagePreviewElem | null;
  unread_count?: number;
  updated_at?: number;
  created_at?: number;
  peer_id?: string | null;
  conversation_type?: string;
  participants?: Array<{ user_id: string; nickname?: string }>;
}

// ============================================================================
// 兼容与扩展（UI 用枚举、旧类型别名等）
// ============================================================================

export enum MessageState {
  Created = 'Created',
  Sent = 'Sent',
  Delivered = 'Delivered',
  Read = 'Read',
  Failed = 'Failed',
  Recalled = 'Recalled',
}

export enum MessageSource {
  User = 'User',
  System = 'System',
  Bot = 'Bot',
  Admin = 'Admin',
}

export enum MessageType {
  Text = 'Text',
  Image = 'Image',
  Video = 'Video',
  Audio = 'Audio',
  File = 'File',
  Location = 'Location',
  Card = 'Card',
  Custom = 'Custom',
  Notification = 'Notification',
}

/** 与 bindings `sdk_send` / `im://send_ack` 一致（camelCase） */
export interface SendAckPayload {
  clientMsgId: string;
  serverMsgId: string;
  seq: number;
  conversationId: string;
  success: boolean;
  errorCode: number;
  errorMessage: string;
}

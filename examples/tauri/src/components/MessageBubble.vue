<template>
  <div class="message-bubble-wrapper" :class="{ 'message-self': isSelf }">
    <!-- 头像 - 接收方显示 -->
    <Avatar
      v-if="!isSelf"
      :user-id="senderId"
      :display-name="senderId"
      :avatar-url="avatarUrl"
      :size="40"
      class="message-avatar"
    />
    
    <div class="message-content-wrapper">
      <!-- 发送者名称 - 群组聊天时显示 -->
      <div v-if="showSenderName && !isSelf" class="message-sender">
        {{ senderDisplayName }}
      </div>
      
      <!-- 气泡与反应同一行；飞书风格：悬停时气泡右上角显示浮动操作条 -->
      <div class="bubble-row">
        <div class="bubble-slot feishu-bubble-slot">
          <!-- 飞书风格浮动操作条：自己发的消息时靠气泡左侧，避免贴边被遮挡 -->
          <div class="feishu-floating-bar" :class="{ 'feishu-floating-bar-self': isSelf }">
            <a-dropdown trigger="click" @select="handleReactionSelect">
              <a-button type="text" class="feishu-bar-btn" title="反应">
                <icon-thumb-up />
              </a-button>
              <template #content>
                <a-doption value="👍">👍 赞</a-doption>
                <a-doption value="❤️">❤️ 喜欢</a-doption>
                <a-doption value="😂">😂 大笑</a-doption>
                <a-doption value="😮">😮 惊讶</a-doption>
                <a-doption value="😢">😢 悲伤</a-doption>
                <a-doption value="😡">😡 愤怒</a-doption>
                <a-doption value="👏">👏 鼓掌</a-doption>
                <a-doption value="🎉">🎉 庆祝</a-doption>
              </template>
            </a-dropdown>
            <a-button type="text" class="feishu-bar-btn" title="回复" @click="onReply">
              <icon-message />
            </a-button>
            <MessageMenu
              trigger="click"
              :message="message"
              :current-user-id="currentUserId"
              @reply="(id: string) => $emit('reply', id)"
              @forward="(id: string) => $emit('forward', id)"
              @edit="(msg: Message) => $emit('startEdit', msg)"
              @recall="(id: string) => $emit('recall', id)"
              @pin="(id: string) => $emit('pin', id)"
              @unpin="(id: string) => $emit('unpin', id)"
              @mark="(id: string, type: number, color?: string) => $emit('mark', id, type, color)"
              @unmark="(id: string, type: number) => $emit('unmark', id, type)"
              @delete="(id: string, canAll: boolean) => $emit('delete', id, canAll)"
            >
              <a-button type="text" class="feishu-bar-btn" title="更多">
                <icon-more />
              </a-button>
            </MessageMenu>
          </div>
          <MessageMenu
            trigger="contextmenu"
            :message="message"
            :current-user-id="currentUserId"
          @reply="(id: string) => $emit('reply', id)"
          @forward="(id: string) => $emit('forward', id)"
          @edit="(msg: Message) => $emit('startEdit', msg)"
          @recall="(id: string) => $emit('recall', id)"
          @pin="(id: string) => $emit('pin', id)"
          @unpin="(id: string) => $emit('unpin', id)"
          @mark="(id: string, type: number, color?: string) => $emit('mark', id, type, color)"
          @unmark="(id: string, type: number) => $emit('unmark', id, type)"
          @delete="(id: string, canAll: boolean) => $emit('delete', id, canAll)"
        >
          <div
            class="message-bubble"
            :class="{
              'message-bubble-self': isSelf,
              'message-bubble-editing-target': isEditing,
            }"
          >
            <!-- 已撤回消息 -->
            <div v-if="isRecalled" class="message-recalled">
              <div class="recalled-content">
                <span class="recalled-text">{{ isSelf ? '你' : senderDisplayName }}撤回了一条消息</span>
                <span v-if="recallReason" class="recall-reason">（{{ recallReason }}）</span>
              </div>
            </div>
            
            <!-- 编辑在底部输入框完成；此处仅高亮当前正在编辑的那条消息 -->
            <div v-else class="message-body-row" :class="{ 'message-body-row-self': isSelf }">
              <!-- 发送方：正文与时间同一 flex 行（可换行），时间不跟对号绑在一起 -->
              <div v-if="isSelf" class="message-self-main">
                <div class="message-body">
                  <div v-if="quoteContent && displayContent?.contentType !== 'quote'" class="message-quote">
                    <div class="quote-header">
                      <span class="quote-sender">回复 {{ quoteSender }}:</span>
                      <span class="quote-content">{{ quoteContent }}</span>
                    </div>
                  </div>
                  <ContentView
                    v-if="displayContent"
                    :content="displayContent"
                    :is-self="isSelf"
                    :message-id="contentViewMessageId"
                  />
                  <div v-else class="message-text message-empty">
                    [消息内容缺失]
                    <div v-if="debugInfo" class="debug-info" style="font-size: 10px; color: #999; margin-top: 4px;">
                      {{ debugInfo }}
                    </div>
                  </div>
                </div>
                <span
                  v-if="!isRecalled"
                  class="bubble-meta-trailing-self"
                >
                  <span v-if="isPinned" class="pinned-icon" title="已置顶">📌</span>
                  <span v-if="markType === 1" class="mark-icon-only" title="重要">
                    <icon-exclamation-circle :style="{ color: '#f53f3f' }" />
                  </span>
                  <span v-else-if="markType === 2" class="mark-icon-only" title="待办">
                    <icon-clock-circle :style="{ color: '#ff7d00' }" />
                  </span>
                  <span v-else-if="markType === 3" class="mark-icon-only" title="已处理">
                    <icon-check-circle-fill :style="{ color: '#00b42a' }" />
                  </span>
                  <span class="bubble-time bubble-time-inline-self">{{ bubbleTimeText }}</span>
                  <MessageStatus
                    v-if="!isRecalled && showMessageStatus"
                    :status="messageStateToNumber"
                  />
                </span>
              </div>
              <!-- 接收方：仅正文块（时间与页脚一起） -->
              <div v-else class="message-body">
                <div v-if="quoteContent && displayContent?.contentType !== 'quote'" class="message-quote">
                  <div class="quote-header">
                    <span class="quote-sender">回复 {{ quoteSender }}:</span>
                    <span class="quote-content">{{ quoteContent }}</span>
                  </div>
                </div>
                <ContentView
                  v-if="displayContent"
                  :content="displayContent"
                  :is-self="isSelf"
                  :message-id="contentViewMessageId"
                />
                <div v-else class="message-text message-empty">
                  [消息内容缺失]
                  <div v-if="debugInfo" class="debug-info" style="font-size: 10px; color: #999; margin-top: 4px;">
                    {{ debugInfo }}
                  </div>
                </div>
              </div>
              <div v-if="!isSelf" class="message-footer bubble-footer">
                <div class="bubble-footer-inner">
                  <span v-if="isPinned" class="pinned-icon" title="已置顶">📌</span>
                  <span v-if="markType === 1" class="mark-icon-only" title="重要">
                    <icon-exclamation-circle :style="{ color: '#f53f3f' }" />
                  </span>
                  <span v-else-if="markType === 2" class="mark-icon-only" title="待办">
                    <icon-clock-circle :style="{ color: '#ff7d00' }" />
                  </span>
                  <span v-else-if="markType === 3" class="mark-icon-only" title="已处理">
                    <icon-check-circle-fill :style="{ color: '#00b42a' }" />
                  </span>
                  <span v-if="!isRecalled" class="bubble-time">{{ bubbleTimeText }}</span>
                </div>
              </div>
            </div>

            <div v-if="reactions.length > 0" class="message-reactions">
              <a-tooltip
                v-for="reaction in reactions"
                :key="reaction.emoji"
                :content="reaction.tooltip"
                position="top"
              >
                <a-tag
                  size="small"
                  class="reaction-tag"
                  :class="{ 
                    'reaction-active': reaction.isActive,
                    'reaction-own': reaction.isActive
                  }"
                  @click.stop="handleReactionClick(reaction.emoji)"
                >
                  <span class="reaction-emoji">{{ reaction.emoji }}</span>
                  <span class="reaction-users">{{ reaction.displayUsers }}</span>
                  <span class="reaction-count" v-if="reaction.count > 1">{{ reaction.count }}</span>
                </a-tag>
              </a-tooltip>
            </div>

            <div v-if="localUpload" class="message-upload-state">
              <div class="message-upload-meta">
                <span class="message-upload-label">{{ uploadLabel }}</span>
                <span v-if="uploadPercentText" class="message-upload-percent">{{ uploadPercentText }}</span>
              </div>
              <div class="message-upload-track">
                <div class="message-upload-fill" :style="{ width: uploadPercentWidth }" />
              </div>
            </div>
            
            <button
              v-if="replyCount > 0"
              class="reply-detail-link"
              type="button"
              @click.stop="emit('openReplyDetail', messageId())"
            >
              💬 {{ replyCount }}条回复
            </button>
            
            <div v-if="isEdited && !isEditing" class="message-edited-mark">
              <span class="edited-text">已编辑</span>
            </div>
            <div v-else-if="isEditing" class="message-editing-hint">
              <span class="editing-hint-text">在下方输入框修改并保存</span>
            </div>
          </div>
        </MessageMenu>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import Avatar from './Avatar.vue';
import MessageStatus from './MessageStatus.vue';
import MessageMenu from './MessageMenu.vue';
import ContentView from './MessagesView/ContentView.vue';
import {
  IconThumbUp,
  IconMessage,
  IconMore,
  IconExclamationCircle,
  IconClockCircle,
  IconCheckCircleFill,
} from '@arco-design/web-vue/es/icon';
import type { Message, ContentElem, IMMessage, LocalUploadState } from '../types';
import { setupCodeCopyListeners } from '../utils/markdown';
import { parseReactions, createReactionTooltip } from '../utils/reactions';
import { getMessageSenderId, isMessageFromSelf, getMessageContent, asRecord } from '../utils/message';

interface Props {
  message: Message;
  currentUserId: string | null;
  /** 当前是否在底部输入框编辑本条（仅高亮，不在气泡内编辑） */
  isEditing?: boolean;
  showSenderName?: boolean;
  replyCount?: number;
}

const props = withDefaults(defineProps<Props>(), {
  isEditing: false,
  showSenderName: false,
  replyCount: 0,
});

const emit = defineEmits<{
  (e: 'resend', messageId: string, text: string): void;
  (e: 'addReaction', messageId: string, emoji: string): void;
  (e: 'removeReaction', messageId: string, emoji: string): void;
  (e: 'startEdit', message: Message): void;
  (e: 'reply', messageId: string): void;
  (e: 'forward', messageId: string): void;
  (e: 'recall', messageId: string): void;
  (e: 'pin', messageId: string): void;
  (e: 'unpin', messageId: string): void;
  (e: 'mark', messageId: string, markType: number, color?: string): void;
  (e: 'unmark', messageId: string, markType: number): void;
  (e: 'delete', messageId: string, canDeleteForEveryone: boolean): void;
  (e: 'openReplyDetail', messageId: string): void;
}>();

// 是否为当前用户发送的消息（兼容 Tauri camelCase：senderId）
const isSelf = computed(() => {
  return isMessageFromSelf(asRecord(props.message), props.currentUserId);
});

// 发送者 ID
const senderId = computed(() => getMessageSenderId(asRecord(props.message)));

// 发送者展示名（IMMessage.senderDisplayName || senderName || senderId）
const senderDisplayName = computed(() => {
  const m = props.message as IMMessage;
  return m.senderDisplayName || m.senderName || senderId.value || '';
});

// 头像 URL（IMMessage.senderAvatar 或 extra）
const avatarUrl = computed(() => {
  const m = props.message as IMMessage;
  return m.senderAvatar || (m.extra?.avatar_url ?? '');
});

// 解码后的 content（IMMessage.content）
const messageContent = computed(() => getMessageContent(asRecord(props.message)));

function quoteCurrentContent(content: ContentElem | null | undefined): ContentElem | null {
  if (!content || content.contentType !== 'quote') return null;
  const q = (content.quote ?? {}) as Record<string, unknown>;
  const direct = q.currentContent as ContentElem | undefined;
  if (direct && typeof direct === 'object') return direct;
  const snake = q.current_content as ContentElem | undefined;
  if (snake && typeof snake === 'object') return snake;
  const legacy = q.replyContent as ContentElem | undefined;
  if (legacy && typeof legacy === 'object') return legacy;
  const legacySnake = q.reply_content as ContentElem | undefined;
  if (legacySnake && typeof legacySnake === 'object') return legacySnake;
  return null;
}

// 展示用 content：无 content 但有 extra.content_text 时构造为文本类型，确保文本能显示
const displayContent = computed(() => {
  const content = messageContent.value;
  const m = props.message as IMMessage & Record<string, unknown>;
  const extraText = m.extra?.content_text;
  if (content?.contentType === 'quote') {
    const current = quoteCurrentContent(content);
    if (current) {
      return current;
    }
    if (typeof extraText === 'string' && extraText.trim()) {
      return { contentType: 'text' as const, text: { text: extraText.trim(), mentions: [] } };
    }
    return null;
  }
  if (content != null) return content;
  if (typeof extraText === 'string' && extraText.trim()) {
    return { contentType: 'text' as const, text: { text: extraText.trim(), mentions: [] } };
  }
  return null;
});

// 是否已撤回
const isRecalled = computed(() => {
  const m = asRecord(props.message);
  return !!(m.isRecalled ?? m.is_recalled);
});
const recallReason = computed(() => asRecord(props.message).recall_reason as string | undefined);

function pickFirstNonEmpty(values: Array<unknown>): string {
  for (const v of values) {
    if (typeof v === 'string' && v.trim()) return v.trim();
  }
  return '';
}

function quotedContentPreview(content: unknown): string {
  if (!content || typeof content !== 'object') return '';
  const c = content as Record<string, unknown>;
  const ct = String(c.contentType ?? c.content_type ?? '');
  if (ct === 'text') {
    const t = c.text;
    if (typeof t === 'string' && t.trim()) return t.trim();
    if (t && typeof t === 'object' && 'text' in t) {
      const nested = (t as Record<string, unknown>).text;
      if (typeof nested === 'string' && nested.trim()) return nested.trim();
    }
  }
  if (ct === 'markdown') {
    const md = c.markdown;
    if (md && typeof md === 'object' && 'text' in md) {
      const mt = (md as Record<string, unknown>).text;
      if (typeof mt === 'string' && mt.trim()) return mt.trim();
    }
  }
  return '';
}

// 引用内容（协议字段）
const quoteContent = computed(() => {
  const r = asRecord(props.message);
  const content = messageContent.value;
  const fromQuote =
    content?.contentType === 'quote'
      ? pickFirstNonEmpty([
          content.quote?.quotedTextPreview,
          quotedContentPreview(content.quote?.quotedContent),
        ])
      : '';
  const quoteText = pickFirstNonEmpty([
    (props.message as IMMessage).quotePreview,
    r.quote_preview,
    fromQuote,
  ]);
  if (!quoteText) return '';
  return String(quoteText).replace(/\s+/g, ' ').trim();
});

// 引用发送者（协议字段）
const quoteSender = computed(() => {
  const content = messageContent.value;
  const sender = pickFirstNonEmpty([
    content?.contentType === 'quote' ? content.quote?.quotedSenderId : '',
  ]);
  return sender || '某人';
});

// 是否已编辑（IMMessage.isEdited）
const isEdited = computed(() => {
  const m = asRecord(props.message);
  return !!(m.isEdited ?? (m.edit_history && Array.isArray(m.edit_history) && (m.edit_history as unknown[]).length > 0));
});

// 调试信息（无 content 时展示）
const debugInfo = computed(() => {
  if (messageContent.value) return null;
  const m = props.message as IMMessage & Record<string, unknown>;
  const parts: string[] = [];
  if (m.messageType != null) parts.push(`messageType: ${m.messageType}`);
  if (!m.extra?.content_text) parts.push('no content');
  return parts.length > 0 ? parts.join(', ') : null;
});

// 表情反应（扩展字段，IMMessage 暂无 reactions，由事件更新后可能存在于对象上）
const reactions = computed(() => {
  const m = asRecord(props.message);
  const r = m.reactions;
  if (!r || !Array.isArray(r)) return [];
  return parseReactions(r, props.currentUserId || '').map(reaction => ({
    ...reaction,
    tooltip: createReactionTooltip(reaction, props.currentUserId || ''),
    displayUsers: reaction.userIds.slice(0, 1).join('、') || (reaction.isActive ? '你' : ''),
  }));
});

const localUpload = computed(() => {
  const upload = (props.message as Message).localUpload;
  return upload ?? null;
});

const uploadPercentValue = computed(() => {
  const raw = localUpload.value?.progressPercent;
  const n = typeof raw === "number" ? raw : Number(raw ?? 0);
  if (!Number.isFinite(n) || Number.isNaN(n)) return 0;
  return Math.min(100, Math.max(0, Math.floor(n)));
});

const uploadPercentText = computed(() => {
  if (!localUpload.value) return "";
  return `${uploadPercentValue.value}%`;
});

const uploadPercentWidth = computed(() => `${uploadPercentValue.value}%`);

function uploadPhaseText(upload: LocalUploadState): string {
  switch (upload.phase) {
    case "Preparing":
      return "正在准备";
    case "Uploading":
      return "正在上传";
    case "Completing":
      return "正在完成";
    case "Finished":
      return "上传完成";
    case "Failed":
      return "上传失败";
    default:
      return String(upload.phase || "正在处理");
  }
}

const uploadLabel = computed(() => {
  const upload = localUpload.value;
  if (!upload) return "";
  const fileName = String(upload.fileName ?? "").trim();
  return fileName ? `${uploadPhaseText(upload)} ${fileName}` : uploadPhaseText(upload);
});

// 是否置顶（IMMessage.extra）
const isPinned = computed(() => {
  const m = props.message as IMMessage & Record<string, unknown>;
  return m.extra?.pinned === 'true' || (m.attributes as Record<string, string> | undefined)?.pinned === 'true';
});

// 标记类型和颜色（extra）
const markType = computed(() => {
  const m = props.message as IMMessage & Record<string, unknown>;
  const markTypeStr = m.extra?.mark_type;
  if (markTypeStr) {
    const type = parseInt(markTypeStr, 10);
    if (!isNaN(type)) return type;
  }
  const attrs = (m.attributes as Record<string, string> | undefined) || {};
  if (attrs['mark:important'] === 'true') return 0;
  if (attrs['mark:todo'] === 'true') return 1;
  if (attrs['mark:done'] === 'true') return 2;
  return null;
});

// 消息状态（IMMessage.status，与 proto MessageStatus 一致）
const showMessageStatus = computed(() => {
  if (!isSelf.value) return false;
  const msg = props.message as IMMessage & { state?: string };
  return typeof msg.status === 'number' && msg.status >= 1 && msg.status <= 6;
});
const messageStateToNumber = computed(() => {
  const msg = props.message as IMMessage & { state?: string };
  if (typeof msg.status === 'number' && msg.status >= 1 && msg.status <= 6) return msg.status;
  const state = msg.state;
  switch (state) {
    case 'Created': return 1;
    case 'Sent': return 2;
    case 'Delivered': return 3;
    case 'Read': return 4;
    case 'Failed': return 5;
    case 'Recalled': return 6;
    default: return 0;
  }
});

/** 气泡内显示的时间（HH:mm） */
const bubbleTimeText = computed(() => {
  const ts = asRecord(props.message).timestamp ?? asRecord(props.message).clientTimestamp;
  if (ts == null) return '';
  const ms = typeof ts === 'number' ? ts : new Date(String(ts)).getTime();
  if (!Number.isFinite(ms)) return '';
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
});

function messageId(): string {
  const m = asRecord(props.message);
  return String(m.clientMsgId ?? m.client_msg_id ?? m.serverId ?? m.server_id ?? '');
}

const contentViewMessageId = computed(() => messageId());

function onReply() {
  emit('reply', messageId());
}

// 处理反应选择（从下拉菜单选择）
function handleReactionSelect(emoji: string) {
  const msg = asRecord(props.message);
  const clientMsgId = String(msg.clientMsgId ?? msg.client_msg_id ?? msg.serverId ?? msg.server_id ?? '');
  console.log("[MessageBubble] 选择反应:", { client_msg_id: clientMsgId, server_id: msg.serverId ?? msg.server_id, emoji });
  // 仅当“当前用户已添加”时才执行移除；存在但非本人添加时应执行添加
  const active = isReactionActive(emoji);
  if (active) {
    console.log("[MessageBubble] 移除已存在的反应（当前用户）");
    emit('removeReaction', clientMsgId, emoji);
  } else {
    console.log("[MessageBubble] 添加新反应");
    emit('addReaction', clientMsgId, emoji);
  }
}

// 处理反应标签点击（点击已存在的反应标签）
function handleReactionClick(emoji: string) {
  const msg = asRecord(props.message);
  const clientMsgId = String(msg.clientMsgId ?? msg.client_msg_id ?? msg.serverId ?? msg.server_id ?? '');
  console.log("[MessageBubble] 点击反应标签:", { client_msg_id: clientMsgId, server_id: msg.serverId ?? msg.server_id, emoji });
  
  // 检查当前用户是否已添加该反应
  if (isReactionActive(emoji)) {
    // 如果已添加，移除反应
    console.log("[MessageBubble] 移除反应（当前用户已添加）");
    emit('removeReaction', clientMsgId, emoji);
  } else {
    // 如果未添加，添加反应
    console.log("[MessageBubble] 添加反应（点击现有反应标签）");
    emit('addReaction', clientMsgId, emoji);
  }
}

// 检查当前用户是否已添加该反应
function isReactionActive(emoji: string): boolean {
  if (!props.currentUserId) return false;
  const reaction = reactions.value.find(r => r.emoji === emoji);
  if (!reaction || !reaction.userIds) return false;
  // 检查当前用户 ID 是否在 userIds 列表中
  return reaction.userIds.includes(props.currentUserId);
}

// 组件挂载时设置代码复制监听器
onMounted(() => {
  setupCodeCopyListeners();
});
</script>

<style scoped>
.message-bubble-wrapper {
  display: flex;
  margin: 4px 0;
  align-items: flex-start;
  gap: var(--spacing-md, 12px);
  max-width: var(--bubble-max-width, 78%);
}

.message-self {
  flex-direction: row-reverse;
}

.message-avatar {
  flex-shrink: 0;
  margin-top: 4px;
}

.message-content-wrapper {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1 1 auto;
  max-width: 100%;
  overflow: visible;
}

/* 自己发的消息：与右边缘留出间距，避免贴滚动条 */
.message-self .message-content-wrapper {
  margin-right: var(--feishu-bubble-edge-gap, 12px);
}

/* 发送者名称：参照图在气泡左上方，略粗或偏红/暗橙 */
.message-sender {
  font-size: var(--font-size-xs, 12px);
  font-weight: 600;
  color: var(--bubble-sender-name, #c45c2a);
  margin-bottom: 4px;
  margin-left: var(--spacing-sm, 8px);
}

.message-self .message-sender {
  display: none;
}

/* 气泡与反应同一行（飞书风格）：反应紧贴气泡右侧、底对齐；不裁切浮动菜单 */
.bubble-row {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  flex-wrap: nowrap;
  overflow: visible;
}

/* 气泡槽：不拉伸，仅按内容宽度，避免短文本被强制换行 */
.bubble-slot {
  width: max-content;
  max-width: 100%;
  flex: 0 0 auto;
  flex-shrink: 0;
}

.bubble-slot.feishu-bubble-slot {
  position: relative;
  overflow: visible;
  /* 向上扩一条可悬停带：浮动条在气泡上方，鼠标从气泡移向工具条经过的缝隙仍在槽内，避免 hover 断开 */
  padding-top: 40px;
  margin-top: -40px;
  z-index: 0;
}

/* 勿对槽内子节点统一 inline-block + fit-content，会与下拉 trigger 的 100% 宽高互相踩塌，右对齐气泡正文消失 */
.bubble-slot .message-menu-trigger {
  display: block;
  max-width: 100%;
}

/* 飞书风格浮动操作条：相对槽的 padding 盒定位（槽有 padding-top 作悬停桥） */
.feishu-floating-bar {
  position: absolute;
  top: 4px;
  right: 0;
  display: flex;
  align-items: center;
  gap: 0;
  padding: 1px;
  background: #fff;
  border-radius: 8px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.12);
  border: 1px solid rgba(0, 0, 0, 0.06);
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.2s ease;
  z-index: 2;
}

/* 自己发的消息：操作条放在气泡左侧，避免短消息时贴右边缘/滚动条无法点击 */
.feishu-floating-bar-self {
  right: auto;
  left: 0;
}

.feishu-bubble-slot:hover .feishu-floating-bar {
  opacity: 1;
  pointer-events: auto;
}

.feishu-bar-btn {
  width: 28px;
  height: 28px;
  min-width: 28px;
  padding: 0 !important;
  color: #86909c !important;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  transition: color 0.15s, background 0.15s;
}

.feishu-bar-btn:hover {
  color: #3370ff !important;
  background: rgba(51, 112, 255, 0.08);
}

.feishu-bar-btn :deep(svg) {
  font-size: 16px;
}

@media (max-width: 768px) {
  .feishu-floating-bar {
    opacity: 1;
    pointer-events: auto;
  }
}

/* 气泡：参照图圆角矩形、扁平、接收方白/米白 */
.message-bubble {
  box-sizing: content-box; /* 避免全局 border-box 导致 width:max-content 含 padding，内容区变窄而换行 */
  background-color: var(--feishu-bubble-received,rgb(226, 228, 234));
  border-radius: 8px;
  padding: 10px 14px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  position: relative;
  word-wrap: break-word;
  word-break: break-word;
  min-width: 0;
  width: max-content;
  max-width: 100%;
}

/*
 * 正文 + 时间：同一行排列，空间不够再换行（短句与时间同一行，不超过气泡 max-width）
 * body 用 flex:0 1 auto + min-width:0，避免再出现「仅 flex:1 + max-content 父级」把正文压成 0 宽
 */
.message-body-row {
  display: flex;
  flex-direction: row;
  flex-wrap: wrap;
  align-items: flex-end;
  column-gap: 8px;
  row-gap: 4px;
  min-width: 0;
  max-width: 100%;
}

.message-body-row .message-body {
  flex: 0 1 auto;
  min-width: 0;
  max-width: 100%;
}

.message-body-row .message-footer.bubble-footer {
  flex: 0 0 auto;
  white-space: nowrap;
}

/* 发送方：主区（正文+行末时间）整块在上，对号独占一行贴气泡右下 */
.message-body-row-self {
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
}

/*
 * TG 式：左侧正文列可换行，右侧「时间+对号」列永远同一行贴底，不与正文抢整行（避免块级正文 100% 宽把时间挤下去）
 */
.message-self-main {
  display: flex;
  flex-direction: row;
  flex-wrap: nowrap;
  align-items: flex-end;
  gap: 6px;
  min-width: 0;
  max-width: 100%;
}

.message-self-main .message-body {
  flex: 1 1 auto;
  min-width: 0;
}

.bubble-meta-trailing-self {
  display: inline-flex;
  flex-direction: row;
  align-items: flex-end;
  flex-shrink: 0;
  gap: 4px;
  white-space: nowrap;
  line-height: 1;
}

.bubble-time-inline-self {
  line-height: 1.4;
  flex-shrink: 0;
}

.bubble-footer-self {
  flex: 0 0 auto !important;
  width: 100%;
  align-self: stretch;
  white-space: normal;
}

.bubble-footer-self .bubble-footer-inner {
  width: 100%;
  display: flex;
  justify-content: flex-end;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}

/* 发送方：浅绿色、右下角小尾巴指向右侧，贴底部 */
.message-bubble-self {
  background-color: var(--wechat-bubble-sent, #DCF8C6);
  box-shadow: 0 1px 1px rgba(0, 0, 0, 0.05);
}
.message-bubble-self::after {
  left: auto;
  right: -6px;
  bottom: -8px;
  border-width: 6px 0 6px 6px;
  border-style: solid;
  border-color: transparent transparent transparent var(--wechat-bubble-sent, #DCF8C6);
}

.message-body {
  font-size: var(--font-size-sm, 14px);
  line-height: var(--line-height, 1.4);
  color: var(--wechat-text-primary, #000000);
}

.message-text {
  white-space: pre-wrap;
  min-height: 20px;
}

/* Markdown样式 */
.message-text :deep(h1),
.message-text :deep(h2),
.message-text :deep(h3),
.message-text :deep(h4),
.message-text :deep(h5),
.message-text :deep(h6) {
  margin: 8px 0 4px 0;
  font-weight: 600;
  color: var(--wechat-text-primary, #000000);
}

.message-text :deep(h1) { font-size: 18px; }
.message-text :deep(h2) { font-size: 16px; }
.message-text :deep(h3) { font-size: 14px; }
.message-text :deep(h4) { font-size: 13px; }
.message-text :deep(h5) { font-size: 12px; }
.message-text :deep(h6) { font-size: 11px; }

.message-text :deep(p) {
  margin: 4px 0;
  line-height: 1.5;
}

.message-text :deep(code) {
  background-color: rgba(0, 0, 0, 0.05);
  padding: 2px 4px;
  border-radius: 3px;
  font-family: 'Monaco', 'Consolas', monospace;
  font-size: 12px;
}

.message-text :deep(pre) {
  background-color: #f5f5f5;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  padding: 8px;
  margin: 8px 0;
  overflow-x: auto;
}

.message-text :deep(pre code) {
  background: none;
  padding: 0;
  border-radius: 0;
  font-size: 12px;
  line-height: 1.4;
}

.message-text :deep(blockquote) {
  border-left: 4px solid #07C160;
  margin: 8px 0;
  padding-left: 12px;
  color: #666;
  background-color: rgba(7, 193, 96, 0.05);
}

.message-text :deep(ul),
.message-text :deep(ol) {
  margin: 8px 0;
  padding-left: 20px;
}

.message-text :deep(li) {
  margin: 2px 0;
}

.message-text :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
  width: 100%;
}

.message-text :deep(th),
.message-text :deep(td) {
  border: 1px solid #e0e0e0;
  padding: 6px 8px;
  text-align: left;
}

.message-text :deep(th) {
  background-color: #f5f5f5;
  font-weight: 600;
}

.message-text :deep(a) {
  color: #165DFF;
  text-decoration: none;
}

.message-text :deep(a:hover) {
  text-decoration: underline;
}

.message-text :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 4px;
  margin: 4px 0;
}

.message-text :deep(hr) {
  border: none;
  border-top: 1px solid #e0e0e0;
  margin: 12px 0;
}

/* 代码块样式 */
.code-block-wrapper {
  margin: 8px 0;
  border: 1px solid #e0e0e0;
  border-radius: 6px;
  overflow: hidden;
}

.code-block-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 12px;
  background-color: #f5f5f5;
  border-bottom: 1px solid #e0e0e0;
  font-size: 12px;
  color: #666;
}

.code-language {
  font-weight: 500;
}

.copy-button {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: white;
  border: 1px solid #d0d0d0;
  border-radius: 4px;
  font-size: 12px;
  color: #666;
  cursor: pointer;
  transition: all 0.2s ease;
}

.copy-button:hover {
  background-color: #f0f0f0;
  border-color: #07C160;
  color: #07C160;
}

.copy-button.copied {
  background-color: #07C160;
  border-color: #07C160;
  color: white;
}

.copy-button svg {
  width: 14px;
  height: 14px;
}

.message-image img {
  max-width: 240px;
  max-height: 320px;
  border-radius: var(--radius-sm, 4px);
  display: block;
  cursor: pointer;
  transition: transform 0.2s ease;
}

.message-image img:hover {
  transform: scale(1.02);
}

.message-file {
  width: 240px;
}

.file-card {
  background-color: transparent;
  border: 1px solid var(--wechat-divider, #E5E5E5);
}

.file-info {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm, 8px);
}

.file-icon {
  font-size: 24px;
  color: var(--wechat-primary, #07C160);
}

.file-details {
  flex: 1;
  min-width: 0;
}

.file-name {
  font-size: var(--font-size-sm, 14px);
  color: var(--wechat-text-primary, #000000);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  font-size: var(--font-size-xs, 12px);
  color: var(--wechat-text-secondary, #888888);
}

/* 引用回复样式 */
.message-quote {
  display: flex;
  align-items: center;
  margin-bottom: 8px;
  padding: 6px 8px;
  border-radius: 8px;
  background-color: rgba(51, 112, 255, 0.10);
  border-left: 3px solid #4a78c2;
  min-width: 0;
}

.quote-header {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  line-height: 1.3;
}

.quote-sender {
  color: #2f5ea9;
  font-weight: 600;
  font-size: 12px;
  flex-shrink: 0;
}

.quote-content {
  color: rgba(0, 0, 0, 0.72);
  font-size: 12px;
  line-height: 1.2;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.message-bubble-self .message-quote {
  background-color: rgba(28, 126, 214, 0.18);
  border-left-color: #2a6dc9;
}

.message-bubble-self .quote-sender {
  color: #1f5cad;
}

.message-edited-mark {
  margin-top: 4px;
  text-align: right;
}

.edited-text {
  font-size: 10px;
  color: #999;
  font-style: italic;
}

.message-recalled {
  text-align: center;
  padding: var(--spacing-sm, 8px);
  color: var(--wechat-text-secondary, #888888);
  font-size: var(--font-size-xs, 12px);
}

.recalled-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.recalled-text {
  font-style: italic;
}

.recall-reason {
  font-size: var(--font-size-xs, 11px);
  color: var(--wechat-text-hint, #aaa);
  font-style: normal;
  margin-top: 2px;
}

/* 底部输入框编辑时：高亮对应气泡 */
.message-bubble-editing-target {
  outline: 2px solid var(--wechat-primary, #3370ff);
  outline-offset: 2px;
  border-radius: 8px;
}

.message-editing-hint {
  margin-top: 4px;
  text-align: right;
}

.editing-hint-text {
  font-size: 10px;
  color: var(--wechat-primary, #3370ff);
  opacity: 0.9;
}

.message-reactions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  align-items: center;
  margin-top: 6px;
}

.message-upload-state {
  margin-top: 8px;
  min-width: 180px;
}

.message-upload-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  color: #5f6b7a;
  margin-bottom: 4px;
}

.message-upload-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.message-upload-percent {
  flex-shrink: 0;
  color: #3370ff;
  font-weight: 600;
}

.message-upload-track {
  width: 100%;
  height: 4px;
  border-radius: 999px;
  background: rgba(51, 112, 255, 0.14);
  overflow: hidden;
}

.message-upload-fill {
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #5b8cff 0%, #3370ff 100%);
  transition: width 0.18s ease;
}

/* 飞书风格反应标签：浅灰底、小圆角、选中为蓝色 */
.reaction-tag {
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, color 0.15s;
  background-color: #eceef2;
  border: none;
  border-radius: 10px;
  padding: 1px 8px;
  user-select: none;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  line-height: 1.3;
  color: #6b7280;
}

.reaction-tag:hover {
  background-color: rgba(255, 255, 255, 0.92);
  color: var(--feishu-primary, #3370ff);
}

.reaction-tag.reaction-active,
.reaction-tag.reaction-own {
  background-color: rgba(51, 112, 255, 0.1);
  color: var(--feishu-primary, #3370ff);
  font-weight: 500;
}

.reaction-emoji {
  font-size: 13px;
  line-height: 1;
}

.reaction-users {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: #7a828f;
}

.reaction-count {
  font-size: 10px;
  color: rgba(0, 0, 0, 0.6);
  min-width: 12px;
  text-align: center;
}

.reaction-tag.reaction-active .reaction-count,
.reaction-tag.reaction-own .reaction-count {
  color: var(--feishu-primary, #3370ff);
  font-weight: 600;
}

.reply-detail-link {
  margin-top: 6px;
  padding: 0;
  border: none;
  background: transparent;
  color: #3370ff;
  font-size: 13px;
  cursor: pointer;
}

.reply-detail-link:hover {
  text-decoration: underline;
}

/* 气泡内底部：时间戳 + 状态（右对齐，小号灰色，参照图） */
.message-footer.bubble-footer {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  min-height: 18px;
  padding: 0;
  margin-top: 0;
}
.bubble-footer-inner {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: nowrap;
}
.bubble-time {
  font-size: 11px;
  color: var(--wechat-timestamp, #B2B2B2);
  white-space: nowrap;
}

.pinned-icon {
  display: inline-flex;
  align-items: center;
  line-height: 1;
  font-size: 13px;
}

.mark-icon-only {
  display: inline-flex;
  align-items: center;
  line-height: 1;
  font-size: 15px;
}

.status-tag {
  margin-right: 4px;
}

.status-tag:last-child {
  margin-right: 0;
}

.message-status {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .message-bubble::after {
    border-right-color: var(--wechat-bubble-received, #2a2a2a);
  }
  .message-bubble-self::after {
    border-left-color: var(--wechat-bubble-sent, #1e3a2a);
  }
  .message-text {
    color: var(--wechat-text-primary, #FFFFFF);
  }
  
  .message-text :deep(h1),
  .message-text :deep(h2),
  .message-text :deep(h3),
  .message-text :deep(h4),
  .message-text :deep(h5),
  .message-text :deep(h6) {
    color: var(--wechat-text-primary, #FFFFFF);
  }
  
  .message-text :deep(blockquote) {
    background-color: rgba(255, 255, 255, 0.05);
    border-left-color: #07C160;
  }
  
  .message-text :deep(pre) {
    background-color: #1a1a1a;
    border-color: #333;
  }
  
  .message-text :deep(code) {
    background-color: rgba(255, 255, 255, 0.1);
  }
  
  .message-text :deep(table th) {
    background-color: #1a1a1a;
    border-color: #333;
  }
  
  .message-text :deep(table td) {
    border-color: #333;
  }
  
  .file-name {
    color: var(--wechat-text-primary, #FFFFFF);
  }
  
  .message-quote {
    background-color: rgba(255, 255, 255, 0.08);
    border-left-color: rgba(150, 175, 199, 0.9);
  }
  
  .quote-sender {
    color: #a9c6e6;
  }

  .quote-content {
    color: #d0d5db;
  }
  
  .edited-text {
    color: #999;
  }
  
  .reaction-tag {
    background-color: rgba(0, 0, 0, 0.28);
    border: none;
  }
  
  .reaction-tag:hover {
    background-color: rgba(255, 255, 255, 0.2);
  }

  .reaction-users {
    color: #c3cad4;
  }

  .message-upload-meta {
    color: #c7d1de;
  }

  .message-upload-track {
    background: rgba(255, 255, 255, 0.16);
  }
  
  .code-block-wrapper {
    border-color: #333;
  }
  
  .code-block-header {
    background-color: #1a1a1a;
    border-color: #333;
    color: #ccc;
  }
  
  .copy-button {
    background-color: #2a2a2a;
    border-color: #444;
    color: #ccc;
  }
  
  .copy-button:hover {
    background-color: #333;
    border-color: #07C160;
    color: #07C160;
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .message-image img {
    max-width: 180px;
    max-height: 240px;
  }
  
  .message-file {
    width: 180px;
  }
  
  .message-text :deep(h1) { font-size: 16px; }
  .message-text :deep(h2) { font-size: 15px; }
  .message-text :deep(h3) { font-size: 14px; }
  .message-text :deep(h4) { font-size: 13px; }
  .message-text :deep(h5) { font-size: 12px; }
  .message-text :deep(h6) { font-size: 11px; }
}
</style>

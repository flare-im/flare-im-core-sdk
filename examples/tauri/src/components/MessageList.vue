<script setup lang="ts">
import { defineProps, defineEmits, computed, ref, nextTick } from "vue";
import MessageBubble from "./MessageBubble.vue";
import TimeStamp from "./TimeStamp.vue";
import type { Message } from "../types";
import { isMessageFromSelf, getMessageContent, asRecord } from "../utils/message";

function tsForUi(v: unknown): string | number {
  if (typeof v === "number" || typeof v === "string") return v;
  return 0;
}

const props = defineProps<{
  messages: Message[];
  currentUserId: string | null;
  loading: boolean;
  activeSessionId: string | null;
  editingMessageId: string | null;
  replyCountMap?: Record<string, number>;
  pinnedFocusMessageId?: string | null;
}>();

const emit = defineEmits<{
  (e: "scrollTop"): void;
  (e: "recall", id: string): void;
  (e: "reply", id: string): void;
  (e: "forward", id: string): void;
  (e: "thread", id: string): void;
  (e: "startEdit", m: Message): void;
  (e: "resend", id: string, text: string): void;
  (e: "addReaction", id: string, emoji: string): void;
  (e: "removeReaction", id: string, emoji: string): void;
  (e: "pin", id: string): void;
  (e: "unpin", id: string): void;
  (e: "mark", id: string, markType: number, color?: string): void;
  (e: "unmark", id: string, markType: number): void;
  (e: "delete", id: string, canDeleteForEveryone: boolean): void;
  (e: "openReplyDetail", id: string): void;
}>();

const messagesContainer = ref<HTMLDivElement | null>(null);

// 记录滚动位置，用于加载历史消息后恢复
const scrollPositionBeforeLoad = ref(0);
const isLoadingMore = ref(false);

function onScroll(e: Event) {
  const el = e.target as HTMLDivElement;
  
  // 当滚动到顶部附近时（距离顶部 50px 内），触发加载更多
  if (el.scrollTop <= 50 && !isLoadingMore.value) {
    isLoadingMore.value = true;
    emit("scrollTop");
    // 记录当前滚动位置
    scrollPositionBeforeLoad.value = el.scrollHeight;
    
    // 延迟重置标志，避免重复触发
    setTimeout(() => {
      isLoadingMore.value = false;
      // 恢复滚动位置（考虑新增内容的高度）
      if (el.scrollHeight > scrollPositionBeforeLoad.value) {
        const heightDiff = el.scrollHeight - scrollPositionBeforeLoad.value;
        el.scrollTop = heightDiff;
      }
    }, 500);
  }
}

// 兼容 Tauri/Rust 返回的 camelCase（IMMessage：serverId、content 为解码后的 Elem）
function msgSeq(m: Message | Record<string, unknown>): number {
  return Number(asRecord(m as Message).seq ?? 0);
}
function msgTimestamp(m: Message | Record<string, unknown>): number {
  const t = asRecord(m as Message).timestamp;
  if (typeof t === 'number') return t;
  return new Date(String(t)).getTime() || 0;
}
function msgServerId(m: Message | Record<string, unknown>): string {
  const r = asRecord(m as Message);
  return String(r.serverId ?? r.server_id ?? '');
}
function msgClientMsgIdForEdit(m: Message | Record<string, unknown>): string {
  const r = asRecord(m as Message);
  return String(r.clientMsgId ?? r.client_msg_id ?? '');
}
/** 与 Chat 中 editingMessageId（clientMsgId 或 serverId）对齐 */
function isRowEditing(message: Message): boolean {
  const id = props.editingMessageId;
  if (!id) return false;
  return id === msgClientMsgIdForEdit(message) || id === msgServerId(message);
}
/** 列表项唯一 key：优先 serverId，乐观消息无 serverId 时用 clientMsgId */
function msgListKey(m: Message | Record<string, unknown>): string {
  const r = asRecord(m as Message);
  const sid = String(r.serverId ?? r.server_id ?? '');
  const cid = String(r.clientMsgId ?? r.client_msg_id ?? '');
  return sid || cid || `local-${msgTimestamp(m)}`;
}

function msgIdentityList(m: Message | Record<string, unknown>): string[] {
  const r = asRecord(m as Message);
  const sid = String(r.serverId ?? r.server_id ?? "").trim();
  const cid = String(r.clientMsgId ?? r.client_msg_id ?? "").trim();
  return [sid, cid].filter(Boolean);
}

function isPinnedFocusRow(message: Message): boolean {
  const target = String(props.pinnedFocusMessageId ?? "").trim();
  if (!target) return false;
  return msgIdentityList(message).includes(target);
}
// 是否有可展示内容：与 getMessageContent / MessageBubble 一致（含 extra.content_text）
function hasDisplayableContent(m: Message | Record<string, unknown>): boolean {
  const r = asRecord(m as Message);
  if (r.isRecalled || r.is_recalled) return true;
  const content = getMessageContent(r);
  if (content != null && typeof content === 'object' && 'contentType' in content) return true;
  // 下行/入库偶发未解码出 Elem 时，仍保留列表项（气泡内展示占位），避免「有 DB 记录但窗口空白」
  const sid = r.serverId ?? r.server_id;
  const cid = r.clientMsgId ?? r.client_msg_id;
  return Boolean(sid || cid);
}

// 过滤有效消息并按时间排序
const filteredMessages = computed(() => {
  // 先排序，再过滤（确保所有消息都参与排序）
  const sorted = [...(props.messages || [])].sort((a, b) => {
    // 优先使用 seq（序列号）；seq 为 0 时不能用 truthy 判断，否则与有 seq 的消息混排错乱
    const sa = msgSeq(a);
    const sb = msgSeq(b);
    if (sa > 0 && sb > 0) return sa - sb;
    const ta = msgTimestamp(a);
    const tb = msgTimestamp(b);
    if (ta !== tb) return ta - tb;
    return msgServerId(a).localeCompare(msgServerId(b));
  });

  // 过滤有效消息（有内容或已撤回；兼容 SDK content 为解码对象）
  return sorted.filter((m) => hasDisplayableContent(m));
});

// 是否需要显示时间戳（参照微信：首条必显，之后仅间隔超过 5 分钟或跨天再显，不每条都显示）
function normalizeTimestamp(ts: unknown): number {
  if (typeof ts === 'number') return ts;
  return new Date(String(ts)).getTime() || 0;
}
const TIME_GAP_MS = 5 * 60 * 1000; // 5 分钟
function isValidTs(ms: number): boolean {
  return ms >= new Date('2020-01-01').getTime() && ms <= Date.now() + 86400000 * 365;
}
function shouldShowTimestamp(currentMsg: Message, index: number): boolean {
  if (index === 0) return true;
  const prevMsg = filteredMessages.value[index - 1];
  if (!prevMsg) return true;
  const currentTime = normalizeTimestamp(asRecord(currentMsg).timestamp);
  const prevTime = normalizeTimestamp(asRecord(prevMsg).timestamp);
  if (!isValidTs(currentTime) && !isValidTs(prevTime)) return false;
  if (!isValidTs(currentTime) || !isValidTs(prevTime)) return false;
  const diff = Math.abs(currentTime - prevTime);
  if (diff > TIME_GAP_MS) return true;
  const currDay = new Date(currentTime).setHours(0, 0, 0, 0);
  const prevDay = new Date(prevTime).setHours(0, 0, 0, 0);
  return currDay !== prevDay;
}

// 是否需要显示发送者名称（群组聊天）
function shouldShowSenderName(message: Message | Record<string, unknown>, index: number): boolean {
  if (!props.activeSessionId) return false;
  if (props.activeSessionId.startsWith('single_')) return false;
  if (isMessageFromSelf(asRecord(message as Message), props.currentUserId)) return false;
  if (index === 0) return true;
  const prevMsg = filteredMessages.value[index - 1];
  const prevSender = asRecord(prevMsg).senderId ?? asRecord(prevMsg).sender_id;
  const currSender = asRecord(message as Message).senderId ?? asRecord(message as Message).sender_id;
  return prevSender !== currSender;
}

// 滚动到底部（考虑输入框高度，确保消息不被遮挡）
async function scrollToBottom(smooth = false) {
  await nextTick();
  if (messagesContainer.value) {
    // 使用 scrollIntoView 或直接设置 scrollTop
    // 添加一些 padding 确保最后一条消息不被输入框遮挡
    const scrollHeight = messagesContainer.value.scrollHeight;
    
    if (smooth) {
      messagesContainer.value.scrollTo({
        top: scrollHeight,
        behavior: 'smooth'
      });
    } else {
      // 直接滚动到底部，添加一点 padding 避免被输入框遮挡
      messagesContainer.value.scrollTop = scrollHeight + 20;
    }
  }
}

/** 定位到指定消息（按 serverId/clientMsgId） */
async function scrollToMessage(messageId: string, smooth = true) {
  await nextTick();
  const targetId = String(messageId ?? "").trim();
  if (!targetId || !messagesContainer.value) return;
  const escaped = typeof CSS !== "undefined" && CSS.escape ? CSS.escape(targetId) : targetId;
  const el = messagesContainer.value.querySelector(
    `[data-message-id="${escaped}"]`
  ) as HTMLElement | null;
  if (!el) return;
  el.scrollIntoView({ behavior: smooth ? "smooth" : "auto", block: "center" });
}

// 暴露滚动方法给父组件
defineExpose({ scrollToBottom, scrollToMessage });
</script>

<template>
  <div 
    ref="messagesContainer"
    class="messages-container" 
    :class="{ empty: !props.activeSessionId }" 
    @scroll="onScroll"
  >
    <!-- 加载更多 -->
    <div v-if="props.activeSessionId" class="load-more">
      <slot name="load" />
    </div>
    
    <!-- 空状态 -->
    <div v-if="!props.activeSessionId" class="empty-state">
      <div class="empty-icon">💬</div>
      <div class="empty-text">请选择一个会话开始聊天</div>
    </div>
    
    <!-- 消息列表 -->
    <div v-else class="messages-list">
      <a-spin :loading="props.loading">
        <template v-for="(message, index) in filteredMessages" :key="msgListKey(message)">
          <!-- 时间戳 -->
          <TimeStamp
            v-if="shouldShowTimestamp(message, index)"
            :timestamp="tsForUi(asRecord(message).timestamp)"
            :previous-timestamp="index > 0 ? tsForUi(asRecord(filteredMessages[index - 1]).timestamp) : undefined"
          />
          
          <!-- 消息行：发送者（自己）整行右对齐 -->
          <div
            class="message-row"
            :class="{
              'message-row-self': isMessageFromSelf(asRecord(message), props.currentUserId),
              'message-row-pinned-focus': isPinnedFocusRow(message),
            }"
            :data-message-id="msgServerId(message) || msgClientMsgIdForEdit(message)"
          >
          <MessageBubble
            :message="message"
            :current-user-id="props.currentUserId"
            :is-editing="isRowEditing(message)"
            :show-sender-name="shouldShowSenderName(message, index)"
            :reply-count="props.replyCountMap?.[msgServerId(message) || msgClientMsgIdForEdit(message)] ?? 0"
            @start-edit="$emit('startEdit', $event)"
            @recall="$emit('recall', $event)"
            @resend="$emit('resend', $event)"
            @reply="$emit('reply', $event)"
            @forward="$emit('forward', $event)"
            @thread="$emit('thread', $event)"
            @add-reaction="(id: string, emoji: string) => $emit('addReaction', id, emoji)"
            @remove-reaction="(id: string, emoji: string) => $emit('removeReaction', id, emoji)"
            @pin="$emit('pin', $event)"
            @unpin="$emit('unpin', $event)"
            @mark="(id: string, type: number, color?: string) => $emit('mark', id, type, color)"
            @unmark="(id: string, type: number) => $emit('unmark', id, type)"
            @delete="(id: string, canAll: boolean) => $emit('delete', id, canAll)"
            @open-reply-detail="$emit('openReplyDetail', $event)"
          />
          </div>
        </template>
        
        <!-- 无消息状态 -->
        <div v-if="!props.loading && filteredMessages.length === 0" class="no-messages">
          <div class="no-messages-icon">📝</div>
          <div class="no-messages-text">暂无消息，开始聊天吧！</div>
        </div>
      </a-spin>
    </div>
  </div>
</template>

<style scoped>
.messages-container {
  height: 100%;
  width: 100%;
  overflow-y: auto;
  overflow-x: hidden; /* 防止横向滚动条 */
  background-color: var(--wechat-background, #F5F5F5);
  position: relative;
  /* 自定义滚动条样式 */
  scrollbar-width: thin;
  scrollbar-color: #CCCCCC transparent;
  /* 确保滚动条在容器内部 */
  /* box-sizing: border-box; */
}

.messages-container::-webkit-scrollbar {
  width: 6px;
}

.messages-container::-webkit-scrollbar-track {
  background: transparent;
}

.messages-container::-webkit-scrollbar-thumb {
  background: #CCCCCC;
  border-radius: 3px;
}

.messages-container::-webkit-scrollbar-thumb:hover {
  background: #AAAAAA;
}

.messages-container.empty {
  display: flex;
  align-items: center;
  justify-content: center;
}

.messages-list {
  padding: var(--spacing-sm, 8px) var(--spacing-md, 12px);
  /*padding-right: max(var(--spacing-md, 12px), 28px);  */
  padding-bottom: var(--spacing-sm, 8px); /* 底部 padding，不需要为输入框预留空间（输入框在外部） */
  min-height: 100%;
  display: flex;
  flex-direction: column;
  /* 移除 justify-content: flex-end，让消息自然从上到下排列 */
}

/* 消息行：整行宽度，自己发的消息右对齐；允许浮动菜单超出不裁切 */
.message-row {
  display: flex;
  width: 100%;
  overflow: visible;
  justify-content: flex-start;
}

.message-row-self {
  justify-content: flex-end;
}

.message-row-pinned-focus {
  animation: pinned-focus-flash 1.6s ease;
}

@keyframes pinned-focus-flash {
  0% {
    background: rgba(47, 128, 237, 0.18);
  }
  100% {
    background: transparent;
  }
}

.load-more {
  position: sticky;
  top: 0;
  display: flex;
  justify-content: center;
  padding: var(--spacing-sm, 8px) 0;
  background: linear-gradient(to bottom, var(--wechat-background, #F5F5F5), transparent);
  z-index: 10;
}

.empty-state {
  text-align: center;
  color: var(--wechat-text-secondary, #888888);
}

.empty-icon {
  font-size: 48px;
  margin-bottom: var(--spacing-md, 12px);
  opacity: 0.5;
}

.empty-text {
  font-size: var(--font-size-md, 16px);
  color: var(--wechat-text-secondary, #888888);
}

.no-messages {
  text-align: center;
  padding: var(--spacing-xl, 24px) 0;
  color: var(--wechat-text-secondary, #888888);
}

.no-messages-icon {
  font-size: 32px;
  margin-bottom: var(--spacing-sm, 8px);
  opacity: 0.6;
}

.no-messages-text {
  font-size: var(--font-size-sm, 14px);
}

/* 滚动条样式 */
.messages-container::-webkit-scrollbar {
  width: 6px;
}

.messages-container::-webkit-scrollbar-track {
  background: transparent;
}

.messages-container::-webkit-scrollbar-thumb {
  background: #CCCCCC;
  border-radius: 3px;
}

.messages-container::-webkit-scrollbar-thumb:hover {
  background: #AAAAAA;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .messages-container {
    background-color: var(--wechat-background, #0A0A0A);
  }
  
  .load-more {
    background: linear-gradient(to bottom, var(--wechat-background, #0A0A0A), transparent);
  }
  
  .empty-icon,
  .no-messages-icon {
    opacity: 0.7;
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .messages-list {
    padding: var(--spacing-xs, 4px) var(--spacing-sm, 8px);
    padding-right: max(var(--spacing-sm, 8px), 16px);
  }
  
  .empty-icon {
    font-size: 36px;
  }
  
  .empty-text {
    font-size: var(--font-size-sm, 14px);
  }
}

/* 加载动画 */
:deep(.arco-spin) {
  display: block;
}

:deep(.arco-spin-mask) {
  background-color: rgba(255, 255, 255, 0.8);
}

@media (prefers-color-scheme: dark) {
  :deep(.arco-spin-mask) {
    background-color: rgba(0, 0, 0, 0.5);
  }
}
</style>

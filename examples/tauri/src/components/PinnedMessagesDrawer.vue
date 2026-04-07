<script setup lang="ts">
import { computed } from "vue";
import type { ContentElem, Message as SDKMessage } from "../types";
import { asRecord, getMessageContent, getContentDecodedPreview } from "../utils/message";

const props = defineProps<{
  visible: boolean;
  messages: SDKMessage[];
}>();

const emit = defineEmits<{
  (e: "update:visible", v: boolean): void;
  (e: "focus", messageId: string): void;
  (e: "unpin", messageId: string): void;
}>();

function toTimeLabel(ts: unknown): string {
  const n = Number(ts ?? 0);
  if (!Number.isFinite(n) || n <= 0) return "";
  const d = new Date(n);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

function getMsgServerId(m: Record<string, unknown>): string {
  return String(m.serverId ?? m.server_id ?? "");
}

function getMsgClientMsgId(m: Record<string, unknown>): string {
  return String(m.clientMsgId ?? m.client_msg_id ?? "");
}

function resolveMessageId(m: SDKMessage): string {
  const row = asRecord(m);
  return getMsgServerId(row) || getMsgClientMsgId(row);
}

function isPinnedMessage(message: SDKMessage): boolean {
  const r = asRecord(message);
  const extra = (r.extra as Record<string, string> | undefined) ?? {};
  const attrs = (r.attributes as Record<string, string> | undefined) ?? {};
  return extra.pinned === "true" || attrs.pinned === "true";
}

function getPreview(message: SDKMessage): string {
  const content = getMessageContent(asRecord(message));
  if (content?.contentType === "quote") {
    const q = (content.quote ?? {}) as Record<string, unknown>;
    const rc = q.currentContent ?? q.current_content ?? q.replyContent ?? q.reply_content;
    if (rc && typeof rc === "object") {
      const decoded = getContentDecodedPreview(rc as ContentElem);
      if (decoded) return decoded;
    }
    return String(content.quote?.quotedTextPreview ?? "[非文本消息]");
  }
  if (content?.contentType === "text" && content.text?.text?.trim()) return content.text.text.trim();
  return getContentDecodedPreview(content) || "[非文本消息]";
}

const pinnedItems = computed(() => {
  return props.messages
    .filter((m) => isPinnedMessage(m))
    .map((m) => {
      const r = asRecord(m);
      return {
        id: resolveMessageId(m),
        sender: String(r.senderDisplayName ?? r.senderName ?? r.senderId ?? r.sender_id ?? ""),
        preview: getPreview(m),
        time: toTimeLabel(r.timestamp),
        timestamp: Number(r.timestamp ?? 0),
      };
    })
    .filter((x) => !!x.id)
    .sort((a, b) => b.timestamp - a.timestamp);
});

function closeDrawer() {
  emit("update:visible", false);
}
</script>

<template>
  <a-drawer
    :visible="props.visible"
    :width="460"
    title="Pinned messages"
    unmount-on-close
    @update:visible="(v) => emit('update:visible', v)"
  >
    <div class="pinned-panel">
      <div class="pinned-header">{{ pinnedItems.length }} pinned messages</div>
      <div v-if="pinnedItems.length === 0" class="pinned-empty">暂无置顶消息</div>
      <div v-else class="pinned-list">
        <button
          v-for="item in pinnedItems"
          :key="item.id"
          class="pinned-item"
          type="button"
          @click="emit('focus', item.id)"
        >
          <div class="pinned-item-main">
            <div class="pinned-item-meta">
              <span class="pinned-item-sender">{{ item.sender || "未知用户" }}</span>
              <span class="pinned-item-time">{{ item.time }}</span>
            </div>
            <div class="pinned-item-preview">{{ item.preview }}</div>
          </div>
          <a-button
            size="mini"
            type="text"
            class="pinned-item-unpin"
            @click.stop="emit('unpin', item.id)"
          >
            取消置顶
          </a-button>
        </button>
      </div>
    </div>
    <template #footer>
      <a-button type="outline" long @click="closeDrawer">关闭</a-button>
    </template>
  </a-drawer>
</template>

<style scoped>
.pinned-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.pinned-header {
  font-size: 16px;
  font-weight: 700;
  color: #111827;
}

.pinned-empty {
  color: #6b7280;
  font-size: 13px;
}

.pinned-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.pinned-item {
  display: flex;
  align-items: stretch;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  border: 1px solid #e5e7eb;
  border-left: 4px solid #2f80ed;
  border-radius: 10px;
  background: #ffffff;
  padding: 10px 12px;
  text-align: left;
  cursor: pointer;
}

.pinned-item-main {
  min-width: 0;
  flex: 1;
}

.pinned-item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}

.pinned-item-sender {
  font-size: 13px;
  font-weight: 600;
  color: #1f2937;
}

.pinned-item-time {
  font-size: 12px;
  color: #9ca3af;
}

.pinned-item-preview {
  font-size: 13px;
  color: #374151;
  word-break: break-word;
}

.pinned-item-unpin {
  color: #2f80ed;
}
</style>

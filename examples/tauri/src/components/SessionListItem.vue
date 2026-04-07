<script setup lang="ts">
import { computed } from "vue";
import type { Conversation, MessagePreviewElem } from "../types";
import Avatar from "./Avatar.vue";
import { asRecord, conversationIdFromSession } from "../utils/message";

const props = defineProps<{
  session: Conversation;
  active: boolean;
  isNew: boolean;
}>();

const emit = defineEmits<{
  (e: "select", id: string): void;
  (e: "contextmenu", payload: { id: string; event: MouseEvent }): void;
}>();

const sessionId = computed(() => conversationIdFromSession(props.session));

function lastMessageOf(session: Conversation): MessagePreviewElem | null | undefined {
  return session.lastMessage ?? session.last_message;
}

function displayNameOf(session: Conversation): string {
  return (
    String(session.displayName ?? asRecord(session).display_name ?? "").trim() ||
    conversationIdFromSession(session)
  );
}

function previewOf(session: Conversation): string {
  const preview = String(
    lastMessageOf(session)?.text ??
      session.lastMessagePreview ??
      asRecord(session).last_message_preview ??
      "",
  ).trim();
  return preview || "暂无消息";
}

function unreadOf(session: Conversation): number {
  const n = session.unreadCount ?? asRecord(session).unread_count;
  const v = typeof n === "number" ? n : Number(n ?? 0);
  if (!Number.isFinite(v) || Number.isNaN(v) || v <= 0) return 0;
  return Math.floor(v);
}

function formatTime(ts: unknown): string {
  const t = Number(ts ?? 0);
  if (!Number.isFinite(t) || t <= 0) return "";
  const d = new Date(t);
  const now = new Date();
  const sameYear = now.getFullYear() === d.getFullYear();
  const sameDay =
    sameYear &&
    now.getMonth() === d.getMonth() &&
    now.getDate() === d.getDate();
  if (sameDay) {
    return d.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  }
  if (sameYear) return `${d.getMonth() + 1}月${d.getDate()}日`;
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

function onSelect(): void {
  emit("select", sessionId.value);
}

function onContextMenu(event: MouseEvent): void {
  event.preventDefault();
  emit("contextmenu", { id: sessionId.value, event });
}
</script>

<template>
  <div
    class="session-item"
    :class="{ active: props.active }"
    @click="onSelect"
    @contextmenu="onContextMenu"
  >
    <div class="avatar-wrap">
      <Avatar
        :user-id="sessionId"
        :display-name="displayNameOf(props.session)"
        :avatar-url="props.session.avatarUrl ?? asRecord(props.session).avatar_url"
        :size="42"
      />
      <span v-if="unreadOf(props.session) > 0" class="avatar-unread">
        {{ unreadOf(props.session) > 99 ? "99+" : unreadOf(props.session) }}
      </span>
    </div>

    <div class="content-wrap">
      <div class="line-top">
        <div class="title-wrap">
          <span class="title">{{ displayNameOf(props.session) }}</span>
          <span v-if="props.isNew" class="tag-new">新</span>
        </div>
        <span class="time">
          {{
            formatTime(
              lastMessageOf(props.session)?.time ??
                props.session.updatedAt ??
                asRecord(props.session).updated_at,
            )
          }}
        </span>
      </div>

      <div class="line-bottom">
        <span class="preview">{{ previewOf(props.session) }}</span>
        <span v-if="unreadOf(props.session) > 0" class="unread-text">
          {{ unreadOf(props.session) }} 条未读
        </span>
        <span v-else-if="props.active" class="active-mark">✓</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.session-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 84px;
  padding: 0 12px;
  border-bottom: 1px solid #e8e8ea;
  cursor: pointer;
  user-select: none;
  background: #fff;
}

.session-item:hover {
  background: #f5f7fa;
}

.session-item.active {
  background: #e4e6ea;
}

.avatar-wrap {
  position: relative;
  flex-shrink: 0;
}

.avatar-unread {
  position: absolute;
  right: -4px;
  top: -4px;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: #f53f3f;
  color: #fff;
  font-size: 10px;
  line-height: 18px;
  text-align: center;
  font-weight: 600;
  box-shadow: 0 0 0 2px #fff;
}

.content-wrap {
  min-width: 0;
  flex: 1;
}

.line-top,
.line-bottom {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.line-bottom {
  margin-top: 6px;
}

.title-wrap {
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.title {
  min-width: 0;
  color: #1f2329;
  font-size: 16px;
  font-weight: 500;
  line-height: 1.2;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tag-new {
  padding: 0 5px;
  border-radius: 4px;
  background: #ebf4ff;
  color: #2475f7;
  font-size: 11px;
  line-height: 16px;
  flex-shrink: 0;
}

.time {
  color: #8b9099;
  font-size: 12px;
  flex-shrink: 0;
}

.preview {
  min-width: 0;
  color: #8b9099;
  font-size: 13px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.unread-text {
  color: #1f2329;
  font-size: 13px;
  font-weight: 500;
  flex-shrink: 0;
}

.active-mark {
  color: #8b9099;
  font-size: 16px;
  line-height: 1;
  flex-shrink: 0;
}
</style>

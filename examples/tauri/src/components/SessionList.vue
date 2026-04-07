<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { IconPlus } from "@arco-design/web-vue/es/icon";
import type { Conversation } from "../types";
import { asRecord, conversationIdFromSession } from "../utils/message";
import SessionListItem from "./SessionListItem.vue";

type SessionMenuAction = "toggle-pin" | "clear-unread";

const props = withDefaults(
  defineProps<{
    sessions: Conversation[];
    activeSessionId: string | null;
    loading: boolean;
    query: string;
    newConversationIds?: string[];
  }>(),
  { newConversationIds: () => [] },
);

const emit = defineEmits<{
  (e: "select", id: string): void;
  (e: "create"): void;
  (e: "delete", id: string): void;
  (e: "update:query", v: string): void;
  (e: "session-action", payload: { id: string; action: SessionMenuAction }): void;
}>();

const menuVisible = ref(false);
const menuX = ref(0);
const menuY = ref(0);
const menuSessionId = ref<string>("");
const menuWidth = 152;
const menuHeight = 90;

function getSessionId(session: Conversation): string {
  return conversationIdFromSession(session);
}

function getUnread(session: Conversation): number {
  const n = session.unreadCount ?? asRecord(session).unread_count;
  const v = typeof n === "number" ? n : Number(n ?? 0);
  if (!Number.isFinite(v) || Number.isNaN(v) || v <= 0) return 0;
  return Math.floor(v);
}

function isPinned(session: Conversation | null | undefined): boolean {
  if (!session) return false;
  return Boolean(session.isPinned ?? asRecord(session).is_pinned ?? false);
}

const menuSession = computed(() =>
  props.sessions.find((s) => getSessionId(s) === menuSessionId.value),
);

const totalUnreadCount = computed(() =>
  props.sessions.reduce((acc, s) => acc + getUnread(s), 0),
);

const filteredSessions = computed(() => {
  const q = props.query.trim().toLowerCase();
  if (!q) return props.sessions;
  return props.sessions.filter((s) => {
    const name = String(s.displayName ?? asRecord(s).display_name ?? "").toLowerCase();
    const preview = String(s.lastMessagePreview ?? asRecord(s).last_message_preview ?? "").toLowerCase();
    return name.includes(q) || preview.includes(q) || getSessionId(s).toLowerCase().includes(q);
  });
});

function onSelect(id: string): void {
  closeMenu();
  emit("select", id);
}

function onCreate(): void {
  emit("create");
}

function onQueryChange(v: string): void {
  emit("update:query", v);
}

function onItemContextMenu(payload: { id: string; event: MouseEvent }): void {
  payload.event.preventDefault();
  menuSessionId.value = payload.id;
  menuX.value = payload.event.clientX;
  menuY.value = payload.event.clientY;
  menuVisible.value = true;
  void nextTick(() => {
    const maxX = Math.max(8, window.innerWidth - menuWidth - 8);
    const maxY = Math.max(8, window.innerHeight - menuHeight - 8);
    menuX.value = Math.max(8, Math.min(menuX.value, maxX));
    menuY.value = Math.max(8, Math.min(menuY.value, maxY));
  });
}

function onMenuAction(action: SessionMenuAction): void {
  const id = menuSessionId.value.trim();
  if (!id) return;
  emit("session-action", { id, action });
  closeMenu();
}

function closeMenu(): void {
  menuVisible.value = false;
  menuSessionId.value = "";
}

function onWindowClick(): void {
  closeMenu();
}

function onWindowBlur(): void {
  closeMenu();
}

onMounted(() => {
  window.addEventListener("click", onWindowClick);
  window.addEventListener("blur", onWindowBlur);
});

onBeforeUnmount(() => {
  window.removeEventListener("click", onWindowClick);
  window.removeEventListener("blur", onWindowBlur);
});
</script>

<template>
  <div class="session-list">
    <div class="session-list-header">
      <a-input
        :model-value="props.query"
        placeholder="搜索会话..."
        allow-clear
        class="search-input"
        @update:model-value="onQueryChange"
      />
      <div class="header-actions">
        <a-button class="create-btn" type="primary" size="small" @click="onCreate">
          <template #icon><icon-plus /></template>
          新建会话
        </a-button>
        <a-badge v-if="totalUnreadCount > 0" :count="totalUnreadCount" :max-count="999">
          <span class="total-unread-text">总未读</span>
        </a-badge>
      </div>
    </div>

    <div class="session-items">
      <a-empty v-if="!props.loading && filteredSessions.length === 0" description="暂无会话" />
      <SessionListItem
        v-for="session in filteredSessions"
        v-else
        :key="getSessionId(session)"
        :session="session"
        :active="getSessionId(session) === props.activeSessionId"
        :is-new="props.newConversationIds?.includes(getSessionId(session))"
        @select="onSelect"
        @contextmenu="onItemContextMenu"
      />
    </div>

    <div class="session-list-footer">
      <span>共 {{ filteredSessions.length }} 个会话</span>
      <span v-if="totalUnreadCount > 0">{{ totalUnreadCount }} 条未读消息</span>
    </div>

    <div
      v-if="menuVisible"
      class="context-menu"
      :style="{ left: `${menuX}px`, top: `${menuY}px` }"
      @click.stop
    >
      <button class="context-item" @click="onMenuAction('toggle-pin')">
        <span class="context-icon">⇧</span>
        <span>{{ isPinned(menuSession) ? "取消置顶" : "置顶" }}</span>
      </button>
      <div class="context-divider" />
      <button class="context-item" @click="onMenuAction('clear-unread')">
        <span class="context-icon">🧹</span>
        <span>清除未读</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.session-list {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: #f5f6f8;
  border-right: 1px solid #e9eaee;
}

.session-list-header {
  padding: 10px 10px 8px;
  background: #fff;
  border-bottom: 1px solid #e9eaee;
}

.search-input {
  margin-bottom: 8px;
}

.header-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.create-btn {
  height: 30px;
  border-radius: 4px;
  font-size: 14px;
}

.total-unread-text {
  color: #8c9199;
  font-size: 12px;
}

.session-items {
  flex: 1;
  overflow-y: auto;
  background: #fff;
}

.session-list-footer {
  height: 34px;
  padding: 0 10px;
  border-top: 1px solid #e9eaee;
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #8c9199;
  font-size: 12px;
  background: #fff;
}

.context-menu {
  position: fixed;
  z-index: 1500;
  width: 152px;
  border-radius: 10px;
  background: #fff;
  border: 1px solid #ebecef;
  box-shadow: 0 10px 24px rgba(31, 35, 41, 0.18);
  padding: 6px;
}

.context-item {
  width: 100%;
  height: 34px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  font-size: 14px;
  color: #1f2329;
  cursor: pointer;
}

.context-item:hover {
  background: #f2f3f5;
}

.context-icon {
  width: 18px;
  text-align: center;
  opacity: 0.8;
}

.context-divider {
  height: 1px;
  background: #ebecef;
  margin: 4px 2px;
}
</style>

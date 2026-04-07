<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    count: number;
    sender?: string;
    preview: string;
  }>(),
  {
    sender: "",
  },
);

const emit = defineEmits<{
  (e: "open"): void;
  (e: "unpin"): void;
  (e: "dismiss"): void;
}>();
</script>

<template>
  <div class="telegram-pinned-bar">
    <div class="telegram-pinned-indicator" />
    <button class="telegram-pinned-main" type="button" @click="emit('open')">
      <div class="telegram-pinned-title">
        <span>📌 置顶消息</span>
        <span v-if="props.count > 1" class="telegram-pinned-count">
          {{ props.count }} 条
        </span>
      </div>
      <div class="telegram-pinned-preview">
        <span v-if="props.sender">{{ props.sender }}: </span>{{ props.preview }}
      </div>
    </button>
  </div>
</template>

<style scoped>
.telegram-pinned-bar {
  display: flex;
  align-items: stretch;
  gap: 10px;
  padding: 10px 16px;
  background: #ffffff;
  border-bottom: 1px solid var(--wechat-divider, #e5e5e5);
  flex-shrink: 0;
}

.telegram-pinned-indicator {
  width: 3px;
  border-radius: 2px;
  background: #2f80ed;
}

.telegram-pinned-main {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: none;
  padding: 0;
  text-align: left;
  cursor: pointer;
}

.telegram-pinned-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: #2f80ed;
}

.telegram-pinned-count {
  color: #9ca3af;
  font-weight: 500;
}

.telegram-pinned-preview {
  margin-top: 2px;
  font-size: 13px;
  color: #374151;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.telegram-pinned-close {
  color: #6b7280;
}

.telegram-pinned-unpin {
  color: #2f80ed;
}

@media (prefers-color-scheme: dark) {
  .telegram-pinned-bar {
    background-color: #1a1a1a;
    border-bottom-color: var(--wechat-divider, #2c2c2c);
  }

  .telegram-pinned-preview {
    color: #d1d5db;
  }
}

@media (max-width: 768px) {
  .telegram-pinned-bar {
    padding: 8px 12px;
    gap: 8px;
  }

  .telegram-pinned-preview {
    font-size: 12px;
  }
}
</style>

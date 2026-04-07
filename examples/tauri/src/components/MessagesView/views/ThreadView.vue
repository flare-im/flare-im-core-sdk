<template>
  <div class="feishu-thread">
    <div class="feishu-thread-card">
      <span class="feishu-thread-icon">💬</span>
      <div class="feishu-thread-info">
        <span class="feishu-thread-title">{{ payload.threadTitle || '话题' }}</span>
        <span class="feishu-thread-hint">查看回复</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ContentElem } from '../../../types';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const payload = computed(() => {
  if (props.content.contentType !== 'thread' || !props.content.thread) {
    return { threadId: '', threadTitle: '' };
  }
  return props.content.thread;
});
</script>

<style scoped>
.feishu-thread {
  max-width: 280px;
}

.feishu-thread-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-thread-icon {
  font-size: 20px;
  flex-shrink: 0;
}

.feishu-thread-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-thread-title {
  font-size: 14px;
  color: var(--feishu-text-primary, #1d2129);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.feishu-thread-hint {
  font-size: 12px;
  color: var(--feishu-primary, #3370ff);
}
</style>

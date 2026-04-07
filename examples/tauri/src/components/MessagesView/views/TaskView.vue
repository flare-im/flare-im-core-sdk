<template>
  <div class="feishu-task">
    <div class="feishu-task-card">
      <span class="feishu-task-icon">✓</span>
      <div class="feishu-task-info">
        <span class="feishu-task-title">{{ payload.title || '任务' }}</span>
        <span class="feishu-task-status" :class="statusClass">{{ payload.status || '' }}</span>
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
  if (props.content.contentType !== 'task' || !props.content.task) {
    return { taskId: '', title: '', status: '' };
  }
  return props.content.task;
});

const statusClass = computed(() => {
  const s = (payload.value.status || '').toLowerCase();
  if (s.includes('done') || s.includes('完成')) return 'feishu-task-status-done';
  if (s.includes('todo') || s.includes('待办')) return 'feishu-task-status-todo';
  return '';
});
</script>

<style scoped>
.feishu-task {
  max-width: 280px;
}

.feishu-task-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-task-icon {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--feishu-primary, #3370ff);
  color: #fff;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  flex-shrink: 0;
}

.feishu-task-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-task-title {
  font-size: 14px;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-task-status {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
}

.feishu-task-status-done {
  color: #00b42a;
}

.feishu-task-status-todo {
  color: var(--feishu-primary);
}
</style>

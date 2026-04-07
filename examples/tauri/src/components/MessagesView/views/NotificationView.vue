<template>
  <div class="feishu-notification">
    <div class="feishu-notification-card">
      <span v-if="payload.title" class="feishu-notification-title">{{ payload.title }}</span>
      <span class="feishu-notification-body">{{ payload.body || '[通知]' }}</span>
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
  if (props.content.contentType !== 'notification' || !props.content.notification) {
    return { title: '', body: '', notificationType: '' };
  }
  return props.content.notification;
});
</script>

<style scoped>
.feishu-notification {
  max-width: 320px;
}

.feishu-notification-card {
  padding: 10px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.feishu-notification-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-notification-body {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  line-height: 1.4;
}
</style>

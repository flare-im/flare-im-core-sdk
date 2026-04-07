<template>
  <div class="feishu-forward">
    <div class="feishu-forward-header">
      <span class="feishu-forward-icon">↗</span>
      <span class="feishu-forward-title">转发 {{ count }} 条消息</span>
    </div>
    <p v-if="payload.forwardReason" class="feishu-forward-reason">{{ payload.forwardReason }}</p>
    <div class="feishu-forward-list">
      <div
        v-for="(preview, i) in previews"
        :key="i"
        class="feishu-forward-item"
      >
        <span class="feishu-forward-sender">{{ preview.senderId }}</span>
        <span class="feishu-forward-text">{{ preview.text || '[消息]' }}</span>
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
  if (props.content.contentType !== 'forward' || !props.content.forward) {
    return { messageIds: [], forwardReason: '', forwardedPreviews: [] };
  }
  return props.content.forward;
});

const count = computed(() => payload.value.messageIds?.length ?? 0);

const previews = computed(() => payload.value.forwardedPreviews?.slice(0, 5) ?? []);
</script>

<style scoped>
.feishu-forward {
  max-width: 320px;
  padding: 10px 12px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-forward-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 6px;
}

.feishu-forward-icon {
  font-size: 14px;
  color: var(--feishu-primary, #3370ff);
}

.feishu-forward-title {
  font-size: 13px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-forward-reason {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  margin: 0 0 8px;
  line-height: 1.4;
}

.feishu-forward-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.feishu-forward-item {
  font-size: 12px;
  color: var(--feishu-text-secondary);
  padding: 4px 0;
  border-bottom: 1px solid var(--feishu-border);
}

.feishu-forward-item:last-child {
  border-bottom: none;
}

.feishu-forward-sender {
  color: var(--feishu-primary);
  margin-right: 6px;
}

.feishu-forward-text {
  color: var(--feishu-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: inline-block;
  max-width: 200px;
  vertical-align: bottom;
}
</style>

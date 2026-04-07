<template>
  <div class="feishu-announcement">
    <div class="feishu-announcement-card">
      <div class="feishu-announcement-header">
        <span class="feishu-announcement-icon">📢</span>
        <span class="feishu-announcement-title">{{ payload.title || '公告' }}</span>
        <span v-if="payload.pinned" class="feishu-announcement-pin">置顶</span>
      </div>
      <p v-if="payload.body" class="feishu-announcement-body">{{ payload.body }}</p>
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
  if (props.content.contentType !== 'announcement' || !props.content.announcement) {
    return { title: '', body: '', pinned: false };
  }
  return props.content.announcement;
});
</script>

<style scoped>
.feishu-announcement {
  max-width: 320px;
}

.feishu-announcement-card {
  padding: 12px 14px;
  background: linear-gradient(135deg, #fef9e7 0%, #fdf6e3 100%);
  border: 1px solid #f0e6c8;
  border-radius: var(--feishu-radius, 8px);
}

.feishu-announcement-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.feishu-announcement-icon {
  font-size: 18px;
  flex-shrink: 0;
}

.feishu-announcement-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--feishu-text-primary, #1d2129);
  flex: 1;
}

.feishu-announcement-pin {
  font-size: 11px;
  color: var(--feishu-primary, #3370ff);
  padding: 2px 6px;
  background: rgba(51, 112, 255, 0.1);
  border-radius: 4px;
}

.feishu-announcement-body {
  font-size: 13px;
  color: var(--feishu-text-secondary, #86909c);
  line-height: 1.5;
  margin: 0;
}
</style>

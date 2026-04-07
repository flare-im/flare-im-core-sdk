<template>
  <div class="feishu-mini-program">
    <div class="feishu-mini-program-card">
      <img
        v-if="payload.thumbnailUrl"
        :src="payload.thumbnailUrl"
        :alt="payload.title"
        class="feishu-mini-program-thumb"
      />
      <div class="feishu-mini-program-body">
        <span class="feishu-mini-program-title">{{ payload.title || '小程序' }}</span>
        <span class="feishu-mini-program-path">{{ payload.pagePath || '' }}</span>
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
  if (props.content.contentType !== 'miniProgram' || !props.content.miniProgram) {
    return { appId: '', title: '', pagePath: '', thumbnailUrl: '' };
  }
  return props.content.miniProgram;
});
</script>

<style scoped>
.feishu-mini-program {
  max-width: 280px;
}

.feishu-mini-program-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-mini-program-thumb {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  object-fit: cover;
  flex-shrink: 0;
}

.feishu-mini-program-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-mini-program-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.feishu-mini-program-path {
  font-size: 11px;
  color: var(--feishu-text-tertiary, #c9cdd4);
}
</style>

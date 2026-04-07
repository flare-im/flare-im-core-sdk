<template>
  <div class="feishu-custom">
    <div class="feishu-custom-card">
      <span class="feishu-custom-type">{{ payload.type || '自定义' }}</span>
      <span v-if="payload.description" class="feishu-custom-desc">{{ payload.description }}</span>
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
  if (props.content.contentType !== 'custom' || !props.content.custom) {
    return { type: '', description: '' };
  }
  return props.content.custom;
});
</script>

<style scoped>
.feishu-custom {
  max-width: 280px;
}

.feishu-custom-card {
  padding: 10px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px dashed var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.feishu-custom-type {
  font-size: 12px;
  color: var(--feishu-text-tertiary, #c9cdd4);
}

.feishu-custom-desc {
  font-size: 13px;
  color: var(--feishu-text-secondary, #86909c);
}
</style>

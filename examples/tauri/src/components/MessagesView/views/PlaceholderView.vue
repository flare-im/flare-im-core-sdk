<template>
  <div class="feishu-placeholder">
    <span class="feishu-placeholder-text">{{ payload.fallbackText || '[占位]' }}</span>
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
  if (props.content.contentType !== 'placeholder' || !props.content.placeholder) {
    return { reason: '', fallbackText: '' };
  }
  return props.content.placeholder;
});
</script>

<style scoped>
.feishu-placeholder {
  padding: 4px 0;
}

.feishu-placeholder-text {
  font-size: 12px;
  color: var(--feishu-text-tertiary, #c9cdd4);
  font-style: italic;
}
</style>

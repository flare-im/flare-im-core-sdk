<template>
  <div class="feishu-system">
    <span class="feishu-system-body">{{ payload.body || '[系统消息]' }}</span>
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
  if (props.content.contentType !== 'system' || !props.content.system) {
    return { eventKind: '', body: '' };
  }
  return props.content.system;
});
</script>

<style scoped>
.feishu-system {
  text-align: center;
  padding: 6px 12px;
}

.feishu-system-body {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  line-height: 1.4;
}
</style>

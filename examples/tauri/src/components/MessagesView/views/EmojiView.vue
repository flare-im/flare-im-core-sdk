<template>
  <div class="feishu-emoji">
    <span class="feishu-emoji-char">{{ payload.emoji }}</span>
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
  if (props.content.contentType !== 'emoji' || !props.content.emoji) {
    return { emoji: '' };
  }
  return props.content.emoji;
});
</script>

<style scoped>
.feishu-emoji {
  display: inline-block;
  font-size: 48px;
  line-height: 1;
}
</style>

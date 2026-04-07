<template>
  <div class="feishu-rich-text" v-html="renderedHtml"></div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ContentElem } from '../../../types';
import { renderMarkdown, isMarkdown } from '../../../utils/markdown';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const rawContent = computed(() => {
  if (props.content.contentType !== 'richText' || !props.content.richText) return '';
  return props.content.richText.content ?? '';
});

const renderedHtml = computed(() => {
  const t = rawContent.value;
  if (!t) return '';
  if (isMarkdown(t)) return renderMarkdown(t);
  return t.replace(/\n/g, '<br>');
});
</script>

<style scoped>
.feishu-rich-text {
  font-size: 14px;
  line-height: 1.5;
  color: var(--feishu-text-primary, #1d2129);
  white-space: pre-wrap;
  word-break: break-word;
}

.feishu-rich-text :deep(a) {
  color: var(--feishu-primary, #3370ff);
  text-decoration: none;
}

.feishu-rich-text :deep(a:hover) {
  text-decoration: underline;
}
</style>

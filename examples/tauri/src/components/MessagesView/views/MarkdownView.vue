<template>
  <div class="feishu-markdown" v-html="renderedHtml"></div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ContentElem } from '../../../types';
import { renderMarkdown } from '../../../utils/markdown';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const renderedHtml = computed(() => {
  if (props.content.contentType !== 'markdown' || !props.content.markdown) return '';
  const t = props.content.markdown.text ?? '';
  return renderMarkdown(t);
});
</script>

<style scoped>
.feishu-markdown {
  font-size: 14px;
  line-height: 1.5;
  color: var(--feishu-text-primary, #1d2129);
  word-break: break-word;
}

.feishu-markdown :deep(h1),
.feishu-markdown :deep(h2),
.feishu-markdown :deep(h3) {
  margin: 12px 0 6px;
  font-weight: 600;
}

.feishu-markdown :deep(p) {
  margin: 6px 0;
}

.feishu-markdown :deep(code) {
  background: var(--feishu-bg-card);
  padding: 2px 6px;
  border-radius: 4px;
  font-size: 13px;
}

.feishu-markdown :deep(pre) {
  background: var(--feishu-bg-card);
  border: 1px solid var(--feishu-border);
  border-radius: 8px;
  padding: 12px;
  overflow-x: auto;
  margin: 8px 0;
}

.feishu-markdown :deep(a) {
  color: var(--feishu-primary);
  text-decoration: none;
}

.feishu-markdown :deep(a:hover) {
  text-decoration: underline;
}
</style>

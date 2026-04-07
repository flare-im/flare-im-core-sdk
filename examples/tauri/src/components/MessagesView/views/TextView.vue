<template>
  <div class="feishu-text" v-html="renderedHtml"></div>
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

const rawText = computed(() => {
  if (props.content.contentType !== 'text') return '';
  const t = props.content.text;
  if (t == null) return '';
  // 兼容后端直接传 text 为字符串
  if (typeof t === 'string') return t;
  return (t as { text?: string }).text ?? '';
});

const renderedHtml = computed(() => {
  const t = rawText.value;
  if (!t) return '';
  if (isMarkdown(t)) return renderMarkdown(t);
  return t.replace(/\n/g, '<br>');
});
</script>

<style scoped>
.feishu-text {
  font-size: 14px;
  line-height: 1.5;
  color: var(--feishu-text-primary, #1d2129);
  white-space: pre-wrap;
  word-break: break-word;
}

.feishu-text :deep(a) {
  color: var(--feishu-primary, #3370ff);
  text-decoration: none;
}

.feishu-text :deep(a:hover) {
  text-decoration: underline;
}

.feishu-text :deep(code) {
  background: var(--feishu-bg-card, #f7f8fa);
  padding: 2px 6px;
  border-radius: var(--feishu-radius-sm, 4px);
  font-size: 13px;
}

.feishu-text :deep(pre) {
  background: var(--feishu-bg-card);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  padding: 12px;
  overflow-x: auto;
  margin: 8px 0;
}

.feishu-text :deep(blockquote) {
  border-left: 3px solid var(--feishu-primary);
  margin: 8px 0;
  padding-left: 12px;
  color: var(--feishu-text-secondary, #86909c);
}
</style>

<template>
  <div class="feishu-quote">
    <div class="feishu-quote-header">
      <span class="feishu-quote-sender">{{ payload.quotedSenderId || '某人' }}</span>
    </div>
    <div class="feishu-quote-body" v-html="quotedHtml"></div>
  </div>
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

const payload = computed(() => {
  if (props.content.contentType !== 'quote' || !props.content.quote) {
    return { quotedSenderId: '', quotedTextPreview: '' };
  }
  return props.content.quote;
});

const quotedHtml = computed(() => {
  const t = payload.value.quotedTextPreview || '';
  if (!t) return '';
  if (isMarkdown(t)) return renderMarkdown(t);
  return t.replace(/\n/g, '<br>');
});
</script>

<style scoped>
.feishu-quote {
  padding: 8px 12px;
  background: var(--feishu-bg-card, #f7f8fa);
  border-left: 3px solid var(--feishu-primary, #3370ff);
  border-radius: 0 var(--feishu-radius-sm, 4px) var(--feishu-radius-sm) 0;
  margin-bottom: 8px;
}

.feishu-quote-header {
  margin-bottom: 4px;
}

.feishu-quote-sender {
  font-size: 12px;
  font-weight: 500;
  color: var(--feishu-primary, #3370ff);
}

.feishu-quote-body {
  font-size: 13px;
  color: var(--feishu-text-secondary, #86909c);
  line-height: 1.45;
}

.feishu-quote-body :deep(p) {
  margin: 2px 0;
}
</style>

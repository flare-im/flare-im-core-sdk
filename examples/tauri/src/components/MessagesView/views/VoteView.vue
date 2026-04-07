<template>
  <div class="feishu-vote">
    <div class="feishu-vote-card">
      <span class="feishu-vote-title">{{ payload.title || '投票' }}</span>
      <div class="feishu-vote-options">
        <div
          v-for="(opt, i) in payload.options"
          :key="i"
          class="feishu-vote-option"
        >
          {{ opt }}
        </div>
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
  if (props.content.contentType !== 'vote' || !props.content.vote) {
    return { voteId: '', title: '', options: [] as string[] };
  }
  return props.content.vote;
});
</script>

<style scoped>
.feishu-vote {
  max-width: 280px;
}

.feishu-vote-card {
  padding: 12px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-vote-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
  display: block;
  margin-bottom: 10px;
}

.feishu-vote-options {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.feishu-vote-option {
  font-size: 13px;
  color: var(--feishu-text-secondary, #86909c);
  padding: 6px 10px;
  background: #fff;
  border: 1px solid var(--feishu-border);
  border-radius: var(--feishu-radius-sm, 4px);
}
</style>

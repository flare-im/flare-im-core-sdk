<template>
  <div class="feishu-card">
    <div class="feishu-card-inner">
      <img
        v-if="payload.avatarUrl"
        :src="payload.avatarUrl"
        :alt="payload.nickname"
        class="feishu-card-avatar"
      />
      <div v-else class="feishu-card-avatar-placeholder">
        {{ (payload.nickname || '名').charAt(0) }}
      </div>
      <div class="feishu-card-info">
        <span class="feishu-card-name">{{ payload.nickname || '用户' }}</span>
        <span v-if="payload.description" class="feishu-card-desc">{{ payload.description }}</span>
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
  if (props.content.contentType !== 'card' || !props.content.card) {
    return { userId: '', nickname: '', avatarUrl: '', description: '' };
  }
  return props.content.card;
});
</script>

<style scoped>
.feishu-card {
  max-width: 260px;
}

.feishu-card-inner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
}

.feishu-card-avatar {
  width: 44px;
  height: 44px;
  border-radius: 8px;
  object-fit: cover;
  flex-shrink: 0;
}

.feishu-card-avatar-placeholder {
  width: 44px;
  height: 44px;
  border-radius: 8px;
  background: var(--feishu-primary, #3370ff);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  font-weight: 600;
  flex-shrink: 0;
}

.feishu-card-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-card-name {
  font-size: 14px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-card-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
}
</style>

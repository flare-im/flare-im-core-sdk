<template>
  <a
    class="feishu-link-card"
    :href="payload.url"
    target="_blank"
    rel="noopener"
  >
    <img
      v-if="payload.thumbnailUrl"
      :src="payload.thumbnailUrl"
      :alt="payload.title"
      class="feishu-link-card-thumb"
    />
    <div class="feishu-link-card-body">
      <span class="feishu-link-card-title">{{ payload.title || '链接' }}</span>
      <span v-if="payload.description" class="feishu-link-card-desc">{{ payload.description }}</span>
      <span v-if="payload.siteName" class="feishu-link-card-site">{{ payload.siteName }}</span>
    </div>
  </a>
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
  if (props.content.contentType !== 'linkCard' || !props.content.linkCard) {
    return { url: '', title: '', description: '', thumbnailUrl: '', siteName: '' };
  }
  return props.content.linkCard;
});
</script>

<style scoped>
.feishu-link-card {
  display: flex;
  flex-direction: column;
  max-width: 360px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  overflow: hidden;
  text-decoration: none;
  color: inherit;
  transition: background 0.2s, border-color 0.2s;
}

.feishu-link-card:hover {
  background: var(--feishu-bg-hover, #f2f3f5);
  border-color: var(--feishu-primary, #3370ff);
}

.feishu-link-card-thumb {
  width: 100%;
  aspect-ratio: 2/1;
  object-fit: cover;
  background: var(--feishu-bg-hover);
}

.feishu-link-card-body {
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.feishu-link-card-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--feishu-text-primary, #1d2129);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.feishu-link-card-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.feishu-link-card-site {
  font-size: 11px;
  color: var(--feishu-text-tertiary, #c9cdd4);
}
</style>

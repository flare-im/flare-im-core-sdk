<template>
  <div class="feishu-location">
    <a
      class="feishu-location-card"
      :href="mapLink"
      target="_blank"
      rel="noopener"
    >
      <span class="feishu-location-icon">📍</span>
      <div class="feishu-location-info">
        <span class="feishu-location-address">{{ payload.address || '位置' }}</span>
        <span v-if="payload.description" class="feishu-location-desc">{{ payload.description }}</span>
      </div>
      <span class="feishu-location-arrow">→</span>
    </a>
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
  if (props.content.contentType !== 'location' || !props.content.location) {
    return { latitude: 0, longitude: 0, address: '', description: '' };
  }
  return props.content.location;
});

const mapLink = computed(() => {
  const p = payload.value;
  return `https://maps.google.com/?q=${p.latitude},${p.longitude}`;
});
</script>

<style scoped>
.feishu-location {
  max-width: 280px;
}

.feishu-location-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  background: var(--feishu-bg-card, #f7f8fa);
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  text-decoration: none;
  color: inherit;
  transition: background 0.2s;
}

.feishu-location-card:hover {
  background: var(--feishu-bg-hover, #f2f3f5);
}

.feishu-location-icon {
  font-size: 24px;
  flex-shrink: 0;
}

.feishu-location-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-location-address {
  font-size: 14px;
  color: var(--feishu-text-primary, #1d2129);
}

.feishu-location-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
}

.feishu-location-arrow {
  font-size: 14px;
  color: var(--feishu-primary, #3370ff);
  flex-shrink: 0;
}
</style>

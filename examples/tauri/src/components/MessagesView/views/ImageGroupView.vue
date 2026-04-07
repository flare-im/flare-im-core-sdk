<template>
  <div class="feishu-image-group">
    <div class="feishu-image-group-grid" :class="gridClass">
      <ImageGroupTile v-for="(img, i) in images" :key="i" :info="img" />
    </div>
    <p v-if="payload.description" class="feishu-image-group-desc">{{ payload.description }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ContentElem } from '../../../types';
import ImageGroupTile from './ImageGroupTile.vue';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const payload = computed(() => {
  if (props.content.contentType !== 'imageGroup' || !props.content.imageGroup) {
    return { images: [], description: '' };
  }
  return props.content.imageGroup;
});

const images = computed(() => payload.value.images ?? []);

const gridClass = computed(() => {
  const n = images.value.length;
  if (n <= 1) return 'feishu-image-group-1';
  if (n === 2) return 'feishu-image-group-2';
  if (n <= 4) return 'feishu-image-group-4';
  return 'feishu-image-group-9';
});
</script>

<style scoped>
.feishu-image-group {
  max-width: 320px;
}

.feishu-image-group-grid {
  display: grid;
  gap: 4px;
  border-radius: var(--feishu-radius, 8px);
  overflow: hidden;
}

.feishu-image-group-1 {
  grid-template-columns: 1fr;
}

.feishu-image-group-2 {
  grid-template-columns: 1fr 1fr;
}

.feishu-image-group-4 {
  grid-template-columns: 1fr 1fr;
}

.feishu-image-group-9 {
  grid-template-columns: 1fr 1fr 1fr;
}

.feishu-image-group-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary);
  margin: 6px 0 0;
}
</style>

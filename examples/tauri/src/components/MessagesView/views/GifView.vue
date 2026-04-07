<template>
  <div class="feishu-gif">
    <img
      :src="imageUrl"
      alt="GIF"
      loading="lazy"
      class="feishu-gif-img"
    />
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
  if (props.content.contentType !== 'gif' || !props.content.gif) return null;
  return props.content.gif;
});

const imageUrl = computed(() => {
  const p = payload.value;
  return p?.url || p?.thumbnail?.url || '';
});
</script>

<style scoped>
.feishu-gif {
  border-radius: var(--feishu-radius, 8px);
  overflow: hidden;
  max-width: 240px;
}

.feishu-gif-img {
  max-width: 100%;
  max-height: 240px;
  display: block;
  vertical-align: top;
}
</style>

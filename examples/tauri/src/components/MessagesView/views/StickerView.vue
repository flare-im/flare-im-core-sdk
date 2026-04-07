<template>
  <div class="feishu-sticker">
    <img
      :src="payload.url"
      :alt="'贴纸'"
      :style="imgStyle"
      loading="lazy"
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
  if (props.content.contentType !== 'sticker' || !props.content.sticker) {
    return { url: '', width: 120, height: 120 };
  }
  return props.content.sticker;
});

const imgStyle = computed(() => {
  const p = payload.value;
  const max = 160;
  let w = p.width || 120;
  let h = p.height || 120;
  if (w > max || h > max) {
    const scale = max / Math.max(w, h);
    w = Math.round(w * scale);
    h = Math.round(h * scale);
  }
  return { maxWidth: `${w}px`, maxHeight: `${h}px` };
});
</script>

<style scoped>
.feishu-sticker {
  display: inline-block;
  line-height: 0;
}

.feishu-sticker img {
  max-width: 160px;
  max-height: 160px;
  vertical-align: top;
  border-radius: var(--feishu-radius-sm, 4px);
}
</style>

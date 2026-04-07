<template>
  <img
    class="feishu-image-group-item"
    :src="displayUrl"
    alt="图片"
    loading="lazy"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { ImageInfoElem } from '../../../types';
import { useMediaAccessUrl } from '../../../composables/useMediaAccessUrl';
import { isLikelyLocalMediaRef, stableImageMediaId } from '../../../utils/mediaRef';
import { toWebviewLocalMediaUrl } from '../../../utils/localMediaUrl';

const props = defineProps<{ info: ImageInfoElem }>();

function resolveMaybeLocalUrl(raw: string): string {
  return toWebviewLocalMediaUrl(String(raw ?? ''));
}

const fallbackUrl = computed(() => {
  const u = String(props.info.url ?? '').trim();
  if (u) return toWebviewLocalMediaUrl(u);
  return resolveMaybeLocalUrl(stableImageMediaId(props.info));
});

const remoteId = computed(() => {
  const id = stableImageMediaId(props.info);
  if (!id || isLikelyLocalMediaRef(id)) return '';
  return id;
});

const { resolvedUrl } = useMediaAccessUrl(
  () => remoteId.value,
  () => fallbackUrl.value,
);

const displayUrl = computed(() => resolvedUrl.value || fallbackUrl.value);
</script>

<style scoped>
.feishu-image-group-item {
  width: 100%;
  aspect-ratio: 1;
  object-fit: cover;
  display: block;
}
</style>

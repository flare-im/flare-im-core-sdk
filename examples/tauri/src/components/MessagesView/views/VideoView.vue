<template>
  <div class="feishu-video">
    <div class="feishu-video-cover" @click="play">
      <video
        v-if="previewVideoUrl"
        class="feishu-video-player"
        :src="previewVideoUrl"
        playsinline
        muted
        preload="metadata"
      />
      <img
        v-else-if="coverUrl"
        :src="coverUrl"
        alt="视频封面"
        loading="lazy"
      />
      <div v-else class="feishu-video-placeholder">
        <span class="icon">▶</span>
      </div>
      <div class="feishu-video-mask">
        <span class="play-icon">▶</span>
        <span v-if="durationText" class="duration">{{ durationText }}</span>
      </div>
    </div>
    <div class="feishu-video-footer">
      <button type="button" class="feishu-video-download" :disabled="downloadBusy || !previewVideoUrl" @click.stop="onDownload">
        {{ downloadBusy ? '保存中…' : '下载视频' }}
      </button>
    </div>
    <p v-if="payload?.description" class="feishu-video-desc">{{ payload?.description }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { Message } from '@arco-design/web-vue';
import type { ContentElem } from '../../../types';
import { useMediaAccessUrl } from '../../../composables/useMediaAccessUrl';
import { toWebviewLocalMediaUrl } from '../../../utils/localMediaUrl';
import { defaultNameWithMime, downloadUrlToDevice } from '../../../utils/mediaDownload';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const downloadBusy = ref(false);

const payload = computed(() => {
  if (props.content.contentType !== 'video' || !props.content.video) return null;
  return props.content.video;
});

function resolveMaybeLocalUrl(raw: string): string {
  return toWebviewLocalMediaUrl(String(raw ?? ''));
}

const fallbackVideoUrl = computed(() => {
  const u = String(payload.value?.source?.url ?? '').trim();
  if (u) return toWebviewLocalMediaUrl(u);
  return resolveMaybeLocalUrl(payload.value?.videoId || '');
});
const fallbackCoverUrl = computed(() => {
  const cover = String(payload.value?.cover?.url ?? '').trim();
  if (cover) return toWebviewLocalMediaUrl(cover);
  const src = String(payload.value?.source?.url ?? '').trim();
  if (src) return toWebviewLocalMediaUrl(src);
  return resolveMaybeLocalUrl(payload.value?.videoId || '');
});

const remoteFileId = computed(() => {
  const videoId = String(payload.value?.videoId ?? '').trim();
  if (!videoId || videoId.startsWith('/') || videoId.startsWith('./') || videoId.startsWith('../') || videoId.startsWith('file://')) {
    return '';
  }
  return videoId;
});

const { resolvedUrl } = useMediaAccessUrl(
  () => remoteFileId.value,
  () => fallbackVideoUrl.value,
);

const previewVideoUrl = computed(() => resolvedUrl.value || fallbackVideoUrl.value);
const coverUrl = computed(() => fallbackCoverUrl.value);

const durationText = computed(() => {
  const ms = payload.value?.source?.durationMs;
  if (ms == null) return '';
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  return m > 0 ? `${m}:${String(s % 60).padStart(2, '0')}` : `0:${String(s).padStart(2, '0')}`;
});

function play() {
  const url = previewVideoUrl.value;
  if (url) window.open(url, '_blank');
}

function videoDownloadName(): string {
  const id = String(payload.value?.videoId ?? 'video').slice(0, 64);
  const mime = String(payload.value?.source?.mimeType ?? 'video/mp4');
  return defaultNameWithMime(`video_${id}`, mime);
}

async function onDownload() {
  const url = String(previewVideoUrl.value ?? '').trim();
  if (!url) {
    Message.warning('视频地址不可用');
    return;
  }
  downloadBusy.value = true;
  try {
    await downloadUrlToDevice(url, videoDownloadName());
    Message.success('已开始下载');
  } catch (e) {
    console.error('[VideoView] download', e);
    Message.error(e instanceof Error ? e.message : '下载失败');
  } finally {
    downloadBusy.value = false;
  }
}
</script>

<style scoped>
.feishu-video {
  border-radius: var(--feishu-radius, 8px);
  overflow: hidden;
  max-width: 320px;
}

.feishu-video-cover {
  position: relative;
  background: var(--feishu-bg-card, #f7f8fa);
  aspect-ratio: 16/10;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.feishu-video-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.feishu-video-player {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  background: #000;
}

.feishu-video-placeholder {
  color: var(--feishu-text-tertiary, #c9cdd4);
  font-size: 48px;
}

.feishu-video-mask {
  position: absolute;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 8px;
}

.play-icon {
  color: #fff;
  font-size: 40px;
}

.duration {
  color: #fff;
  font-size: 12px;
}

.feishu-video-footer {
  margin-top: 8px;
}

.feishu-video-download {
  border: none;
  background: transparent;
  padding: 0;
  font-size: 13px;
  color: var(--feishu-primary, #3370ff);
  cursor: pointer;
}

.feishu-video-download:hover:not(:disabled) {
  text-decoration: underline;
}

.feishu-video-download:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.feishu-video-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary);
  margin: 6px 0 0;
}
</style>

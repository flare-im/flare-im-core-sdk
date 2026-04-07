<template>
  <div class="feishu-audio-wrap">
    <div
      class="feishu-audio-bubble"
      :class="{ 'feishu-audio-bubble-self': isSelf }"
    >
      <audio
        ref="audioRef"
        :src="audioUrl || undefined"
        preload="metadata"
        class="feishu-audio-el"
        @ended="onEnded"
        @play="onPlayEvt"
        @pause="playing = false"
      />
      <button
        type="button"
        class="feishu-audio-wave"
        :disabled="!audioUrl"
        :aria-label="playing ? '暂停' : '播放'"
        @click="togglePlay"
      >
        <svg class="feishu-audio-wave-svg" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
          <circle cx="5" cy="12" r="1.8" fill="currentColor" />
          <path
            d="M9 8c2 2.5 2 7.5 0 10"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
          />
          <path
            d="M13 5c3.2 3.5 3.2 10.5 0 14"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
          />
          <path
            d="M17 2c4.2 4.2 4.2 15.8 0 20"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
          />
        </svg>
      </button>
      <span class="feishu-audio-duration">{{ durationLabel }}</span>
      <button
        v-if="audioUrl"
        type="button"
        class="feishu-audio-dl"
        :disabled="dlBusy"
        @click.stop="onDownload"
      >
        {{ dlBusy ? '…' : '下载' }}
      </button>
    </div>
    <span v-if="showUnreadDot" class="feishu-audio-unread" title="未播放" />
    <p v-if="payload?.description" class="feishu-audio-desc">{{ payload?.description }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { Message } from '@arco-design/web-vue';
import type { ContentElem } from '../../../types';
import { useMediaAccessUrl } from '../../../composables/useMediaAccessUrl';
import { toWebviewLocalMediaUrl } from '../../../utils/localMediaUrl';
import { defaultNameWithMime, downloadUrlToDevice } from '../../../utils/mediaDownload';

interface Props {
  content: ContentElem;
  isSelf: boolean;
  /** 用于对方语音未读红点（本地记录） */
  messageId?: string;
}

const props = withDefaults(defineProps<Props>(), {
  messageId: '',
});

const audioRef = ref<HTMLAudioElement | null>(null);
const playing = ref(false);
const played = ref(false);
const dlBusy = ref(false);

const payload = computed(() => {
  if (props.content.contentType !== 'audio' || !props.content.audio) return null;
  return props.content.audio;
});

function resolveMaybeLocalUrl(raw: string): string {
  return toWebviewLocalMediaUrl(String(raw ?? ''));
}

const fallbackAudioUrl = computed(() => {
  const u = String(payload.value?.source?.url ?? '').trim();
  if (u) return toWebviewLocalMediaUrl(u);
  return resolveMaybeLocalUrl(payload.value?.audioId || '');
});

const remoteFileId = computed(() => {
  const audioId = String(payload.value?.audioId ?? '').trim();
  if (!audioId || audioId.startsWith('/') || audioId.startsWith('./') || audioId.startsWith('../') || audioId.startsWith('file://')) {
    return '';
  }
  return audioId;
});

const { resolvedUrl } = useMediaAccessUrl(
  () => remoteFileId.value,
  () => fallbackAudioUrl.value,
);

const audioUrl = computed(() => resolvedUrl.value || fallbackAudioUrl.value);

const storageKey = computed(() => {
  const id = String(props.messageId || '').trim();
  return id ? `flare-im-audio-played:${id}` : '';
});

const durationLabel = computed(() => {
  const ms = payload.value?.source?.durationMs;
  if (ms == null) return '0"';
  const s = Math.max(0, Math.round(ms / 1000));
  return `${s}"`;
});

const showUnreadDot = computed(
  () => !props.isSelf && Boolean(storageKey.value) && !played.value,
);

onMounted(() => {
  if (storageKey.value && typeof localStorage !== 'undefined') {
    if (localStorage.getItem(storageKey.value) === '1') {
      played.value = true;
    }
  }
});

watch(storageKey, (k) => {
  if (!k || typeof localStorage === 'undefined') return;
  played.value = localStorage.getItem(k) === '1';
});

function markPlayed() {
  played.value = true;
  if (storageKey.value && typeof localStorage !== 'undefined') {
    localStorage.setItem(storageKey.value, '1');
  }
}

function onPlayEvt() {
  playing.value = true;
  markPlayed();
}

function onEnded() {
  playing.value = false;
  markPlayed();
}

function togglePlay() {
  const el = audioRef.value;
  if (!el || !audioUrl.value) return;
  if (el.paused) {
    void el.play().catch(() => {
      Message.error('无法播放语音');
    });
  } else {
    el.pause();
    playing.value = false;
  }
}

watch(
  () => audioUrl.value,
  () => {
    playing.value = false;
  },
);

async function onDownload() {
  const url = String(audioUrl.value ?? '').trim();
  if (!url) return;
  const id = String(payload.value?.audioId ?? 'audio').slice(0, 64);
  const mime = String(payload.value?.source?.mimeType ?? 'audio/mpeg');
  const name = defaultNameWithMime(`audio_${id}`, mime);
  dlBusy.value = true;
  try {
    await downloadUrlToDevice(url, name);
    Message.success('已开始下载');
  } catch (e) {
    console.error('[AudioView] download', e);
    Message.error(e instanceof Error ? e.message : '下载失败');
  } finally {
    dlBusy.value = false;
  }
}
</script>

<style scoped>
.feishu-audio-wrap {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  max-width: 280px;
}

.feishu-audio-bubble {
  position: relative;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  min-width: 120px;
  padding: 10px 14px;
  background: #fff;
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: 10px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.04);
}

.feishu-audio-bubble-self {
  background: #fff;
}

.feishu-audio-el {
  position: absolute;
  left: 0;
  top: 0;
  width: 0;
  height: 0;
  opacity: 0;
  pointer-events: none;
}

.feishu-audio-wave {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  color: #1d2129;
  cursor: pointer;
  border-radius: 6px;
}

.feishu-audio-wave:hover:not(:disabled) {
  background: rgba(0, 0, 0, 0.04);
}

.feishu-audio-wave:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.feishu-audio-wave-svg {
  display: block;
}

.feishu-audio-duration {
  font-size: 15px;
  font-weight: 500;
  color: #1d2129;
  letter-spacing: 0.02em;
  min-width: 2ch;
}

.feishu-audio-dl {
  margin-left: 4px;
  border: none;
  background: transparent;
  padding: 2px 4px;
  font-size: 13px;
  color: var(--feishu-primary, #3370ff);
  cursor: pointer;
}

.feishu-audio-dl:hover:not(:disabled) {
  text-decoration: underline;
}

.feishu-audio-dl:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.feishu-audio-unread {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #f53f3f;
  flex-shrink: 0;
}

.feishu-audio-desc {
  flex-basis: 100%;
  width: 100%;
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  margin: 0;
  line-height: 1.4;
}
</style>

<template>
  <div v-if="payload" class="feishu-image">
    <!-- 列表：仅缩略图（不自动 GetFileUrl 原图） -->
    <div v-if="listThumbLoading" class="feishu-image-loading">加载缩略图…</div>
    <div
      v-else-if="listShowPlaceholder"
      class="feishu-image-placeholder"
      role="button"
      tabindex="0"
      @click="onPlaceholderClick"
      @keydown.enter.prevent="onPlaceholderClick"
      @keydown.space.prevent="onPlaceholderClick"
    >
      <span class="feishu-image-placeholder-icon">🖼</span>
      <span class="feishu-image-placeholder-text">{{ thumbLoadFailed ? '图片加载失败，点击重试' : '查看原图' }}</span>
    </div>
    <img
      v-else
      :src="listImageUrl"
      :alt="payload.description || '图片'"
      loading="lazy"
      class="feishu-image-thumb"
      @click="openPreview"
      @load="onLoad"
      @error="onError"
    />
    <p v-if="payload.description" class="feishu-image-desc">{{ payload.description }}</p>

    <Teleport to="body">
      <div
        v-if="previewOpen"
        class="feishu-image-lightbox"
        role="dialog"
        aria-modal="true"
        @click.self="closePreview"
      >
        <div class="feishu-image-lightbox-toolbar">
          <button
            type="button"
            class="feishu-image-lightbox-download"
            :disabled="!fullDisplayUrl || fullLoading || downloadBusy"
            @click="downloadFullImage"
          >
            {{ downloadBusy ? '保存中…' : '下载' }}
          </button>
          <button type="button" class="feishu-image-lightbox-close" aria-label="关闭" @click="closePreview">
            ×
          </button>
        </div>
        <div class="feishu-image-lightbox-inner" @click.stop>
          <div v-if="fullLoading" class="feishu-image-lightbox-loading">加载原图中…</div>
          <img
            v-else-if="fullDisplayUrl"
            :src="fullDisplayUrl"
            :alt="payload.description || '原图'"
            class="feishu-image-lightbox-img"
          />
          <div v-else class="feishu-image-lightbox-loading">无法加载原图</div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { Message } from '@arco-design/web-vue';
import { invoke } from '@tauri-apps/api/core';
import type { ContentElem } from '../../../types';
import { pickPreferredRemoteMediaUrl, useMediaAccessUrl } from '../../../composables/useMediaAccessUrl';
import { isLikelyLocalMediaRef, stableImageMediaId } from '../../../utils/mediaRef';
import { toWebviewLocalMediaUrl } from '../../../utils/localMediaUrl';
import { defaultNameWithMime, downloadUrlToDevice } from '../../../utils/mediaDownload';

interface MediaCacheEntryPayload {
  localPath: string;
}

interface MediaResolvedPayload {
  source: string;
  localPath?: string | null;
  remote?: { url?: string; cdnUrl?: string | null; cdn_url?: string | null };
}

function pickRemoteUrl(r: MediaResolvedPayload['remote']): string {
  return pickPreferredRemoteMediaUrl(r ?? undefined);
}

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const previewOpen = ref(false);
/** 列表缩略图 <img> 加载失败时展示占位，避免只剩 description 像一条纯文字 */
const thumbLoadFailed = ref(false);
const downloadBusy = ref(false);

const payload = computed(() => {
  if (props.content.contentType !== 'image' || !props.content.image) return null;
  return props.content.image;
});

function resolveMaybeLocalUrl(raw: string): string {
  return toWebviewLocalMediaUrl(String(raw ?? ''));
}

/** 仅缩略图侧：直链或本地路径，不包含原图远程 url（避免列表偷跑原图流量） */
const thumbInlineFallback = computed(() => {
  const p = payload.value;
  if (!p) return '';
  // 协议层常把「裸绝对路径」写在 url 里，不能直接作 img.src，必须 convertFileSrc
  if (p.thumbnail?.url) return toWebviewLocalMediaUrl(String(p.thumbnail.url).trim());
  const tid = stableImageMediaId(p.thumbnail);
  if (tid && isLikelyLocalMediaRef(tid)) return resolveMaybeLocalUrl(tid);
  return '';
});

/** 乐观单文件：本地原图路径可作为列表预览（尚未上传时 source/thumbnail 同路径） */
const optimisticLocalPreview = computed(() => {
  const p = payload.value;
  if (!p) return '';
  const sid = stableImageMediaId(p.source);
  const tid = stableImageMediaId(p.thumbnail);
  if (sid && isLikelyLocalMediaRef(sid)) return resolveMaybeLocalUrl(sid);
  if (tid && isLikelyLocalMediaRef(tid)) return resolveMaybeLocalUrl(tid);
  return '';
});

/** 列表仅对「缩略图」稳定 id 发起 GetFileUrl */
const thumbRemoteFileId = computed(() => {
  const p = payload.value;
  if (!p) return '';
  const id = stableImageMediaId(p.thumbnail);
  if (!id || isLikelyLocalMediaRef(id)) return '';
  return id;
});

const {
  resolvedUrl: thumbResolvedUrl,
  loading: thumbLoading,
} = useMediaAccessUrl(
  () => thumbRemoteFileId.value,
  () => thumbInlineFallback.value || optimisticLocalPreview.value,
);

const listImageUrl = computed(() => {
  const direct = thumbInlineFallback.value || optimisticLocalPreview.value;
  return thumbResolvedUrl.value || direct;
});

watch(
  () => [listImageUrl.value, thumbRemoteFileId.value, thumbInlineFallback.value, optimisticLocalPreview.value] as const,
  () => {
    thumbLoadFailed.value = false;
  },
);

/** 远程缩略图且尚无直链/本地兜底时，仅在请求进行中显示加载（避免失败时永远转圈） */
const listThumbLoading = computed(() => {
  if (!thumbRemoteFileId.value) return false;
  if (thumbInlineFallback.value || optimisticLocalPreview.value) return false;
  return thumbLoading.value;
});

/** 列表无可展示地址但存在远程资源时：占位，点击再走大图逻辑（含仅原图 id、缩略图拉取失败） */
const listShowPlaceholder = computed(() => {
  const p = payload.value;
  if (!p) return false;
  if (listThumbLoading.value) return false;
  if (thumbLoadFailed.value) return true;
  if (String(listImageUrl.value ?? '').trim()) return false;
  const src = stableImageMediaId(p.source);
  const th = stableImageMediaId(p.thumbnail);
  const hasRemote =
    (src && !isLikelyLocalMediaRef(src)) || (th && !isLikelyLocalMediaRef(th));
  return hasRemote;
});

/** 大图：仅原图稳定 id（无则退化用缩略图 id） */
const fullRemoteFileId = computed(() => {
  const p = payload.value;
  if (!p) return '';
  const src = stableImageMediaId(p.source);
  if (src && !isLikelyLocalMediaRef(src)) return src;
  return thumbRemoteFileId.value;
});

const fullInlineFallback = computed(() => {
  const p = payload.value;
  if (!p) return '';
  if (p.source?.url) return toWebviewLocalMediaUrl(String(p.source.url).trim());
  const sid = stableImageMediaId(p.source);
  if (sid && isLikelyLocalMediaRef(sid)) return resolveMaybeLocalUrl(sid);
  return thumbInlineFallback.value || optimisticLocalPreview.value;
});

const fullDisplayUrl = ref('');
const fullLoading = ref(false);

watch(previewOpen, async (open) => {
  if (!open) {
    fullDisplayUrl.value = '';
    fullLoading.value = false;
    return;
  }
  const fb = fullInlineFallback.value;
  const id = fullRemoteFileId.value;
  fullLoading.value = true;
  fullDisplayUrl.value = '';
  try {
    if (!id || isLikelyLocalMediaRef(id)) {
      fullDisplayUrl.value = fb || resolveMaybeLocalUrl(id);
      return;
    }
    try {
      const entry = await invoke<MediaCacheEntryPayload>('sdk_cache_remote_media', {
        fileId: id,
        expiresIn: 3600,
      });
      const p = String(entry?.localPath ?? '').trim();
      fullDisplayUrl.value = p ? resolveMaybeLocalUrl(p) : fb;
    } catch {
      const r = await invoke<MediaResolvedPayload>('sdk_resolve_media_access', {
        fileId: id,
        expiresIn: 3600,
      });
      const src = String(r?.source ?? '').toLowerCase();
      if (src === 'local' && r.localPath) {
        fullDisplayUrl.value = resolveMaybeLocalUrl(r.localPath);
      } else {
        const ru = pickRemoteUrl(r.remote);
        fullDisplayUrl.value = ru || fb;
      }
    }
  } finally {
    fullLoading.value = false;
  }
});

function onPlaceholderClick() {
  thumbLoadFailed.value = false;
  openPreview();
}

function openPreview() {
  previewOpen.value = true;
}

function closePreview() {
  previewOpen.value = false;
}

function imageDownloadBaseName(): string {
  const p = payload.value;
  if (!p) return 'image';
  const id = stableImageMediaId(p.source) || stableImageMediaId(p.thumbnail) || 'image';
  return `image_${id.slice(0, 48)}`;
}

async function downloadFullImage() {
  const url = String(fullDisplayUrl.value ?? '').trim();
  if (!url) {
    Message.warning('暂无可下载的原图');
    return;
  }
  const p = payload.value;
  const mime = String(p?.source?.mimeType ?? p?.thumbnail?.mimeType ?? 'image/jpeg');
  const name = defaultNameWithMime(imageDownloadBaseName(), mime);
  downloadBusy.value = true;
  try {
    await downloadUrlToDevice(url, name);
    Message.success('已开始下载');
  } catch (e) {
    console.error('[ImageView] download', e);
    Message.error(e instanceof Error ? e.message : '下载失败');
  } finally {
    downloadBusy.value = false;
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && previewOpen.value) {
    e.preventDefault();
    closePreview();
  }
}

watch(previewOpen, (open) => {
  if (typeof document === 'undefined') return;
  if (open) {
    document.addEventListener('keydown', onKeydown);
    document.body.style.overflow = 'hidden';
  } else {
    document.removeEventListener('keydown', onKeydown);
    document.body.style.overflow = '';
  }
});

onBeforeUnmount(() => {
  if (typeof document === 'undefined') return;
  document.removeEventListener('keydown', onKeydown);
  document.body.style.overflow = '';
});

function onLoad(_e: Event) {
  thumbLoadFailed.value = false;
}
function onError(_e: Event) {
  thumbLoadFailed.value = true;
}
</script>

<style scoped>
.feishu-image {
  border-radius: var(--feishu-radius, 8px);
  overflow: hidden;
  max-width: 280px;
}

.feishu-image-thumb {
  max-width: 100%;
  max-height: 320px;
  display: block;
  cursor: zoom-in;
  vertical-align: top;
}

.feishu-image-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 120px;
  padding: 16px;
  cursor: pointer;
  background: var(--feishu-bg-card, #f2f3f5);
  color: var(--feishu-text-secondary, #86909c);
  border: 1px dashed var(--feishu-border, #e5e6eb);
  user-select: none;
}

.feishu-image-placeholder:focus {
  outline: 2px solid var(--feishu-primary, #165dff);
  outline-offset: 2px;
}

.feishu-image-placeholder-icon {
  font-size: 28px;
  line-height: 1;
  opacity: 0.85;
}

.feishu-image-placeholder-text {
  font-size: 13px;
}

.feishu-image-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
  padding: 16px;
  font-size: 13px;
  color: var(--feishu-text-secondary, #86909c);
  background: var(--feishu-bg-card, #f2f3f5);
}

.feishu-image-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
  margin: 6px 0 0;
  line-height: 1.4;
}

.feishu-image-lightbox {
  position: fixed;
  inset: 0;
  z-index: 10000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.82);
  padding: 48px 24px 24px;
  box-sizing: border-box;
}

.feishu-image-lightbox-toolbar {
  position: absolute;
  top: 12px;
  left: 16px;
  right: 16px;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  pointer-events: none;
}

.feishu-image-lightbox-toolbar > button {
  pointer-events: auto;
}

.feishu-image-lightbox-download {
  border: none;
  border-radius: 8px;
  padding: 8px 16px;
  font-size: 14px;
  cursor: pointer;
  background: rgba(255, 255, 255, 0.16);
  color: #fff;
}

.feishu-image-lightbox-download:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.26);
}

.feishu-image-lightbox-download:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.feishu-image-lightbox-close {
  width: 40px;
  height: 40px;
  margin-left: auto;
  border: none;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
  font-size: 28px;
  line-height: 1;
  cursor: pointer;
  flex-shrink: 0;
}

.feishu-image-lightbox-close:hover {
  background: rgba(255, 255, 255, 0.2);
}

.feishu-image-lightbox-inner {
  max-width: 100%;
  max-height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.feishu-image-lightbox-img {
  max-width: 100%;
  max-height: calc(100vh - 96px);
  object-fit: contain;
  border-radius: 4px;
}

.feishu-image-lightbox-loading {
  color: #fff;
  font-size: 14px;
  padding: 24px;
}
</style>

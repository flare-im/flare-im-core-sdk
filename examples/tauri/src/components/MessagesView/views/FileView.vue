<template>
  <div class="feishu-file">
    <div class="feishu-file-card">
      <span class="feishu-file-icon">📄</span>
      <div class="feishu-file-info">
        <span class="feishu-file-name">{{ payload.fileName || '文件' }}</span>
        <span v-if="payload.fileSize" class="feishu-file-size">{{ formatSize(payload.fileSize) }}</span>
      </div>
      <div class="feishu-file-actions">
        <button type="button" class="feishu-file-action feishu-file-dl" :disabled="busy" @click.stop="onDownload">
          {{ busy ? '…' : '下载' }}
        </button>
        <button
          type="button"
          class="feishu-file-action feishu-file-open"
          :disabled="!fileUrl"
          title="在默认应用中打开"
          @click.stop="onOpen"
        >
          →
        </button>
      </div>
    </div>
    <p v-if="payload.description" class="feishu-file-desc">{{ payload.description }}</p>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { Message } from '@arco-design/web-vue';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { ContentElem } from '../../../types';
import { useMediaAccessUrl } from '../../../composables/useMediaAccessUrl';
import { toWebviewLocalMediaUrl } from '../../../utils/localMediaUrl';
import { downloadUrlToDevice, sanitizeDownloadFileName } from '../../../utils/mediaDownload';

interface Props {
  content: ContentElem;
  isSelf: boolean;
}

const props = defineProps<Props>();

const busy = ref(false);

const payload = computed(() => {
  if (props.content.contentType !== 'file' || !props.content.file) {
    return { fileId: '', fileName: '', fileSize: 0, url: '', description: '', mimeType: '' };
  }
  const f = props.content.file;
  return {
    ...f,
    mimeType: String(f.mimeType ?? ''),
  };
});

function resolveMaybeLocalUrl(raw: string): string {
  return toWebviewLocalMediaUrl(String(raw ?? ''));
}

const fallbackFileUrl = computed(() => {
  const u = String(payload.value.url ?? '').trim();
  if (u) return toWebviewLocalMediaUrl(u);
  return resolveMaybeLocalUrl(payload.value.fileId || '');
});

const remoteFileId = computed(() => {
  const fileId = String(payload.value.fileId ?? '').trim();
  if (!fileId) return '';
  if (
    fileId.startsWith('/') ||
    fileId.startsWith('./') ||
    fileId.startsWith('../') ||
    fileId.toLowerCase().startsWith('file://') ||
    /^[A-Za-z]:[\\/]/.test(fileId) ||
    fileId.startsWith('\\\\')
  ) {
    return '';
  }
  return fileId;
});

const { resolvedUrl } = useMediaAccessUrl(
  () => remoteFileId.value,
  () => fallbackFileUrl.value,
);

const fileUrl = computed(() => resolvedUrl.value || fallbackFileUrl.value);

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

async function onDownload() {
  const url = String(fileUrl.value ?? '').trim();
  if (!url) {
    Message.warning('文件地址不可用');
    return;
  }
  const name = sanitizeDownloadFileName(payload.value.fileName || 'file', 'file');
  busy.value = true;
  try {
    await downloadUrlToDevice(url, name);
    Message.success('已开始下载');
  } catch (e) {
    console.error('[FileView] download', e);
    Message.error(e instanceof Error ? e.message : '下载失败');
  } finally {
    busy.value = false;
  }
}

async function onOpen() {
  const url = String(fileUrl.value ?? '').trim();
  if (!url) return;
  try {
    await openUrl(url);
  } catch (e) {
    console.error('[FileView] open', e);
    Message.error('无法打开文件');
  }
}
</script>

<style scoped>
.feishu-file {
  max-width: 320px;
}

.feishu-file-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  background: #fff;
  border: 1px solid var(--feishu-border, #e5e6eb);
  border-radius: var(--feishu-radius, 8px);
  color: inherit;
}

.feishu-file-icon {
  font-size: 28px;
  flex-shrink: 0;
}

.feishu-file-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feishu-file-name {
  font-size: 14px;
  color: var(--feishu-text-primary, #1d2129);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.feishu-file-size {
  font-size: 12px;
  color: var(--feishu-text-secondary, #86909c);
}

.feishu-file-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.feishu-file-action {
  border: none;
  background: transparent;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 6px;
  font-size: 14px;
  line-height: 1.2;
}

.feishu-file-dl {
  color: var(--feishu-primary, #3370ff);
}

.feishu-file-dl:hover:not(:disabled) {
  background: rgba(51, 112, 255, 0.08);
}

.feishu-file-open {
  color: var(--feishu-primary, #3370ff);
  font-weight: 600;
  min-width: 28px;
}

.feishu-file-open:hover:not(:disabled) {
  background: rgba(51, 112, 255, 0.08);
}

.feishu-file-action:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.feishu-file-desc {
  font-size: 12px;
  color: var(--feishu-text-secondary);
  margin: 6px 0 0;
}
</style>

<script setup lang="ts">
import { defineProps, defineEmits, ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { IconBold, IconItalic, IconCode, IconH1, IconUnorderedList, IconOrderedList, IconLink, IconFaceSmileFill, IconSend, IconQuote, IconAlignLeft, IconDown, IconImage, IconSound, IconFile } from '@arco-design/web-vue/es/icon'
import { renderMarkdown } from '../utils/markdown'

const props = defineProps<{
  activeSessionId: string | null,
  modelValue: string,
  mediaSending?: boolean,
  mediaSendingLabel?: string | null,
  mediaProgressPercent?: number | null,
  editingMessageId?: string | null,
  targetName?: string | null,
  replyingToLabel?: string | null,
  replyingToPreview?: string | null,
  replyingToMessageId?: string | null,
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: string): void
  (e: 'send'): void
  (e: 'sendMedia', kind: 'imageOrVideo' | 'audio' | 'file'): void
  (e: 'cancelEdit'): void
  (e: 'cancelReply'): void
  (e: 'typing', action: 'typing' | 'stop'): void
}>()

const isComposing = ref(false)
const showEmoji = ref(false)
const showMedia = ref(false)
const showPreview = ref(false)
const showMarkdownTools = ref(false)
const isMarkdownMode = ref(false)
const isMultiline = ref(false)
const composerContainerRef = ref<HTMLElement | null>(null)
const previewHtml = computed(() => renderMarkdown(props.modelValue || ''))
const hasContent = computed(() => !!props.modelValue.trim())

const emojiList = ['👍','❤️','😂','😮','😢','😡','👏','🎉','💯','👀','🚀','🤔','👌','✅']

function onInput(v: string) { emit('update:modelValue', v) }
function onFocus() { if (props.activeSessionId) emit('typing', 'typing') }
function onBlur() { if (props.activeSessionId) emit('typing', 'stop') }
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    if (e.shiftKey) return
    if (isComposing.value) return
    e.preventDefault()
    if (props.modelValue.trim()) emit('send')
  }
}
function onCompositionStart() { isComposing.value = true }
function onCompositionEnd() { isComposing.value = false }
function onSend() { if (props.modelValue.trim()) emit('send') }

function insertAtCursor(prefix: string, suffix: string = '') {
  const text = props.modelValue || ''
  const next = text + (text && !text.endsWith('\n') ? '\n' : '') + prefix + suffix
  emit('update:modelValue', next)
}

function fmtBold() { insertAtCursor('**粗体**') }
function fmtItalic() { insertAtCursor('*斜体*') }
function fmtCode() { insertAtCursor('```lang\n代码块\n```') }
function fmtH1() { insertAtCursor('# 标题') }
function fmtUL() { insertAtCursor('- 列表项') }
function fmtOL() { insertAtCursor('1. 列表项') }
function fmtQuote() { insertAtCursor('> 引用内容') }
function fmtLink() { insertAtCursor('[链接文本](https://example.com)') }

function insertEmoji(emoji: string) {
  const next = (props.modelValue || '') + emoji
  emit('update:modelValue', next)
  showEmoji.value = false
}

function onSendMedia(kind: 'imageOrVideo' | 'audio' | 'file') {
  showMedia.value = false
  emit('sendMedia', kind)
}

function togglePreview() { showPreview.value = !showPreview.value }
function toggleMarkdownMode() {
  isMarkdownMode.value = !isMarkdownMode.value
  if (!isMarkdownMode.value) showMarkdownTools.value = false
}

const placeholderText = computed(() => {
  const target = (props.targetName || '').trim()
  if (props.editingMessageId) return '编辑消息...'
  return target ? `发送给 ${target}` : '输入消息...'
})

function updateLineMode() {
  nextTick(() => {
    const textarea = composerContainerRef.value?.querySelector('textarea')
    if (!textarea) {
      isMultiline.value = (props.modelValue || '').includes('\n')
      return
    }
    const hasLineBreak = (props.modelValue || '').includes('\n')
    const multilineByHeight = textarea.scrollHeight > 42
    isMultiline.value = hasLineBreak || multilineByHeight
  })
}

watch(() => props.modelValue, () => updateLineMode(), { flush: 'post' })
onMounted(() => {
  updateLineMode()
  window.addEventListener('resize', updateLineMode)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', updateLineMode)
})
</script>

<template>
  <div class="enhanced-composer">
    <div
      ref="composerContainerRef"
      class="composer-container"
      :class="{ 'composer-multiline': isMultiline, 'composer-singleline': !isMultiline }"
    >
      <div v-if="props.replyingToMessageId" class="reply-context">
        <a-button
          type="text"
          size="mini"
          class="reply-close-btn"
          @click="emit('cancelReply')"
        >
          ×
        </a-button>
        <div class="reply-context-main">
          <div class="reply-context-title">
            回复 {{ props.replyingToLabel || '对方' }}:
            <span class="reply-context-preview">{{ props.replyingToPreview || '[消息]' }}</span>
          </div>
        </div>
      </div>
      <div v-if="isMarkdownMode" class="markdown-toolbar">
        <a-button type="text" size="mini" class="tb-btn" title="粗体" @click="fmtBold"><icon-bold /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="斜体" @click="fmtItalic"><icon-italic /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="代码块" @click="fmtCode"><icon-code /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="一级标题" @click="fmtH1"><icon-h1 /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="无序列表" @click="fmtUL"><icon-unordered-list /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="有序列表" @click="fmtOL"><icon-ordered-list /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="引用" @click="fmtQuote"><icon-quote /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="链接" @click="fmtLink"><icon-link /></a-button>
        <a-button type="text" size="mini" class="tb-btn" title="预览" @click="togglePreview"><icon-align-left /></a-button>
      </div>
      <div class="input-area" :class="{ 'input-area-markdown': isMarkdownMode }">
        <a-textarea
          :model-value="props.modelValue"
          :placeholder="placeholderText"
          allow-clear
          :auto-size="{ minRows: 1, maxRows: 6 }"
          :disabled="!props.activeSessionId"
          class="message-input"
          :class="{ 'editing-mode': props.editingMessageId, 'markdown-mode': isMarkdownMode }"
          @keydown="onKeydown"
          @compositionstart="onCompositionStart"
          @compositionend="onCompositionEnd"
          @update:model-value="onInput"
          @focus="onFocus"
          @blur="onBlur"
        />
      </div>

      <div class="bottom-actions">
        <div class="left-actions" />
        <div class="right-actions">
          <a-button
            type="text"
            size="small"
            class="action-btn md-btn"
            :class="{ 'is-active': isMarkdownMode }"
            title="Markdown 模式"
            @click="toggleMarkdownMode"
          >
              Aa
          </a-button>

          <a-popover position="top" v-model:popup-visible="showEmoji">
            <a-button type="text" size="small" class="action-btn" title="表情"><icon-face-smile-fill /></a-button>
            <template #content>
              <div class="emoji-grid">
                <button v-for="e in emojiList" :key="e" class="emoji-item" @click="insertEmoji(e)">{{ e }}</button>
              </div>
            </template>
          </a-popover>

          <a-button type="text" size="small" class="action-btn" title="提及">@</a-button>
          <a-button type="text" size="small" class="action-btn" title="剪贴">✂</a-button>
          <a-popover
            position="top"
            trigger="click"
            content-class="enhanced-composer-media-pop"
            v-model:popup-visible="showMedia"
          >
            <a-button
              type="text"
              size="small"
              class="action-btn"
              title="发送媒体"
              :disabled="!props.activeSessionId || !!props.mediaSending"
            >
              ＋
            </a-button>
            <template #content>
              <div class="media-menu" role="menu">
                <button
                  type="button"
                  class="media-menu-item"
                  role="menuitem"
                  :disabled="!!props.mediaSending"
                  @click="onSendMedia('imageOrVideo')"
                >
                  <span class="media-menu-icon" aria-hidden="true"><icon-image /></span>
                  <span class="media-menu-label">图片/视频</span>
                </button>
                <button
                  type="button"
                  class="media-menu-item"
                  role="menuitem"
                  :disabled="!!props.mediaSending"
                  @click="onSendMedia('audio')"
                >
                  <span class="media-menu-icon" aria-hidden="true"><icon-sound /></span>
                  <span class="media-menu-label">音频</span>
                </button>
                <button
                  type="button"
                  class="media-menu-item"
                  role="menuitem"
                  :disabled="!!props.mediaSending"
                  @click="onSendMedia('file')"
                >
                  <span class="media-menu-icon" aria-hidden="true"><icon-file /></span>
                  <span class="media-menu-label">文件</span>
                </button>
              </div>
            </template>
          </a-popover>
          <a-button type="text" size="small" class="action-btn" title="展开">↗</a-button>

          <a-button
            v-if="props.editingMessageId"
            size="small"
            class="cancel-edit-btn"
            @click="emit('cancelEdit')"
          >
            取消编辑
          </a-button>
          <a-button
            type="primary"
            size="small"
            class="send-btn"
            :class="{ 'send-btn-active': hasContent }"
            :disabled="!props.activeSessionId || !props.modelValue.trim()"
            @click="onSend"
          >
            <icon-send />
            <span class="send-label">{{ props.editingMessageId ? '保存' : '发送' }}</span>
            <icon-down class="send-down" />
          </a-button>
        </div>
      </div>
    </div>

    <div v-if="showPreview && props.modelValue" class="md-preview">
      <div class="md-preview-inner" v-html="previewHtml"></div>
    </div>

    <div v-if="props.activeSessionId" class="composer-tips">
      <span v-if="props.editingMessageId" class="tip-text editing-tip">
        ✏️ 正在编辑 | Enter 保存，Shift+Enter 换行
      </span>
      <span v-else-if="props.mediaSending" class="tip-text media-sending-tip">
        ⏳ {{ props.mediaSendingLabel || '正在处理媒体，请稍候...' }}
        <template v-if="props.mediaProgressPercent != null">
          （{{ Math.max(0, Math.min(100, props.mediaProgressPercent)) }}%）
        </template>
      </span>
      <span v-else class="tip-text">Shift + Enter 换行，支持 Markdown</span>
    </div>
  </div>
</template>

<style scoped>
.enhanced-composer { width: 100%; display: flex; flex-direction: column; gap: 0; }
.composer-container {
  border: 1px solid #b8bcc6;
  border-radius: 10px;
  background: #f7f8fa;
  display: flex;
  flex-direction: column;
  position: relative;
  transition: border-color 0.2s, background-color 0.2s;
  padding: 8px 10px 6px;
}

.reply-context {
  display: flex;
  align-items: center;
  gap: 8px;
  border-bottom: 1px solid var(--wechat-divider, #E5E5E5);
  padding: 6px 10px;
  background: #fafbfc;
  border-radius: 10px 10px 0 0;
}

.reply-close-btn {
  color: #8a919f !important;
  min-width: 22px;
  width: 22px;
  height: 22px;
  padding: 0 !important;
  border-radius: 4px;
}

.reply-close-btn:hover {
  color: #57606a !important;
  background: rgba(0, 0, 0, 0.06);
}

.reply-context-main {
  min-width: 0;
  flex: 1;
}

.reply-context-title {
  color: #6b7280;
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.reply-context-preview {
  color: #8a919f;
  font-weight: 600;
}
.composer-container:focus-within {
  border-color: #b8bcc6;
  box-shadow: none;
}
.input-area {
  padding: 0;
  background: transparent;
}
.input-area-markdown {
  background: transparent;
  border-radius: 0;
}
.markdown-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 8px 0 8px;
  background: transparent;
  border-radius: 0;
}
/* 覆盖 Arco Textarea 默认样式，移除边框和背景，使其融入容器 */
.message-input :deep(.arco-textarea-wrapper) {
  background-color: transparent !important;
  border: none !important;
  padding: 0 10px;
  font-size: 14px;
  line-height: 1.35;
  box-shadow: none !important;
  border-radius: 4px;
  min-height: 40px;
}
.message-input :deep(.arco-textarea-wrapper:hover),
.message-input :deep(.arco-textarea-wrapper:focus-within) {
  background-color: transparent !important;
  border: none !important;
  box-shadow: none !important;
}
.message-input :deep(.arco-textarea-wrapper::before),
.message-input :deep(.arco-textarea-wrapper::after) {
  border: none !important;
  box-shadow: none !important;
}
.message-input :deep(.arco-textarea) {
  border: none !important;
  outline: none !important;
  box-shadow: none !important;
  background: transparent !important;
}
.message-input :deep(textarea) {
  border: none !important;
  outline: none !important;
  box-shadow: none !important;
  background: transparent !important;
  color: #2d313a !important;
  padding-top: 10px !important;
  padding-bottom: 10px !important;
}
.message-input :deep(textarea:focus) {
  border: none !important;
  outline: none !important;
  box-shadow: none !important;
}
.message-input :deep(.arco-textarea-focus) {
  border: none !important;
  box-shadow: none !important;
}
.message-input :deep(.arco-textarea-wrapper.arco-textarea-focus) {
  border: none !important;
  outline: none !important;
  box-shadow: none !important;
}
.message-input.markdown-mode :deep(textarea) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
}
.bottom-actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  padding: 0;
  gap: 8px;
}
.left-actions {
  flex: 1;
}
.right-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
.tb-btn { padding: 2px 6px !important; color: var(--wechat-text-secondary,#888); transition: all .2s ease; }
.tb-btn:hover { color: var(--wechat-primary,#07C160); background: rgba(7,193,96,.1); }
.md-tools { display: flex; align-items: center; gap: 2px; max-width: 340px; flex-wrap: wrap; }
.emoji-grid { display:grid; grid-template-columns: repeat(8,1fr); gap:6px; max-width:280px; }
.emoji-item { background:transparent; border:1px solid var(--wechat-divider,#E5E5E5); border-radius:6px; cursor:pointer; line-height:28px; }
.action-btn { min-width: 24px; padding: 2px 4px; color: #606774; border-radius: 4px; }
.md-btn { font-weight: 500; }
.md-btn.is-active {
  color: #2f6ff8;
  background: rgba(47, 111, 248, 0.12);
}
.action-btn:hover { color: #39404c; background: rgba(0,0,0,0.05); }
.send-btn {
  background-color: #eef0f4;
  border-color: #eef0f4;
  color:#9ca3af;
  border-radius: 6px;
  height: 30px;
  padding: 0 12px;
}
.send-btn-active {
  background-color: #2f6ff8 !important;
  border-color: #2f6ff8 !important;
  color: #fff !important;
}
.send-label { margin-left: 2px; }
.send-down { margin-left: 2px; font-size: 10px; opacity: 0.85; }
.composer-singleline .input-area {
  padding-right: 366px;
}
.composer-singleline .input-area-markdown {
  padding-right: 366px;
}
.composer-singleline .bottom-actions {
  position: absolute;
  right: 14px;
  top: 50%;
  transform: translateY(-50%);
  padding: 0;
  gap: 4px;
  background: transparent;
}
.composer-singleline .right-actions {
  gap: 0;
}
.composer-singleline .action-btn {
  min-width: 24px;
}
.composer-singleline .send-btn {
  height: 28px;
}
.composer-multiline .bottom-actions {
  position: static;
  transform: none;
  margin-top: 10px;
}
.composer-multiline .input-area {
  padding: 0;
}
.md-preview { margin-top:8px; background:#fff; border:1px solid var(--wechat-divider,#E5E5E5); border-radius:8px; padding:8px; }
.md-preview-inner { font-size:14px; line-height:1.6; }
.composer-tips { margin-top:4px; text-align:right; padding-right: 8px; }
.tip-text { font-size:12px; color: var(--wechat-text-secondary,#888); opacity:.8; }
.media-sending-tip {
  color: #2f6ff8;
  opacity: 1;
}
.media-menu {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 176px;
  padding: 2px 0;
}
.media-menu-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  margin: 0;
  padding: 10px 12px;
  border: none;
  border-radius: 8px;
  background: transparent;
  cursor: pointer;
  font-size: 14px;
  color: #1d2129;
  text-align: left;
  line-height: 1.4;
  transition: background-color 0.15s ease;
}
.media-menu-item:hover:not(:disabled) {
  background: #f2f3f5;
}
.media-menu-item:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.media-menu-icon {
  display: inline-flex;
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  align-items: center;
  justify-content: center;
  color: #4e5969;
}
.media-menu-icon :deep(svg) {
  width: 20px;
  height: 20px;
}
.media-menu-label {
  flex: 1;
  min-width: 0;
}
@media (prefers-color-scheme: dark) {
  .composer-container { background: #1A1A1A; border-color: #333; }
  .reply-context {
    background: #24262a;
    border-bottom-color: #333;
  }
  .input-area-markdown,
  .markdown-toolbar {
    background: transparent;
  }
  .reply-context-title {
    color: #b7bec8;
  }
  .reply-context-preview {
    color: #8f98a4;
  }
  .md-preview { background:#1A1A1A; border-color:#333; }
  .tb-btn { color:#999; }
  .tb-btn:hover { background: rgba(7,193,96,.2); }
  .action-btn { color: #999; }
  .action-btn:hover { color: #fff; background: rgba(255,255,255,0.1); }
  .send-btn {
    background: #2b62d6;
    border-color: #2b62d6;
  }
  .media-menu-item {
    color: #e5e7eb;
  }
  .media-menu-item:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
  .media-menu-icon {
    color: #9ca3af;
  }
}
</style>

<style>
/* Popover 内容 teleport 到 body，外壳样式用全局类名 */
.enhanced-composer-media-pop.arco-popover-popup-content {
  padding: 6px 4px !important;
  border-radius: 10px !important;
  box-shadow: 0 4px 14px rgba(15, 23, 42, 0.08) !important;
  border: 1px solid #e5e6eb !important;
  background: #fff !important;
}
.enhanced-composer-media-pop .arco-popover-title {
  display: none !important;
}
@media (prefers-color-scheme: dark) {
  .enhanced-composer-media-pop.arco-popover-popup-content {
    background: #1e2024 !important;
    border-color: #3f444d !important;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.45) !important;
  }
}
</style>

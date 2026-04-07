<script setup lang="ts">
import { defineProps, defineEmits, ref, computed } from "vue";
import { IconFaceSmileFill, IconFolderAdd, IconSend } from '@arco-design/web-vue/es/icon';
import MarkdownIt from "markdown-it";

const props = defineProps<{
  activeSessionId: string | null;
  modelValue: string;
  editingMessageId?: string | null;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", v: string): void;
  (e: "send"): void;
  (e: "typing", action: "typing" | "stop"): void;
}>();

const isComposing = ref(false);
const showEmoji = ref(false);
const showPreview = ref(false);
const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
const previewHtml = computed(() => md.render(props.modelValue || ""));
const emojiList = [
  "😀","😁","😂","🤣","😊","😍","👍","🙏","🎉","🔥","💡","✅","❌","💬","🚀",
  "😉","😎","😢","😭","🤔","😮","😴","😇","🤝","📎","📷"
];

// 输入处理
function onInput(v: string) { 
  emit("update:modelValue", v); 
}

// 焦点处理
function onFocus() { 
  if (props.activeSessionId) emit("typing", "typing"); 
}

function onBlur() { 
  if (props.activeSessionId) emit("typing", "stop"); 
}

// 键盘事件处理
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Enter") {
    if (e.shiftKey) return; // Shift+Enter 换行
    if (isComposing.value) return; // 输入法组合中
    
    e.preventDefault();
    if (props.modelValue.trim()) {
      emit("send");
    }
  }
}

// 输入法事件处理
function onCompositionStart() {
  isComposing.value = true;
}

function onCompositionEnd() {
  isComposing.value = false;
}

// 发送按钮点击
function onSend() {
  if (props.modelValue.trim()) {
    emit("send");
  }
}

function insertEmoji(emoji: string) {
  const next = (props.modelValue || "") + emoji;
  emit("update:modelValue", next);
  showEmoji.value = false;
}

function togglePreview() { showPreview.value = !showPreview.value; }
</script>

<template>
  <div class="composer">
    <div class="composer-container">
      <!-- 输入区域 -->
      <div class="input-wrapper">
        <a-textarea 
          :model-value="props.modelValue" 
          :placeholder="props.editingMessageId ? '编辑消息...' : '输入消息...'" 
          allow-clear 
          :auto-size="{ minRows: 1, maxRows: 4 }" 
          :disabled="!props.activeSessionId"
          class="message-input"
          :class="{ 'editing-mode': props.editingMessageId }"
          @keydown="onKeydown"
          @compositionstart="onCompositionStart"
          @compositionend="onCompositionEnd"
          @update:model-value="onInput" 
          @focus="onFocus" 
          @blur="onBlur"
        />
        
        <!-- 输入状态指示器 -->
        <div v-if="props.activeSessionId && props.modelValue" class="typing-indicator">
          <span class="typing-text">正在输入...</span>
        </div>
      </div>
      
      <!-- 操作按钮区域 -->
      <div class="actions-wrapper">
        <a-space>
          <!-- 表情按钮 -->
          <a-popover position="top" v-model:popup-visible="showEmoji">
            <a-button 
              type="text" 
              size="small" 
              class="action-btn"
              title="表情"
            >
              <icon-face-smile-fill />
            </a-button>
            <template #content>
              <div class="emoji-grid">
                <button v-for="e in emojiList" :key="e" class="emoji-item" @click="insertEmoji(e)">{{ e }}</button>
              </div>
            </template>
          </a-popover>
          
          <!-- 文件按钮 -->
          <a-button 
            type="text" 
            size="small" 
            class="action-btn"
            title="发送文件"
          >
            <icon-folder-add />
          </a-button>
          
          <!-- 发送/保存按钮 -->
          <a-button 
            type="primary" 
            size="small"
            class="send-btn"
            :disabled="!props.activeSessionId || !props.modelValue.trim()"
            @click="onSend"
          >
            <icon-send />
            {{ props.editingMessageId ? '保存' : '发送' }}
          </a-button>
          <!-- Markdown 预览 -->
          <a-button type="text" size="small" class="action-btn" title="Markdown 预览" @click="togglePreview">
            MD
          </a-button>
        </a-space>
      </div>
    </div>
    
    <!-- Markdown 预览面板 -->
    <div v-if="showPreview && props.modelValue" class="md-preview">
      <div class="md-preview-inner" v-html="previewHtml"></div>
    </div>

    <!-- 快捷提示 -->
    <div v-if="props.activeSessionId" class="composer-tips">
      <span v-if="props.editingMessageId" class="tip-text editing-tip">
        <span class="editing-indicator">✏️ 正在编辑消息</span>
        <span class="tip-separator">|</span>
        <span>Enter 保存，Shift+Enter 换行</span>
      </span>
      <span v-else class="tip-text">Enter 发送，Shift+Enter 换行</span>
    </div>
  </div>
</template>

<style scoped>
.composer {
  width: 100%;
}

.composer-container {
  display: flex;
  align-items: flex-end;
  gap: var(--spacing-sm, 8px);
  background-color: #FFFFFF;
  border: 1px solid var(--wechat-divider, #E5E5E5);
  border-radius: var(--radius-lg, 12px);
  padding: var(--spacing-sm, 8px);
  box-shadow: var(--shadow-card, 0 2px 8px rgba(0,0,0,0.1));
  transition: all 0.2s ease;
}

.composer-container:focus-within {
  border-color: var(--wechat-primary, #07C160);
  box-shadow: 0 0 0 2px rgba(7, 193, 96, 0.1);
}

.input-wrapper {
  flex: 1;
  position: relative;
}

.message-input {
  border: none !important;
  background: transparent !important;
  padding: 0 !important;
  font-size: var(--font-size-sm, 14px);
  line-height: var(--line-height, 1.4);
  color: var(--wechat-text-primary, #000000);
  resize: none !important;
}

.message-input :deep(textarea) {
  border: none !important;
  box-shadow: none !important;
  background: transparent !important;
  padding: 0 !important;
  font-size: var(--font-size-sm, 14px);
  line-height: var(--line-height, 1.4);
  color: var(--wechat-text-primary, #000000);
}

.message-input :deep(textarea:focus) {
  border: none !important;
  box-shadow: none !important;
}

.message-input :deep(textarea::placeholder) {
  color: var(--wechat-text-secondary, #888888);
}

.message-input.editing-mode :deep(textarea) {
  border-left: 3px solid var(--wechat-primary, #07C160) !important;
  padding-left: 8px !important;
}

.editing-tip {
  display: flex;
  align-items: center;
  gap: 8px;
}

.editing-indicator {
  color: var(--wechat-primary, #07C160);
  font-weight: 500;
}

.tip-separator {
  color: var(--wechat-text-secondary, #888888);
  opacity: 0.5;
}

.typing-indicator {
  position: absolute;
  bottom: -20px;
  left: 0;
  font-size: var(--font-size-xs, 12px);
  color: var(--wechat-text-secondary, #888888);
  animation: fadeIn 0.2s ease-out;
}

.typing-text {
  display: inline-block;
  padding: 2px 6px;
  background-color: rgba(0, 0, 0, 0.05);
  border-radius: var(--radius-sm, 4px);
}

.actions-wrapper {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs, 4px);
  flex-shrink: 0;
}

.emoji-grid {
  display: grid;
  grid-template-columns: repeat(8, 1fr);
  gap: 6px;
  max-width: 280px;
}

.emoji-item {
  background: transparent;
  border: 1px solid var(--wechat-divider, #E5E5E5);
  border-radius: 6px;
  cursor: pointer;
  line-height: 28px;
}

.action-btn {
  color: var(--wechat-text-secondary, #888888);
  transition: all 0.2s ease;
  padding: 4px 8px !important;
}

.action-btn:hover {
  color: var(--wechat-primary, #07C160);
  background-color: rgba(7, 193, 96, 0.1);
  transform: scale(1.1);
}

.send-btn {
  background-color: var(--wechat-primary, #07C160);
  border-color: var(--wechat-primary, #07C160);
  color: #FFFFFF;
  font-weight: 500;
  padding: 4px 12px !important;
  transition: all 0.2s ease;
}

.send-btn:hover:not(:disabled) {
  background-color: #06A850;
  border-color: #06A850;
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(7, 193, 96, 0.3);
}

.send-btn:disabled {
  background-color: #CCCCCC;
  border-color: #CCCCCC;
  color: #FFFFFF;
  cursor: not-allowed;
}

.composer-tips {
  margin-top: var(--spacing-xs, 4px);
  text-align: center;
}

.md-preview {
  margin-top: 8px;
  background: #FFFFFF;
  border: 1px solid var(--wechat-divider, #E5E5E5);
  border-radius: 8px;
  padding: 8px;
}

.md-preview-inner {
  font-size: 14px;
  line-height: 1.6;
}

.tip-text {
  font-size: var(--font-size-xs, 12px);
  color: var(--wechat-text-secondary, #888888);
  opacity: 0.7;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .composer-container {
    background-color: #1A1A1A;
    border-color: var(--wechat-divider, #2C2C2C);
  }
  
  .message-input :deep(textarea) {
    color: var(--wechat-text-primary, #FFFFFF);
  }
  
  .message-input :deep(textarea::placeholder) {
    color: var(--wechat-text-secondary, #999999);
  }
  
  .typing-text {
    background-color: rgba(255, 255, 255, 0.1);
  }
  
  .action-btn {
    color: var(--wechat-text-secondary, #999999);
  }
  
  .action-btn:hover {
    color: var(--wechat-primary, #07C160);
    background-color: rgba(7, 193, 96, 0.2);
  }
  
  .tip-text {
    color: var(--wechat-text-secondary, #999999);
  }

  .md-preview {
    background: #1A1A1A;
    border-color: var(--wechat-divider, #2C2C2C);
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .composer-container {
    padding: var(--spacing-xs, 6px);
    gap: var(--spacing-xs, 6px);
  }
  
  .actions-wrapper {
    gap: 2px;
  }
  
  .action-btn {
    padding: 2px 6px !important;
  }
  
  .send-btn {
    padding: 2px 8px !important;
  }
  
  .composer-tips {
    display: none;
  }
}

/* 动画效果 */
@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(-5px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* 性能优化 */
.composer-container,
.action-btn,
.send-btn {
  will-change: transform;
  backface-visibility: hidden;
}
</style>

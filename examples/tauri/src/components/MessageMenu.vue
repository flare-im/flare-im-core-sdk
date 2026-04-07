<template>
  <a-dropdown
    :trigger="triggerList"
    @select="handleSelect"
    position="br"
  >
    <div class="message-menu-trigger">
      <slot />
    </div>
    <template #content>
      <!-- 回复 -->
      <a-doption
        v-if="!isSelf && !isRecalled"
        value="reply"
      >
        <template #icon>
          <icon-reply />
        </template>
        回复
      </a-doption>

      <!-- 转发 -->
      <a-doption
        value="forward"
        v-if="!isRecalled"
      >
        <template #icon>
          <icon-forward />
        </template>
        转发
      </a-doption>

      <!-- 分隔线 -->
      <div v-if="canEditAsPlainText" class="menu-separator" />

      <!-- 编辑 -->
      <a-doption
        v-if="canEditAsPlainText"
        value="edit"
      >
        <template #icon>
          <icon-edit />
        </template>
        编辑
      </a-doption>

      <!-- 分隔线 -->
      <div v-if="!isRecalled" class="menu-separator" />

      <!-- 置顶 -->
      <a-doption
        value="pin"
        v-if="!isRecalled && !isPinned"
      >
        <template #icon>
          <icon-pushpin :style="{ color: '#E53935' }" />
        </template>
        置顶
      </a-doption>

      <!-- 取消置顶 -->
      <a-doption
        value="unpin"
        v-if="isPinned && !isRecalled"
      >
        <template #icon>
          <icon-pushpin :style="{ color: '#E53935' }" />
        </template>
        取消置顶
      </a-doption>

      <!-- 分隔线 -->
      <div v-if="!isRecalled" class="menu-separator" />

      <!-- 标记 -->
      <a-doption
        value="mark"
        v-if="!isRecalled"
      >
        <template #icon>
          <icon-tag />
        </template>
        标记
      </a-doption>

      <!-- 重要 -->
      <a-doption
        value="mark-important"
        v-if="!isRecalled"
        :class="{ 'mark-active': markType === 1 }"
      >
        <template #icon>
          <div class="mark-icon-wrapper">
            <icon-check-circle-fill v-if="markType === 1" :style="{ color: '#000000', fontSize: '16px' }" />
            <icon-check-circle v-else :style="{ color: '#999999', fontSize: '16px' }" />
          </div>
        </template>
        <div class="mark-option-content">
          <icon-exclamation-circle :style="{ color: '#f53f3f', fontSize: '16px', marginRight: '4px' }" />
          <span>重要</span>
        </div>
      </a-doption>

      <!-- 待办 -->
      <a-doption
        value="mark-todo"
        v-if="!isRecalled"
        :class="{ 'mark-active': markType === 2 }"
      >
        <template #icon>
          <div class="mark-icon-wrapper">
            <icon-check-circle-fill v-if="markType === 2" :style="{ color: '#000000', fontSize: '16px' }" />
            <icon-check-circle v-else :style="{ color: '#999999', fontSize: '16px' }" />
          </div>
        </template>
        <div class="mark-option-content">
          <icon-clock-circle :style="{ color: markType === 2 ? '#ff7d00' : '#999999', fontSize: '16px', marginRight: '4px' }" />
          <span>待办</span>
        </div>
      </a-doption>

      <!-- 已处理 -->
      <a-doption
        value="mark-done"
        v-if="!isRecalled"
        :class="{ 'mark-active': markType === 3 }"
      >
        <template #icon>
          <div class="mark-icon-wrapper">
            <icon-check-circle-fill v-if="markType === 3" :style="{ color: '#000000', fontSize: '16px' }" />
            <icon-check-circle v-else :style="{ color: '#999999', fontSize: '16px' }" />
          </div>
        </template>
        <div class="mark-option-content">
          <icon-check-circle-fill :style="{ color: '#00b42a', fontSize: '16px', marginRight: '4px' }" />
          <span>已处理</span>
        </div>
      </a-doption>

      <!-- 分隔线 -->
      <div class="menu-separator" />

      <!-- 删除 -->
      <a-doption
        value="delete"
        class="danger-option"
        v-if="!isRecalled"
      >
        <template #icon>
          <icon-delete :style="{ color: '#E53935' }" />
        </template>
        删除
      </a-doption>
    </template>
  </a-dropdown>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { Message } from '../types';
import { asRecord } from '../utils/message';
import { getMessageContent, isMessageFromSelf } from '../utils/message';
import {
  IconReply,
  IconForward,
  IconEdit,
  IconDelete,
  IconPushpin,
  IconTag,
  IconExclamationCircle,
  IconClockCircle,
  IconCheckCircle,
  IconCheckCircleFill,
} from '@arco-design/web-vue/es/icon';

interface Props {
  message: Message;
  currentUserId: string | null;
  /** 触发方式：click 点击、contextmenu 右键、both 两者 */
  trigger?: 'click' | 'contextmenu' | 'both';
}

const props = withDefaults(defineProps<Props>(), {
  trigger: 'click'
});

const triggerList = computed(() => {
  if (props.trigger === 'contextmenu') return ['contextmenu'];
  if (props.trigger === 'both') return ['click', 'contextmenu'];
  return ['click'];
});

const emit = defineEmits<{
  (e: 'reply', messageId: string): void;
  (e: 'forward', messageId: string): void;
  (e: 'edit', message: Message): void;
  (e: 'recall', messageId: string, reason?: string): void;
  (e: 'pin', messageId: string): void;
  (e: 'unpin', messageId: string): void;
  (e: 'mark', messageId: string, markType: number, color?: string): void;
  (e: 'unmark', messageId: string, markType: number): void;
  (e: 'delete', messageId: string, canDeleteForEveryone: boolean): void;
}>();

const isSelf = computed(() => {
  return isMessageFromSelf(asRecord(props.message), props.currentUserId);
});

/** 与后端 `edit_text` 一致：仅纯文本类（含 Markdown / 富文本类型号） */
const EDITABLE_MESSAGE_TYPES = new Set([1, 30, 31]);

const canEditAsPlainText = computed(() => {
  if (!isSelf.value || isRecalled.value) return false;
  const m = asRecord(props.message);
  const mt = Number(m.messageType ?? m.message_type ?? 0);
  if (EDITABLE_MESSAGE_TYPES.has(mt)) return true;
  const content = getMessageContent(m);
  const ct = content?.contentType;
  return ct === 'text' || ct === 'markdown' || ct === 'richText';
});

const isRecalled = computed(() => {
  const m = asRecord(props.message);
  return !!(m.isRecalled ?? m.is_recalled);
});

const isPinned = computed(() => {
  const msg = asRecord(props.message);
  const extraPinned = (msg.extra as Record<string, string> | undefined)?.pinned;
  const attrPinned = props.message.attributes?.pinned;
  return extraPinned === 'true' || attrPinned === 'true';
});

const markType = computed(() => {
  const msg = asRecord(props.message);
  const extra = (msg.extra as Record<string, string> | undefined) || {};
  const fromExtra = Number(extra.mark_type ?? '');
  if (Number.isFinite(fromExtra) && fromExtra > 0) return fromExtra;
  const attrs = props.message.attributes || {};
  if (attrs['mark:important'] === 'true') return 1;
  if (attrs['mark:todo'] === 'true') return 2;
  if (attrs['mark:done'] === 'true') return 3;
  return null;
});

function handleSelect(value: string) {
  console.log("[MessageMenu] handleSelect", {
    value,
    message: props.message,
  });
  // 统一使用 clientMsgId（IMMessage camelCase）或 serverId，兼容 snake_case
  const msg = asRecord(props.message);
  const clientMsgId = String(msg.clientMsgId ?? msg.client_msg_id ?? msg.serverId ?? msg.server_id ?? '');
  console.log("[MessageMenu] clientMsgId", clientMsgId);
  switch (value) {
    case 'reply':
      emit('reply', clientMsgId);
      break;
    case 'forward':
      emit('forward', clientMsgId);
      break;
    case 'edit':
      emit('edit', props.message);
      break;
    case 'recall':
      emit('recall', clientMsgId, undefined); // 传递undefined让Chat.vue处理原因输入
      break;
    case 'pin':
      emit('pin', clientMsgId);
      break;
    case 'unpin':
      emit('unpin', clientMsgId);
      break;
    case 'mark':
      // 标记菜单项，不做操作，仅作为分组标题
      break;
    case 'mark-important':
      if (markType.value === 1) emit('unmark', clientMsgId, 1);
      else emit('mark', clientMsgId, 1, '#FF0000');
      break;
    case 'mark-todo':
      if (markType.value === 2) emit('unmark', clientMsgId, 2);
      else emit('mark', clientMsgId, 2, '#FFA500');
      break;
    case 'mark-done':
      if (markType.value === 3) emit('unmark', clientMsgId, 3);
      else emit('mark', clientMsgId, 3);
      break;
    case 'delete':
      emit('delete', clientMsgId, isSelf.value);
      break;
  }
}
</script>

<style scoped>
/* 由子节点（气泡）撑开尺寸；勿用 height:100% — 父级多为 auto，会塌成 0 高，导致自己发的消息只剩页脚时间戳可见 */
.message-menu-trigger {
  width: max-content;
  max-width: 100%;
  height: auto;
  min-height: 0;
  display: block;
}

/* 删除选项样式 */
.danger-option {
  color: #f53f3f !important;
}

.danger-option :deep(.arco-dropdown-option-icon) {
  color: #f53f3f !important;
}

/* 标记激活状态样式 - 浅灰色背景 */
.mark-active {
  background-color: rgba(0, 0, 0, 0.04) !important;
}

.mark-active:hover {
  background-color: rgba(0, 0, 0, 0.08) !important;
}

/* 优化下拉菜单整体样式 */
:deep(.arco-dropdown) {
  min-width: 160px;
  padding: 2px 0;
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

/* 优化菜单项样式 */
:deep(.arco-dropdown-option) {
  padding: 2px 10px;
  min-height: 30px;
  line-height: 1.2;
  font-size: 14px;
}

/* 优化图标样式 */
:deep(.arco-dropdown-option-icon) {
  margin-right: 6px;
  font-size: 15px;
}

/* 自定义分隔线 */
.menu-separator {
  height: 1px;
  margin: 2px 0;
  background: var(--color-neutral-3, #eceff3);
}

/* 标记选项内容布局 */
.mark-option-content {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

/* 标记选中状态的勾选符号 */
.mark-check {
  font-size: 14px;
  font-weight: 600;
}

/* 标记图标包装器 */
.mark-icon-wrapper {
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
</style>

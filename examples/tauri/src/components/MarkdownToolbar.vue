<template>
  <div class="markdown-toolbar">
    <div class="toolbar-group">
      <!-- 基础格式 -->
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('bold')"
        :class="{ active: isActive('bold') }"
        title="加粗 (Ctrl+B)"
      >
        <icon-font name="bold" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('italic')"
        :class="{ active: isActive('italic') }"
        title="斜体 (Ctrl+I)"
      >
        <icon-font name="italic" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('strikethrough')"
        :class="{ active: isActive('strikethrough') }"
        title="删除线 (Ctrl+D)"
      >
        <icon-font name="strikethrough" />
      </a-button>
      
      <a-divider direction="vertical" class="toolbar-divider" />
    </div>

    <div class="toolbar-group">
      <!-- 标题 -->
      <a-dropdown @select="handleHeadingSelect" trigger="click">
        <a-button
          class="toolbar-btn"
          size="mini"
          type="text"
          title="标题"
        >
          <icon-font name="heading" />
        </a-button>
        <template #content>
          <a-doption value="1">一级标题</a-doption>
          <a-doption value="2">二级标题</a-doption>
          <a-doption value="3">三级标题</a-doption>
          <a-doption value="4">四级标题</a-doption>
          <a-doption value="5">五级标题</a-doption>
          <a-doption value="6">六级标题</a-doption>
        </template>
      </a-dropdown>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('quote')"
        :class="{ active: isActive('quote') }"
        title="引用 (Ctrl+Q)"
      >
        <icon-font name="quote" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('code')"
        :class="{ active: isActive('code') }"
        title="代码 (Ctrl+E)"
      >
        <icon-font name="code" />
      </a-button>
      
      <a-divider direction="vertical" class="toolbar-divider" />
    </div>

    <div class="toolbar-group">
      <!-- 列表 -->
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('unorderedList')"
        :class="{ active: isActive('unorderedList') }"
        title="无序列表 (Ctrl+L)"
      >
        <icon-font name="unordered-list" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('orderedList')"
        :class="{ active: isActive('orderedList') }"
        title="有序列表 (Ctrl+Shift+L)"
      >
        <icon-font name="ordered-list" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('taskList')"
        :class="{ active: isActive('taskList') }"
        title="任务列表 (Ctrl+T)"
      >
        <icon-font name="task-list" />
      </a-button>
      
      <a-divider direction="vertical" class="toolbar-divider" />
    </div>

    <div class="toolbar-group">
      <!-- 链接和媒体 -->
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('link')"
        :class="{ active: isActive('link') }"
        title="链接 (Ctrl+K)"
      >
        <icon-font name="link" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('image')"
        :class="{ active: isActive('image') }"
        title="图片 (Ctrl+Shift+I)"
      >
        <icon-font name="image" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('table')"
        :class="{ active: isActive('table') }"
        title="表格 (Ctrl+Shift+T)"
      >
        <icon-font name="table" />
      </a-button>
      
      <a-divider direction="vertical" class="toolbar-divider" />
    </div>

    <div class="toolbar-group">
      <!-- 分割线 -->
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('horizontalRule')"
        title="分割线 (Ctrl+H)"
      >
        <icon-font name="minus" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="format('footnote')"
        title="脚注"
      >
        <icon-font name="info-circle" />
      </a-button>
      
      <a-divider direction="vertical" class="toolbar-divider" />
    </div>

    <div class="toolbar-group">
      <!-- 预览切换 -->
      <a-button
        class="toolbar-btn preview-btn"
        size="mini"
        type="text"
        @click="togglePreview"
        :class="{ active: previewMode }"
        title="预览模式"
      >
        <icon-font name="eye" />
      </a-button>
      
      <a-button
        class="toolbar-btn"
        size="mini"
        type="text"
        @click="clearContent"
        title="清空内容"
      >
        <icon-font name="delete" />
      </a-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { defineEmits, defineProps } from 'vue';

defineProps({
  previewMode: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits({
  format: (_action: string) => true,
  togglePreview: () => true,
  clearContent: () => true
});

// 图标字体组件
const IconFont = {
  props: {
    name: String
  },
  template: `<i :class="'iconfont icon-' + name"></i>`
};

// 格式化操作
const format = (action: string) => {
  emit('format', action);
};

// 标题选择
const handleHeadingSelect = (_value: string) => {
  emit('format', 'heading');
};

// 切换预览模式
const togglePreview = () => {
  emit('togglePreview');
};

// 清空内容
const clearContent = () => {
  emit('clearContent');
};

// 检查是否激活状态（简化版）
const isActive = (_action: string): boolean => {
  // 这里可以根据当前选中的文本内容来判断
  // 暂时返回false，后续可以扩展
  return false;
};
</script>

<style scoped>
.markdown-toolbar {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  background: var(--color-bg-2);
  border: 1px solid var(--color-border-2);
  border-radius: 8px 8px 0 0;
  gap: 4px;
  flex-wrap: wrap;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.toolbar-btn {
  width: 32px;
  height: 32px;
  padding: 0;
  border-radius: 6px;
  transition: all 0.2s ease;
  border: 1px solid transparent;
}

.toolbar-btn:hover {
  background: var(--color-fill-2);
  border-color: var(--color-border-2);
  transform: translateY(-1px);
}

.toolbar-btn.active {
  background: var(--color-primary-light-1);
  border-color: var(--color-primary-light-2);
  color: var(--color-primary-6);
}

.toolbar-btn.preview-btn.active {
  background: var(--color-success-light-1);
  border-color: var(--color-success-light-2);
  color: var(--color-success-6);
}

.toolbar-divider {
  height: 20px;
  margin: 0 4px;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .markdown-toolbar {
    background: var(--color-bg-3);
    border-color: var(--color-border-3);
  }
  
  .toolbar-btn:hover {
    background: var(--color-fill-3);
    border-color: var(--color-border-3);
  }
  
  .toolbar-btn.active {
    background: rgba(22, 93, 255, 0.2);
    border-color: rgba(22, 93, 255, 0.4);
  }
  
  .toolbar-btn.preview-btn.active {
    background: rgba(7, 193, 96, 0.2);
    border-color: rgba(7, 193, 96, 0.4);
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .markdown-toolbar {
    padding: 6px 8px;
    gap: 2px;
  }
  
  .toolbar-btn {
    width: 28px;
    height: 28px;
  }
  
  .toolbar-divider {
    margin: 0 2px;
  }
}

/* 图标样式 */
.iconfont {
  font-size: 14px;
  line-height: 1;
}

/* 飞书风格动画 */
.toolbar-btn {
  position: relative;
  overflow: hidden;
}

.toolbar-btn::before {
  content: '';
  position: absolute;
  top: 50%;
  left: 50%;
  width: 0;
  height: 0;
  background: rgba(22, 93, 255, 0.1);
  border-radius: 50%;
  transform: translate(-50%, -50%);
  transition: width 0.3s, height 0.3s;
}

.toolbar-btn:hover::before {
  width: 100%;
  height: 100%;
}

.toolbar-btn:active {
  transform: translateY(0);
  transition: transform 0.1s;
}
</style>
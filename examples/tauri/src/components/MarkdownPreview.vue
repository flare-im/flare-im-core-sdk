<template>
  <div class="markdown-preview">
    <div class="preview-header">
      <div class="preview-title">
        <icon-font name="eye" />
        <span>预览</span>
      </div>
      <div class="preview-stats">
        <span class="stat-item">
          <icon-font name="edit" />
          {{ wordCount }} 字
        </span>
        <span class="stat-item">
          <icon-font name="clock" />
          {{ readingTime }} 分钟
        </span>
      </div>
    </div>
    
    <div class="preview-content" v-html="renderedContent"></div>
    
    <div v-if="!content" class="preview-empty">
      <icon-font name="edit" class="empty-icon" />
      <p>开始输入内容，预览将在这里显示</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, watch } from 'vue';
import { renderMarkdown, addCopyButtons, setupCodeCopy, countWords, estimateReadingTime } from '../utils/markdown';

interface Props {
  content: string;
  showStats?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  showStats: true
});

// 渲染内容
const renderedContent = computed(() => {
  if (!props.content) return '';
  return renderMarkdown(props.content);
});

// 字数统计
const wordCount = computed(() => {
  return countWords(props.content);
});

// 阅读时间
const readingTime = computed(() => {
  return estimateReadingTime(props.content);
});

// 图标字体组件
const IconFont = {
  props: {
    name: String
  },
  template: `<i :class="'iconfont icon-' + name"></i>`
};

// 监听内容变化，添加复制按钮
watch(() => props.content, () => {
  setTimeout(() => {
    addCopyButtons();
  }, 100);
});

onMounted(() => {
  setupCodeCopy();
  if (props.content) {
    addCopyButtons();
  }
});
</script>

<style scoped>
.markdown-preview {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--color-bg-1);
  border: 1px solid var(--color-border-2);
  border-radius: 0 0 8px 8px;
  overflow: hidden;
}

.preview-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  background: var(--color-bg-2);
  border-bottom: 1px solid var(--color-border-2);
  flex-shrink: 0;
}

.preview-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 500;
  color: var(--color-text-1);
}

.preview-stats {
  display: flex;
  align-items: center;
  gap: 12px;
}

.stat-item {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--color-text-3);
}

.preview-content {
  flex: 1;
  padding: 16px;
  overflow-y: auto;
  line-height: 1.6;
  color: var(--color-text-1);
}

.preview-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--color-text-3);
  gap: 12px;
}

.empty-icon {
  font-size: 48px;
  opacity: 0.3;
}

.preview-empty p {
  margin: 0;
  font-size: 14px;
}

/* 飞书风格的Markdown样式 */
.preview-content :deep(h1) {
  font-size: 24px;
  font-weight: 600;
  margin: 16px 0 12px;
  color: var(--color-text-1);
  border-bottom: 2px solid var(--color-border-2);
  padding-bottom: 8px;
}

.preview-content :deep(h2) {
  font-size: 20px;
  font-weight: 600;
  margin: 14px 0 10px;
  color: var(--color-text-1);
}

.preview-content :deep(h3) {
  font-size: 18px;
  font-weight: 600;
  margin: 12px 0 8px;
  color: var(--color-text-1);
}

.preview-content :deep(h4),
.preview-content :deep(h5),
.preview-content :deep(h6) {
  font-size: 16px;
  font-weight: 600;
  margin: 10px 0 6px;
  color: var(--color-text-1);
}

.preview-content :deep(p) {
  margin: 8px 0;
  line-height: 1.6;
}

.preview-content :deep(a) {
  color: #165dff;
  text-decoration: none;
  transition: color 0.2s;
}

.preview-content :deep(a:hover) {
  color: #0e42ba;
  text-decoration: underline;
}

.preview-content :deep(ul),
.preview-content :deep(ol) {
  margin: 8px 0;
  padding-left: 24px;
}

.preview-content :deep(li) {
  margin: 4px 0;
  line-height: 1.5;
}

.preview-content :deep(blockquote) {
  margin: 12px 0;
  padding: 8px 16px;
  background: var(--color-fill-2);
  border-left: 4px solid #165dff;
  border-radius: 4px;
  color: var(--color-text-2);
}

.preview-content :deep(code) {
  background: var(--color-fill-2);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: 'Monaco', 'Consolas', 'monospace';
  font-size: 0.9em;
  color: var(--color-text-1);
}

.preview-content :deep(pre) {
  margin: 12px 0;
  padding: 16px;
  background: var(--color-bg-3);
  border-radius: 8px;
  overflow-x: auto;
  position: relative;
  border: 1px solid var(--color-border-2);
}

.preview-content :deep(pre code) {
  background: none;
  padding: 0;
  border-radius: 0;
  font-size: 14px;
  line-height: 1.4;
}

.preview-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 12px 0;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  overflow: hidden;
}

.preview-content :deep(th),
.preview-content :deep(td) {
  padding: 12px 16px;
  text-align: left;
  border-bottom: 1px solid var(--color-border-2);
}

.preview-content :deep(th) {
  background: var(--color-fill-2);
  font-weight: 600;
  color: var(--color-text-1);
}

.preview-content :deep(tr:last-child td) {
  border-bottom: none;
}

.preview-content :deep(tr:hover) {
  background: var(--color-fill-1);
}

.preview-content :deep(hr) {
  margin: 16px 0;
  border: none;
  border-top: 1px solid var(--color-border-2);
}

.preview-content :deep(img) {
  max-width: 100%;
  height: auto;
  border-radius: 8px;
  margin: 8px 0;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
}

.preview-content :deep(.task-list-item) {
  list-style: none;
  margin-left: -20px;
}

.preview-content :deep(.task-list-item-checkbox) {
  margin-right: 8px;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .markdown-preview {
    background: var(--color-bg-3);
    border-color: var(--color-border-3);
  }
  
  .preview-header {
    background: var(--color-bg-4);
    border-color: var(--color-border-3);
  }
  
  .preview-content :deep(a) {
    color: #4e8fff;
  }
  
  .preview-content :deep(a:hover) {
    color: #165dff;
  }
  
  .preview-content :deep(blockquote) {
    background: var(--color-fill-3);
    border-left-color: #4e8fff;
  }
  
  .preview-content :deep(code) {
    background: var(--color-fill-3);
  }
  
  .preview-content :deep(pre) {
    background: var(--color-bg-4);
    border-color: var(--color-border-3);
  }
  
  .preview-content :deep(table) {
    border-color: var(--color-border-3);
  }
  
  .preview-content :deep(th),
  .preview-content :deep(td) {
    border-color: var(--color-border-3);
  }
  
  .preview-content :deep(th) {
    background: var(--color-fill-3);
  }
  
  .preview-content :deep(tr:hover) {
    background: var(--color-fill-2);
  }
  
  .preview-content :deep(hr) {
    border-top-color: var(--color-border-3);
  }
}

/* 代码复制按钮样式 */
.preview-content :deep(.copy-code-btn) {
  position: absolute;
  top: 8px;
  right: 8px;
  background: rgba(0, 0, 0, 0.6);
  color: white;
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s;
}

.preview-content :deep(pre:hover .copy-code-btn) {
  opacity: 1;
}

.preview-content :deep(.copy-code-btn:hover) {
  background: rgba(0, 0, 0, 0.8);
}

/* 滚动条样式 */
.preview-content::-webkit-scrollbar {
  width: 6px;
}

.preview-content::-webkit-scrollbar-track {
  background: transparent;
}

.preview-content::-webkit-scrollbar-thumb {
  background: var(--color-border-2);
  border-radius: 3px;
}

.preview-content::-webkit-scrollbar-thumb:hover {
  background: var(--color-border-3);
}
</style>
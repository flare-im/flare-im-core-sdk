import MarkdownIt from 'markdown-it';
import hljs from 'highlight.js';
import clipboard from 'clipboard';
import taskLists from 'markdown-it-task-lists';
import toc from 'markdown-it-table-of-contents';
import footnote from 'markdown-it-footnote';
import mark from 'markdown-it-mark';
import anchor from 'markdown-it-anchor';

// 创建markdown-it实例，配置语法高亮和插件
export const md: any = new MarkdownIt({
  html: false,              // 禁用HTML标签（安全考虑）
  linkify: true,            // 自动识别链接
  breaks: true,             // 转换换行符为<br>
  highlight: function (str: string, lang: string) {
    if (lang && hljs.getLanguage(lang)) {
      try {
        return `<pre class="hljs"><code class="language-${lang}">${hljs.highlight(str, { language: lang }).value}</code></pre>`;
      } catch (__) {}
    }
    return `<pre class="hljs"><code>${hljs.highlightAuto(str).value}</code></pre>`;
  }
});

// 添加自定义插件
md.use(taskLists);
md.use(toc);
md.use(footnote);
md.use(mark);
md.use(anchor);

// 渲染Markdown内容
export function renderMarkdown(content: string): string {
  return md.render(content);
}

// 代码复制功能
export function setupCodeCopy() {
  // 初始化clipboard.js
  const clipboardInstance = new clipboard('.copy-code-btn', {
    text: function(trigger) {
      const codeBlock = trigger.closest('pre')?.querySelector('code');
      return codeBlock?.textContent || '';
    }
  });

  clipboardInstance.on('success', function(e) {
    const originalText = e.trigger.textContent;
    e.trigger.textContent = '已复制!';
    setTimeout(() => {
      e.trigger.textContent = originalText;
    }, 2000);
  });

  return clipboardInstance;
}

// 添加代码复制按钮到代码块
export function addCopyButtons() {
  const codeBlocks = document.querySelectorAll('pre code');
  codeBlocks.forEach((codeBlock) => {
    const pre = codeBlock.parentElement;
    if (pre && !pre.querySelector('.copy-code-btn')) {
      const copyBtn = document.createElement('button');
      copyBtn.className = 'copy-code-btn';
      copyBtn.textContent = '复制';
      copyBtn.style.cssText = `
        position: absolute;
        top: 8px;
        right: 8px;
        background: rgba(0,0,0,0.6);
        color: white;
        border: none;
        border-radius: 4px;
        padding: 4px 8px;
        font-size: 12px;
        cursor: pointer;
        opacity: 0;
        transition: opacity 0.2s;
      `;
      
      pre.style.position = 'relative';
      pre.appendChild(copyBtn);
      
      pre.addEventListener('mouseenter', () => {
        copyBtn.style.opacity = '1';
      });
      
      pre.addEventListener('mouseleave', () => {
        copyBtn.style.opacity = '0';
      });
    }
  });
}

// 检测内容类型
export function detectContentType(content: string): 'PlainText' | 'Markdown' {
  // 简单的Markdown检测逻辑
  const markdownPatterns = [
    /^#{1,6}\s+/m,           // 标题
    /^\*\s+/m,               // 无序列表
    /^\d+\.\s+/m,            // 有序列表
    /```[\s\S]*?```/,        // 代码块
    /\[.*?\]\(.*?\)/,         // 链接
    /\*\*.*?\*\*/,            // 粗体
    /\*.*?\*/,                // 斜体
    /^>\s+/m,                // 引用
    /^\|.*\|.*$/m            // 表格
  ];
  
  const hasMarkdown = markdownPatterns.some(pattern => pattern.test(content));
  return hasMarkdown ? 'Markdown' : 'PlainText';
}

// 判定是否为 Markdown 内容
export function isMarkdown(content: string): boolean {
  return detectContentType(content) === 'Markdown';
}

// 清理HTML标签（安全考虑）
export function sanitizeHtml(html: string): string {
  const div = document.createElement('div');
  div.innerHTML = html;
  
  // 移除危险的标签和属性
  const dangerousTags = ['script', 'style', 'iframe', 'object', 'embed'];
  dangerousTags.forEach(tag => {
    const elements = div.querySelectorAll(tag);
    elements.forEach(el => el.remove());
  });
  
  // 移除危险属性
  const allElements = div.querySelectorAll('*');
  allElements.forEach(el => {
    const attrs = Array.from(el.attributes);
    attrs.forEach(attr => {
      if (attr.name.startsWith('on') || attr.name === 'href' && attr.value.startsWith('javascript:')) {
        el.removeAttribute(attr.name);
      }
    });
  });
  
  return div.innerHTML;
}

// 字数统计
export function countWords(text: string): number {
  return text.trim().split(/\s+/).filter(word => word.length > 0).length;
}

// 字符统计
export function countCharacters(text: string): number {
  return text.length;
}

// 阅读时间估算（分钟）
export function estimateReadingTime(text: string): number {
  const wordsPerMinute = 200;
  const wordCount = countWords(text);
  return Math.max(1, Math.ceil(wordCount / wordsPerMinute));
}

// 统一对外：设置代码复制监听（添加按钮并绑定事件）
export function setupCodeCopyListeners() {
  addCopyButtons();
  setupCodeCopy();
}

export class MarkdownFormatter {
  private textarea: HTMLTextAreaElement | null = null;

  constructor(textarea: HTMLTextAreaElement) {
    this.textarea = textarea;
  }

  // 获取选中的文本
  private getSelection(): { start: number; end: number; text: string } {
    if (!this.textarea) return { start: 0, end: 0, text: '' };
    
    const start = this.textarea.selectionStart;
    const end = this.textarea.selectionEnd;
    const text = this.textarea.value.substring(start, end);
    
    return { start, end, text };
  }

  // 替换选中的文本
  private replaceSelection(before: string, after: string = ''): void {
    if (!this.textarea) return;
    
    const { start, end, text } = this.getSelection();
    const newText = before + text + after;
    const newValue = this.textarea.value.substring(0, start) + newText + this.textarea.value.substring(end);
    
    this.textarea.value = newValue;
    this.textarea.focus();
    
    // 设置光标位置
    const newCursorPos = start + before.length + text.length;
    this.textarea.setSelectionRange(newCursorPos, newCursorPos);
    
    // 触发input事件
    this.textarea.dispatchEvent(new Event('input', { bubbles: true }));
  }

  // 在光标处插入文本
  private insertAtCursor(text: string): void {
    if (!this.textarea) return;
    
    const { start } = this.getSelection();
    const newValue = this.textarea.value.substring(0, start) + text + this.textarea.value.substring(start);
    
    this.textarea.value = newValue;
    this.textarea.focus();
    
    // 设置光标位置
    const newCursorPos = start + text.length;
    this.textarea.setSelectionRange(newCursorPos, newCursorPos);
    
    // 触发input事件
    this.textarea.dispatchEvent(new Event('input', { bubbles: true }));
  }

  // 加粗
  bold(): void {
    this.replaceSelection('**', '**');
  }

  // 斜体
  italic(): void {
    this.replaceSelection('*', '*');
  }

  // 删除线
  strikethrough(): void {
    this.replaceSelection('~~', '~~');
  }

  // 标题
  heading(level: number = 1): void {
    const prefix = '#'.repeat(Math.max(1, Math.min(6, level))) + ' ';
    this.replaceSelection(prefix, '');
  }

  // 引用
  quote(): void {
    const lines = this.getSelection().text.split('\n');
    const quotedLines = lines.map(line => line.trim() ? '> ' + line : '> ').join('\n');
    this.replaceSelection(quotedLines, '');
  }

  // 代码
  code(): void {
    const { text } = this.getSelection();
    if (text.includes('\n')) {
      // 多行代码块
      this.replaceSelection('```\n', '\n```');
    } else {
      // 行内代码
      this.replaceSelection('`', '`');
    }
  }

  // 链接
  link(): void {
    const { text } = this.getSelection();
    if (text) {
      this.replaceSelection('[', '](url)');
    } else {
      this.insertAtCursor('[链接文本](url)');
    }
  }

  // 图片
  image(): void {
    const { text } = this.getSelection();
    if (text) {
      this.replaceSelection('![', '](image-url)');
    } else {
      this.insertAtCursor('![图片描述](image-url)');
    }
  }

  // 无序列表
  unorderedList(): void {
    const lines = this.getSelection().text.split('\n').filter(line => line.trim());
    if (lines.length > 0) {
      const listItems = lines.map(line => '- ' + line).join('\n');
      this.replaceSelection(listItems, '');
    } else {
      this.insertAtCursor('- 列表项');
    }
  }

  // 有序列表
  orderedList(): void {
    const lines = this.getSelection().text.split('\n').filter(line => line.trim());
    if (lines.length > 0) {
      const listItems = lines.map((line, index) => `${index + 1}. ${line}`).join('\n');
      this.replaceSelection(listItems, '');
    } else {
      this.insertAtCursor('1. 列表项');
    }
  }

  // 任务列表
  taskList(): void {
    const lines = this.getSelection().text.split('\n').filter(line => line.trim());
    if (lines.length > 0) {
      const listItems = lines.map(line => '- [ ] ' + line).join('\n');
      this.replaceSelection(listItems, '');
    } else {
      this.insertAtCursor('- [ ] 任务项');
    }
  }

  // 表格
  table(rows: number = 3, cols: number = 3): void {
    let table = '';
    
    // 表头
    const headerRow = Array(cols).fill('表头').map((h, i) => `| ${h}${i + 1} `).join('') + '|\n';
    table += headerRow;
    
    // 分隔符
    const separatorRow = Array(cols).fill('| --- ').join('') + '|\n';
    table += separatorRow;
    
    // 数据行
    for (let i = 0; i < rows - 1; i++) {
      const dataRow = Array(cols).fill('数据').map((d, j) => `| ${d}${i + 1}-${j + 1} `).join('') + '|\n';
      table += dataRow;
    }
    
    this.insertAtCursor('\n' + table);
  }

  // 水平分割线
  horizontalRule(): void {
    this.insertAtCursor('\n---\n');
  }

  // 脚注
  footnote(): void {
    const { text } = this.getSelection();
    if (text) {
      this.replaceSelection('[^1]', '');
      this.insertAtCursor('\n\n[^1]: 脚注内容');
    } else {
      this.insertAtCursor('需要脚注的文本[^1]\n\n[^1]: 脚注内容');
    }
  }

  // 上标
  superscript(): void {
    this.replaceSelection('^', '^');
  }

  // 下标
  subscript(): void {
    this.replaceSelection('~', '~');
  }

  // 标记
  mark(): void {
    this.replaceSelection('==', '==');
  }

  // 获取当前内容
  getContent(): string {
    return this.textarea?.value || '';
  }

  // 设置内容
  setContent(content: string): void {
    if (!this.textarea) return;
    this.textarea.value = content;
    this.textarea.dispatchEvent(new Event('input', { bubbles: true }));
  }

  // 清空内容
  clear(): void {
    if (!this.textarea) return;
    this.textarea.value = '';
    this.textarea.dispatchEvent(new Event('input', { bubbles: true }));
  }

  // 撤销
  undo(): void {
    if (!this.textarea) return;
    this.textarea.dispatchEvent(new KeyboardEvent('keydown', { 
      key: 'z', 
      ctrlKey: true,
      bubbles: true 
    }));
  }

  // 重做
  redo(): void {
    if (!this.textarea) return;
    this.textarea.dispatchEvent(new KeyboardEvent('keydown', { 
      key: 'y', 
      ctrlKey: true,
      bubbles: true 
    }));
  }
}

// 创建格式化器实例的工厂函数
export function createMarkdownFormatter(textarea: HTMLTextAreaElement): MarkdownFormatter {
  return new MarkdownFormatter(textarea);
}

// 键盘快捷键映射
export const markdownShortcuts = {
  'Ctrl+b': 'bold',
  'Ctrl+i': 'italic',
  'Ctrl+d': 'strikethrough',
  'Ctrl+k': 'link',
  'Ctrl+Shift+i': 'image',
  'Ctrl+l': 'unorderedList',
  'Ctrl+Shift+l': 'orderedList',
  'Ctrl+t': 'taskList',
  'Ctrl+Shift+t': 'table',
  'Ctrl+q': 'quote',
  'Ctrl+e': 'code',
  'Ctrl+h': 'horizontalRule'
} as const;

export type MarkdownShortcut = keyof typeof markdownShortcuts;
export type MarkdownAction = typeof markdownShortcuts[MarkdownShortcut];
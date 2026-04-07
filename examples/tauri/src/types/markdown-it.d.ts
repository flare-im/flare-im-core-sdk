declare module 'markdown-it' {
  interface MarkdownItOptions {
    html?: boolean;
    linkify?: boolean;
    breaks?: boolean;
    [key: string]: any;
  }

  class MarkdownIt {
    constructor(options?: MarkdownItOptions);
    render(markdown: string): string;
  }

  export default MarkdownIt;
}


/**
 * 消息展示用工具
 * Tauri 绑定返回 SDK 的 IMMessage（camelCase），content 为解码后的 ContentElem。
 * 兼容旧数据中的 contentDecoded 或 content 为解码对象的情况。
 */

import type { ContentElem, Message } from '../types';

/** 结构化对象 → Record（避免 TS2352：Message/Conversation 无 index signature） */
export function asRecord<T>(v: T): Record<string, unknown> {
  return v as unknown as Record<string, unknown>;
}

/** 事件/invoke 返回的松散对象 → SDK Message */
export function asSdkMessage(v: Record<string, unknown>): Message {
  return v as unknown as Message;
}

/** 解码后的消息内容（与 types.ContentElem 一致，保留别名便于迁移） */
export type ContentDecoded = ContentElem;

/**
 * Rust `Elem` 使用 serde internally tagged（`tag = "contentType"`），JSON 常为
 * `{ "contentType": "text", "text": "正文", "mentions": [] }`，即 `text` 为**字符串**；
 * 而 TS 的 ContentElem 类型写成嵌套 `text: { text, mentions }`。展示与回显需两种都认。
 */
export function textBodyFromRustTaggedContent(c: Record<string, unknown>): string {
  const t = c.text;
  if (typeof t === 'string') return t;
  if (t && typeof t === 'object' && t !== null && 'text' in t) {
    return String((t as { text?: unknown }).text ?? '');
  }
  return '';
}

/** 从 content 取用于列表/气泡展示的短文案 */
export function getContentDecodedPreview(decoded: ContentElem | null | undefined): string {
  if (!decoded) return '';
  const asRec = decoded as unknown as Record<string, unknown>;
  switch (decoded.contentType) {
    case 'text':
      return textBodyFromRustTaggedContent(asRec);
    case 'markdown': {
      const md = asRec.markdown;
      let body = '';
      if (md && typeof md === 'object' && md !== null && 'text' in md) {
        body = String((md as { text?: unknown }).text ?? '');
      } else {
        const t = asRec.text;
        body = typeof t === 'string' ? t : '';
      }
      return body || '[Markdown]';
    }
    case 'richText': {
      const rt = asRec.richText;
      let body = '';
      if (rt && typeof rt === 'object' && rt !== null && 'content' in rt) {
        body = String((rt as { content?: unknown }).content ?? '');
      } else {
        const co = asRec.content;
        body = typeof co === 'string' ? co : '';
      }
      return body || '[富文本]';
    }
    case 'image': return decoded.image?.description || '[图片]';
    case 'video': return decoded.video?.description || '[视频]';
    case 'audio': return decoded.audio?.description || '[语音]';
    case 'file': return decoded.file?.fileName ? `[文件] ${decoded.file.fileName}` : '[文件]';
    case 'location': return decoded.location?.address ? `[位置] ${decoded.location.address}` : '[位置]';
    case 'card': return decoded.card?.nickname ? `[名片] ${decoded.card.nickname}` : '[名片]';
    case 'sticker': return '[贴纸]';
    case 'emoji': return decoded.emoji?.emoji ?? '';
    case 'gif': return '[动图]';
    case 'quote': {
      const q = (decoded.quote ?? {}) as Record<string, unknown>;
      const current =
        (q.currentContent as ContentElem | undefined) ??
        (q.current_content as ContentElem | undefined) ??
        (q.replyContent as ContentElem | undefined) ??
        (q.reply_content as ContentElem | undefined);
      if (current) return getContentDecodedPreview(current) || (decoded.quote?.quotedTextPreview ?? '');
      return decoded.quote?.quotedTextPreview ?? '';
    }
    case 'linkCard': return decoded.linkCard?.title || '[链接]';
    case 'forward': return `[转发] ${decoded.forward?.messageIds?.length ?? 0} 条消息`;
    case 'thread': return decoded.thread?.threadTitle ?? '';
    case 'miniProgram': return decoded.miniProgram?.title || '[小程序]';
    case 'imageGroup': return '[多图]';
    case 'system': return decoded.system?.body || '[系统消息]';
    case 'notification': return decoded.notification?.body || decoded.notification?.title || '[通知]';
    case 'vote': return '[投票]';
    case 'task': return decoded.task?.title || '[任务]';
    case 'schedule': return '[日程]';
    case 'announcement': return decoded.announcement?.title || '[公告]';
    case 'custom': return decoded.custom?.description || '[自定义]';
    case 'placeholder': return decoded.placeholder?.fallbackText || '[占位]';
    default: return '';
  }
}

/**
 * Rust `Elem` 为 serde internally tagged：各变体字段摊平在 JSON 根上（如 `contentType: "image"` 与 `source`/`thumbnail` 同级），
 * 而前端视图（ImageView 等）按 types.ts 约定使用嵌套字段（`content.image`）。
 * 乐观发送在 Chat.vue 里已构造嵌套形状；从 DB / invoke 拉取时需在此处对齐，否则切换会话后媒体 payload 为空。
 */
export function normalizeFlattenedElemForUi(raw: Record<string, unknown>): Record<string, unknown> {
  const ct = String(raw.contentType ?? raw.content_type ?? '').trim();
  if (!ct) return raw;

  const pick = <T = unknown>(...keys: string[]): T | undefined => {
    for (const k of keys) {
      if (raw[k] !== undefined && raw[k] !== null) return raw[k] as T;
    }
    return undefined;
  };

  if (ct === 'image') {
    const nested = raw.image;
    if (nested && typeof nested === 'object') {
      return raw;
    }
    if (pick('source') !== undefined || pick('thumbnail') !== undefined || pick('description') !== undefined) {
      const { source, thumbnail, description, contentType, content_type, ...rest } = raw;
      return {
        ...rest,
        contentType: 'image',
        image: {
          source,
          thumbnail,
          description: String(description ?? ''),
        },
      };
    }
  }

  if (ct === 'video') {
    const nested = raw.video;
    if (nested && typeof nested === 'object') {
      return raw;
    }
    const vid = pick('videoId', 'video_id');
    if (vid !== undefined || pick('source') !== undefined || pick('cover') !== undefined) {
      const { videoId, video_id, source, cover, description, contentType, content_type, ...rest } = raw;
      return {
        ...rest,
        contentType: 'video',
        video: {
          videoId: String(videoId ?? video_id ?? ''),
          source,
          cover,
          description: String(description ?? ''),
        },
      };
    }
  }

  if (ct === 'audio') {
    const nested = raw.audio;
    if (nested && typeof nested === 'object') {
      return raw;
    }
    const aid = pick('audioId', 'audio_id');
    if (aid !== undefined || pick('source') !== undefined) {
      const { audioId, audio_id, source, description, contentType, content_type, ...rest } = raw;
      return {
        ...rest,
        contentType: 'audio',
        audio: {
          audioId: String(audioId ?? audio_id ?? ''),
          source,
          description: String(description ?? ''),
        },
      };
    }
  }

  if (ct === 'file') {
    const nested = raw.file;
    if (nested && typeof nested === 'object') {
      return raw;
    }
    const fid = pick('fileId', 'file_id');
    if (fid !== undefined || pick('url') !== undefined) {
      const {
        fileId,
        file_id,
        fileName,
        file_name,
        mimeType,
        mime_type,
        fileSize,
        file_size,
        url,
        description,
        contentType,
        content_type,
        ...rest
      } = raw;
      return {
        ...rest,
        contentType: 'file',
        file: {
          fileId: String(fileId ?? file_id ?? ''),
          fileName: String(fileName ?? file_name ?? ''),
          mimeType: String(mimeType ?? mime_type ?? ''),
          fileSize: Number(fileSize ?? file_size ?? 0),
          url: String(url ?? ''),
          description: String(description ?? ''),
        },
      };
    }
  }

  if (ct === 'gif') {
    const nested = raw.gif;
    if (nested && typeof nested === 'object') {
      return raw;
    }
    if (pick('gifId', 'gif_id') !== undefined || pick('url') !== undefined) {
      const {
        gifId,
        gif_id,
        url,
        thumbnail,
        durationMs,
        duration_ms,
        width,
        height,
        contentType,
        content_type,
        ...rest
      } = raw;
      return {
        ...rest,
        contentType: 'gif',
        gif: {
          gifId: String(gifId ?? gif_id ?? ''),
          url: String(url ?? ''),
          thumbnail,
          durationMs: Number(durationMs ?? duration_ms ?? 0),
          width: Number(width ?? 0),
          height: Number(height ?? 0),
        },
      };
    }
  }

  return raw;
}

/**
 * 从消息对象中取解码后的 content（IMMessage.content 或兼容 contentDecoded / content 为对象）
 * 与 MessageBubble 的 displayContent 一致：无解码 content 时可用 extra.content_text（入库/下行偶发解码失败时仍展示）
 */
export function getMessageContent(msg: Record<string, unknown>): ContentElem | null | undefined {
  const raw = msg.content ?? msg.contentDecoded;
  if (raw && typeof raw === 'object' && 'contentType' in raw) {
    const normalized = normalizeFlattenedElemForUi(raw as Record<string, unknown>);
    return normalized as ContentElem;
  }
  const extra = msg.extra;
  if (extra && typeof extra === 'object' && extra !== null) {
    const t = (extra as Record<string, unknown>).content_text;
    if (typeof t === 'string' && t.trim()) {
      return { contentType: 'text', text: { text: t.trim(), mentions: [] } };
    }
  }
  return undefined;
}

/**
 * 底部输入框「编辑消息」回显：与协议中多种文本类 Elem 对齐，避免仅识别 contentType=text 时回显为空。
 */
export function getEditablePlainTextFromMessage(msg: Record<string, unknown>): string {
  const content = getMessageContent(msg);
  if (content && typeof content === 'object' && 'contentType' in content) {
    const c = content as unknown as Record<string, unknown>;
    const ct = String(c.contentType ?? '');
    switch (ct) {
      case 'text':
        return textBodyFromRustTaggedContent(c);
      case 'markdown': {
        const md = c.markdown;
        if (md && typeof md === 'object' && md !== null && 'text' in md) {
          return String((md as { text?: unknown }).text ?? '');
        }
        const t = c.text;
        return typeof t === 'string' ? t : '';
      }
      case 'richText': {
        const rt = c.richText;
        if (rt && typeof rt === 'object' && rt !== null && 'content' in rt) {
          return String((rt as { content?: unknown }).content ?? '');
        }
        const co = c.content;
        return typeof co === 'string' ? co : '';
      }
      case 'placeholder': {
        const p = c.placeholder;
        if (p && typeof p === 'object' && p !== null && 'fallbackText' in p) {
          return String((p as { fallbackText?: unknown }).fallbackText ?? '');
        }
        const ft = c.fallbackText;
        return typeof ft === 'string' ? ft : '';
      }
      case 'emoji': {
        const em = c.emoji;
        if (typeof em === 'string') return em;
        if (em && typeof em === 'object' && em !== null && 'emoji' in em) {
          return String((em as { emoji?: unknown }).emoji ?? '');
        }
        return '';
      }
      case 'system': {
        const s = c.system;
        if (s && typeof s === 'object' && 'body' in s) return String((s as { body?: unknown }).body ?? '');
        const b = c.body;
        return typeof b === 'string' ? b : '';
      }
      case 'notification': {
        const n = c.notification;
        if (n && typeof n === 'object') {
          const o = n as { body?: unknown; title?: unknown };
          return String(o.body ?? o.title ?? '');
        }
        const body = c.body;
        const title = c.title;
        if (typeof body === 'string') return body;
        if (typeof title === 'string') return title;
        return '';
      }
      case 'announcement': {
        const a = c.announcement;
        if (a && typeof a === 'object') {
          const o = a as { body?: unknown; title?: unknown };
          return String(o.body ?? o.title ?? '');
        }
        return '';
      }
      default:
        return getContentDecodedPreview(content as ContentElem);
    }
  }
  const extra = msg.extra;
  if (extra && typeof extra === 'object' && extra !== null) {
    const t = (extra as Record<string, unknown>).content_text;
    if (typeof t === 'string') return t;
  }
  return '';
}

/** 从消息对象中取发送者 ID（兼容 camelCase / snake_case） */
export function getMessageSenderId(message: Record<string, unknown> | { sender_id?: string; senderId?: string }): string {
  const sid = (message as Record<string, unknown>).sender_id ?? (message as Record<string, unknown>).senderId;
  return typeof sid === 'string' ? sid : '';
}

/** 判断是否为当前用户发送的消息（用于右侧展示） */
export function isMessageFromSelf(
  message: Record<string, unknown> | { sender_id?: string; senderId?: string },
  currentUserId: string | null
): boolean {
  if (!currentUserId) return false;
  return getMessageSenderId(message) === currentUserId;
}

/** 解包下行消息 payload（兼容将来 envelope / message 嵌套） */
export function unwrapMessagePayload(raw: unknown): Record<string, unknown> | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (o.conversationId != null || o.conversation_id != null) return o;
  const inner = o.message;
  if (inner && typeof inner === "object") return unwrapMessagePayload(inner);
  return o;
}

/** im://message 与 merge 逻辑统一取会话 ID */
export function conversationIdFromPayload(raw: unknown): string {
  const o = unwrapMessagePayload(raw);
  if (!o) return "";
  const v = o.conversationId ?? o.conversation_id;
  return v == null ? "" : String(v).trim();
}

/** 会话列表项取 ID（camelCase / snake_case） */
export function conversationIdFromSession(session: unknown): string {
  const r = session as Record<string, unknown>;
  const v = r.conversationId ?? r.conversation_id;
  return v == null ? "" : String(v).trim();
}

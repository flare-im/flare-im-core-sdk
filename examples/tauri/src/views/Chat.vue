<script setup lang="ts">
import { ref, onMounted, nextTick, watch, computed } from "vue";
import { useRouter } from "vue-router";
import { Message, Modal } from "@arco-design/web-vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import SessionList from "../components/SessionList.vue";
import MessageList from "../components/MessageList.vue";
import EnhancedComposer from "../components/EnhancedComposer.vue";
import PinnedMessageBar from "../components/PinnedMessageBar.vue";
import PinnedMessagesDrawer from "../components/PinnedMessagesDrawer.vue";
import { useImEvents } from "../composables/useImEvents";
import {
  getEditablePlainTextFromMessage,
  getMessageContent,
  getContentDecodedPreview,
  asRecord,
  asSdkMessage,
  conversationIdFromPayload,
  conversationIdFromSession,
  unwrapMessagePayload,
} from "../utils/message";
import type {
  Message as SDKMessage,
  Conversation,
  ConversationTypeStr,
  ContentElem,
  LocalUploadState,
  MessagePreviewElem,
  SendAckPayload,
} from "../types";
import { resolveSdkDataUrl } from "../utils/dataUrl";
import { toWebviewLocalMediaUrl } from "../utils/localMediaUrl";
import { captureVideoFrameDataUrl, isVideoFilePath } from "../utils/videoPoster";

/** 当前会话正在输入的用户 ID 列表（带自动消失，约 5s） */
const typingUserIds = ref<string[]>([]);
const TYPING_EXPIRE_MS = 5000;
const typingExpireTimers = ref<Record<string, ReturnType<typeof setTimeout>>>({});

/** 展示「新」角标的会话 ID（创建或收到新会话后加入，约 60s 后移除） */
const newConversationIds = ref<string[]>([]);
const NEW_BADGE_DURATION_MS = 60_000;
const newBadgeTimers = ref<Record<string, ReturnType<typeof setTimeout>>>({});

function markConversationAsNew(conversationId: string) {
  if (!conversationId || newConversationIds.value.includes(conversationId)) return;
  newConversationIds.value = [...newConversationIds.value, conversationId];
  if (newBadgeTimers.value[conversationId]) clearTimeout(newBadgeTimers.value[conversationId]);
  newBadgeTimers.value[conversationId] = setTimeout(() => {
    newConversationIds.value = newConversationIds.value.filter((id) => id !== conversationId);
    delete newBadgeTimers.value[conversationId];
  }, NEW_BADGE_DURATION_MS);
}

const router = useRouter();
const sessions = ref<Conversation[]>([]);
const activeSessionId = ref<string | null>(null);
const totalUnread = ref(0);
const messages = ref<SDKMessage[]>([]);
const textToSend = ref("");
const replyToMessageId = ref<string | null>(null);
const threadReplyText = ref("");
const threadTargetId = ref<string | null>(null);
const loadingSessions = ref(false);
const loadingMessages = ref(false);
const createVisible = ref(false);
const createSessionType = ref("single");
const createBusinessType = ref("chat");
const createDisplayName = ref("");
const createPeerId = ref("");
const currentUserId = ref<string | null>(null);
/** 加载更多消息时使用：当前已加载消息的最小 seq，请求 before_seq 比它小的消息 */
const nextBeforeSeq = ref<number | null>(null);
const editingMessageId = ref<string | null>(null);
const mediaSending = ref(false);
const mediaSendingLabel = ref("");
const mediaProgressPercent = ref<number | null>(null);
/** 图片/视频：选文件后的预览弹窗 */
const mediaPreviewVisible = ref(false);
const mediaPreviewPath = ref("");
const mediaPreviewIsVideo = ref(false);
const mediaPreviewCaption = ref("");
const mediaPreviewCoverPath = ref<string | null>(null);
const mediaPreviewGenerating = ref(false);
const activeUploadingMessageId = ref<string | null>(null);
const sessionQuery = ref("");
const sessionDrafts = ref<Record<string, string>>({});
/** 单聊会话 ID → 对方 user_id，用于发送时填充 receiver_id（创建会话时写入并持久化） */
const SINGLE_CHAT_PEERS_KEY = "flare_im_single_chat_peers";
const singleChatPeerMap = ref<Record<string, string>>(loadSingleChatPeers());

type MediaKind = "image" | "video" | "audio" | "file";
/** 输入栏「+」菜单三项 */
type MediaMenuKind = "imageOrVideo" | "audio" | "file";

type SendMediaOptions = {
  caption?: string;
  coverPath?: string | null;
};

function normalizeFileUrl(filePath: string): string {
  return toWebviewLocalMediaUrl(String(filePath ?? ""));
}

function mediaKindLabel(kind: MediaKind): string {
  switch (kind) {
    case "image":
      return "图片";
    case "video":
      return "视频";
    case "audio":
      return "语音/音频";
    case "file":
      return "文件";
    default:
      return "媒体";
  }
}

function getFileName(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  return idx >= 0 ? normalized.slice(idx + 1) : normalized;
}

function buildLocalUploadState(kind: MediaKind, filePath: string): LocalUploadState {
  return {
    mediaKind: kind,
    filePath,
    fileName: getFileName(filePath),
    phase: "Preparing",
    progressPercent: 0,
    uploadedBytes: 0,
    totalBytes: 0,
  };
}

function withOptimisticMediaPreview(
  message: SDKMessage,
  kind: MediaKind,
  filePath: string,
  extra?: SendMediaOptions,
): SDKMessage {
  const localUrl = normalizeFileUrl(filePath);
  const row = { ...asRecord(message) } as Record<string, unknown>;
  const content = getMessageContent(row);
  const fileName = getFileName(filePath);
  const cap = String(extra?.caption ?? "").trim();
  if (kind === "image") {
    const image =
      content && typeof content === "object" && "contentType" in content && content.contentType === "image"
        ? ({ ...(content.image ?? {}) } as Record<string, any>)
        : ({} as Record<string, any>);
    const sourceBase = {
      uuid: String(image.source?.uuid ?? row.clientMsgId ?? filePath),
      imageId: String(image.source?.imageId ?? filePath),
      url: String(image.source?.url || localUrl),
      mimeType: String(image.source?.mimeType ?? image.thumbnail?.mimeType ?? ""),
      size: Number(image.source?.size ?? image.thumbnail?.size ?? 0),
      width: Number(image.source?.width ?? image.thumbnail?.width ?? 0),
      height: Number(image.source?.height ?? image.thumbnail?.height ?? 0),
    };
    const thumbBase = {
      ...sourceBase,
      uuid: String(image.thumbnail?.uuid ?? sourceBase.uuid),
      imageId: String(image.thumbnail?.imageId ?? sourceBase.imageId),
      url: String(image.thumbnail?.url || image.source?.url || localUrl),
      mimeType: String(image.thumbnail?.mimeType ?? sourceBase.mimeType),
      size: Number(image.thumbnail?.size ?? sourceBase.size),
      width: Number(image.thumbnail?.width ?? sourceBase.width),
      height: Number(image.thumbnail?.height ?? sourceBase.height),
    };
    row.content = {
      contentType: "image",
      image: {
        description: cap || String(image.description ?? "").trim() || "",
        source: sourceBase,
        thumbnail: thumbBase,
      },
    } as SDKMessage["content"];
  } else if (kind === "video") {
    const video =
      content && typeof content === "object" && "contentType" in content && content.contentType === "video"
        ? ({ ...(content.video ?? {}) } as Record<string, any>)
        : ({} as Record<string, any>);
    const sourceInfo = video.source?.url
      ? video.source
      : {
          uuid: String(video.videoId ?? row.clientMsgId ?? filePath),
          url: localUrl,
          mimeType: String(video.source?.mimeType ?? ""),
          size: Number(video.source?.size ?? 0),
          durationMs: Number(video.source?.durationMs ?? 0),
          width: Number(video.source?.width ?? 0),
          height: Number(video.source?.height ?? 0),
        };
    const coverPath = String(extra?.coverPath ?? "").trim();
    const coverBlock = coverPath
      ? {
          uuid: coverPath,
          imageId: coverPath,
          url: normalizeFileUrl(coverPath),
          mimeType: "image/jpeg",
          size: 0,
          width: Number(video.cover?.width ?? video.source?.width ?? 0),
          height: Number(video.cover?.height ?? video.source?.height ?? 0),
        }
      : video.cover?.url
        ? video.cover
        : {
            uuid: String(video.videoId ?? row.clientMsgId ?? filePath),
            url: localUrl,
            mimeType: "video/*",
            size: 0,
            width: Number(video.source?.width ?? 0),
            height: Number(video.source?.height ?? 0),
          };
    row.content = {
      contentType: "video",
      video: {
        ...video,
        videoId: String(video.videoId ?? row.clientMsgId ?? filePath),
        description: cap || String(video.description ?? fileName),
        source: sourceInfo,
        cover: coverBlock,
      },
    } as SDKMessage["content"];
  } else if (kind === "audio") {
    const audio =
      content && typeof content === "object" && "contentType" in content && content.contentType === "audio"
        ? ({ ...(content.audio ?? {}) } as Record<string, any>)
        : ({} as Record<string, any>);
    row.content = {
      contentType: "audio",
      audio: {
        ...audio,
        audioId: String(audio.audioId ?? row.clientMsgId ?? filePath),
        description: String(audio.description ?? fileName),
        source: audio.source?.url
          ? audio.source
          : {
              uuid: String(audio.audioId ?? row.clientMsgId ?? filePath),
              url: localUrl,
              mimeType: String(audio.source?.mimeType ?? ""),
              size: Number(audio.source?.size ?? 0),
              durationMs: Number(audio.source?.durationMs ?? 0),
            },
      },
    } as SDKMessage["content"];
  } else if (kind === "file") {
    const file =
      content && typeof content === "object" && "contentType" in content && content.contentType === "file"
        ? ({ ...(content.file ?? {}) } as Record<string, any>)
        : ({} as Record<string, any>);
    row.content = {
      contentType: "file",
      file: {
        ...file,
        fileId: String(file.fileId ?? row.clientMsgId ?? filePath),
        fileName: String(file.fileName ?? fileName),
        mimeType: String(file.mimeType ?? ""),
        fileSize: Number(file.fileSize ?? 0),
        url: String(file.url ?? localUrl),
        description: String(file.description ?? fileName),
      },
    } as SDKMessage["content"];
  }
  row.timestamp = Number(row.timestamp ?? Date.now()) || Date.now();
  row.clientTimestamp = Number(row.clientTimestamp ?? row.timestamp) || Number(row.timestamp);
  row.status = 1;
  row.localUpload = buildLocalUploadState(kind, filePath);
  return asSdkMessage(row);
}

/** 发往 Rust 的 IMMessage.content 需为 serde 扁平 Elem（与 message_elem.rs 一致） */
function applyFlatImagePayload(msg: SDKMessage, filePath: string, description: string): SDKMessage {
  const row = { ...asRecord(msg) } as Record<string, unknown>;
  const desc = String(description ?? "").trim();
  const part = {
    uuid: filePath,
    imageId: filePath,
    url: "",
    mimeType: "",
    size: 0,
    width: 0,
    height: 0,
  };
  row.content = {
    contentType: "image",
    description: desc,
    source: { ...part },
    thumbnail: { ...part },
  };
  return asSdkMessage(row);
}

function applyFlatVideoPayload(
  msg: SDKMessage,
  filePath: string,
  description: string,
  coverPath: string,
): SDKMessage {
  const row = { ...asRecord(msg) } as Record<string, unknown>;
  const desc = String(description ?? "").trim();
  const cover = coverPath.trim();
  row.content = {
    contentType: "video",
    videoId: filePath,
    description: desc,
    source: {
      uuid: filePath,
      url: "",
      mimeType: "video/mp4",
      size: 0,
      durationMs: 0,
      width: 0,
      height: 0,
    },
    cover: {
      uuid: cover,
      imageId: cover,
      url: "",
      mimeType: "image/jpeg",
      size: 0,
      width: 0,
      height: 0,
    },
  };
  return asSdkMessage(row);
}

function hasRenderableContent(content: ContentElem | null | undefined): boolean {
  if (!content) return false;
  switch (content.contentType) {
    case "image": {
      const img = content.image;
      if (!img) return false;
      const u = String(img.source?.url ?? img.thumbnail?.url ?? "").trim();
      if (u) return true;
      const sid = String(img.source?.imageId ?? img.source?.uuid ?? "").trim();
      const tid = String(img.thumbnail?.imageId ?? img.thumbnail?.uuid ?? "").trim();
      const localish = (s: string) => {
        const v = String(s ?? "").trim();
        if (!v) return false;
        if (v.startsWith("/") || v.startsWith("./") || v.startsWith("../") || v.toLowerCase().startsWith("file://")) {
          return true;
        }
        if (/^[A-Za-z]:[\\/]/.test(v) || v.startsWith("\\\\")) return true;
        return false;
      };
      return Boolean((sid && localish(sid)) || (tid && localish(tid)));
    }
    case "video":
      return !!String(content.video?.source?.url ?? content.video?.cover?.url ?? "").trim();
    case "audio":
      return !!String(content.audio?.source?.url ?? "").trim();
    case "file":
      return !!String(content.file?.url ?? "").trim();
    default:
      return true;
  }
}

function shouldReusePreviousContent(
  nextContent: ContentElem | null | undefined,
  previousContent: ContentElem | null | undefined,
): boolean {
  if (!previousContent) return false;
  if (!nextContent) return true;
  if (nextContent.contentType !== previousContent.contentType) return false;
  switch (nextContent.contentType) {
    case "image":
    case "video":
    case "audio":
    case "file":
      return !hasRenderableContent(nextContent) && hasRenderableContent(previousContent);
    default:
      return false;
  }
}

function reconcileMessageDisplay(nextMessage: SDKMessage, previousMessage?: SDKMessage): SDKMessage {
  if (!previousMessage) return nextMessage;
  const nextRow = { ...asRecord(nextMessage) } as Record<string, unknown>;
  const nextContent = getMessageContent(nextRow);
  const previousContent = getMessageContent(asRecord(previousMessage));
  if (shouldReusePreviousContent(nextContent, previousContent)) {
    nextRow.content = previousContent as SDKMessage["content"];
  }
  if (!nextRow.localUpload && (asRecord(previousMessage).localUpload as LocalUploadState | undefined)) {
    const status = Number(nextRow.status ?? 0);
    if (status <= 1 || (status >= 2 && shouldReusePreviousContent(nextContent, previousContent))) {
      nextRow.localUpload = asRecord(previousMessage).localUpload as LocalUploadState;
    }
  }
  return asSdkMessage(nextRow);
}

function reconcileDisplayMessages(nextList: SDKMessage[], previousList: SDKMessage[]): SDKMessage[] {
  if (!previousList.length) return nextList;
  const previousById = new Map<string, SDKMessage>();
  for (const item of previousList) {
    const row = asRecord(item);
    const ids = [
      String(row.serverId ?? row.server_id ?? "").trim(),
      String(row.clientMsgId ?? row.client_msg_id ?? "").trim(),
    ].filter(Boolean);
    for (const id of ids) previousById.set(id, item);
  }
  return nextList.map((item) => {
    const row = asRecord(item);
    const ids = [
      String(row.serverId ?? row.server_id ?? "").trim(),
      String(row.clientMsgId ?? row.client_msg_id ?? "").trim(),
    ].filter(Boolean);
    const previous = ids.map((id) => previousById.get(id)).find(Boolean);
    return reconcileMessageDisplay(item, previous);
  });
}

function patchMessageByIdentity(
  identity: string,
  updater: (row: Record<string, unknown>) => Record<string, unknown>,
): boolean {
  const target = String(identity ?? "").trim();
  if (!target) return false;
  const idx = messages.value.findIndex((m) => {
    const r = asRecord(m);
    const sid = String(r.serverId ?? r.server_id ?? "").trim();
    const cid = String(r.clientMsgId ?? r.client_msg_id ?? "").trim();
    return sid === target || cid === target;
  });
  if (idx < 0) return false;
  messages.value[idx] = asSdkMessage(updater({ ...asRecord(messages.value[idx]) }));
  messages.value = enrichQuoteContext(sortMessages([...messages.value]));
  return true;
}

function addOptimisticMediaMessage(message: SDKMessage): void {
  messages.value = enrichQuoteContext(sortMessages([...messages.value, message]));
  upsertSessionFromIncomingMessage(asRecord(message), String(asRecord(message).conversationId ?? ""));
  nextTick(() => {
    setTimeout(() => messageListRef.value?.scrollToBottom(), 60);
  });
}

function clearLocalUploadState(messageId: string | null | undefined): void {
  const target = String(messageId ?? "").trim();
  if (!target) return;
  patchMessageByIdentity(target, (row) => {
    delete row.localUpload;
    return row;
  });
  if (activeUploadingMessageId.value === target) {
    activeUploadingMessageId.value = null;
  }
}

function updateActiveUploadProgress(payload: {
  phase?: string;
  uploadedBytes?: number;
  totalBytes?: number;
  fileName?: string;
}): void {
  const target = String(activeUploadingMessageId.value ?? "").trim();
  if (!target) return;
  patchMessageByIdentity(target, (row) => {
    const existing = (row.localUpload as LocalUploadState | undefined) ?? {
      mediaKind: "file",
      filePath: "",
      fileName: "",
      phase: "Preparing",
    };
    const total = Number(payload.totalBytes ?? existing.totalBytes ?? 0);
    const uploaded = Number(payload.uploadedBytes ?? existing.uploadedBytes ?? 0);
    const progressPercent =
      total > 0 ? Math.min(100, Math.max(0, Math.floor((uploaded / total) * 100))) : existing.progressPercent ?? 0;
    row.localUpload = {
      ...existing,
      fileName: String(payload.fileName ?? existing.fileName ?? ""),
      phase: String(payload.phase ?? existing.phase ?? "Uploading"),
      uploadedBytes: uploaded,
      totalBytes: total,
      progressPercent,
    };
    return row;
  });
}

function loadSingleChatPeers(): Record<string, string> {
  try {
    const raw = localStorage.getItem(SINGLE_CHAT_PEERS_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveSingleChatPeer(conversationId: string, peerId: string) {
  singleChatPeerMap.value = { ...singleChatPeerMap.value, [conversationId]: peerId };
  localStorage.setItem(SINGLE_CHAT_PEERS_KEY, JSON.stringify(singleChatPeerMap.value));
}

const messageListRef = ref<InstanceType<typeof MessageList> | null>(null);
const replyDetailVisible = ref(false);
const replyDetailMessageId = ref<string | null>(null);
const pinnedFocusMessageId = ref<string | null>(null);
const pinnedDrawerVisible = ref(false);
const pinnedBarDismissedByConversation = ref<Record<string, boolean>>({});

function toSafeUnread(value: unknown): number {
  const n = typeof value === "number" ? value : Number(value ?? 0);
  if (!Number.isFinite(n) || Number.isNaN(n) || n <= 0) return 0;
  return Math.floor(n);
}

function applySendAck(ack: SendAckPayload) {
  const ackRaw = asRecord((ack ?? {}) as unknown as Record<string, unknown>);
  const ackCid = String(ackRaw.conversationId ?? ackRaw.conversation_id ?? "").trim();
  const ackClientMsgId = String(ackRaw.clientMsgId ?? ackRaw.client_msg_id ?? "").trim();
  const ackServerMsgId = String(ackRaw.serverMsgId ?? ackRaw.server_msg_id ?? "").trim();
  const ackSeq = Number(ackRaw.seq ?? 0);
  const ackSuccess = Boolean(ackRaw.success);
  if (!ackClientMsgId) return;
  let idx = messages.value.findIndex((m) => getMsgClientMsgId(asRecord(m)) === ackClientMsgId);
  if (idx < 0 && ackServerMsgId) {
    idx = messages.value.findIndex((m) => getMsgServerId(asRecord(m)) === ackServerMsgId);
  }
  if (idx < 0 && ackCid) {
    idx = messages.value.findIndex((m) => {
      const r = asRecord(m);
      const conv = String(r.conversationId ?? r.conversation_id ?? "");
      return conv === ackCid && Number(r.status ?? 0) === 1;
    });
  }
  if (idx < 0) return;
  const row = asRecord(messages.value[idx]);
  const rowConvId = String(row.conversationId ?? row.conversation_id ?? "");
  if (ackCid && rowConvId && ackCid !== rowConvId) return;
  if (ackServerMsgId) {
    row.serverId = ackServerMsgId;
    row.server_id = ackServerMsgId;
  }
  if (!String(row.clientMsgId ?? row.client_msg_id ?? "").trim()) {
    row.clientMsgId = ackClientMsgId;
    row.client_msg_id = ackClientMsgId;
  }
  if (ackSeq > 0) row.seq = ackSeq;
  row.status = ackSuccess ? 2 : 5;
  delete row.localUpload;
  if (ackSuccess && activeUploadingMessageId.value === ackClientMsgId) {
    activeUploadingMessageId.value = null;
  }
  messages.value = [...messages.value];
}

function applySendFailed(payload: { client_msg_id?: string; clientMsgId?: string; reason?: string }) {
  const failedClientMsgId = String(payload.clientMsgId ?? payload.client_msg_id ?? "").trim();
  if (!failedClientMsgId) return;
  const idx = messages.value.findIndex(
    (m) => getMsgClientMsgId(asRecord(m)).trim() === failedClientMsgId,
  );
  if (idx < 0) return;
  const row = asRecord(messages.value[idx]);
  row.status = 5;
  const existing = row.localUpload as LocalUploadState | undefined;
  if (existing) {
    row.localUpload = {
      ...existing,
      phase: "Failed",
    };
  }
  if (activeUploadingMessageId.value === failedClientMsgId) {
    activeUploadingMessageId.value = null;
  }
  messages.value = [...messages.value];
}

function normalizeConversationType(v: unknown): ConversationTypeStr {
  if (typeof v === "number") {
    if (v === 1) return "single";
    if (v === 2) return "group";
    return "unspecified";
  }
  const s = String(v ?? "").trim().toLowerCase();
  if (s === "1" || s === "single" || s === "private") return "single";
  if (s === "2" || s === "group" || s === "channel") return "group";
  return "unspecified";
}

function upsertSessionFromIncomingMessage(
  raw: Record<string, unknown>,
  conversationId: string,
): void {
  if (!conversationId) return;
  const now = Date.now();
  const senderId = String(raw.senderId ?? raw.sender_id ?? "");
  const senderName = String(
    raw.senderDisplayName ?? raw.sender_display_name ?? raw.senderName ?? raw.sender_name ?? senderId,
  ).trim();
  const channelId = String(raw.channelId ?? raw.channel_id ?? "").trim();
  const convType = normalizeConversationType(raw.conversationType ?? raw.conversation_type);
  const isSingle = convType === "single" || convType === "private" || convType === "Single";
  const selfId = currentUserId.value ?? "";
  const isSelfMessage = !!selfId && senderId === selfId;
  const peerId = isSingle ? (!isSelfMessage ? senderId : channelId) : null;
  const displayName = isSingle
    ? (peerId || senderName || channelId || conversationId)
    : (channelId || conversationId);
  const content = getMessageContent(raw);
  const preview = getContentDecodedPreview(content) || String(raw.text ?? "").trim() || "[消息]";
  const timestamp = Number(raw.timestamp ?? now) || now;
  const messageType = Number(raw.messageType ?? raw.message_type ?? 0);
  const messageId = String(raw.serverId ?? raw.server_id ?? raw.clientMsgId ?? raw.client_msg_id ?? "");

  const idx = sessions.value.findIndex((s) => conversationIdFromSession(s) === conversationId);
  if (idx >= 0) {
    const prev = sessions.value[idx];
    const updated: Conversation = {
      ...prev,
      conversationId,
      conversationType: prev.conversationType ?? convType,
      displayName: prev.displayName || displayName,
      peerId: prev.peerId ?? (peerId || null),
      lastMessagePreview: preview,
      lastMessageAt: timestamp,
      updatedAt: timestamp,
      // 未读数统一由 SDK 基于 max_seq/last_read_seq 计算并回传，前端不再手工 +1。
      unreadCount: toSafeUnread(prev.unreadCount ?? asRecord(prev).unread_count ?? 0),
      lastMessage: {
        messageId,
        senderId,
        type: messageType,
        text: preview,
        time: timestamp,
      },
    };
    sessions.value[idx] = updated;
  } else {
    const temp: Conversation = {
      conversationId,
      conversationType: convType,
      businessType: "chat",
      displayName,
      avatarUrl: "",
      unreadCount: 0,
      lastReadSeq: 0,
      maxSeq: Number(raw.seq ?? 0) || 0,
      isPinned: false,
      isMuted: false,
      updatedAt: timestamp,
      createdAt: timestamp,
      lastMessageId: messageId || null,
      lastSenderId: senderId || null,
      lastMessageAt: timestamp,
      lastMessagePreview: preview,
      lastMessage: {
        messageId,
        senderId,
        type: messageType,
        text: preview,
        time: timestamp,
      },
      peerId: peerId || null,
      participants: isSingle
        ? [
            { user_id: selfId || "self" },
            { user_id: peerId || senderId || "peer", nickname: senderName || undefined },
          ]
        : undefined,
    };
    sessions.value = [temp, ...sessions.value];
    if (peerId && convType === "single") saveSingleChatPeer(conversationId, peerId);
  }
  sessions.value = [...sessions.value].sort((a, b) => {
    const ta = Number(a.lastMessageAt ?? a.updatedAt ?? 0);
    const tb = Number(b.lastMessageAt ?? b.updatedAt ?? 0);
    return tb - ta;
  });
}

function mergeIncomingMessage(payload: unknown) {
  const convId = conversationIdFromPayload(payload);
  const raw =
    unwrapMessagePayload(payload) ??
    (payload && typeof payload === "object" ? (payload as Record<string, unknown>) : null);
  if (!raw) return;
  const serverId = (raw.serverId ?? raw.server_id) as string | undefined;
  const clientMsgId = (raw.clientMsgId ?? raw.client_msg_id) as string | undefined;
  if (!convId) {
    console.warn("[Chat] 下行消息缺少 conversationId，将尝试从 DB 刷新当前会话", payload);
    scheduleReloadActiveMessagesFromDb();
    return;
  }
  upsertSessionFromIncomingMessage(raw, convId);
  if (convId !== activeSessionId.value) {
    if (!activeSessionId.value) {
      activeSessionId.value = convId;
    } else {
      return;
    }
  }
  const idx = messages.value.findIndex((x) => {
    const xr = asRecord(x);
    const xSid = xr.serverId ?? xr.server_id;
    const xCid = xr.clientMsgId ?? xr.client_msg_id;
    return (xSid && serverId && xSid === serverId) || (xCid && clientMsgId && xCid === clientMsgId);
  });
  if (idx >= 0) {
    const prev = asRecord(messages.value[idx]);
    const prevStatus = prev.status;
    const merged = { ...raw } as SDKMessage & Record<string, unknown>;
    if (!String(merged.clientMsgId ?? merged.client_msg_id ?? "").trim()) {
      const prevCid = String(prev.clientMsgId ?? prev.client_msg_id ?? "").trim();
      if (prevCid) {
        merged.clientMsgId = prevCid;
        merged.client_msg_id = prevCid;
      }
    }
    if (!String(merged.serverId ?? merged.server_id ?? "").trim()) {
      const prevSid = String(prev.serverId ?? prev.server_id ?? "").trim();
      if (prevSid) {
        merged.serverId = prevSid;
        merged.server_id = prevSid;
      }
    }
    const mergedStatus = Number(merged.status ?? 0);
    const prevStatusNum = Number(prevStatus ?? 0);
    const senderId = String(merged.senderId ?? merged.sender_id ?? "");
    const isSelfIncoming = !!currentUserId.value && senderId === currentUserId.value;
    const hasServerIdentity =
      !!String(merged.serverId ?? merged.server_id ?? "").trim() || Number(merged.seq ?? 0) > 0;
    if ((prevStatus === 1 || prevStatus === "1") && (merged.status == null || merged.status === 0)) {
      merged.status = 3;
    }
    // 对自己发送的消息：若已具备 server_id/seq，则至少应为 Sent，避免被下行回写成「发送中」。
    if (isSelfIncoming && hasServerIdentity && mergedStatus > 0 && mergedStatus < 2) {
      merged.status = 2;
    }
    // 避免状态回退（例如本地已 ACK=2，下行又给了 1）。
    if (prevStatusNum >= 2 && mergedStatus > 0 && mergedStatus < prevStatusNum) {
      merged.status = prevStatusNum;
    }
    if ((hasServerIdentity && Number(merged.status ?? 0) >= 2) || Number(prevStatusNum) >= 2) {
      delete merged.localUpload;
      const mergedClientMsgId = String(merged.clientMsgId ?? merged.client_msg_id ?? "").trim();
      if (mergedClientMsgId && activeUploadingMessageId.value === mergedClientMsgId) {
        activeUploadingMessageId.value = null;
      }
    } else if (prev.localUpload) {
      merged.localUpload = prev.localUpload as LocalUploadState;
    }
    const payloadContent = merged.content ?? merged.contentDecoded;
    const hasPayloadContent = payloadContent && typeof payloadContent === "object" && "contentType" in payloadContent;
    if (!hasPayloadContent) {
      const prevContent = prev.content ?? prev.contentDecoded;
      if (prevContent && typeof prevContent === "object" && "contentType" in prevContent) {
        merged.content = prevContent as SDKMessage["content"];
      }
    } else {
      const previousContent = getMessageContent(prev);
      const nextContent = getMessageContent(merged);
      if (shouldReusePreviousContent(nextContent, previousContent)) {
        merged.content = previousContent as SDKMessage["content"];
      }
    }
    // 若本次下行未携带 reactions（常见于非消息全量推送），保留前端内存中的反应状态，避免 UI 闪烁。
    if (!Array.isArray(merged.reactions) && Array.isArray(prev.reactions)) {
      merged.reactions = prev.reactions as unknown as SDKMessage["reactions"];
    }
    messages.value[idx] = merged as SDKMessage;
    messages.value = enrichQuoteContext(sortMessages(messages.value));
    return;
  }
  messages.value = enrichQuoteContext(sortMessages([...messages.value, asSdkMessage(raw)]));
  nextTick(() => {
    setTimeout(() => messageListRef.value?.scrollToBottom(), 100);
  });
}

const loggingOut = ref(false);

const {
  connectionState,
  syncState,
  syncProgress,
  syncError,
  listenersReady,
  waitForFullSync,
  clearSyncState,
} = useImEvents({
  onSendAck(ack) {
    applySendAck(ack);
  },
  onSendFailed(payload) {
    applySendFailed(payload);
  },
  onMessage(m: unknown) {
    const convId = conversationIdFromPayload(m);
    mergeIncomingMessage(m);
    if (convId && convId === activeSessionId.value) {
      scheduleMarkActiveSessionRead();
    }
    void loadSessions(true);
    scheduleReloadActiveMessagesFromDb();
  },
  onMessageBatch(items: unknown[]) {
    let hasActiveConversationMessage = false;
    for (const item of items) {
      const convId = conversationIdFromPayload(item);
      if (convId && convId === activeSessionId.value) {
        hasActiveConversationMessage = true;
      }
      mergeIncomingMessage(item);
    }
    if (hasActiveConversationMessage) {
      scheduleMarkActiveSessionRead();
    }
    void loadSessions(true);
    scheduleReloadActiveMessagesFromDb();
  },
  onUnread() {
    loadSessions();
  },
  onUnreadCountChanged(p) {
    const convId = String(p.conversation_id ?? "").trim();
    if (!convId) return;
    const nextUnread = toSafeUnread(p.unread_count);
    const idx = sessions.value.findIndex((s) => conversationIdFromSession(s) === convId);
    if (idx >= 0) {
      const next = { ...sessions.value[idx], unreadCount: nextUnread } as Conversation;
      sessions.value[idx] = next;
      sessions.value = [...sessions.value];
      totalUnread.value = sessions.value.reduce(
        (acc, s) => acc + toSafeUnread(s.unreadCount ?? asRecord(s).unread_count),
        0,
      );
    } else {
      // 会话尚未落到前端列表（例如刚收到首条消息）时，触发重载避免未读统计漂移。
      void loadSessions(true);
    }
  },
  onMessageRecalled(p) {
    if (p.conversation_id === activeSessionId.value && p.message_id) {
      const mid = p.message_id;
      const idx = findMessageIndexById(mid);
      if (idx >= 0) {
        const row = {
          ...asRecord(messages.value[idx]),
          isRecalled: true,
          is_recalled: true,
          status: 6,
        } as Record<string, unknown>;
        messages.value[idx] = asSdkMessage(row);
        messages.value = [...messages.value];
      }
      scheduleReloadActiveMessagesFromDb();
    }
    void loadSessions();
  },
  onMessageEdited(p) {
    if (p.conversation_id === activeSessionId.value && p.message_id) {
      const mid = p.message_id;
      const idx = findMessageIndexById(mid);
      if (idx >= 0) {
        const row = { ...asRecord(messages.value[idx]), isEdited: true } as Record<string, unknown>;
        row.is_edited = true;
        messages.value[idx] = asSdkMessage(row);
        messages.value = [...messages.value];
      }
      scheduleReloadActiveMessagesFromDb();
    }
    void loadSessions();
  },
  onMessageReactionChanged(p) {
    applyReactionChange(p);
  },
  onMessageDeleted(p) {
    if (p?.message_id) removeMessageFromCurrentSession(p.message_id);
    loadSessions();
  },
  onMessageReadReceipt(p) {
    applyReadReceipt(p);
    scheduleReloadActiveMessagesFromDb();
  },
  onMessagePinned(p) {
    applyPinnedChange({ ...p, pinned: true });
    scheduleReloadActiveMessagesFromDb();
  },
  onMessageUnpinned(p) {
    applyPinnedChange({ ...p, pinned: false });
    scheduleReloadActiveMessagesFromDb();
  },
  onMessageMarked(p) {
    applyMarkChange({ ...p, marked: true, mark_type: p.mark_type, color: p.color });
    scheduleReloadActiveMessagesFromDb();
  },
  onMessageUnmarked(p) {
    applyMarkChange({ ...p, marked: false, mark_type: p.mark_type });
    scheduleReloadActiveMessagesFromDb();
  },
  onConversationsSynced() {
    loadSessions();
  },
  onConversationCreated(p) {
    loadSessions();
    if (p?.conversation_id) {
      markConversationAsNew(p.conversation_id);
      Message.info("收到新会话");
    }
  },
  onConversationUpdated(p) {
    if (p?.conversation_id && activeSessionId.value === p.conversation_id) {
      loadSessions();
    } else {
      loadSessions();
    }
  },
  onConversationDeleted(p) {
    if (p?.conversation_id === activeSessionId.value) {
      activeSessionId.value = null;
      messages.value = [];
    }
    loadSessions();
  },
  onTyping(p) {
    if (!p || p.conversation_id !== activeSessionId.value) return;
    const uid = p.user_id;
    // 只展示“对方正在输入”，忽略自己输入回流事件。
    if (!uid || uid === currentUserId.value) return;
    if (p.typing) {
      if (typingExpireTimers.value[uid]) clearTimeout(typingExpireTimers.value[uid]);
      if (!typingUserIds.value.includes(uid)) {
        typingUserIds.value = [...typingUserIds.value, uid];
      }
      typingExpireTimers.value[uid] = setTimeout(() => {
        typingUserIds.value = typingUserIds.value.filter((id) => id !== uid);
        delete typingExpireTimers.value[uid];
      }, TYPING_EXPIRE_MS);
    } else {
      if (typingExpireTimers.value[uid]) {
        clearTimeout(typingExpireTimers.value[uid]);
        delete typingExpireTimers.value[uid];
      }
      typingUserIds.value = typingUserIds.value.filter((id) => id !== uid);
    }
  },
  onPresenceChanged(p) {
    console.log("[Chat] presence_changed", p);
  },
  onCallSignal(p) {
    console.log("[Chat] call_signal", p);
  },
  onMessageCustomEvent(p) {
    console.log("[Chat] message_custom_event", p);
  },
  onKickedOff(p) {
    Message.error(`已被踢下线: ${p?.reason ?? ""}`);
    router.replace("/");
  },
  onTokenExpired() {
    Message.warning("登录已过期，请重新登录");
    router.replace("/");
  },
  onUploadProgress(p) {
    mediaSending.value = true;
    const total = Number(p.totalBytes ?? 0);
    const uploaded = Number(p.uploadedBytes ?? 0);
    mediaProgressPercent.value = total > 0 ? Math.min(100, Math.max(0, Math.floor((uploaded / total) * 100))) : null;
    updateActiveUploadProgress({
      phase: p.phase,
      uploadedBytes: uploaded,
      totalBytes: total,
      fileName: p.fileName,
    });
    const phase = String(p.phase ?? "");
    const name = String(p.fileName ?? "").trim();
    if (phase === "Preparing") {
      mediaSendingLabel.value = `正在准备${name ? `：${name}` : ""}`;
    } else if (phase === "Uploading") {
      mediaSendingLabel.value = `正在上传${name ? `：${name}` : ""}`;
    } else if (phase === "Completing") {
      mediaSendingLabel.value = `正在完成${name ? `：${name}` : ""}`;
    } else if (phase === "Finished") {
      mediaSendingLabel.value = `上传完成${name ? `：${name}` : ""}`;
      mediaProgressPercent.value = 100;
    }
  },
});

/** 退出登录：断开 SDK、清同步状态、回登录页（本地 SQLite 数据保留） */
function handleLogout() {
  Modal.confirm({
    title: "退出登录",
    content: "将断开与服务器的连接并返回登录页。本地已缓存的会话与消息仍会保留在本机数据库中。",
    okText: "退出",
    cancelText: "取消",
    okButtonProps: { status: "danger" },
    async onBeforeOk() {
      loggingOut.value = true;
      try {
        try {
          await invoke("sdk_logout");
        } catch (e) {
          console.warn("[Chat] sdk_logout:", e);
          Message.warning("断开连接时出现异常，仍将返回登录页");
        }
        try {
          localStorage.removeItem("userId");
        } catch {
          /* ignore */
        }
        clearSyncState();
        sessions.value = [];
        messages.value = [];
        activeSessionId.value = null;
        currentUserId.value = null;
        editingMessageId.value = null;
        clearReplyContext();
        threadTargetId.value = null;
        await router.replace("/");
        return true;
      } finally {
        loggingOut.value = false;
      }
    },
  });
}

watch(activeSessionId, () => {
  typingUserIds.value = [];
  clearReplyContext();
  replyDetailVisible.value = false;
  replyDetailMessageId.value = null;
  Object.values(typingExpireTimers.value).forEach((t) => clearTimeout(t));
  typingExpireTimers.value = {};
});

watch(syncState, async (state) => {
  if (state !== "finished") return;
  await loadSessions(true);
  if (activeSessionId.value) {
    await selectSession(activeSessionId.value, { markRead: false });
  }
});

/** 当前会话「正在输入」展示文案（对方昵称或 user_id） */
function getTypingDisplayName(userId: string): string {
  const session = sessions.value.find((s) => s.conversationId === activeSessionId.value);
  if (!session) return userId;
  const participants = session.participants ?? asRecord(session).participants;
  const peer = Array.isArray(participants) ? participants.find((p: { user_id: string; nickname?: string }) => p.user_id === userId) : undefined;
  if (peer?.nickname) return peer.nickname;
  const convType = String(session.conversationType ?? asRecord(session).conversation_type ?? "");
  if (convType === "single" || convType === "private" || convType === "Single") {
    return (session.displayName ?? asRecord(session).display_name) || "对方";
  }
  return userId;
}

const typingHintText = computed(() => {
  if (typingUserIds.value.length === 0) return "";
  if (typingUserIds.value.length === 1) {
    return `${getTypingDisplayName(typingUserIds.value[0])} 正在输入...`;
  }
  return `${typingUserIds.value.map(getTypingDisplayName).join("、")} 正在输入...`;
});

const activeSessionDisplayName = computed(() => {
  const sid = activeSessionId.value;
  if (!sid) return "";
  const s = sessions.value.find((x) => x.conversationId === sid);
  if (!s) return "";
  return String(s.displayName ?? asRecord(s).display_name ?? "").trim();
});

function handleSdkError(error: unknown, context: string) {
  let msg = "";
  let detail: any = null;
  if (typeof error === "object" && error) {
    const e: any = error;
    msg = e.message || e.msg || "";
    if (!msg && e.payload && typeof e.payload === "object") {
      msg = e.payload.message || e.payload.msg || "";
      detail = e.payload;
    }
    if (!msg && "code" in e && "message" in e) {
      msg = e.message;
      detail = e;
    }
  }
  if (!msg) msg = String(error);
  console.error(`[${context}]`, detail ?? msg);

  const i18nErrorMap: Record<string, string> = {
    "sdk.message.quote.invalid_quoted_message_id": "引用消息参数异常：缺少被引用消息 ID",
    "sdk.message.quote.missing_quoted_message_id": "引用消息缺少被引用消息 ID",
    "sdk.message.quote.missing_quoted_content": "引用消息缺少被引用内容",
    "sdk.message.quote.missing_current_content": "引用消息缺少当前发送内容",
    "sdk.message.quote.current_content_quote_not_allowed": "引用消息不允许嵌套引用内容",
    "sdk.message.quote.missing_content": "引用消息内容为空",
    "sdk.message.quote.content_type_mismatch": "引用消息内容类型不匹配",
    "sdk.message.invalid_content_encoding": "消息内容编码异常",
    "sdk.sync.query_events.timeout_or_canceled": "关键事件回放超时，已跳过本轮补偿",
  };
  if (i18nErrorMap[msg]) {
    Message.error(i18nErrorMap[msg]);
    return;
  }

  if (msg.includes("SDK not connected") || msg.includes("SDK not initialized")) {
    Message.error("连接已断开，请重新登录");
    router.replace("/");
    return;
  }
  Message.error(msg);
}

/** 从消息对象取 server_id（Tauri 为 camelCase：serverId） */
function getMsgServerId(m: Record<string, unknown>): string {
  return String((m.serverId ?? m.server_id ?? ""));
}
/** 从消息对象取 client_msg_id（Tauri 为 camelCase：clientMsgId） */
function getMsgClientMsgId(m: Record<string, unknown>): string {
  return String((m.clientMsgId ?? m.client_msg_id ?? ""));
}

function toTimestampMs(ts: unknown): number {
  if (typeof ts === 'number') return ts;
  return new Date(String(ts)).getTime() || 0;
}

function sortMessages(list: SDKMessage[]): SDKMessage[] {
  return [...list].sort((a, b) => {
    const sa = Number(asRecord(a).seq ?? 0);
    const sb = Number(asRecord(b).seq ?? 0);
    if (sa > 0 && sb > 0) return sa - sb;
    const ta = toTimestampMs(asRecord(a).timestamp);
    const tb = toTimestampMs(asRecord(b).timestamp);
    if (ta !== tb) return ta - tb;
    const aSid = asRecord(a).serverId ?? asRecord(a).server_id ?? "";
    const bSid = asRecord(b).serverId ?? asRecord(b).server_id ?? "";
    return String(aSid).localeCompare(String(bSid));
  });
}

function enrichQuoteContext(list: SDKMessage[]): SDKMessage[] {
  if (!Array.isArray(list) || list.length === 0) return list;
  const previewById = new Map<string, string>();
  const senderById = new Map<string, string>();
  for (const m of list) {
    const r = asRecord(m);
    const sid = getMsgServerId(r);
    const cid = getMsgClientMsgId(r);
    const preview = getQuotePreview(m);
    const sender = String(r.senderDisplayName ?? r.senderName ?? r.senderId ?? r.sender_id ?? "");
    if (sid) {
      if (preview) previewById.set(sid, preview);
      if (sender) senderById.set(sid, sender);
    }
    if (cid) {
      if (preview) previewById.set(cid, preview);
      if (sender) senderById.set(cid, sender);
    }
  }

  let anyChanged = false;
  const next = list.map((m) => {
    const row = { ...asRecord(m) } as Record<string, unknown>;
    const content = getMessageContent(row);
    if (!content || content.contentType !== "quote" || !content.quote) return m;
    const quoteObj = content.quote as Record<string, unknown>;
    const quotedIdRaw = quoteObj.quotedMessageId ?? quoteObj.quoted_message_id;
    const quotedId = typeof quotedIdRaw === "string" ? quotedIdRaw : "";
    const currentPreviewRaw = quoteObj.quotedTextPreview ?? quoteObj.quoted_text_preview;
    const currentPreview = typeof currentPreviewRaw === "string" ? currentPreviewRaw.trim() : "";
    const currentSenderRaw = quoteObj.quotedSenderId ?? quoteObj.quoted_sender_id;
    const currentSender = typeof currentSenderRaw === "string" ? currentSenderRaw.trim() : "";
    const fallbackPreview = quotedId ? (previewById.get(quotedId) ?? "") : "";
    const fallbackSender = quotedId ? (senderById.get(quotedId) ?? "") : "";
    if (!fallbackPreview && !fallbackSender) return m;

    const updatedQuote = { ...(quoteObj as Record<string, unknown>) };
    let rowChanged = false;
    if (!currentPreview && fallbackPreview) {
      updatedQuote.quotedTextPreview = fallbackPreview;
      rowChanged = true;
    }
    if (!currentSender && fallbackSender) {
      updatedQuote.quotedSenderId = fallbackSender;
      rowChanged = true;
    }
    if (!rowChanged) return m;
    anyChanged = true;
    row.content = { ...(content as Record<string, unknown>), quote: updatedQuote } as SDKMessage["content"];
    return asSdkMessage(row);
  });
  return anyChanged ? next : list;
}

/** SDK 连接后自动同步会话与消息，无需单独拉取增量；刷新列表直接 sdk_conversation_list */
async function syncActiveSession(): Promise<void> {
  if (!activeSessionId.value) return;
  await loadSessions();
}

function removeMessageFromCurrentSession(messageId: string): void {
  messages.value = messages.value.filter(
    (m) => getMsgServerId(asRecord(m)) !== messageId && getMsgClientMsgId(asRecord(m)) !== messageId,
  );
}

function findMessageIndexById(messageId: string): number {
  if (!messageId) return -1;
  return messages.value.findIndex((m) => {
    const r = asRecord(m);
    const sid = String(r.serverId ?? r.server_id ?? "");
    const cid = String(r.clientMsgId ?? r.client_msg_id ?? "");
    return sid === messageId || cid === messageId;
  });
}

function applyPinnedChange(payload: {
  conversation_id: string;
  message_id: string;
  pinned: boolean;
}): void {
  if (!payload || payload.conversation_id !== activeSessionId.value) return;
  if (payload.pinned && activeSessionId.value) {
    pinnedBarDismissedByConversation.value = {
      ...pinnedBarDismissedByConversation.value,
      [activeSessionId.value]: false,
    };
  }
  const idx = findMessageIndexById(payload.message_id);
  if (idx < 0) return;
  const row = { ...asRecord(messages.value[idx]) } as Record<string, unknown>;
  const extra = { ...(row.extra as Record<string, string> | undefined) };
  extra.pinned = payload.pinned ? "true" : "false";
  row.extra = extra;
  messages.value[idx] = asSdkMessage(row);
  messages.value = [...messages.value];
}

function applyMarkChange(payload: {
  conversation_id: string;
  message_id: string;
  marked: boolean;
  mark_type?: number;
  color?: string;
}): void {
  if (!payload || payload.conversation_id !== activeSessionId.value) return;
  const idx = findMessageIndexById(payload.message_id);
  if (idx < 0) return;
  const row = { ...asRecord(messages.value[idx]) } as Record<string, unknown>;
  const extra = { ...(row.extra as Record<string, string> | undefined) };
  if (payload.marked) {
    if (typeof payload.mark_type === "number" && Number.isFinite(payload.mark_type)) {
      extra.mark_type = String(payload.mark_type);
    }
    if (payload.color && String(payload.color).trim()) {
      extra.mark_color = String(payload.color);
    }
  } else {
    delete extra.mark_type;
    delete extra.mark_color;
  }
  row.extra = extra;
  messages.value[idx] = asSdkMessage(row);
  messages.value = [...messages.value];
}

function applyReadReceipt(payload: {
  conversation_id: string;
  user_id: string;
  read_seq: number;
  message_ids?: string[];
}): void {
  if (!payload || payload.conversation_id !== activeSessionId.value) return;
  const seq = Number(payload.read_seq ?? 0);
  if (!(seq > 0)) return;
  const targetIds = new Set((payload.message_ids ?? []).map((x) => String(x)));
  let changed = false;
  messages.value = messages.value.map((m) => {
    const r = asRecord(m);
    const mSeq = Number(r.seq ?? 0);
    const sid = String(r.serverId ?? r.server_id ?? "");
    const shouldRead = targetIds.size > 0 ? targetIds.has(sid) : mSeq > 0 && mSeq <= seq;
    if (!shouldRead) return m;
    const row = { ...r } as Record<string, unknown>;
    row.isRead = true;
    row.is_read = true;
    // 对方已读自己发送的消息：升级到已读状态（双对号）。
    const senderId = String(row.senderId ?? row.sender_id ?? "");
    const selfId = String(currentUserId.value ?? "");
    if (selfId && payload.user_id && payload.user_id !== selfId && senderId === selfId) {
      row.status = 4;
    }
    changed = true;
    return asSdkMessage(row);
  });
  if (changed) messages.value = [...messages.value];
}

function applyReactionChange(payload: {
  conversation_id: string;
  message_id: string;
  user_id: string;
  emoji: string;
  action: number;
}): void {
  if (!payload || payload.conversation_id !== activeSessionId.value) return;
  if (!payload.message_id || !payload.user_id || !payload.emoji) return;

  const idx = messages.value.findIndex((m) => {
    const r = asRecord(m);
    const sid = String(r.serverId ?? r.server_id ?? "");
    const cid = String(r.clientMsgId ?? r.client_msg_id ?? "");
    return sid === payload.message_id || cid === payload.message_id;
  });
  if (idx < 0) return;

  const row = { ...asRecord(messages.value[idx]) } as Record<string, unknown>;
  const raw = row.reactions;
  const reactions = Array.isArray(raw) ? [...raw] as Array<Record<string, unknown>> : [];
  const reactionIdx = reactions.findIndex((r) => String(r.emoji ?? "") === payload.emoji);
  const isRemove = payload.action === 2;
  if (reactionIdx < 0) {
    if (!isRemove) {
      reactions.push({
        emoji: payload.emoji,
        userIds: [payload.user_id],
        count: 1,
      });
    }
  } else {
    const target = { ...reactions[reactionIdx] };
    const currentUserIdsRaw = target.userIds ?? target.user_ids;
    const currentUserIds = Array.isArray(currentUserIdsRaw)
      ? currentUserIdsRaw.map((v) => String(v))
      : [];
    const nextUserIds = isRemove
      ? currentUserIds.filter((uid) => uid !== payload.user_id)
      : (currentUserIds.includes(payload.user_id)
          ? currentUserIds
          : [...currentUserIds, payload.user_id]);
    if (nextUserIds.length === 0) {
      reactions.splice(reactionIdx, 1);
    } else {
      target.userIds = nextUserIds;
      target.user_ids = nextUserIds;
      target.count = nextUserIds.length;
      reactions[reactionIdx] = target;
    }
  }

  row.reactions = reactions;
  messages.value[idx] = asSdkMessage(row);
  messages.value = [...messages.value];
}

async function addReactionById(messageId: string, emoji: string) {
  if (!activeSessionId.value) return;
  if (!emoji || !emoji.trim()) {
    console.warn("[Chat] 添加反应失败: emoji 为空");
    return;
  }
  try {
    console.log("[Chat] 添加反应:", { client_msg_id: messageId, emoji, sessionId: activeSessionId.value });
    // 前端传入 client_msg_id，SDK 内部会转换为 server_id
    await invoke("sdk_add_reaction", {
      messageId,
      emoji,
    });
    console.log("[Chat] 添加反应请求已发送");
    // 注意：不需要立即同步和重新获取消息列表
    // SDK 会发布 MessageReactionAdded 事件，后端会重新查询消息并发送 im://message 事件
    // 前端通过 im://message 事件处理器自动更新消息状态（包含 reactions）
  } catch (e) {
    handleSdkError(e, "addReaction");
  }
}

async function removeReactionById(messageId: string, emoji: string) {
  if (!activeSessionId.value) return;
  try {
    console.log("[Chat] 移除反应:", { client_msg_id: messageId, emoji, sessionId: activeSessionId.value });
    // 前端传入 client_msg_id，SDK 内部会转换为 server_id
    await invoke("sdk_remove_reaction", {
      messageId,
      emoji,
    });
    console.log("[Chat] 移除反应请求已发送");
    // 注意：不需要立即同步和重新获取消息列表
    // SDK 会发布 MessageReactionRemoved 事件，后端会重新查询消息并发送 im://message 事件
    // 前端通过 im://message 事件处理器自动更新消息状态（包含 reactions）
  } catch (e) {
    handleSdkError(e, "removeReaction");
  }
}

/** 从消息列表计算「加载更多」的 before_seq（取最小 seq，下次请求 seq < 该值） */
function computeNextBeforeSeq(list: SDKMessage[]): number | null {
  if (!list.length) return null;
  const seqs = list.map((m) => Number(asRecord(m).seq ?? 0)).filter((s) => s > 0);
  if (!seqs.length) return null;
  const minSeq = Math.min(...seqs);
  return minSeq;
}

/** @param skipEmptyPrompt 为 true 时不展示「暂无会话」提示且不自动打开创建会话弹窗（用于创建会话后刷新列表，避免重复提示） */
async function loadSessions(skipEmptyPrompt = false) {
  loadingSessions.value = true;
  try {
    const list = await invoke<Conversation[]>("sdk_conversation_list");
    const dedup = new Map<string, Conversation>();
    for (const session of list) {
      const cid = conversationIdFromSession(session);
      if (!cid) continue;
      dedup.set(cid, { ...session, unreadCount: toSafeUnread(session.unreadCount ?? asRecord(session).unread_count) });
    }
    const normalized = [...dedup.values()];
    sessions.value = normalized.sort((a, b) => {
      const ua = toSafeUnread(a.unreadCount ?? asRecord(a).unread_count);
      const ub = toSafeUnread(b.unreadCount ?? asRecord(b).unread_count);
      if (ua !== ub) return ub - ua;
      const lastA: MessagePreviewElem | null | undefined = a.lastMessage ?? a.last_message;
      const lastB: MessagePreviewElem | null | undefined = b.lastMessage ?? b.last_message;
      const ta =
        lastA?.time != null
          ? typeof lastA.time === "number"
            ? lastA.time
            : new Date(lastA.time).getTime()
          : a.updatedAt ?? asRecord(a).updated_at ?? 0;
      const tb =
        lastB?.time != null
          ? typeof lastB.time === "number"
            ? lastB.time
            : new Date(lastB.time).getTime()
          : b.updatedAt ?? asRecord(b).updated_at ?? 0;
      return Number(tb) - Number(ta);
    });
    totalUnread.value = sessions.value.reduce(
      (acc, s) => acc + toSafeUnread(s.unreadCount ?? (asRecord(s).unread_count as number | undefined)),
      0,
    );
    if (!activeSessionId.value && list.length) {
      const firstId = conversationIdFromSession(list[0]);
      if (firstId) void selectSession(firstId, { markRead: false });
    } else if (list.length === 0 && !skipEmptyPrompt && !activeSessionId.value) {
      Message.info("暂无会话，请先创建一个会话");
      createVisible.value = true;
    }
  } catch (e) {
    handleSdkError(e, "loadSessions");
    sessions.value = [];
    totalUnread.value = 0;
  } finally {
    loadingSessions.value = false;
  }
}

// 会话列表由 SDK 连接后自动同步，无 sdk_sync 命令；刷新列表直接拉取
async function refreshSessions() {
  await loadSessions();
}

const INITIAL_BEFORE_SEQ = Number.MAX_SAFE_INTEGER; // 首次拉取「最新」消息：seq < 该值

/** SDK 已落库后防抖拉当前会话，弥补 merge 与路由空窗 */
let refreshMessagesTimer: ReturnType<typeof setTimeout> | null = null;
let markReadTimer: ReturnType<typeof setTimeout> | null = null;

async function markSessionReadAll(conversationId: string): Promise<void> {
  if (!conversationId) return;
  // 协议语义：readSeq=0 表示“标记到会话当前最新位点”。
  await invoke("sdk_mark_session_read", { conversationId, readSeq: 0 });
}

function scheduleMarkActiveSessionRead(): void {
  if (markReadTimer) clearTimeout(markReadTimer);
  markReadTimer = setTimeout(async () => {
    markReadTimer = null;
    const id = activeSessionId.value;
    if (!id) return;
    try {
      await markSessionReadAll(id);
      await loadSessions(true);
    } catch {
      // 忽略短时失败，后续消息会再次触发。
    }
  }, 180);
}
/** 开发态：观察从 DB 拉取后的 content 形状（含 Rust 扁平 vs 嵌套规范化结果） */
function debugLogLoadedMessages(tag: string, list: SDKMessage[]) {
  if (!import.meta.env.DEV) return;
  const brief = list.map((m) => {
    const r = asRecord(m);
    const c = getMessageContent(r);
    return {
      serverId: r.serverId ?? r.server_id,
      clientMsgId: r.clientMsgId ?? r.client_msg_id,
      seq: r.seq,
      messageType: r.messageType ?? r.message_type,
      contentType: c?.contentType,
      content: c,
    };
  });
  console.log(`[Chat] ${tag} count=${list.length}`, brief);
}

function scheduleReloadActiveMessagesFromDb() {
  if (refreshMessagesTimer) clearTimeout(refreshMessagesTimer);
  refreshMessagesTimer = setTimeout(async () => {
    refreshMessagesTimer = null;
    const id = activeSessionId.value;
    if (!id) return;
    try {
      const list = await invoke<SDKMessage[]>("sdk_list_messages", {
        conversationId: id,
        beforeSeq: INITIAL_BEFORE_SEQ,
        limit: 50,
      });
      messages.value = enrichQuoteContext(sortMessages(reconcileDisplayMessages(list, messages.value)));
      nextBeforeSeq.value = computeNextBeforeSeq(list);
      debugLogLoadedMessages(`scheduleReloadActiveMessagesFromDb conv=${id}`, messages.value);
    } catch {
      /* 忽略短时竞争 */
    }
  }, 120);
}

type SelectSessionOptions = {
  markRead?: boolean;
};

async function selectSession(id: string, options?: SelectSessionOptions) {
  if (!id) return;
  const shouldMarkRead = options?.markRead ?? true;
  activeSessionId.value = id;
  editingMessageId.value = null;
  loadingMessages.value = true;
  try {
    const list = await invoke<SDKMessage[]>("sdk_list_messages", {
      conversationId: id,
      beforeSeq: INITIAL_BEFORE_SEQ,
      limit: 50,
    });
    messages.value = enrichQuoteContext(sortMessages(reconcileDisplayMessages(list, messages.value)));
    nextBeforeSeq.value = computeNextBeforeSeq(list);
    debugLogLoadedMessages(`selectSession conv=${id}`, messages.value);
    await nextTick();
    messageListRef.value?.scrollToBottom();
    if (shouldMarkRead) {
      await markSessionReadAll(id);
    }
    await loadSessions();
    textToSend.value = sessionDrafts.value[id] ?? "";
  } catch (e) {
    handleSdkError(e, "selectSession");
  } finally {
    loadingMessages.value = false;
  }
}

async function markAllSessionsRead() {
  for (const session of sessions.value) {
    const conversationId = conversationIdFromSession(session);
    if (!conversationId) continue;
    try {
      await markSessionReadAll(conversationId);
    } catch (e) {
      console.warn("[Chat] markAllSessionsRead failed", conversationId, e);
    }
  }
  await loadSessions();
}

async function sendText() {
  console.log("[Chat] sendText", {
    activeSessionId: activeSessionId.value,
    textLength: textToSend.value?.length || 0,
    editingMessageId: editingMessageId.value,
    isEditing: !!editingMessageId.value
  });
  
  if (!activeSessionId.value || !textToSend.value || !textToSend.value.trim()) {
    console.warn("[Chat] sendText 参数无效，无法发送");
    return;
  }
  
  // 检查是否处于编辑模式
  if (editingMessageId.value) {
    // 编辑模式：直接提交编辑（使用 textToSend.value）
    console.log("[Chat] 检测到编辑模式，提交编辑");
    await submitEditMessage();
    return;
  }

  // 回复/引用模式：使用主输入框直接发送引用消息
  if (activeReplyMessageId.value) {
    const quotedMsg = await resolveReplySourceById(activeReplyMessageId.value);
    if (!quotedMsg) {
      Message.error("未找到被引用消息，请重试");
      return;
    }
    const preview = getQuotePreview(quotedMsg);
    const replyText = textToSend.value.trim();
    if (!replyText) return;
    try {
      const created = await invoke<SDKMessage>("sdk_create_quote", {
        conversationId: activeSessionId.value,
        quotedMessageId: activeReplyMessageId.value,
        text: replyText,
        quotedMessage: quotedMsg ?? null,
        quotedTextPreview: preview || null,
      });
      const ack = await invoke<SendAckPayload>("sdk_send", { message: created });
      if (!ack.success) throw new Error(ack.errorMessage || "send failed");
      textToSend.value = "";
      if (activeSessionId.value) sessionDrafts.value[activeSessionId.value] = "";
      clearReplyContext();
      await syncActiveSession();
      await selectSession(activeSessionId.value);
      return;
    } catch (e) {
      handleSdkError(e, "replyMessage");
      return;
    }
  }
  
  // 发送新消息模式
  const text = textToSend.value;
  textToSend.value = "";
  if (activeSessionId.value) sessionDrafts.value[activeSessionId.value] = "";
  
  try {
    console.log("[Chat] 准备发送普通文本消息:", { 
      sessionId: activeSessionId.value, 
      textLength: text.length,
      editingMessageId: editingMessageId.value // 应该是 null
    });

    // 前端只负责触发 create + send；路由字段与发送状态由 SDK 内部处理。
    const created = await invoke<SDKMessage>("sdk_create_text", {
      conversationId: activeSessionId.value,
      text,
    });
    const ack = await invoke<SendAckPayload>("sdk_send", { message: created });
    if (!ack.success) {
      throw new Error(ack.errorMessage || "sdk_send failed");
    }
    scheduleReloadActiveMessagesFromDb();
    await loadSessions();
    await nextTick();
    setTimeout(() => {
      messageListRef.value?.scrollToBottom();
    }, 200);
  } catch (e) {
    handleSdkError(e, "sendText");
    textToSend.value = text;
    if (activeSessionId.value) sessionDrafts.value[activeSessionId.value] = text;
  }
}

function mediaFilters(kind: MediaKind): Array<{ name: string; extensions: string[] }> {
  switch (kind) {
    case "image":
      return [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }];
    case "video":
      return [{ name: "Videos", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];
    case "audio":
      return [{ name: "Audio", extensions: ["mp3", "wav", "aac", "m4a", "ogg", "flac"] }];
    case "file":
      // 勿使用 extensions: ["*"]：在 macOS 上会被当成字面后缀，导致普通文件全部灰显不可选。
      return [];
    default:
      return [];
  }
}

const mediaFiltersImageOrVideo: Array<{ name: string; extensions: string[] }> = [
  { name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] },
  { name: "Videos", extensions: ["mp4", "mov", "mkv", "avi", "webm", "m4v"] },
];

function resetMediaPreview() {
  mediaPreviewPath.value = "";
  mediaPreviewIsVideo.value = false;
  mediaPreviewCaption.value = "";
  mediaPreviewCoverPath.value = null;
  mediaPreviewGenerating.value = false;
}

function cancelMediaPreview() {
  mediaPreviewVisible.value = false;
  resetMediaPreview();
}

/** 输入栏「+」：图片/视频 | 音频 | 文件 */
async function sendMediaFromComposer(menu: MediaMenuKind) {
  if (mediaSending.value) {
    Message.warning("媒体正在发送中，请稍候");
    return;
  }
  if (!activeSessionId.value) {
    Message.warning("请先选择会话");
    return;
  }
  if (menu === "imageOrVideo") {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: mediaFiltersImageOrVideo,
        title: "选择图片或视频",
      });
      const filePath = Array.isArray(selected) ? selected[0] : selected;
      if (!filePath || typeof filePath !== "string") return;
      mediaPreviewPath.value = filePath;
      mediaPreviewIsVideo.value = isVideoFilePath(filePath);
      mediaPreviewCaption.value = "";
      mediaPreviewCoverPath.value = null;
      mediaPreviewVisible.value = true;
      if (mediaPreviewIsVideo.value) {
        mediaPreviewGenerating.value = true;
        try {
          const dataUrl = await captureVideoFrameDataUrl(filePath);
          const comma = dataUrl.indexOf(",");
          const b64 = comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
          const tmpPath = await invoke<string>("sdk_save_preview_jpeg_temp", { base64Jpeg: b64 });
          mediaPreviewCoverPath.value = tmpPath;
        } catch (e) {
          console.error("[Chat] video poster", e);
          Message.error("无法生成视频封面，请换一段视频或稍后重试");
          cancelMediaPreview();
        } finally {
          mediaPreviewGenerating.value = false;
        }
      }
    } catch (e) {
      handleSdkError(e, "sendMedia.pickImageOrVideo");
    }
    return;
  }
  const kind: MediaKind = menu === "audio" ? "audio" : "file";
  await openFilePickerAndSendMedia(kind);
}

async function openFilePickerAndSendMedia(kind: MediaKind) {
  if (mediaSending.value) {
    Message.warning("媒体正在发送中，请稍候");
    return;
  }
  if (!activeSessionId.value) {
    Message.warning("请先选择会话");
    return;
  }

  try {
    const selected = await open({
      multiple: false,
      directory: false,
      ...(kind === "file" ? {} : { filters: mediaFilters(kind) }),
      title: "选择要发送的文件",
    });
    const filePath = Array.isArray(selected) ? selected[0] : selected;
    if (!filePath || typeof filePath !== "string") return;
    await sendMediaWithRetry(kind, filePath);
  } catch (e) {
    handleSdkError(e, "sendMedia");
  }
}

async function confirmMediaPreviewSend() {
  const path = mediaPreviewPath.value.trim();
  if (!path) return;
  if (mediaPreviewIsVideo.value && !mediaPreviewCoverPath.value) {
    Message.warning(mediaPreviewGenerating.value ? "正在生成视频封面…" : "缺少视频封面，请关闭后重新选择");
    return;
  }
  const caption = mediaPreviewCaption.value.trim();
  const isVid = mediaPreviewIsVideo.value;
  const cover = mediaPreviewCoverPath.value;
  mediaPreviewVisible.value = false;
  const opts: SendMediaOptions = { caption, coverPath: isVid ? cover : null };
  try {
    await sendMediaWithRetry(isVid ? "video" : "image", path, opts);
  } finally {
    resetMediaPreview();
  }
}

function askRetryForMedia(kind: MediaKind, filePath: string): Promise<boolean> {
  const fileName = getFileName(filePath);
  return new Promise((resolve) => {
    Modal.confirm({
      title: "媒体发送失败",
      content: `${mediaKindLabel(kind)}「${fileName}」发送失败，是否重试？`,
      okText: "重试",
      cancelText: "取消",
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    });
  });
}

async function sendMediaOnce(
  kind: MediaKind,
  filePath: string,
  options?: SendMediaOptions,
): Promise<void> {
  if (!activeSessionId.value) {
    throw new Error("会话不存在");
  }
  mediaSending.value = true;
  mediaSendingLabel.value = `正在准备${mediaKindLabel(kind)}...`;
  mediaProgressPercent.value = null;
  let optimisticClientMsgId = "";
  try {
    let created: SDKMessage;
    switch (kind) {
      case "image":
        created = await invoke<SDKMessage>("sdk_create_image", {
          conversationId: activeSessionId.value,
          imageId: filePath,
        });
        break;
      case "video":
        created = await invoke<SDKMessage>("sdk_create_video", {
          conversationId: activeSessionId.value,
          videoId: filePath,
        });
        break;
      case "audio":
        created = await invoke<SDKMessage>("sdk_create_audio", {
          conversationId: activeSessionId.value,
          audioId: filePath,
        });
        break;
      case "file":
        created = await invoke<SDKMessage>("sdk_create_file", {
          conversationId: activeSessionId.value,
          fileId: filePath,
        });
        break;
      default:
        return;
    }

    const cap = String(options?.caption ?? "").trim();
    let toSend = created;
    if (kind === "image") {
      toSend = applyFlatImagePayload(created, filePath, cap);
    } else if (kind === "video") {
      const cov = String(options?.coverPath ?? "").trim();
      if (!cov) {
        throw new Error("发送视频需要封面路径");
      }
      toSend = applyFlatVideoPayload(created, filePath, cap, cov);
    }

    const optimistic = withOptimisticMediaPreview(toSend, kind, filePath, options);
    const optimisticId = String(asRecord(optimistic).clientMsgId ?? asRecord(optimistic).client_msg_id ?? "").trim();
    optimisticClientMsgId = optimisticId;
    activeUploadingMessageId.value = optimisticId || null;
    addOptimisticMediaMessage(optimistic);
    mediaSendingLabel.value = `正在上传并发送${mediaKindLabel(kind)}...`;
    const ack = await invoke<SendAckPayload>("sdk_send_with_media_progress", { message: toSend });
    if (!ack.success) {
      throw new Error(ack.errorMessage || "sdk_send failed");
    }
    clearLocalUploadState(optimisticId || ack.clientMsgId);
    scheduleReloadActiveMessagesFromDb();
    await loadSessions();
    await nextTick();
    setTimeout(() => {
      messageListRef.value?.scrollToBottom();
    }, 120);
    Message.success(`${mediaKindLabel(kind)}发送成功`);
  } catch (error) {
    if (optimisticClientMsgId) {
      patchMessageByIdentity(optimisticClientMsgId, (row) => {
        row.status = 5;
        const existing = row.localUpload as LocalUploadState | undefined;
        if (existing) {
          row.localUpload = { ...existing, phase: "Failed" };
        }
        return row;
      });
      if (activeUploadingMessageId.value === optimisticClientMsgId) {
        activeUploadingMessageId.value = null;
      }
    }
    throw error;
  } finally {
    mediaSending.value = false;
    mediaSendingLabel.value = "";
    mediaProgressPercent.value = null;
  }
}

async function sendMediaWithRetry(
  kind: MediaKind,
  filePath: string,
  options?: SendMediaOptions,
): Promise<void> {
  try {
    await sendMediaOnce(kind, filePath, options);
    return;
  } catch (e) {
    const retry = await askRetryForMedia(kind, filePath);
    if (!retry) {
      handleSdkError(e, "sendMedia.cancelled");
      return;
    }
    try {
      await sendMediaOnce(kind, filePath, options);
    } catch (retryErr) {
      handleSdkError(retryErr, "sendMedia.retry");
    }
  }
}

async function sendThreadReply() {
  if (!activeSessionId.value || !threadTargetId.value || !threadReplyText.value) return;
  const created = await invoke<SDKMessage>("sdk_create_thread_reply", {
    conversationId: activeSessionId.value,
    threadId: threadTargetId.value,
    text: threadReplyText.value,
  });
  const ack = await invoke<SendAckPayload>("sdk_send", { message: created });
  if (!ack.success) throw new Error(ack.errorMessage || "send failed");
  threadReplyText.value = "";
  threadTargetId.value = null;
  await syncActiveSession();
  await selectSession(activeSessionId.value);
}

async function createSession() {
  try {
    const peerIdForSingle = createPeerId.value?.trim() || null;
    const typeStr = createSessionType.value === "group" ? "group" : "single";
    const conversationTypeNum = typeStr === "group" ? 2 : 1;
    const sourceId =
      typeStr === "single" && peerIdForSingle
        ? peerIdForSingle
        : createDisplayName.value?.trim() || createBusinessType.value || `group-${Date.now()}`;
    const conv = await invoke<Conversation>("sdk_conversation_get_one", {
      sourceId: sourceId,
      conversationType: conversationTypeNum,
    });
    const sid = String(asRecord(conv).conversation_id ?? asRecord(conv).conversationId ?? "");
    createVisible.value = false;
    createDisplayName.value = "";
    createPeerId.value = "";
    if (sid) {
      markConversationAsNew(sid);
      if (peerIdForSingle && typeStr === "single") saveSingleChatPeer(sid, peerIdForSingle);
    }
    await loadSessions(true);
    if (sessions.value.length === 0 && conv) {
      sessions.value = [conv];
    }
    if (sid) selectSession(sid);
  } catch (e) {
    handleSdkError(e, "createSession");
  }
}

async function loadMore() {
  if (!activeSessionId.value || nextBeforeSeq.value == null) return;
  try {
    const more = await invoke<SDKMessage[]>("sdk_list_messages", {
      conversationId: activeSessionId.value,
      beforeSeq: nextBeforeSeq.value,
      limit: 50,
    });
    if (more.length === 0) {
      nextBeforeSeq.value = null;
      return;
    }
    const existingIds = new Set(messages.value.map((m) => getMsgServerId(asRecord(m))));
    const newMessages = more.filter((m) => !existingIds.has(getMsgServerId(asRecord(m))));
    if (newMessages.length > 0) {
      messages.value = enrichQuoteContext(sortMessages([...newMessages, ...messages.value]));
    }
    nextBeforeSeq.value = computeNextBeforeSeq(more);
    await nextTick();
  } catch (e) {
    handleSdkError(e, "loadMore");
  }
}

async function recallMessage(messageId: string) {
  if (!activeSessionId.value) {
    Message.warning("请先选择会话");
    return;
  }
  const id = String(messageId ?? "").trim();
  if (!id) {
    Message.warning("无法识别消息 ID");
    return;
  }
  try {
    await invoke("sdk_recall", { messageId: id });
    await syncActiveSession();
    await loadSessions();
    Message.success("消息已撤回");
  } catch (e) {
    handleSdkError(e, "recallMessage");
  }
}

function startEditMessage(m: SDKMessage) {
  const r = asRecord(m);
  editingMessageId.value = getMsgClientMsgId(r) || getMsgServerId(r);
  const messageText = getEditablePlainTextFromMessage(r);
  textToSend.value = messageText;
  const sid = activeSessionId.value;
  if (sid) {
    sessionDrafts.value = { ...sessionDrafts.value, [sid]: messageText };
  }
  void nextTick(() => {
    const el = document.querySelector(".composer-area textarea") as HTMLTextAreaElement | null;
    el?.focus();
    el?.setSelectionRange(el.value.length, el.value.length);
  });
}

function cancelEditMode() {
  editingMessageId.value = null;
  textToSend.value = "";
  const id = activeSessionId.value;
  if (id) sessionDrafts.value = { ...sessionDrafts.value, [id]: "" };
}

async function submitEditMessage() {
  if (!activeSessionId.value || !editingMessageId.value) {
    return;
  }

  const editText = textToSend.value.trim();
  if (!editText) {
    Message.warning("编辑内容不能为空");
    return;
  }

  try {
    const targetMessageId = editingMessageId.value;
    await invoke("sdk_edit_text_by_message_id", {
      messageId: targetMessageId,
      text: editText,
    });

    editingMessageId.value = null;
    textToSend.value = "";

    await syncActiveSession();
    await loadSessions();
    Message.success("消息已编辑");
  } catch (e) {
    handleSdkError(e, "editMessage");
  }
}

async function resendMessage(messageId: string, text: string) {
  if (!activeSessionId.value || !text) return;

  try {
    const created = await invoke<SDKMessage>("sdk_create_text", {
      conversationId: activeSessionId.value,
      text,
    });
    const ack = await invoke<SendAckPayload>("sdk_send", { message: created });
    if (!ack.success) throw new Error(ack.errorMessage || "send failed");
    removeMessageFromCurrentSession(messageId);
    scheduleReloadActiveMessagesFromDb();
    await syncActiveSession();
    await loadSessions();
    Message.success("消息重发成功");
  } catch (e) {
    handleSdkError(e, "resendMessage");
  }
}

onMounted(async () => {
  const isTauriEnv = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  if (!isTauriEnv) {
    Message.warning("当前为浏览器预览环境，部分功能可能无法使用");
  }
  currentUserId.value = localStorage.getItem("userId");

  async function ensureSdkInit() {
    const dataUrl = await resolveSdkDataUrl();
    await invoke("sdk_init", {
      args: { environment: "development", sdkConfig: { dataUrl } },
    });
  }

  // 若已从登录页完成 sdk_login，则不再重复调用，避免重复登录
  const connected = isTauriEnv && (await invoke<boolean>("sdk_is_connected").catch(() => false));
  if (connected) {
    await ensureSdkInit().catch(() => {});
    await listenersReady;
    refreshSessions();
    return;
  }
  const doConnect = async (uid: string, token: string) => {
    try {
      await ensureSdkInit();
      clearSyncState();
      await listenersReady;
      await invoke("sdk_login", { userId: uid, token });
      await waitForFullSync(20000);
      await refreshSessions();
    } catch (e: unknown) {
      handleSdkError(e, "sdk_login");
    }
  };
  if (currentUserId.value) {
    try {
      const t = await invoke<string>("sdk_generate_test_token", {
        secret: "insecure-secret",
        issuer: "flare-im-core",
        userId: currentUserId.value,
      });
      await doConnect(currentUserId.value, t);
    } catch {
      await doConnect(currentUserId.value, "");
    }
  } else {
    const defaultUser = "123456";
    localStorage.setItem("userId", defaultUser);
    currentUserId.value = defaultUser;
    try {
      const t = await invoke<string>("sdk_generate_test_token", {
        secret: "insecure-secret",
        issuer: "flare-im-core",
        userId: defaultUser,
      });
      await doConnect(defaultUser, t);
    } catch {
      await doConnect(defaultUser, "");
    }
  }
});

function onDraftChange(v: string) {
  textToSend.value = v;
  if (activeSessionId.value) sessionDrafts.value[activeSessionId.value] = v;
}

async function onTyping(action: "typing" | "stop") {
  if (!activeSessionId.value) return;
  await invoke("sdk_typing", {
    conversationId: activeSessionId.value,
    typing: action === "typing",
  });
}

// 转发消息
const forwardVisible = ref(false);
const forwardMessageIds = ref<string[]>([]);
const forwardTargetSessionId = ref<string | null>(null);
const forwardMerge = ref(false);
const forwardReason = ref("");

async function handleForward(messageId: string) {
  forwardMessageIds.value = [messageId];
  forwardTargetSessionId.value = null;
  forwardMerge.value = false;
  forwardReason.value = "";
  forwardVisible.value = true;
}

async function confirmForward() {
  if (!forwardTargetSessionId.value || forwardMessageIds.value.length === 0) {
    Message.warning("请选择目标会话");
    return;
  }
  try {
    const created = await invoke<SDKMessage>("sdk_create_forward", {
      conversationId: forwardTargetSessionId.value,
      messageIds: forwardMessageIds.value,
    });
    const ack = await invoke<SendAckPayload>("sdk_send", { message: created });
    if (!ack.success) throw new Error(ack.errorMessage || "send failed");
    Message.success("转发成功");
    forwardVisible.value = false;
    await selectSession(activeSessionId.value!);
  } catch (e) {
    handleSdkError(e, "forwardMessage");
  }
}

// 引用/回复消息
const quoteMessageId = ref<string | null>(null);

function getQuotePreview(message?: SDKMessage): string {
  if (!message) return "[非文本消息]";
  const content = getMessageContent(asRecord(message));
  if (content?.contentType === "quote") {
    const q = (content.quote ?? {}) as Record<string, unknown>;
    const rc =
      (q.currentContent as ContentElem | undefined) ??
      (q.current_content as ContentElem | undefined) ??
      (q.replyContent as ContentElem | undefined) ??
      (q.reply_content as ContentElem | undefined);
    if (!rc) return content.quote?.quotedTextPreview || "[非文本消息]";
    if (rc.contentType === "text" && rc.text?.text?.trim()) return rc.text.text.trim();
    return getContentDecodedPreview(rc) || "[非文本消息]";
  }
  if (content?.contentType === 'text' && content.text?.text?.trim()) return content.text.text.trim();
  return getContentDecodedPreview(content) || "[非文本消息]";
}

function isPinnedMessage(message: SDKMessage): boolean {
  const r = asRecord(message);
  const extra = (r.extra as Record<string, string> | undefined) ?? {};
  const attrs = (r.attributes as Record<string, string> | undefined) ?? {};
  return extra.pinned === "true" || attrs.pinned === "true";
}

const pinnedMessages = computed(() => {
  return [...messages.value]
    .filter((m) => isPinnedMessage(m))
    .sort((a, b) => Number(asRecord(b).timestamp ?? 0) - Number(asRecord(a).timestamp ?? 0));
});

const activePinnedMessage = computed(() => pinnedMessages.value[0]);

const showPinnedBar = computed(() => {
  if (!activeSessionId.value || !activePinnedMessage.value) return false;
  return !pinnedBarDismissedByConversation.value[activeSessionId.value];
});

const pinnedBarPreview = computed(() => getQuotePreview(activePinnedMessage.value).slice(0, 120));

const pinnedBarSender = computed(() => {
  const m = activePinnedMessage.value;
  if (!m) return "";
  const r = asRecord(m);
  return String(r.senderDisplayName ?? r.senderName ?? r.senderId ?? r.sender_id ?? "");
});

function dismissPinnedBar() {
  if (!activeSessionId.value) return;
  pinnedBarDismissedByConversation.value = {
    ...pinnedBarDismissedByConversation.value,
    [activeSessionId.value]: true,
  };
}

function focusPinnedMessageById(id: string) {
  if (!id) return;
  pinnedDrawerVisible.value = false;
  pinnedFocusMessageId.value = id;
  (messageListRef.value as any)?.scrollToMessage?.(id, true);
  setTimeout(() => {
    if (pinnedFocusMessageId.value === id) pinnedFocusMessageId.value = null;
  }, 1800);
}

function openPinnedDrawer() {
  if (!activePinnedMessage.value) return;
  pinnedDrawerVisible.value = true;
}

async function unpinActivePinnedMessage() {
  const m = activePinnedMessage.value;
  if (!m) return;
  const r = asRecord(m);
  const id = getMsgClientMsgId(r) || getMsgServerId(r);
  if (!id) return;
  await handleUnpin(id);
}

async function unpinPinnedMessageById(id: string) {
  if (!id) return;
  await handleUnpin(id);
}

function findMessageInCurrentList(messageId: string): SDKMessage | undefined {
  if (!messageId) return undefined;
  return messages.value.find((m) => {
    const r = asRecord(m);
    return getMsgClientMsgId(r) === messageId || getMsgServerId(r) === messageId;
  });
}

async function resolveReplySourceById(messageId: string): Promise<SDKMessage | undefined> {
  const local = findMessageInCurrentList(messageId);
  if (local) return local;
  try {
    const fetched = await invoke<SDKMessage | null>("sdk_get_message", { messageId });
    if (fetched) return fetched;
  } catch {
    // ignore; caller will handle missing source
  }
  return undefined;
}

function activateReplyContext(messageId: string) {
  replyToMessageId.value = messageId;
  quoteMessageId.value = messageId;
}

function clearReplyContext() {
  replyToMessageId.value = null;
  quoteMessageId.value = null;
}

const activeReplyMessageId = computed(() => quoteMessageId.value || replyToMessageId.value || null);
const activeReplySource = computed(() => {
  const targetId = activeReplyMessageId.value;
  if (!targetId) return undefined;
  return messages.value.find(
    (m) =>
      getMsgClientMsgId(asRecord(m)) === targetId ||
      getMsgServerId(asRecord(m)) === targetId,
  );
});
const activeReplySender = computed(() => {
  const m = activeReplySource.value;
  if (!m) return "对方";
  const r = asRecord(m);
  return String(r.senderDisplayName ?? r.senderName ?? r.senderId ?? r.sender_id ?? "对方");
});
const activeReplyPreview = computed(() => getQuotePreview(activeReplySource.value).slice(0, 120));

function getReplyTargetId(message: SDKMessage): string {
  const raw = asRecord(message);
  const direct = raw.replyTo ?? raw.reply_to;
  if (typeof direct === "string" && direct.trim()) return direct.trim();
  const content = getMessageContent(raw);
  if (content?.contentType === "quote" && content.quote?.quotedMessageId) {
    return String(content.quote.quotedMessageId);
  }
  return "";
}

const replyCountMap = computed<Record<string, number>>(() => {
  const counts: Record<string, number> = {};
  const idMap = new Map<string, string[]>();
  for (const m of messages.value) {
    const r = asRecord(m);
    const ids = [getMsgServerId(r), getMsgClientMsgId(r)].filter((x) => x);
    if (ids.length === 0) continue;
    ids.forEach((id) => idMap.set(id, ids));
  }
  for (const m of messages.value) {
    const target = getReplyTargetId(m);
    if (!target) continue;
    const related = idMap.get(target) ?? [target];
    for (const id of related) counts[id] = (counts[id] ?? 0) + 1;
  }
  return counts;
});

function getSenderName(message?: SDKMessage): string {
  if (!message) return "未知用户";
  const r = asRecord(message);
  return String(r.senderDisplayName ?? r.senderName ?? r.senderId ?? r.sender_id ?? "未知用户");
}

function openReplyDetail(messageId: string) {
  replyDetailMessageId.value = messageId;
  replyDetailVisible.value = true;
}

const replyDetailRoot = computed(() => {
  if (!replyDetailMessageId.value) return undefined;
  return messages.value.find((m) => {
    const r = asRecord(m);
    return getMsgServerId(r) === replyDetailMessageId.value || getMsgClientMsgId(r) === replyDetailMessageId.value;
  });
});

const replyDetailRootSender = computed(() => getSenderName(replyDetailRoot.value));

const replyDetailItems = computed(() => {
  const root = replyDetailRoot.value;
  if (!root) return [];
  const rootIds = new Set([getMsgServerId(asRecord(root)), getMsgClientMsgId(asRecord(root))].filter((x) => x));
  return messages.value.filter((m) => {
    const target = getReplyTargetId(m);
    return !!target && rootIds.has(target);
  }).sort((a, b) => Number(asRecord(a).timestamp ?? 0) - Number(asRecord(b).timestamp ?? 0));
});

function formatTimeForDetail(message?: SDKMessage): string {
  if (!message) return "";
  const ts = Number(asRecord(message).timestamp ?? 0);
  if (!ts) return "";
  const d = new Date(ts);
  const pad = (v: number) => String(v).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}月${pad(d.getDate())}日 ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

// 置顶消息（使用 sdk_pin_by_message_id / sdk_unpin_by_message_id）
async function handlePin(messageId: string) {
  if (!activeSessionId.value) return;
  try {
    await invoke("sdk_pin_by_message_id", { messageId });
    pinnedBarDismissedByConversation.value = {
      ...pinnedBarDismissedByConversation.value,
      [activeSessionId.value]: false,
    };
    await syncActiveSession();
    await loadSessions();
    Message.success("消息已置顶");
  } catch (e) {
    handleSdkError(e, "pinMessage");
  }
}

async function handleUnpin(messageId: string) {
  if (!activeSessionId.value) return;
  try {
    await invoke("sdk_unpin_by_message_id", { messageId });
    await syncActiveSession();
    await loadSessions();
    Message.success("已取消置顶");
  } catch (e) {
    handleSdkError(e, "unpinMessage");
  }
}

// 标记消息（sdk_mark_by_message_id: message_id, mark_type, color）
async function handleMark(messageId: string, markType: number, color?: string) {
  if (!activeSessionId.value) return;
  try {
    await invoke("sdk_mark_by_message_id", {
      messageId,
      markType,
      color: color ?? "",
    });
    const labels: Record<number, string> = { 1: "重要", 2: "待办", 3: "已处理" };
    await syncActiveSession();
    await loadSessions();
    Message.success(`消息已标记为${labels[markType] ?? "标记"}`);
  } catch (e) {
    handleSdkError(e, "markMessage");
  }
}

async function handleUnmark(messageId: string, markType: number) {
  if (!activeSessionId.value) return;
  try {
    await invoke("sdk_unmark_by_message_id", {
      messageId,
      markType,
    });
    await syncActiveSession();
    await loadSessions();
    Message.success("消息标记已取消");
  } catch (e) {
    handleSdkError(e, "unmarkMessage");
  }
}

async function doDeleteMessage(messageId: string, deleteScope: number) {
  if (!activeSessionId.value) return;
  await invoke("sdk_delete_message", {
    messageId,
    deleteScope,
    reason: "",
  });
  removeMessageFromCurrentSession(messageId);
  await syncActiveSession();
  await loadSessions();
  Message.success(deleteScope === 2 ? "已为所有人删除消息" : "已删除自己的消息");
}

// 删除消息：菜单只保留一个删除按钮；自己发送的消息通过弹窗选择范围
async function handleDelete(messageId: string, canDeleteForEveryone: boolean = false) {
  if (!activeSessionId.value) return;
  if (!canDeleteForEveryone) {
    try {
      await doDeleteMessage(messageId, 1);
    } catch (e) {
      handleSdkError(e, "deleteMessage");
    }
    return;
  }

  Modal.confirm({
    title: "删除消息",
    content: "请选择删除范围",
    okText: "删除自己",
    cancelText: "删除所有人",
    okButtonProps: { status: "danger" },
    cancelButtonProps: { status: "danger" },
    async onBeforeOk() {
      try {
        await doDeleteMessage(messageId, 1);
        return true;
      } catch (e) {
        handleSdkError(e, "deleteMessage");
        return false;
      }
    },
    onCancel() {
      doDeleteMessage(messageId, 2).catch((e) => handleSdkError(e, "deleteMessage"));
    },
  });
}

// 删除会话（sdk_conversation_delete）
async function handleDeleteSession(conversationId: string) {
  try {
    await invoke("sdk_conversation_delete", { conversationId: conversationId });
    if (activeSessionId.value === conversationId) {
      activeSessionId.value = null;
      messages.value = [];
    }
    await loadSessions();
    Message.success("会话已删除");
  } catch (e) {
    handleSdkError(e, "deleteConversation");
  }
}

async function handleSessionAction(payload: { id: string; action: "toggle-pin" | "clear-unread" }) {
  const conversationId = String(payload?.id ?? "").trim();
  if (!conversationId) return;
  try {
    if (payload.action === "toggle-pin") {
      const session = sessions.value.find((s) => conversationIdFromSession(s) === conversationId);
      const pinned = Boolean(session?.isPinned ?? asRecord(session ?? {}).is_pinned ?? false);
      await invoke("sdk_conversation_set_pinned", {
        conversationId,
        pinned: !pinned,
      });
      await loadSessions(true);
      return;
    }
    if (payload.action === "clear-unread") {
      await markSessionReadAll(conversationId);
      await loadSessions(true);
      return;
    }
  } catch (e) {
    handleSdkError(e, "sessionAction");
  }
}
</script>

<template>
  <div class="chat-layout">
    <!-- 同步/连接状态条 -->
    <div v-if="syncState === 'syncing' || connectionState === 'Reconnecting' || connectionState === 'Disconnected'" class="status-bar">
      <template v-if="syncState === 'syncing'">
        <a-spin size="small" />
        <span>{{ syncProgress ? `同步中 ${Math.round((syncProgress.progress ?? 0) * 100)}%` : "正在同步..." }}</span>
      </template>
      <template v-else-if="connectionState === 'Reconnecting'">
        <a-spin size="small" />
        <span>正在重连...</span>
      </template>
      <template v-else-if="connectionState === 'Disconnected'">
        <span class="status-warn">连接已断开</span>
      </template>
      <span v-if="syncError" class="status-error">{{ syncError }}</span>
    </div>
    <!-- 会话列表侧边栏 -->
    <div class="sidebar">
      <div class="sidebar-header">
        <a-space style="width: 100%; justify-content: space-between; align-items: center;">
          <a-typography-text class="unread-count">未读 {{ toSafeUnread(totalUnread) }}</a-typography-text>
          <a-button
            size="mini"
            @click="markAllSessionsRead"
          >
            全部已读
          </a-button>
        </a-space>
        <a-typography-text v-if="currentUserId" class="current-user-id" :title="`当前登录用户: ${currentUserId}`">
          当前用户: {{ currentUserId }}
        </a-typography-text>
        <a-button
          class="logout-btn"
          type="outline"
          status="danger"
          size="mini"
          long
          :loading="loggingOut"
          @click="handleLogout"
        >
          退出登录
        </a-button>
      </div>
      
      <SessionList 
        :sessions="sessions" 
        :active-session-id="activeSessionId" 
        :loading="loadingSessions" 
        :query="sessionQuery" 
        :new-conversation-ids="newConversationIds"
        @select="selectSession" 
        @create="createVisible = true" 
        @delete="handleDeleteSession"
        @session-action="handleSessionAction"
        @update:query="(v: string) => sessionQuery = v" 
      />
    </div>

    <!-- 聊天内容区域 -->
    <div class="chat-content">
      <!-- 聊天头部 -->
      <div class="chat-header">
        <a-typography-title :heading="5" class="chat-title">
          {{ activeSessionId ? '消息' : '选择一个会话开始聊天' }}
        </a-typography-title>
      </div>

      <PinnedMessageBar
        v-if="showPinnedBar"
        :count="pinnedMessages.length"
        :sender="pinnedBarSender"
        :preview="pinnedBarPreview"
        @open="openPinnedDrawer"
        @unpin="unpinActivePinnedMessage"
        @dismiss="dismissPinnedBar"
      />
      
      <!-- 消息列表 -->
      <div class="messages-area">
          <MessageList 
          ref="messageListRef"
          :messages="messages" 
          :current-user-id="currentUserId" 
          :loading="loadingMessages" 
          :active-session-id="activeSessionId" 
          :editing-message-id="editingMessageId" 
          :reply-count-map="replyCountMap"
          :pinned-focus-message-id="pinnedFocusMessageId"
          @scrollTop="loadMore" 
          @recall="recallMessage" 
          @reply="activateReplyContext" 
          @forward="handleForward"
          @thread="(id: string) => threadTargetId = id" 
          @startEdit="startEditMessage" 
          @resend="resendMessage" 
          @addReaction="addReactionById" 
          @removeReaction="removeReactionById"
          @pin="handlePin"
          @unpin="handleUnpin"
          @mark="handleMark"
          @unmark="handleUnmark"
          @delete="handleDelete"
          @openReplyDetail="openReplyDetail"
        >
          <template #load>
            <a-button 
              size="mini" 
              :disabled="!activeSessionId || nextBeforeSeq == null" 
              @click="loadMore"
            >
              加载更多
            </a-button>
          </template>
        </MessageList>
      </div>

      <!-- 线程回复面板 -->
      <div class="reply-panel" v-if="threadTargetId">
        <div class="reply-header">
          <span class="reply-title">线程回复</span>
          <a-button size="mini" type="text" @click="threadTargetId = null">取消</a-button>
        </div>
        <a-space style="width: 100%;">
          <a-input 
            v-model="threadReplyText" 
            placeholder="输入线程回复..." 
            allow-clear 
            style="flex: 1;"
            @press-enter="sendThreadReply"
          />
          <a-button type="primary" :disabled="!activeSessionId || !threadReplyText" @click="sendThreadReply">
            发送线程回复
          </a-button>
        </a-space>
      </div>
      
      <!-- 正在输入提示（仅当前会话） -->
      <div v-if="typingHintText" class="typing-hint">
        <span class="typing-dots">
          <span class="dot" />
          <span class="dot" />
          <span class="dot" />
        </span>
        <span class="typing-text">{{ typingHintText }}</span>
      </div>

      <!-- 消息输入框 -->
      <div class="composer-area">
        <EnhancedComposer 
          :active-session-id="activeSessionId" 
          :model-value="textToSend" 
          :media-sending="mediaSending"
          :media-sending-label="mediaSendingLabel"
          :media-progress-percent="mediaProgressPercent"
          :editing-message-id="editingMessageId"
          :target-name="activeSessionDisplayName"
          :replying-to-message-id="activeReplyMessageId"
          :replying-to-label="activeReplySender"
          :replying-to-preview="activeReplyPreview"
          @update:modelValue="onDraftChange" 
          @send="sendText" 
          @cancel-edit="cancelEditMode"
          @cancel-reply="clearReplyContext"
          @send-media="sendMediaFromComposer"
          @typing="onTyping" 
        />
      </div>
    </div>
  </div>

  <a-modal
    v-model:visible="mediaPreviewVisible"
    title="发送图片 / 视频"
    :mask-closable="false"
    width="480"
    unmount-on-close
    @cancel="cancelMediaPreview"
  >
    <div class="media-preview-modal">
      <div v-if="mediaPreviewIsVideo && mediaPreviewGenerating" class="media-preview-loading">
        正在截取视频封面…
      </div>
      <img
        v-else-if="!mediaPreviewIsVideo && mediaPreviewPath"
        :src="normalizeFileUrl(mediaPreviewPath)"
        class="media-preview-img"
        alt="预览"
      />
      <video
        v-else-if="mediaPreviewIsVideo && mediaPreviewPath"
        :src="normalizeFileUrl(mediaPreviewPath)"
        class="media-preview-video"
        controls
        playsinline
      />
      <a-textarea
        v-model="mediaPreviewCaption"
        placeholder="添加说明（可选）"
        :auto-size="{ minRows: 2, maxRows: 5 }"
        allow-clear
        class="media-preview-caption"
      />
    </div>
    <template #footer>
      <a-button @click="cancelMediaPreview">取消</a-button>
      <a-button
        type="primary"
        :loading="mediaSending"
        :disabled="mediaPreviewIsVideo && (mediaPreviewGenerating || !mediaPreviewCoverPath)"
        @click="confirmMediaPreviewSend"
      >
        发送
      </a-button>
    </template>
  </a-modal>

  <PinnedMessagesDrawer
    v-model:visible="pinnedDrawerVisible"
    :messages="messages"
    @focus="focusPinnedMessageById"
    @unpin="unpinPinnedMessageById"
  />

  <a-drawer
    v-model:visible="replyDetailVisible"
    width="420"
    title="详情页"
    unmount-on-close
  >
    <div class="reply-detail" v-if="replyDetailRoot">
      <div class="reply-detail-root">
        <div class="reply-detail-meta">
          <span class="reply-detail-name">{{ replyDetailRootSender }}</span>
          <span class="reply-detail-time">{{ formatTimeForDetail(replyDetailRoot) }}</span>
        </div>
        <div class="reply-detail-text">{{ getQuotePreview(replyDetailRoot) }}</div>
      </div>

      <div class="reply-detail-list">
        <div class="reply-detail-item" v-for="item in replyDetailItems" :key="getMsgClientMsgId(asRecord(item)) || getMsgServerId(asRecord(item))">
          <div class="reply-detail-meta">
            <span class="reply-detail-name">{{ getSenderName(item) }}</span>
            <span class="reply-detail-time">{{ formatTimeForDetail(item) }}</span>
          </div>
          <div class="reply-detail-text">{{ getQuotePreview(item) }}</div>
        </div>
      </div>
    </div>
  </a-drawer>

  <!-- 创建会话模态框 -->
  <a-modal 
    v-model:visible="createVisible" 
    title="新建会话" 
    :mask-closable="false"
    @ok="createSession"
    @cancel="createVisible = false"
  >
    <a-form layout="vertical" :model="{ createSessionType, createBusinessType, createPeerId, createDisplayName }">
      <a-form-item label="会话类型">
        <a-select 
          v-model="createSessionType" 
          :options="[
            {value:'single',label:'单聊'},
            {value:'group',label:'群聊'}
          ]" 
        />
      </a-form-item>
      <a-form-item label="业务类型">
        <a-input v-model="createBusinessType" />
      </a-form-item>
      <a-form-item label="对方用户ID" v-if="createSessionType === 'single'">
        <a-input v-model="createPeerId" placeholder="输入对方ID" />
      </a-form-item>
      <a-form-item label="显示名称">
        <a-input v-model="createDisplayName" placeholder="会话显示名称" />
      </a-form-item>
    </a-form>
  </a-modal>

  <!-- 转发消息模态框 -->
  <a-modal 
    v-model:visible="forwardVisible" 
    title="转发消息" 
    :mask-closable="false"
    @ok="confirmForward"
    @cancel="forwardVisible = false"
  >
    <a-form layout="vertical" :model="{ forwardTargetSessionId, forwardMerge, forwardReason }">
      <a-form-item label="目标会话">
        <a-select 
          v-model="forwardTargetSessionId" 
          placeholder="选择目标会话"
          :options="sessions.map(s => ({ value: s.conversationId, label: s.display_name || s.conversationId }))"
        />
      </a-form-item>
      <a-form-item label="转发方式">
        <a-radio-group v-model="forwardMerge">
          <a-radio :value="false">逐条转发</a-radio>
          <a-radio :value="true">合并转发</a-radio>
        </a-radio-group>
      </a-form-item>
      <a-form-item label="转发原因（可选）">
        <a-input v-model="forwardReason" placeholder="输入转发原因..." />
      </a-form-item>
    </a-form>
  </a-modal>

</template>

<style scoped>
.chat-layout {
  display: grid;
  grid-template-columns: 320px 1fr;
  grid-template-rows: auto 1fr;
  height: 100vh;
  background-color: var(--wechat-background, #F5F5F5);
}

.status-bar {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 16px;
  font-size: 13px;
  background: var(--wechat-primary, #07C160);
  color: #fff;
}
.status-bar .status-warn {
  color: #ffcb00;
}
.status-bar .status-error {
  margin-left: auto;
  color: #ff4d4f;
}

.media-preview-modal {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.media-preview-loading {
  padding: 28px 12px;
  text-align: center;
  color: var(--wechat-text-secondary, #666);
  font-size: 14px;
}

.media-preview-img,
.media-preview-video {
  display: block;
  width: 100%;
  max-height: 320px;
  border-radius: 8px;
  object-fit: contain;
  background: #0f0f0f;
}

.media-preview-caption {
  margin-top: 0;
}

.sidebar {
  grid-row: 2;
  background-color: #FFFFFF;
  border-right: 1px solid var(--wechat-divider, #E5E5E5);
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.sidebar-header {
  padding: 8px 16px;
  border-bottom: 1px solid var(--wechat-divider, #E5E5E5);
  background-color: #FFFFFF;
}

.unread-count {
  font-weight: 500;
  color: var(--wechat-text-primary, #000000);
}

.current-user-id {
  display: block;
  margin-top: 6px;
  font-size: 12px;
  color: var(--wechat-text-tertiary, #8E8E93);
  font-weight: 400;
  word-break: break-all;
}

.logout-btn {
  margin-top: 10px;
}

.chat-content {
  grid-row: 2;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background-color: var(--wechat-background, #F5F5F5);
  overflow: hidden;
}

.chat-header {
  padding: var(--spacing-lg, 16px);
  background-color: #FFFFFF;
  border-bottom: 1px solid var(--wechat-divider, #E5E5E5);
  flex-shrink: 0; /* 确保头部不会被压缩 */
}

.chat-title {
  margin: 0 !important;
  color: var(--wechat-text-primary, #000000) !important;
}

.messages-area {
  flex: 1; /* 占用剩余空间 */
  overflow: hidden;
  position: relative;
  min-height: 0; /* 确保 flex 子元素可以正确收缩 */
}

.reply-panel {
  background-color: #FFFFFF;
  border-top: 1px solid var(--wechat-divider, #E5E5E5);
  padding: var(--spacing-md, 12px) var(--spacing-lg, 16px);
}

.reply-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--spacing-sm, 8px);
}

.reply-title {
  font-size: var(--font-size-sm, 14px);
  font-weight: 500;
  color: var(--wechat-text-primary, #000000);
}

.reply-detail {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.reply-detail-root,
.reply-detail-item {
  background: #f7f8fa;
  border: 1px solid #e5e8ef;
  border-radius: 10px;
  padding: 10px 12px;
}

.reply-detail-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.reply-detail-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
}

.reply-detail-name {
  font-size: 13px;
  font-weight: 600;
  color: #374151;
}

.reply-detail-time {
  font-size: 12px;
  color: #9ca3af;
}

.reply-detail-text {
  font-size: 18px;
  line-height: 1.45;
  color: #111827;
  white-space: pre-wrap;
  word-break: break-word;
}

/* 正在输入提示条 */
.typing-hint {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px var(--spacing-lg, 16px);
  font-size: 13px;
  color: var(--wechat-text-secondary, #888888);
  background-color: var(--wechat-background, #F5F5F5);
  border-top: 1px solid var(--wechat-divider, #E5E5E5);
  flex-shrink: 0;
}
.typing-dots {
  display: inline-flex;
  gap: 4px;
  align-items: center;
}
.typing-dots .dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--wechat-text-secondary, #888888);
  animation: typing-bounce 1.4s ease-in-out infinite both;
}
.typing-dots .dot:nth-child(1) { animation-delay: 0s; }
.typing-dots .dot:nth-child(2) { animation-delay: 0.2s; }
.typing-dots .dot:nth-child(3) { animation-delay: 0.4s; }
@keyframes typing-bounce {
  0%, 80%, 100% { transform: scale(0.6); opacity: 0.5; }
  40% { transform: scale(1); opacity: 1; }
}
.typing-text {
  flex: 1;
}

.composer-area {
  background-color: #FFFFFF;
  border-top: 1px solid var(--wechat-divider, #E5E5E5);
  padding: var(--spacing-md, 12px) var(--spacing-lg, 16px);
  /* 使用 flex-shrink: 0 确保输入框不会被压缩 */
  flex: 0 0 auto; /* 不允许增长或收缩，保持固定大小 */
  /* 确保输入框始终可见 */
  min-height: fit-content;
  z-index: 5;
  /* 确保输入框在视口底部 */
  position: relative;
}

/* 深色模式适配 */
@media (prefers-color-scheme: dark) {
  .sidebar {
    background-color: #1A1A1A;
    border-right-color: var(--wechat-divider, #2C2C2C);
  }
  
  .sidebar-header {
    background-color: #1A1A1A;
    border-bottom-color: var(--wechat-divider, #2C2C2C);
  }
  
  .unread-count {
    color: var(--wechat-text-primary, #FFFFFF);
  }

  .current-user-id {
    color: var(--wechat-text-tertiary, #8E8E93);
  }
  
  .chat-header {
    background-color: #1A1A1A;
    border-bottom-color: var(--wechat-divider, #2C2C2C);
  }
  
  .chat-title {
    color: var(--wechat-text-primary, #FFFFFF) !important;
  }

  .reply-panel {
    background-color: #1A1A1A;
    border-top-color: var(--wechat-divider, #2C2C2C);
  }
  
  .reply-title {
    color: var(--wechat-text-primary, #FFFFFF);
  }

  .reply-detail-root,
  .reply-detail-item {
    background: #23262b;
    border-color: #31363d;
  }

  .reply-detail-name {
    color: #e5e7eb;
  }

  .reply-detail-time {
    color: #9aa3af;
  }

  .reply-detail-text {
    color: #f3f4f6;
  }
  
  .typing-hint {
    background-color: #1A1A1A;
    border-top-color: var(--wechat-divider, #2C2C2C);
    color: var(--wechat-text-secondary, #999999);
  }
  .typing-dots .dot {
    background: var(--wechat-text-secondary, #999999);
  }
  .composer-area {
    background-color: #1A1A1A;
    border-top-color: var(--wechat-divider, #2C2C2C);
  }
}

/* 移动端适配 */
@media (max-width: 768px) {
  .chat-layout {
    grid-template-columns: 280px 1fr;
  }
  
  .sidebar {
    width: 280px;
  }
  
  .chat-header,
  .sidebar-header {
    padding: var(--spacing-md, 12px);
  }

  .reply-panel,
  .composer-area {
    padding: var(--spacing-sm, 8px) var(--spacing-md, 12px);
  }
}

/* 小屏幕适配 */
@media (max-width: 640px) {
  .chat-layout {
    grid-template-columns: 1fr;
  }
  
  .sidebar {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    z-index: 100;
    transform: translateX(-100%);
    transition: transform 0.3s ease;
  }
  
  .sidebar.show {
    transform: translateX(0);
  }
  
  .chat-content {
    width: 100%;
  }
}
</style>

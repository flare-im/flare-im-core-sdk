/**
 * 统一管理 Tauri IM 事件与同步/连接状态
 * 对接 bindings 转发的 im://* 事件，供 Login、Chat 等使用
 */
import { ref, onMounted, onUnmounted, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import type { SendAckPayload } from "../types";
import {
  ensureImMessageHub,
  subscribeImMessage,
  subscribeImMessageBatch,
} from "../im/imMessageHub";

export type ConnectionState =
  | "Disconnected"
  | "Connecting"
  | "Connected"
  | "Ready"
  | "Reconnecting"
  | "";

export type SyncState = "idle" | "syncing" | "finished" | "error";

export interface SyncProgressDetail {
  task: string;
  progress: number;
  detail: string;
}

export interface UploadProgressDetail {
  fileName: string;
  uploadId: string;
  phase: "Preparing" | "Uploading" | "Completing" | "Finished" | string;
  uploadedBytes: number;
  totalBytes: number;
  chunkIndex?: number;
  totalChunks?: number;
}

/** 兼容 bindings 发出的 camelCase 与 snake_case 字段 */
function convId(p: Record<string, unknown>): string {
  return String(p.conversationId ?? p.conversation_id ?? "");
}
function msgId(p: Record<string, unknown>): string {
  return String(p.messageId ?? p.message_id ?? "");
}
function userId(p: Record<string, unknown>): string {
  return String(p.userId ?? p.user_id ?? "");
}

export interface UseImEventsCallbacks {
  onMessage?: (payload: unknown) => void;
  onMessageBatch?: (payload: unknown[]) => void;
  /** 未读变化：bindings 已移除 `im://unread`，由 `im://unread_count_changed` 触发 */
  onUnread?: () => void;
  onSendAck?: (payload: SendAckPayload) => void;
  onSendFailed?: (payload: { client_msg_id?: string; clientMsgId?: string; reason?: string }) => void;
  onMessageRecalled?: (payload: { conversation_id: string; message_id: string }) => void;
  /** 消息正文已在本地库更新（推送/同步），宜刷新当前会话；`edit_version` 来自服务端时可用于去重 */
  onMessageEdited?: (payload: {
    conversation_id: string;
    message_id: string;
    edit_version?: number;
  }) => void;
  onMessageReactionChanged?: (payload: {
    conversation_id: string;
    message_id: string;
    user_id: string;
    emoji: string;
    action: number;
  }) => void;
  onMessageDeleted?: (payload: { conversation_id: string; message_id: string }) => void;
  onMessageReadReceipt?: (payload: {
    conversation_id: string;
    user_id: string;
    read_seq: number;
    message_ids: string[];
  }) => void;
  onMessagePinned?: (payload: {
    conversation_id: string;
    message_id: string;
    pinned_by: string;
  }) => void;
  onMessageUnpinned?: (payload: { conversation_id: string; message_id: string }) => void;
  onMessageMarked?: (payload: {
    conversation_id: string;
    message_id: string;
    user_id: string;
    mark_type: number;
    color: string;
  }) => void;
  onMessageUnmarked?: (payload: {
    conversation_id: string;
    message_id: string;
    user_id: string;
    mark_type: number;
  }) => void;
  /** `conversationIds` 与 Rust `ConversationsSyncedPayload` 一致 */
  onConversationsSynced?: (payload: { conversationIds: string[] }) => void;
  onConversationCreated?: (payload: { conversation_id: string }) => void;
  onConversationUpdated?: (payload: { conversation_id: string }) => void;
  onConversationDeleted?: (payload: { conversation_id: string }) => void;
  onUnreadCountChanged?: (payload: { conversation_id: string; unread_count: number }) => void;
  onTyping?: (payload: { conversation_id: string; user_id: string; typing: boolean }) => void;
  onPresenceChanged?: (payload: {
    conversation_id: string;
    user_id: string;
    status: string;
    extra: Record<string, string>;
  }) => void;
  onCallSignal?: (payload: {
    conversation_id: string;
    call_id: string;
    signal_type: string;
    payload: number[];
    metadata: Record<string, string>;
  }) => void;
  onMessageCustomEvent?: (payload: {
    conversation_id: string;
    namespace: string;
    name: string;
    version: string;
    payload: number[];
    metadata: Record<string, string>;
  }) => void;
  onKickedOff?: (payload: { reason: string }) => void;
  onTokenExpired?: (payload: { message: string }) => void;
  onDisconnected?: (payload: { reason: string }) => void;
  onServerError?: (payload: { code: number; message: string }) => void;
  onUploadProgress?: (payload: UploadProgressDetail) => void;
}

export function useImEvents(callbacks: UseImEventsCallbacks = {}) {
  const unlistenFns: (() => void)[] = [];
  const connectionState = ref<ConnectionState>("");
  const syncState = ref<SyncState>("idle");
  const syncProgress = ref<SyncProgressDetail | null>(null);
  const syncError = ref<string | null>(null);
  /** Init / Background 阶段是否已完成 */
  const syncPhaseFinished = ref<Record<string, boolean>>({ Init: false, Background: false });
  /** 所有 im:// 监听注册完成后再 invoke sdk_login，避免与后端事件竞态 */
  let resolveListenersReady: (() => void) | undefined;
  const listenersReady = new Promise<void>((resolve) => {
    resolveListenersReady = resolve;
  });
  /** 用于 Login 等待 Init 同步完成 */
  let resolveInitSync: (() => void) | null = null;
  let rejectInitSync: ((e: Error) => void) | null = null;
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  function waitForInitSync(timeoutMs = 15000): Promise<void> {
    if (syncPhaseFinished.value.Init) return Promise.resolve();
    return new Promise((resolve, reject) => {
      resolveInitSync = () => {
        if (timeoutId) clearTimeout(timeoutId);
        timeoutId = null;
        resolveInitSync = null;
        rejectInitSync = null;
        resolve();
      };
      rejectInitSync = (err: Error) => {
        if (timeoutId) clearTimeout(timeoutId);
        timeoutId = null;
        resolveInitSync = null;
        rejectInitSync = null;
        reject(err);
      };
      timeoutId = setTimeout(() => {
        if (rejectInitSync) {
          rejectInitSync(new Error("等待同步超时"));
        }
      }, timeoutMs);
    });
  }

  function waitForFullSync(timeoutMs = 20000): Promise<void> {
    if (syncPhaseFinished.value.Init && syncPhaseFinished.value.Background) {
      return Promise.resolve();
    }
    return new Promise((resolve, reject) => {
      if (syncPhaseFinished.value.Init && syncPhaseFinished.value.Background) {
        resolve();
        return;
      }
      let timer: ReturnType<typeof setTimeout> | null = null;
      const stop = watch(syncPhaseFinished, () => {
        if (syncState.value === "error") {
          if (timer) clearTimeout(timer);
          stop();
          reject(new Error(syncError.value ?? "同步失败"));
          return;
        }
        if (syncPhaseFinished.value.Init && syncPhaseFinished.value.Background) {
          if (timer) clearTimeout(timer);
          stop();
          resolve();
        }
      }, { deep: true });
      timer = setTimeout(() => {
        stop();
        reject(new Error("等待全量同步超时"));
      }, timeoutMs);
    });
  }

  function setupListeners() {
    const events: Array<[string, (e: { payload: unknown }) => void]> = [
      ["im://state", (e) => {
        const p = e.payload as { state?: string };
        connectionState.value = (p?.state as ConnectionState) ?? "";
      }],
      ["im://connected", () => {
        connectionState.value = "Connected";
      }],
      ["im://disconnected", (e) => {
        connectionState.value = "Disconnected";
        callbacks.onDisconnected?.(e.payload as { reason: string });
      }],
      ["im://reconnecting", () => {
        connectionState.value = "Reconnecting";
      }],
      ["im://kicked_off", (e) => {
        connectionState.value = "Disconnected";
        callbacks.onKickedOff?.(e.payload as { reason: string });
      }],
      ["im://token_expired", (e) => {
        callbacks.onTokenExpired?.(e.payload as { message: string });
      }],
      ["im://server_error", (e) => {
        callbacks.onServerError?.(e.payload as { code: number; message: string });
      }],
      ["im://upload_progress", (e) => {
        callbacks.onUploadProgress?.(e.payload as UploadProgressDetail);
      }],
      ["im://sync_started", () => {
        syncState.value = "syncing";
        syncError.value = null;
        syncPhaseFinished.value = { Init: false, Background: false };
      }],
      ["im://sync_state_changed", (e) => {
        const p = e.payload as { state?: string };
        const s = (p?.state ?? "").toLowerCase();
        if (s.includes("syncing") || s.includes("catching")) {
          syncState.value = "syncing";
        } else if (s.includes("idle")) {
          syncState.value = "finished";
        }
      }],
      ["im://sync_progress", (e) => {
        const p = e.payload as SyncProgressDetail;
        syncProgress.value = p;
      }],
      ["im://sync_finished", (e) => {
        const p = e.payload as { phase?: string };
        const phase = p?.phase ?? "";
        syncPhaseFinished.value[phase] = true;
        if (phase === "Init" || phase === "Background") {
          syncState.value = "finished";
          if (phase === "Init" && resolveInitSync) {
            resolveInitSync();
          }
        }
        if (phase === "Background") {
          syncProgress.value = null;
        }
      }],
      ["im://sync_completed", () => {
        // 某些场景只上报 task completed；这里做兜底，避免 UI 卡在“正在同步”。
        if (syncState.value === "syncing") {
          syncState.value = "finished";
        }
      }],
      ["im://sync_failed", (e) => {
        const p = e.payload as { task?: string; error?: string };
        syncState.value = "error";
        syncError.value = p?.error ?? "同步失败";
        if (rejectInitSync) {
          rejectInitSync(new Error(syncError.value));
        }
      }],
      ["im://conversations_synced", (e) => {
        const p = e.payload as { conversationIds?: string[]; conversation_ids?: string[] };
        const conversationIds = p.conversationIds ?? p.conversation_ids ?? [];
        callbacks.onConversationsSynced?.({ conversationIds });
      }],
      ["im://send_ack", (e) => {
        callbacks.onSendAck?.(e.payload as SendAckPayload);
      }],
      ["im://send_failed", (e) => {
        callbacks.onSendFailed?.(
          e.payload as { client_msg_id?: string; clientMsgId?: string; reason?: string },
        );
      }],
      ["im://message_recalled", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageRecalled?.({ conversation_id: convId(p), message_id: msgId(p) });
      }],
      ["im://message_edited", (e) => {
        const p = e.payload as Record<string, unknown>;
        const ev = p.editVersion ?? p.edit_version;
        callbacks.onMessageEdited?.({
          conversation_id: convId(p),
          message_id: msgId(p),
          edit_version:
            ev !== undefined && ev !== null && String(ev).trim() !== ""
              ? Number(ev)
              : undefined,
        });
      }],
      ["im://message_reaction_changed", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageReactionChanged?.({
          conversation_id: convId(p),
          message_id: msgId(p),
          user_id: userId(p),
          emoji: String(p.emoji ?? ""),
          action: Number(p.action ?? 0),
        });
      }],
      ["im://message_deleted", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageDeleted?.({ conversation_id: convId(p), message_id: msgId(p) });
      }],
      ["im://message_read_receipt", (e) => {
        const p = e.payload as Record<string, unknown>;
        const mids = p.messageIds ?? p.message_ids;
        callbacks.onMessageReadReceipt?.({
          conversation_id: convId(p),
          user_id: userId(p),
          read_seq: Number(p.readSeq ?? p.read_seq ?? 0),
          message_ids: Array.isArray(mids) ? mids.map((v) => String(v)) : [],
        });
      }],
      ["im://message_pinned", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessagePinned?.({
          conversation_id: convId(p),
          message_id: msgId(p),
          pinned_by: String(p.pinnedBy ?? p.pinned_by ?? ""),
        });
      }],
      ["im://message_unpinned", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageUnpinned?.({ conversation_id: convId(p), message_id: msgId(p) });
      }],
      ["im://message_marked", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageMarked?.({
          conversation_id: convId(p),
          message_id: msgId(p),
          user_id: userId(p),
          mark_type: Number(p.markType ?? p.mark_type ?? 0),
          color: String(p.color ?? ""),
        });
      }],
      ["im://message_unmarked", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageUnmarked?.({
          conversation_id: convId(p),
          message_id: msgId(p),
          user_id: userId(p),
          mark_type: Number(p.markType ?? p.mark_type ?? 0),
        });
      }],
      ["im://conversation_created", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onConversationCreated?.({ conversation_id: convId(p) });
      }],
      ["im://conversation_updated", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onConversationUpdated?.({ conversation_id: convId(p) });
      }],
      ["im://conversation_deleted", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onConversationDeleted?.({ conversation_id: convId(p) });
      }],
      ["im://unread_count_changed", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onUnreadCountChanged?.({
          conversation_id: convId(p),
          unread_count: Number(p.unreadCount ?? p.unread_count ?? 0),
        });
        callbacks.onUnread?.();
      }],
      ["im://typing", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onTyping?.({
          conversation_id: convId(p),
          user_id: userId(p),
          typing: Boolean(p.typing),
        });
      }],
      ["im://presence_changed", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onPresenceChanged?.({
          conversation_id: convId(p),
          user_id: userId(p),
          status: String(p.status ?? ""),
          extra: (p.extra as Record<string, string>) ?? {},
        });
      }],
      ["im://call_signal", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onCallSignal?.({
          conversation_id: convId(p),
          call_id: String(p.callId ?? p.call_id ?? ""),
          signal_type: String(p.signalType ?? p.signal_type ?? ""),
          payload: Array.isArray(p.payload) ? (p.payload as number[]) : [],
          metadata: (p.metadata as Record<string, string>) ?? {},
        });
      }],
      ["im://message_custom_event", (e) => {
        const p = e.payload as Record<string, unknown>;
        callbacks.onMessageCustomEvent?.({
          conversation_id: convId(p),
          namespace: String(p.namespace ?? ""),
          name: String(p.name ?? ""),
          version: String(p.version ?? ""),
          payload: Array.isArray(p.payload) ? (p.payload as number[]) : [],
          metadata: (p.metadata as Record<string, string>) ?? {},
        });
      }],
    ];

    unlistenFns.push(
      subscribeImMessage((p) => {
        callbacks.onMessage?.(p);
      }),
      subscribeImMessageBatch((items) => {
        callbacks.onMessageBatch?.(items);
      }),
    );

    Promise.all([
      ensureImMessageHub(),
      ...events.map(([name, handler]) =>
        listen(name, handler)
          .then((unlisten) => {
            unlistenFns.push(unlisten);
          })
          .catch((err: unknown) => {
            console.warn(`[useImEvents] listen ${name} failed:`, err);
          }),
      ),
    ])
      .then(() => {
        resolveListenersReady?.();
      })
      .catch(() => {
        resolveListenersReady?.();
      });
  }

  function teardown() {
    unlistenFns.splice(0, unlistenFns.length).forEach((fn) => fn());
  }

  function clearSyncState() {
    syncState.value = "idle";
    syncProgress.value = null;
    syncError.value = null;
    syncPhaseFinished.value = { Init: false, Background: false };
  }

  onMounted(() => {
    setupListeners();
  });

  onUnmounted(teardown);

  return {
    connectionState,
    syncState,
    syncProgress,
    syncError,
    syncPhaseFinished,
    /** 在调用 sdk_login 之前 await，确保已订阅 im://sync_finished 等事件 */
    listenersReady,
    waitForInitSync,
    waitForFullSync,
    clearSyncState,
    teardown,
  };
}

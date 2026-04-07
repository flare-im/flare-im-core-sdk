/**
 * 全局单例监听 im://message / im://message_batch。
 *
 * 解决：Login → Chat 路由切换时，子组件 onUnmounted 先卸掉 Tauri listen，
 * Chat onMounted 后才重新注册，中间空窗会丢下行推送；Hub 只 install 一次，不因页面卸载而 remove。
 */
import { listen } from "@tauri-apps/api/event";

type MsgFn = (payload: unknown) => void;
type BatchFn = (items: unknown[]) => void;

const msgFns = new Set<MsgFn>();
const batchFns = new Set<BatchFn>();

let installPromise: Promise<void> | null = null;

export function ensureImMessageHub(): Promise<void> {
  if (!installPromise) {
    installPromise = Promise.all([
      listen("im://message", (e) => {
        for (const fn of msgFns) {
          try {
            fn(e.payload);
          } catch (err) {
            console.warn("[imMessageHub] im://message handler", err);
          }
        }
      }),
      listen("im://message_batch", (e) => {
        const payload = Array.isArray(e.payload) ? e.payload : [];
        for (const fn of batchFns) {
          try {
            fn(payload);
          } catch (err) {
            console.warn("[imMessageHub] im://message_batch handler", err);
          }
        }
      }),
    ]).then(() => {});
  }
  return installPromise;
}

/** 注册后务必在组件 onUnmounted 调用返回的函数（仅从 Hub 的 Set 移除，不断全局 listen） */
export function subscribeImMessage(fn: MsgFn): () => void {
  void ensureImMessageHub();
  msgFns.add(fn);
  return () => {
    msgFns.delete(fn);
  };
}

export function subscribeImMessageBatch(fn: BatchFn): () => void {
  void ensureImMessageHub();
  batchFns.add(fn);
  return () => {
    batchFns.delete(fn);
  };
}

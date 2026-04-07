import { computed, ref, toValue, watch, type MaybeRefOrGetter } from "vue";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { toWebviewLocalMediaUrl } from "../utils/localMediaUrl";

type MediaAccessUrlPayload = {
  url?: string | null;
  cdnUrl?: string | null;
  cdn_url?: string | null;
};

type MediaResolvedPayload = {
  source: string;
  localPath?: string | null;
  remote?: MediaAccessUrlPayload | null;
};

const CACHE_TTL_MS = 60_000;
const accessUrlCache = new Map<string, { url: string; expiresAt: number }>();
const inflight = new Map<string, Promise<string>>();

/**
 * 与 Rust `pick_download_url` 对齐：私有媒体预签名在 `url`，无签名的 CDN 直链在 `cdn_url`；
 * 若优先 cdn，WebView 会对私有桶拿到 403。
 */
export function pickPreferredRemoteMediaUrl(
  payload: MediaAccessUrlPayload | null | undefined,
): string {
  const u = String(payload?.url ?? "").trim();
  const c = String(payload?.cdnUrl ?? payload?.cdn_url ?? "").trim();
  if (
    u &&
    (u.includes("X-Amz-Algorithm=") ||
      u.includes("X-Amz-Signature=") ||
      u.includes("AWSAccessKeyId="))
  ) {
    return u;
  }
  if (c) return c;
  return u;
}

function pickAccessUrl(payload: MediaAccessUrlPayload | null | undefined): string {
  return pickPreferredRemoteMediaUrl(payload);
}

function isDirectRenderableUrl(raw: string): boolean {
  const v = String(raw ?? "").trim();
  if (/^(blob|data|asset):/i.test(v)) return true;
  if (/^https?:/i.test(v)) return true;
  // 纯浏览器 dev：file 尚可用；Tauri WebView 禁止 file://
  if (/^file:/i.test(v) && !isTauri()) return true;
  return false;
}

function isTauriEnv(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 通过 SDK `resolve_media_access`：优先 SQLite 本地缓存路径，否则短时远程 URL。 */
async function resolveAccessUrl(fileId: string, fallbackUrl: string): Promise<string> {
  const normalizedFileId = String(fileId ?? "").trim();
  const normalizedFallback = String(fallbackUrl ?? "").trim();

  if (!normalizedFileId) return normalizedFallback;
  if (!isTauriEnv()) return normalizedFallback;
  if (isDirectRenderableUrl(normalizedFallback)) return normalizedFallback;

  const now = Date.now();
  const cached = accessUrlCache.get(normalizedFileId);
  if (cached && cached.expiresAt > now) {
    return cached.url || normalizedFallback;
  }

  const existing = inflight.get(normalizedFileId);
  if (existing) {
    const url = await existing;
    return url || normalizedFallback;
  }

  const request = invoke<MediaResolvedPayload>("sdk_resolve_media_access", {
    fileId: normalizedFileId,
    expiresIn: 3600,
  })
    .then((payload) => {
      const src = String(payload?.source ?? "").toLowerCase();
      let url = "";
      if (src === "local") {
        const p = String(payload?.localPath ?? "").trim();
        if (p) url = toWebviewLocalMediaUrl(p);
      }
      if (!url && payload?.remote) {
        url = pickAccessUrl(payload.remote);
      }
      if (url) {
        accessUrlCache.set(normalizedFileId, {
          url,
          expiresAt: Date.now() + CACHE_TTL_MS,
        });
      }
      return url;
    })
    .catch(() => "")
    .finally(() => {
      inflight.delete(normalizedFileId);
    });

  inflight.set(normalizedFileId, request);
  const resolved = await request;
  return resolved || normalizedFallback;
}

export type UseMediaAccessUrlOptions = {
  /**
   * 为 false 时不发起解析（用于「点击后再拉原图」等场景）。
   * 支持 ref、computed 或 getter。
   */
  enabled?: MaybeRefOrGetter<boolean>;
};

export function useMediaAccessUrl(
  fileIdSource: () => string,
  fallbackUrlSource: () => string,
  options?: UseMediaAccessUrlOptions,
) {
  const resolvedUrl = ref("");
  const loading = ref(false);

  const fileId = computed(() => String(fileIdSource() ?? "").trim());
  const fallbackUrl = computed(() => String(fallbackUrlSource() ?? "").trim());
  const enabled = computed(() => {
    if (options?.enabled === undefined) return true;
    return Boolean(toValue(options.enabled));
  });

  async function refresh(): Promise<void> {
    if (!enabled.value) {
      resolvedUrl.value = "";
      loading.value = false;
      return;
    }
    loading.value = true;
    try {
      resolvedUrl.value = await resolveAccessUrl(fileId.value, fallbackUrl.value);
    } finally {
      loading.value = false;
    }
  }

  watch([fileId, fallbackUrl, enabled], () => {
    void refresh();
  }, { immediate: true });

  return {
    resolvedUrl,
    loading,
    refresh,
  };
}

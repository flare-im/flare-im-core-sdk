/**
 * 通过 fetch 拉取可访问 URL 后触发浏览器保存（Tauri WebView 下对 blob: 链接通常可用）。
 */
export function sanitizeDownloadFileName(name: string, fallback: string): string {
  const base = String(name || fallback || "download").trim() || "download";
  return base.replace(/[/\\?%*:|"<>]/g, "_").slice(0, 200);
}

function extensionFromMime(mime: string): string {
  const m = String(mime || "").split(";")[0].trim().toLowerCase();
  const map: Record<string, string> = {
    "image/jpeg": ".jpg",
    "image/jpg": ".jpg",
    "image/png": ".png",
    "image/webp": ".webp",
    "image/gif": ".gif",
    "video/mp4": ".mp4",
    "video/quicktime": ".mov",
    "audio/mpeg": ".mp3",
    "audio/mp4": ".m4a",
    "audio/aac": ".aac",
    "audio/wav": ".wav",
    "application/zip": ".zip",
    "application/pdf": ".pdf",
  };
  return map[m] || "";
}

export function defaultNameWithMime(base: string, mime: string): string {
  const clean = sanitizeDownloadFileName(base, "download");
  if (/\.\w{2,8}$/i.test(clean)) return clean;
  const ext = extensionFromMime(mime);
  return ext ? `${clean}${ext}` : clean;
}

export async function downloadUrlToDevice(url: string, fileName: string): Promise<void> {
  const u = String(url || "").trim();
  if (!u) throw new Error("缺少下载地址");

  const res = await fetch(u);
  if (!res.ok) {
    throw new Error(`下载失败（${res.status}）`);
  }
  const blob = await res.blob();
  const name = sanitizeDownloadFileName(fileName, "download");
  const finalName = /\.\w{2,8}$/i.test(name)
    ? name
    : defaultNameWithMime(name, blob.type || res.headers.get("content-type") || "");

  const objectUrl = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = objectUrl;
    a.download = finalName;
    a.rel = "noopener";
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
}

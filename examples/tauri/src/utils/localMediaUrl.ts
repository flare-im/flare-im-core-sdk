/**
 * Tauri WebView 禁止在 img/video 等标签中直接使用 file:// 或裸磁盘路径（会报 unsupported URL / Not allowed to load local resource）。
 * 需用 convertFileSrc 转成 asset 等受控 URL。与路径是否含中文无关。
 */
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";

function convertFileSrcAvailable(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as unknown as { __TAURI_INTERNALS__?: { convertFileSrc?: unknown } };
  return typeof w.__TAURI_INTERNALS__?.convertFileSrc === "function";
}

/** 将 file:// URL 还原为文件系统路径（含 Unicode 文件名）。 */
export function fileUrlToFilesystemPath(fileUrl: string): string {
  const s = String(fileUrl ?? "").trim();
  if (!s.toLowerCase().startsWith("file:")) {
    return s;
  }
  try {
    const u = new URL(s);
    let pathname = u.pathname;
    // Windows: file:///C:/Users/... → pathname 常为 /C:/Users/...
    if (/^\/[A-Za-z]:\//.test(pathname)) {
      pathname = pathname.slice(1);
    }
    return decodeURIComponent(pathname);
  } catch {
    const stripped = s.replace(/^file:\/\//i, "");
    return decodeURIComponent(stripped);
  }
}

/**
 * 本地绝对路径、Windows 路径、或 file:// → WebView 可加载的 URL。
 * 非 Tauri 环境（纯浏览器 dev）回退为 file://（可能仍被浏览器拦截）。
 */
export function toWebviewLocalMediaUrl(input: string): string {
  const value = String(input ?? "").trim();
  if (!value) return "";
  if (/^(https?|blob|data):/i.test(value)) return value;
  if (/^asset:/i.test(value)) return value;

  let fsPath = value;
  if (value.toLowerCase().startsWith("file:")) {
    fsPath = fileUrlToFilesystemPath(value);
  }

  // isTauri() 在部分构建下未注入；以 convertFileSrc 是否可用为准
  if (isTauri() || convertFileSrcAvailable()) {
    try {
      return convertFileSrc(fsPath);
    } catch {
      /* WebView 未注入 internals 时回退 */
    }
  }

  const normalized = fsPath.replace(/\\/g, "/");
  const prefixed = normalized.startsWith("/") ? `file://${normalized}` : `file:///${normalized}`;
  return encodeURI(prefixed);
}

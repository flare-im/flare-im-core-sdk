import { toWebviewLocalMediaUrl } from "./localMediaUrl";

const VIDEO_EXTS = new Set(["mp4", "mov", "mkv", "avi", "webm", "m4v"]);

export function isVideoFilePath(path: string): boolean {
  const p = path.replace(/\\/g, "/").toLowerCase();
  const dot = p.lastIndexOf(".");
  if (dot < 0) return false;
  return VIDEO_EXTS.has(p.slice(dot + 1));
}

/**
 * 从本地视频截取一帧为 JPEG data URL（WebView 内用 video + canvas，需 asset 协议可读路径）。
 */
export async function captureVideoFrameDataUrl(
  filePath: string,
  seekRatio = 0.08,
): Promise<string> {
  const src = toWebviewLocalMediaUrl(filePath.trim());
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.crossOrigin = "anonymous";
  video.src = src;

  await new Promise<void>((resolve, reject) => {
    const t = window.setTimeout(() => reject(new Error("video load timeout")), 25_000);
    video.onloadeddata = () => {
      window.clearTimeout(t);
      resolve();
    };
    video.onerror = () => {
      window.clearTimeout(t);
      reject(new Error("video load error"));
    };
  });

  const dur = Number.isFinite(video.duration) && video.duration > 0 ? video.duration : 1;
  const t = Math.min(Math.max(dur * seekRatio, 0.05), Math.max(dur - 0.05, 0.05));
  video.currentTime = t;

  await new Promise<void>((resolve, reject) => {
    const timer = window.setTimeout(() => reject(new Error("video seek timeout")), 15_000);
    video.onseeked = () => {
      window.clearTimeout(timer);
      resolve();
    };
    video.onerror = () => {
      window.clearTimeout(timer);
      reject(new Error("video seek error"));
    };
  });

  const vw = video.videoWidth || 640;
  const vh = video.videoHeight || 360;
  const maxW = 720;
  const scale = vw > maxW ? maxW / vw : 1;
  const cw = Math.max(1, Math.floor(vw * scale));
  const ch = Math.max(1, Math.floor(vh * scale));

  const canvas = document.createElement("canvas");
  canvas.width = cw;
  canvas.height = ch;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas unsupported");
  ctx.drawImage(video, 0, 0, cw, ch);
  video.src = "";
  video.remove();

  return canvas.toDataURL("image/jpeg", 0.88);
}

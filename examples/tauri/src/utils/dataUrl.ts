/**
 * SDK `SdkConfigOverlay.dataUrl` 需为 `file://` 根目录（与 core `parse_data_url_to_path` 一致）
 */
import { appDataDir, join } from "@tauri-apps/api/path";

function toFileUrl(absPath: string): string {
  const normalized = absPath.replace(/\\/g, "/");
  if (normalized.startsWith("/")) {
    return `file://${normalized}`;
  }
  return `file:///${normalized}`;
}

/** 应用数据目录下专用子目录，避免污染宿主 app_data */
export async function resolveSdkDataUrl(): Promise<string> {
  const root = await appDataDir();
  const dir = await join(root, "flare-im-tauri-example");
  return toFileUrl(dir);
}

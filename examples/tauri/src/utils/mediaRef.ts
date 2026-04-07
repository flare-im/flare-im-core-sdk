/** 与 SDK ImageInfoElem 一致：优先稳定 imageId，回退 uuid（历史消息兼容） */
export function stableImageMediaId(
  info: { imageId?: string; uuid?: string } | undefined | null,
): string {
  if (!info) return '';
  const id = String(info.imageId ?? '').trim();
  if (id) return id;
  return String(info.uuid ?? '').trim();
}

export function isLikelyLocalMediaRef(id: string): boolean {
  const v = String(id ?? '').trim();
  if (!v) return false;
  if (
    v.startsWith('/') ||
    v.startsWith('./') ||
    v.startsWith('../') ||
    v.toLowerCase().startsWith('file://')
  ) {
    return true;
  }
  // Windows: C:\path、C:/path
  if (/^[A-Za-z]:[\\/]/.test(v)) return true;
  // UNC \\server\share
  if (v.startsWith('\\\\')) return true;
  return false;
}

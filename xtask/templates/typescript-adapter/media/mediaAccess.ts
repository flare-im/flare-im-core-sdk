// GENERATED. Do not edit by hand.
import type { FlareJsonObject } from '../../api/types';

export type MediaAccessLike = FlareJsonObject & {
  url?: string;
  cdnUrl?: string;
};

export type MediaResolvedAccessLike = FlareJsonObject & {
  localPath?: string;
  remote?: MediaAccessLike | null;
};

/** Pick the core media URL first; cdnUrl may be an unsigned storage hint for private media. */
export function pickMediaAccessUrl(access: MediaAccessLike | undefined | null): string {
  if (!access) return '';
  const url = String(access.url ?? '').trim();
  if (url) return url;
  return String(access.cdnUrl ?? '').trim();
}

export function readResolvedRemote(resolved: MediaResolvedAccessLike): MediaAccessLike | undefined {
  const remote = resolved.remote;
  if (remote && typeof remote === 'object') {
    return remote as MediaAccessLike;
  }
  return undefined;
}

/** Normalize canonical camelCase core media access into a display-ready URL. */
export function pickDisplayUrlFromResolved(resolved: MediaResolvedAccessLike): string {
  const remote = readResolvedRemote(resolved);
  const fromRemote = pickMediaAccessUrl(remote);
  if (fromRemote) return fromRemote;
  const local = String(resolved.localPath ?? '').trim();
  if (local.startsWith('http://') || local.startsWith('https://') || local.startsWith('blob:')) {
    return local;
  }
  return '';
}

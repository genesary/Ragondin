// The browser's storage, for per-viewer conveniences only (the theme, the
// last pipeline viewed) and the build handshake's one-reload guard. Every access is guarded: storage can be
// missing, or refused, in a private window or with site data blocked, and
// merely reading `window.localStorage` then throws. Each caller decides what
// "unavailable" means for it; nothing here pretends a write happened.

export type StorageKind = 'local' | 'session';

const area = (kind: StorageKind): Storage => (kind === 'local' ? window.localStorage : window.sessionStorage);

/** The stored value; null when there is none or storage is unavailable. */
export function readStored(kind: StorageKind, key: string): string | null {
  try {
    return area(kind).getItem(key);
  } catch {
    // Unavailable storage reads as nothing stored: the caller's default.
    return null;
  }
}

/** Whether the value was stored. */
export function writeStored(kind: StorageKind, key: string, value: string): boolean {
  try {
    area(kind).setItem(key, value);
    return true;
  } catch {
    return false;
  }
}

/** Whether the key is now absent. */
export function removeStored(kind: StorageKind, key: string): boolean {
  try {
    area(kind).removeItem(key);
    return true;
  } catch {
    return false;
  }
}

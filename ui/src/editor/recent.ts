// The pipelines this browser opened in the editor most recently: a per-viewer
// convenience, kept in local storage and never sent anywhere. Unavailable
// storage keeps nothing, and the editor offers every pipeline all the same.
import { readStored, writeStored } from '../shell/storage.ts';

const KEY = 'ragondin.editor.recent';
/** How many are kept: the empty state lists them all. */
export const RECENT_KEPT = 5;

/** The names opened most recently, newest first. */
export function recentPipelines(): string[] {
  try {
    const kept: unknown = JSON.parse(readStored('local', KEY) ?? '[]');
    return Array.isArray(kept) ? kept.filter((n): n is string => typeof n === 'string').slice(0, RECENT_KEPT) : [];
  } catch {
    return [];
  }
}

/** `name` was opened: first in the list. */
export function rememberPipeline(name: string): void {
  writeStored('local', KEY, JSON.stringify([name, ...recentPipelines().filter((n) => n !== name)].slice(0, RECENT_KEPT)));
}

/** `from` was renamed `to`: it keeps its place in the list. */
export function renameRecent(from: string, to: string): void {
  writeStored('local', KEY, JSON.stringify(recentPipelines().map((n) => (n === from ? to : n))));
}

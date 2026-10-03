// The last pipeline viewed, remembered per viewer so the top bar's Pipeline
// link reopens it: a convenience only, through the guarded storage — without
// storage the link opens the bare screen. ARCHITECTURE.md § The Pipeline screen.
import type { Route } from '../routes.ts';
import { readStored, writeStored } from '../shell/storage.ts';

export const LAST_PIPELINE = 'ragondin.pipeline';

/** Remembers `name` as the last pipeline viewed. */
export const rememberPipeline = (name: string) => void writeStored('local', LAST_PIPELINE, name);

/** The Pipeline screen as the top bar opens it: the last pipeline viewed, or none. */
export function pipelineEntry(): Route {
  const name = readStored('local', LAST_PIPELINE);
  return name === null || name === '' ? { screen: 'pipeline' } : { screen: 'pipeline', name };
}

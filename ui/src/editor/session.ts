// What the editor remembers for the browser session, per pipeline: the choice
// made in the hand-written-file warning, and the run a pipeline was forked
// from. Session storage, for per-viewer conveniences only (src/shell/storage.ts):
// unavailable storage means the warning is asked again and the header names no
// run, never that a file is written unasked. ARCHITECTURE.md § The editor.
import { readStored, writeStored } from '../shell/storage.ts';

const REWRITE = (name: string) => `ragondin.editor.rewrite.${name}`;
const FORKED = (name: string) => `ragondin.editor.forked-from.${name}`;

/** Whether "Rewrite this file" was chosen for `name` this session. */
export const rewriteChosen = (name: string) => readStored('session', REWRITE(name)) === 'yes';
export const chooseRewrite = (name: string) => void writeStored('session', REWRITE(name), 'yes');

/** The run `name` was forked from this session, if it was. */
export const forkedFrom = (name: string) => readStored('session', FORKED(name));
export const rememberFork = (name: string, run: string) => void writeStored('session', FORKED(name), run);

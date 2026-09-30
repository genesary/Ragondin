// The build identity handshake (ADR-C36 § 1): the UI and the API it talks to
// must be the same build. ARCHITECTURE.md § The build identity handshake.
import { readStored, removeStored, writeStored } from './storage.ts';

/**
 * This bundle's build identity, baked in at build time by vite.config.ts from
 * the source the binary reads its own from — never taken from a response,
 * which would compare the server with itself.
 */
export const BUILD: string = __RAGONDIN_BUILD__;

/** The served identity the page last reloaded for, in this tab only. */
export const RELOAD_KEY = 'ragondin.reloaded-for';

export type BuildJudgement =
  | { verdict: 'same' }
  | { verdict: 'reload' }
  | { verdict: 'different'; served: string | null; expected: string };

/**
 * What the page does about the build that answered, `served` (null when the
 * answer carried no identity), given its own, `expected`. Another build gets
 * one reload, which fetches that build's own UI; the same other build after
 * that reload is refused, naming both, rather than reloaded forever. When the
 * tab cannot remember having reloaded, it refuses at once for the same reason.
 */
export function judgeBuild(served: string | null, expected: string): BuildJudgement {
  if (served === expected) {
    removeStored('session', RELOAD_KEY);
    return { verdict: 'same' };
  }
  const answered = served ?? '';
  if (readStored('session', RELOAD_KEY) !== answered && writeStored('session', RELOAD_KEY, answered)) return { verdict: 'reload' };
  return { verdict: 'different', served, expected };
}

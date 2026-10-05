// The first-launch example (design document § 3, the first-run journey): a
// retrieval-only pipeline that runs without any service or any file beyond the
// benchmark — the lexical leg, `bm25` — built from what this build carries,
// never a stored fixture. A build without it opens on an empty canvas, its
// palette saying why. A dense leg is left to the palette: it needs a model and
// a tokenizer on disk, which a new workspace does not hold.
// ARCHITECTURE.md § The editor.
import type { Capabilities } from '../api/types.ts';
import { int } from '../parameters.ts';
import type { WireDocument } from './document.ts';

const carries = (caps: Capabilities, family: string, impl: string) => caps.families.find((f) => f.family === family)?.local.includes(impl) === true;

/** The lexical leg on the query, or null when the build carries no `bm25`. */
export function exampleDocument(caps: Capabilities): WireDocument | null {
  if (!carries(caps, 'retriever', 'bm25')) return null;
  return { pipeline: { inputs: ['query'], nodes: [{ id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['query'], params: { top_k: int('10') } }] } };
}

/**
 * `base`, or the first of `base-2`, `base-3`, … no pipeline holds — compared
 * without case, since on a filesystem that ignores case two such names are one
 * file and the server refuses the second.
 */
export function freshName(base: string, taken: readonly string[]): string {
  const held = new Set(taken.map((t) => t.toLowerCase()));
  if (!held.has(base.toLowerCase())) return base;
  let n = 2;
  while (held.has(`${base}-${n}`.toLowerCase())) n += 1;
  return `${base}-${n}`;
}

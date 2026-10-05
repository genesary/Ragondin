// The first-launch example (design document § 3, the first-run journey): a
// retrieval-only pipeline built from what this build carries, never a stored
// fixture — a build with nothing to compose it from opens on an empty canvas,
// its palette saying why. It names only `Local` components, so it runs
// without any service. ARCHITECTURE.md § The editor.
import type { Capabilities } from '../api/types.ts';
import { int, str } from '../parameters.ts';
import type { WireDocument, WireNode } from './document.ts';

const carries = (caps: Capabilities, family: string, impl: string) => caps.families.find((f) => f.family === family)?.local.includes(impl) === true;

/**
 * The lexical leg (`bm25`) and, when the build embeds locally (`dense` and
 * the ONNX embedder), the dense leg — fused by `rrf` when both are there;
 * null when it carries neither. The dense leg names its model and tokenizer
 * under the workspace's `models/`, which the person supplies.
 */
export function exampleDocument(caps: Capabilities): WireDocument | null {
  const lexical: WireNode | null = carries(caps, 'retriever', 'bm25') ? { id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['query'], params: { top_k: int('10') } } : null;
  const vectors: WireNode | null =
    carries(caps, 'retriever', 'dense') && carries(caps, 'embedder', 'onnx')
      ? { id: 'vectors', component: 'retriever', impl: 'dense', inputs: ['query'], params: { top_k: int('10'), embedder: str('onnx'), model: str('models/embedder.onnx'), tokenizer: str('models/tokenizer.json') } }
      : null;
  const legs = [lexical, vectors].filter((n): n is WireNode => n !== null);
  if (legs.length === 0) return null;
  const fused: WireNode[] = legs.length === 2 && carries(caps, 'fusion', 'rrf') ? [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: legs.map((l) => l.id), params: { k: int('60') } }] : [];
  // Two legs and no fusion to join them would leave two outputs: the lexical leg alone, then.
  const nodes = fused.length === 0 ? legs.slice(0, 1) : [...legs, ...fused];
  return { pipeline: { inputs: ['query'], nodes } };
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

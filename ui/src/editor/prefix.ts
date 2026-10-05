// "Run up to this node": whether the node menu's entry and the inspector's
// button may launch a prefix run, and what they say. The cut itself is the
// API's (`POST /runs` with `up_to`, runtime/ragondin-api/ARCHITECTURE.md
// § Prefix runs): these checks only anticipate its refusals, so neither
// control offers a launch the server would refuse. ARCHITECTURE.md § The
// editor.
import { listed } from '../../design/index.ts';
import type { WireDocument } from './document.ts';

/** Whether a node can be run up to: open, with the second line that says what the prefix keeps and skips, or refused, with why. */
export type RunUpTo = { kind: 'open'; line: string } | { kind: 'refused'; reason: string };

/** The pipeline as it is stored: its name, null before it is a workspace document, and whether the canvas still holds it unchanged. */
export type Stored = { name: string | null; unchanged: boolean };

/**
 * Whether `node` of `doc` can be run up to. Refused on the pipeline's output
 * — the one node no node reads, so the prefix would be the whole pipeline — and
 * on a context builder, whose context nothing scores (ADR-C30 § 3); and while
 * the canvas is not the stored document, since a run takes the document on
 * disk. Otherwise the line names the nodes kept — the node and every node it
 * reads, transitively — and those skipped, and says "no generation cost"
 * when a generator is among them: a sentence, since cost accounting is
 * outside M4.
 */
export function runUpTo(doc: WireDocument, node: string, stored: Stored): RunUpTo {
  const nodes = doc.pipeline.nodes;
  const target = nodes.find((n) => n.id === node);
  if (target === undefined) return { kind: 'refused', reason: 'Only a node can be run up to.' };
  // The output as the server finds it (`ragondin_experiments::terminal`): the one node nothing reads, when there is
  // exactly one. With a stray node nothing reads either there is none, and the server cuts at both.
  const unread = nodes.filter((n) => !nodes.some((m) => m.inputs.includes(n.id)));
  if (unread.length === 1 && unread[0]?.id === node) return { kind: 'refused', reason: 'This node is the pipeline’s output: the prefix would be the whole pipeline.' };
  if (target.component === 'context_builder') return { kind: 'refused', reason: 'A context is scored by nothing: run up to the node that feeds it chunks.' };
  if (stored.name === null) return { kind: 'refused', reason: 'Not a workspace pipeline yet: a run takes a stored document.' };
  if (!stored.unchanged) return { kind: 'refused', reason: 'The canvas differs from the stored document, and a run takes the stored one.' };

  const kept = new Set<string>();
  const pending = [node];
  while (pending.length > 0) {
    const id = pending.pop() as string;
    if (kept.has(id)) continue;
    kept.add(id);
    pending.push(...(nodes.find((n) => n.id === id)?.inputs ?? []));
  }
  const keeps = nodes.filter((n) => kept.has(n.id));
  const skips = nodes.filter((n) => !kept.has(n.id));
  const cost = skips.some((n) => n.component === 'generator') ? ', so no generation cost' : '';
  return { kind: 'open', line: `Keeps ${listed(keeps.map((n) => n.id))}. Skips ${listed(skips.map((n) => n.id))}${cost}.` };
}

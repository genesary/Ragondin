// The port rules the editor applies while an edge is drawn. The kinds come
// from the grammar `GET /workspace` serves — each family's ports as
// `ragondin-pipeline` declares them — never from a table here; with none, no
// kind is refused during the drag and the server's verdict after it is the
// only one.
// ARCHITECTURE.md § The editor.
import type { Capabilities, EdgeKind } from '../api/types.ts';
import type { WireDocument, WireNode } from './document.ts';

/** What a family's node consumes: an exact sequence of kinds, or one kind any number of times. */
export type Consumes = { fixed: EdgeKind[] } | { variadic: EdgeKind };

/** One node family's ports: what it puts out, and what it takes in. */
export type FamilyPorts = { produces: EdgeKind; consumes: Consumes };

/** Every node family's ports, by its `component:` name. */
export type PortGrammar = Readonly<Record<string, FamilyPorts>>;

/**
 * The grammar from `GET /workspace`'s capabilities: each node family's
 * ports, as the server derives them from `ragondin-pipeline`. A family no
 * node is (`embedder`) has none.
 */
export function grammarOf(capabilities: Capabilities): PortGrammar {
  const out: Record<string, FamilyPorts> = {};
  for (const { family, ports } of capabilities.families) {
    if (ports === null) continue;
    const { consumes } = ports;
    out[family] = { produces: ports.produces, consumes: consumes.shape === 'fixed' ? { fixed: consumes.kinds } : { variadic: consumes.kind } };
  }
  return out;
}

/** What a declared input carries: the query, for every pipeline this build reads (the generated `GraphInput`'s words). */
export const INPUT_KIND: EdgeKind = 'query';

/** What a family puts out, when the grammar says. */
export const producedBy = (grammar: PortGrammar | null, family: string): EdgeKind | null => grammar?.[family]?.produces ?? null;

/** The ports a node draws: one input port per slot, typed when the grammar says, and its output. */
export type NodePorts = { inputs: EdgeKind[]; output: EdgeKind };

/**
 * A node's ports in write mode. A fixed family draws its declared ports,
 * filled or not; a variadic one a port per input and one open after them.
 * With no grammar, a node draws a port per input and one open after, of no
 * known kind (`opaque`), and puts out a kind nothing names.
 */
export function portsOf(node: WireNode, grammar: PortGrammar | null): NodePorts {
  const ports = grammar?.[node.component];
  if (ports === undefined) return { inputs: Array<EdgeKind>(node.inputs.length + 1).fill('opaque'), output: 'opaque' };
  const { consumes } = ports;
  if ('fixed' in consumes) {
    const extra = Math.max(0, node.inputs.length - consumes.fixed.length);
    return { inputs: [...consumes.fixed, ...Array<EdgeKind>(extra).fill('opaque')], output: ports.produces };
  }
  return { inputs: Array<EdgeKind>(node.inputs.length + 1).fill(consumes.variadic), output: ports.produces };
}

/** Whether `from` reaches `to` along the document's edges. */
function feeds(doc: WireDocument, from: string, to: string): boolean {
  const consumers = (id: string) => doc.pipeline.nodes.filter((n) => n.inputs.includes(id)).map((n) => n.id);
  const seen = new Set<string>();
  const stack = [from];
  while (stack.length > 0) {
    const at = stack.pop()!;
    if (at === to) return true;
    if (seen.has(at)) continue;
    seen.add(at);
    stack.push(...consumers(at));
  }
  return false;
}

/**
 * Why an edge from `from` into port `port` of `to` cannot be made, in words,
 * or null when nothing here refuses it. A declared input consumes nothing; a
 * cycle is refused as such; a port holding an edge, a port past the next free
 * one — inputs are a list — and a port a fixed family does not declare are
 * refused structurally; and a kind is refused only when the grammar gives
 * both ends', naming the two in the order the validation's report does
 * (`ragondin-config`'s `incompatible_wiring`: the edge, then expected, then
 * found). Whatever passes here the server still judges.
 */
export function refusal(doc: WireDocument, grammar: PortGrammar | null, from: string, to: string, port: number): string | null {
  const consumer = doc.pipeline.nodes.find((n) => n.id === to);
  if (consumer === undefined) return doc.pipeline.inputs.includes(to) ? `\`${to}\` is a declared input: it consumes nothing.` : `\`${to}\` is not a node.`;
  if (from === to) return `\`${from}\` feeding \`${to}\` would close a cycle: a node cannot feed itself.`;
  if (feeds(doc, to, from)) return `\`${from}\` feeding \`${to}\` would close a cycle: \`${to}\` already feeds \`${from}\`.`;
  if (port < consumer.inputs.length) return `Port ${port} of \`${to}\` already holds \`${consumer.inputs[port]!}\`.`;
  if (port > consumer.inputs.length) return `Fill port ${consumer.inputs.length} of \`${to}\` first: its inputs are a list, in port order.`;
  const ports = grammar?.[consumer.component];
  if (ports === undefined) return null;
  if ('fixed' in ports.consumes && port >= ports.consumes.fixed.length) return `\`${to}\` declares no port at position ${port}.`;
  const expected = 'fixed' in ports.consumes ? ports.consumes.fixed[port]! : ports.consumes.variadic;
  const producer = doc.pipeline.nodes.find((n) => n.id === from);
  const found = doc.pipeline.inputs.includes(from) ? INPUT_KIND : producer === undefined ? null : producedBy(grammar, producer.component);
  if (found === null || found === expected) return null;
  return `\`${from}\` feeds \`${to}\` at port ${port}: expected ${expected}, found ${found}.`;
}

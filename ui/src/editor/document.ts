// The one in-memory shape of a pipeline the editor holds: the API's typed
// document (ADR-C40), which has the wire schema's shape — `RawPipeline`
// (`core/ragondin-pipeline/src/raw.rs`) as a configuration writes it — with
// every parameter value tagged with its kind; never a lowered or logical form
// (INV-9). The browser never reads or writes the configuration format.
// ARCHITECTURE.md § The editor.
import type { EdgeKind, Graph, TypedDocument, TypedNode, ValidationRequest } from '../api/types.ts';
import { INPUT_KIND, producedBy, type PortGrammar } from './ports.ts';

/** One node, as § 5.1 of the system architecture writes it, each parameter with its kind. */
export type WireNode = TypedNode;

/** A whole pipeline document. No `version` means the version the server reads. */
export type WireDocument = TypedDocument;

/** A new pipeline: one declared input, the query, and no node. */
export const emptyDocument = (): WireDocument => ({ pipeline: { inputs: ['query'], nodes: [] } });

/**
 * The first free id derived from an `impl:` name: `bm25`, `bm25-2`, … An id
 * is taken by a declared input, by a node, and by any node's input naming it —
 * a consumer of a deleted node still names it, and a new node under that name
 * would silently take its edges back.
 */
export function freshId(doc: WireDocument, impl: string): string {
  const taken = new Set([...doc.pipeline.inputs, ...doc.pipeline.nodes.flatMap((n) => [n.id, ...n.inputs])]);
  if (!taken.has(impl)) return impl;
  let n = 2;
  while (taken.has(`${impl}-${n}`)) n += 1;
  return `${impl}-${n}`;
}

/**
 * What `POST /pipelines/validate` is sent: the typed document and nothing
 * else — no position, which lives beside it (ADR-016 § 4). The server renders
 * it as the configuration format and checks that rendering, so the hash it
 * answers is the one of the bytes a write would store (ADR-C40 § 5).
 */
export const validationRequest = (doc: WireDocument): ValidationRequest => ({ typed: doc });

/**
 * The document as the canvas draws it: the generated `Graph`, as
 * `GET /runs/{id}` serves one — nodes sorted by id, one edge per input in
 * port order, each typed by what its producer puts out when the grammar says,
 * else `opaque`. An input naming nothing draws no edge.
 */
export function toGraph(doc: WireDocument, grammar: PortGrammar | null): Graph {
  const { inputs, nodes } = doc.pipeline;
  const families = new Map(nodes.map((n) => [n.id, n.component]));
  const kindOf = (id: string): EdgeKind | null => {
    if (inputs.includes(id)) return INPUT_KIND;
    const family = families.get(id);
    if (family === undefined) return null;
    return producedBy(grammar, family) ?? 'opaque';
  };
  return {
    inputs: inputs.map((id) => ({ id, kind: INPUT_KIND })),
    nodes: [...nodes]
      .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
      .map((n) => ({ id: n.id, family: n.component, implementation: n.impl, parameters: n.params })),
    edges: [...nodes]
      .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
      .flatMap((n) =>
        n.inputs.flatMap((from, port) => {
          const kind = kindOf(from);
          return kind === null ? [] : [{ from, to: n.id, port, kind }];
        }),
      ),
  };
}

/**
 * The consumers an input of which names nothing — what deleting a node
 * leaves, since its consumers are not silently rewired — each with the first
 * such port, in words. The server's validation says the same, as a dangling
 * input; this is said on every such node at once, where the server names one.
 */
export function danglingInputs(doc: WireDocument): Record<string, string> {
  const known = new Set([...doc.pipeline.inputs, ...doc.pipeline.nodes.map((n) => n.id)]);
  const out: Record<string, string> = {};
  for (const node of doc.pipeline.nodes) {
    const port = node.inputs.findIndex((id) => !known.has(id));
    if (port >= 0) out[node.id] = `Port ${port} names \`${node.inputs[port]!}\`, which is not a node or an input.`;
  }
  return out;
}

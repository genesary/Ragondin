import type { EdgeKind, Graph, ParameterValue } from '../api/types.ts';
import type { Family } from '../../design/index.ts';

/** What a port carries: the API's edge kind, drawn by shape (Port.tsx). */
export type PortKind = EdgeKind;

/** One card of the canvas, derived from the lowered graph and nothing else. */
export type CanvasNode = {
  id: string;
  /** The design system's family: the pigment and the glyph of its tile. */
  family: Family;
  /** The line under the name: `<family>/<impl>` as the configuration spells it. */
  impl: string;
  /** The one parameter worth seeing at a glance, when the node sets it. */
  param?: { name: string; value: string };
  /** One port per input, in port order, typed by the edge feeding it. */
  inputs: PortKind[];
  /** What the node puts out, or null when nothing says. */
  output: PortKind | null;
};

/**
 * What a caller knows of one node's execution, for one query — Replay passes
 * what it has. Every field is optional and draws on its own; a node with no
 * overlay draws the plain card.
 */
export type NodeOverlay = {
  /** The node's wall time, in milliseconds. */
  durationMs?: number;
  /** Its share of the query's total time, from 0 to 1: the duration bar's length. */
  share?: number;
  /** The metric measured after this node, formatted by the caller. */
  metric?: { name: string; value: string };
  /** The 1-based ranks at which gold passages landed in its output: the rank strip's filled cells. */
  ranks?: readonly number[];
  /** How many candidates the node dropped. */
  discarded?: number;
  /** The node failed, with this message. */
  error?: string;
  /** The node is absent from the run beside this one: the tag the card wears, e.g. "only in B". */
  onlyHere?: string;
};

export type CanvasEdge ={ id: string; from: string; to: string; port: number; kind: PortKind };

export type CanvasModel = {
  /** In topological order — the order Tab walks. */
  nodes: CanvasNode[];
  edges: CanvasEdge[];
};

const FAMILY: Record<string, Family> = {
  retriever: 'retriever',
  fusion: 'fusion',
  reranker: 'reranker',
  context_builder: 'context',
  generator: 'generator',
};

// The parameter the design system shows on each family's card.
const KEY_PARAMETER: Partial<Record<Family, string>> = {
  retriever: 'top_k',
  reranker: 'top_k',
  fusion: 'k',
  context: 'max_chunks',
  generator: 'temperature',
};

// What a family puts out when no edge leaves the node to say so.
const DEFAULT_OUTPUT: Partial<Record<Family, PortKind>> = {
  retriever: 'chunks',
  fusion: 'chunks',
  reranker: 'chunks',
  context: 'context',
  generator: 'answer',
};

const show = (value: ParameterValue): string => (Array.isArray(value) ? value.map(show).join(', ') : String(value));

export const edgeId = (from: string, to: string, port: number) => `${from}->${to}:${port}`;

/**
 * The lowered graph as the canvas draws it. A family this build has no card
 * for (an extension node, Branch or Loop) takes the neutral diamond tile.
 */
export function toModel(graph: Graph): CanvasModel {
  const edges: CanvasEdge[] = graph.edges.map((e) => ({ id: edgeId(e.from, e.to, e.port), from: e.from, to: e.to, port: e.port, kind: e.kind }));
  const outputOf = (id: string) => edges.find((e) => e.from === id)?.kind;

  const inputs: CanvasNode[] = graph.inputs.map((input) => ({
    id: input.id,
    family: 'query',
    impl: 'pipeline input',
    inputs: [],
    output: outputOf(input.id) ?? input.kind,
  }));
  const nodes: CanvasNode[] = graph.nodes.map((node) => {
    const family = FAMILY[node.family] ?? 'control';
    const ports: PortKind[] = [];
    for (const e of edges) if (e.to === node.id) ports[e.port] = e.kind;
    const key = KEY_PARAMETER[family];
    const value = key === undefined ? undefined : node.parameters[key];
    return {
      id: node.id,
      family,
      impl: `${node.family}/${node.implementation}`,
      ...(key !== undefined && value !== undefined ? { param: { name: key, value: show(value) } } : {}),
      inputs: Array.from(ports, (kind) => kind ?? 'opaque'),
      output: outputOf(node.id) ?? DEFAULT_OUTPUT[family] ?? null,
    };
  });
  return { nodes: topological([...inputs, ...nodes], edges), edges };
}

// Kahn's algorithm, always taking the first ready node in the given order, so
// the walk is the canonical order wherever the graph leaves a choice. A node
// on a cycle (which the API's validation refuses) is appended in that order.
function topological(nodes: CanvasNode[], edges: CanvasEdge[]): CanvasNode[] {
  const pending = new Map(nodes.map((n) => [n.id, edges.filter((e) => e.to === n.id && e.from !== n.id).length]));
  const out: CanvasNode[] = [];
  let rest = nodes;
  while (rest.length > 0) {
    const next = rest.find((n) => pending.get(n.id) === 0) ?? rest[0]!;
    out.push(next);
    rest = rest.filter((n) => n !== next);
    for (const e of edges) if (e.from === next.id) pending.set(e.to, (pending.get(e.to) ?? 1) - 1);
  }
  return out;
}

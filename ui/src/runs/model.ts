// What the Runs screen shows of a run, apart from where it was read. The
// screen renders `RunRow`s and `RunGroup`s only; `rowsFromListing` fills them
// from `GET /runs`, and later sources — the job queue's queued, running and
// failed rows, the prefix relation — fill the same fields rather than add a
// second row shape. A field the listing does not carry stays null and its
// column or label is not drawn: nothing here is invented to fill a cell.
// ARCHITECTURE.md § The Runs screen.
import type { Family } from '../../design/index.ts';
import type { Graph, RunListing } from '../api/types.ts';

/** Which ground truth a metric reads: qrels score `ranking`, reference answers `answers` (ADR-008, ADR-C30 § 1). */
export type MetricFamily = 'ranking' | 'answers';

/** Metrics of one family, or of a family the source does not say (`null`). */
export type MetricGroup = { family: MetricFamily | null; metrics: { name: string; value: number }[] };

/**
 * Where a run stands. `done` and `failed` are full rows; `queued` and
 * `running` are the slot the job queue fills, drawn as their status chip only.
 */
export type RowStatus =
  | { state: 'done' }
  | { state: 'failed'; /** The node that failed, when the failure names one. */ node: string | null; error: string }
  | { state: 'queued' }
  | { state: 'running'; /** The real fraction done, 0 to 1. */ fraction: number };

export type RunRow = {
  /** The run's content address. */
  id: string;
  /** The canonical hash of the pipeline it ran. */
  pipeline: string;
  /** The pipeline's name, when the source knows it. */
  pipelineName: string | null;
  /** The benchmark's identity: its dataset version, what "one benchmark" compares. */
  benchmark: string;
  /** The benchmark's name, when the source knows it. */
  benchmarkName: string | null;
  status: RowStatus;
  /** One group per family the run recorded, none when it recorded no metric. */
  metrics: MetricGroup[];
  /** Median query latency, in milliseconds, when the source reports one. */
  latencyMs: number | null;
  /** When the run started, as an ISO 8601 instant, when the source reports one. */
  startedAt: string | null;
  /** The group of the pipeline this run is a prefix of, and the node it stops at. */
  prefix: { parent: string; upTo: string | null } | null;
};

/** A pipeline's runs, under one heading. */
export type RunGroup = {
  /** The pipeline's name, else its canonical hash: what the group links to. */
  key: string;
  name: string | null;
  /** The canonical hash of the group's own pipeline. */
  pipeline: string;
  /** The run whose graph draws the group's shape: one of the pipeline's own, not a prefix. */
  shapeFrom: string;
  rows: RunRow[];
};

/**
 * The listing's runs as rows, in its order. Every run the store holds is
 * finished, so each is `done`. The listing carries neither the benchmark's
 * nor the pipeline's name, nor a latency, a start time, the metric families
 * or the prefix relation (#376 asks for them), so those are null and the
 * metrics are one group of unsaid family.
 */
export function rowsFromListing(listing: RunListing): RunRow[] {
  return listing.runs.map((run) => {
    const metrics = Object.entries(run.metrics).map(([name, value]) => ({ name, value }));
    return {
      id: run.id,
      pipeline: run.pipeline,
      pipelineName: null,
      benchmark: run.dataset_version,
      benchmarkName: null,
      status: { state: 'done' },
      metrics: metrics.length === 0 ? [] : [{ family: null, metrics }],
      latencyMs: null,
      startedAt: null,
      prefix: null,
    };
  });
}

const groupKey = (row: RunRow) => row.prefix?.parent ?? row.pipelineName ?? row.pipeline;

/**
 * Rows grouped by pipeline — by name when there is one, by canonical hash
 * otherwise — each group in the order its first row appears, each row in
 * listing order. A prefix run joins its parent's group.
 */
export function groupRows(rows: readonly RunRow[]): RunGroup[] {
  const groups = new Map<string, RunRow[]>();
  for (const row of rows) {
    const key = groupKey(row);
    groups.set(key, [...(groups.get(key) ?? []), row]);
  }
  return [...groups].map(([key, members]) => {
    const own = members.find((r) => r.prefix === null) ?? (members[0] as RunRow);
    return { key, name: own.prefix === null ? own.pipelineName : null, pipeline: own.pipeline, shapeFrom: own.id, rows: members };
  });
}

/** A hash as the screen prints it: its first twelve digits. */
export const shortHash = (hash: string) => hash.slice(0, 12);

/** The benchmark in words: its name, else its short dataset digest. */
export const benchmarkLabel = (row: Pick<RunRow, 'benchmark' | 'benchmarkName'>) => row.benchmarkName ?? `dataset ${shortHash(row.benchmark)}`;

/**
 * A metric's value as its chip prints it (design/'s MetricChip): four
 * decimals for a ranking metric, a percentage to one decimal for an answer
 * metric; four decimals while the family is not known.
 */
export function formatMetric(family: MetricFamily | null, value: number): string {
  return family === 'answers' ? (value * 100).toFixed(1) : value.toFixed(4);
}

/** The design system's tile for a graph family, spelled as a configuration's `component:` value. */
const TILE: Record<string, Family> = {
  retriever: 'retriever',
  fusion: 'fusion',
  reranker: 'reranker',
  context_builder: 'context',
  generator: 'generator',
};

/** One node of a pipeline's shape: its family's tile, or its family as a word when no tile draws it. */
export type ShapeNode = { node: string; family: Family; word?: never } | { node: string; family: null; word: string };

/**
 * A pipeline's nodes in pipeline order — every node after the nodes it
 * reads — ties kept in the graph's canonical order, each as its family.
 */
export function shapeOf(graph: Graph): ShapeNode[] {
  const ids = new Set(graph.nodes.map((n) => n.id));
  const waiting = new Map(graph.nodes.map((n) => [n.id, new Set(graph.edges.filter((e) => e.to === n.id && ids.has(e.from)).map((e) => e.from))]));
  const order: string[] = [];
  while (order.length < graph.nodes.length) {
    const ready = graph.nodes.find((n) => !order.includes(n.id) && [...(waiting.get(n.id) ?? [])].every((from) => order.includes(from)));
    // A cycle the API would never send: keep the rest in canonical order rather than loop.
    const next = ready ?? graph.nodes.find((n) => !order.includes(n.id));
    if (next === undefined) break;
    order.push(next.id);
  }
  return order.map((id) => {
    const family = graph.nodes.find((n) => n.id === id)?.family ?? '';
    const tile = TILE[family];
    return tile === undefined ? { node: id, family: null, word: family } : { node: id, family: tile };
  });
}

// What the Runs screen shows of a run, apart from where it was read. The
// screen renders `RunRow`s and `RunGroup`s only; `rowsFromListing` fills them
// from `GET /runs`, and later sources — the job queue's queued, running,
// failed and cancelled rows, the prefix relation — fill the same fields rather
// than add a second row shape. A field the listing does not carry stays null
// and its column or label is not drawn: nothing here is invented to fill a
// cell. ARCHITECTURE.md § The Runs screen.
import { familyOfComponent, type Family } from '../../design/index.ts';
import type { Graph, RunListing, RunSummary } from '../api/types.ts';
import type { Route } from '../routes.ts';

/** Which ground truth a metric reads: qrels score `ranking`, reference answers `answers` (ADR-008, ADR-C30 § 1). */
export type MetricFamily = 'ranking' | 'answers';

/** Metrics of one family, or of a family the source does not say (`null`). */
export type MetricGroup = { family: MetricFamily | null; metrics: { name: string; value: number }[] };

/**
 * What a row stands for: a run in the store, or a job of the queue — which
 * may name the run it produced. Only a run can be selected for Compare.
 */
export type RowSource = { kind: 'run'; id: string } | { kind: 'job'; id: string; runId: string | null };

/**
 * Where a row stands. `done` and `failed` are full rows; `queued`, `running`
 * and `cancelled` are the slot the job queue fills, drawn as their status
 * chip only.
 */
export type RowStatus =
  | { state: 'done' }
  | { state: 'failed'; /** The node that failed, when the failure names one. */ node: string | null; error: string }
  | { state: 'queued' }
  | { state: 'running'; /** Queries done, out of `total`, as the queue reports them. */ done: number; total: number }
  | { state: 'cancelled' };

export type RunRow = {
  source: RowSource;
  /** The canonical hash of the pipeline it ran. */
  pipeline: string;
  /**
   * Every name the pipeline goes by — each workspace document whose canonical
   * hash is `pipeline` — sorted; empty when the source knows none. A list,
   * never a pick: several documents can be one pipeline.
   */
  pipelineNames: string[];
  /** The benchmark's identity: its dataset version, what "one benchmark" compares. */
  benchmark: string;
  /** Every registry entry pinned to `benchmark`, sorted; empty when the source knows none. */
  benchmarkNames: string[];
  status: RowStatus;
  /** One group per family the run recorded, none when it recorded no metric. */
  metrics: MetricGroup[];
  /** Median query latency, in milliseconds, when the source reports one. */
  latencyMs: number | null;
  /** When the run started, as an ISO 8601 instant, when the source reports one. */
  startedAt: string | null;
  /**
   * The pipeline this run is a prefix of — as the prefix relation names it:
   * a pipeline's name, a pipeline's hash or a run's id — and the node it
   * stops at.
   */
  prefix: { parent: string; upTo: string | null } | null;
};

/** A pipeline's runs, under one heading. */
export type RunGroup = {
  /** The pipeline's canonical hash — or, for a prefix run's parent with no row of its own here, the name the prefix relation gives it. */
  key: string;
  /** Every name the group's own pipeline goes by, each linked; empty when it has none, and the group is then named by its hash. */
  names: string[];
  /** The canonical hash of the group's own pipeline. */
  pipeline: string;
  /** The key of the listing's `shapes` that draws the group: its own pipeline's hash; null when it has no row of its own. */
  shapeKey: string | null;
  rows: RunRow[];
};

/** A row's key among all rows: a run and a job never share one. */
export const rowKey = (row: RunRow) => `${row.source.kind}:${row.source.id}`;

/** The run a row selects, when it is one: a store run, never a job. */
export const runId = (row: RunRow) => (row.source.kind === 'run' ? row.source.id : null);

/**
 * Where opening a row leads: a run to Replay, before a query is chosen. A job
 * has no address in this build — the job view is the launch flow's — so it
 * opens nowhere rather than as a run it is not.
 */
export function openRoute(row: RunRow): Route | null {
  return row.source.kind === 'run' ? { screen: 'replay', run: row.source.id } : null;
}

/** The running chip's words: the real count, as the queue reports it. */
export const runningLabel = (status: Extract<RowStatus, { state: 'running' }>) =>
  `running ${status.done.toLocaleString('en-US')} / ${status.total.toLocaleString('en-US')}`;

/**
 * Most recent first by start time; a run whose start is unknown after every
 * run whose start is known — never chosen over one — and ties, unknown
 * included, broken by run id so the order is stable.
 */
export function byMostRecent(a: Pick<RunSummary, 'id' | 'started_at_ms'>, b: Pick<RunSummary, 'id' | 'started_at_ms'>): number {
  if (a.started_at_ms !== b.started_at_ms) {
    if (a.started_at_ms === null) return 1;
    if (b.started_at_ms === null) return -1;
    return b.started_at_ms - a.started_at_ms;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/**
 * The listing's runs as rows, most recent first (`byMostRecent`). Every run
 * the store holds is finished, so each is `done`. The names are every one the
 * listing gives; the start time is the run's own record, or null. The listing
 * carries no latency, metric family or prefix relation (#383, #357), so those
 * are null and the metrics are one group of unsaid family.
 */
export function rowsFromListing(listing: RunListing): RunRow[] {
  return [...listing.runs].sort(byMostRecent).map((run) => {
    const metrics = Object.entries(run.metrics).map(([name, value]) => ({ name, value }));
    return {
      source: { kind: 'run', id: run.id },
      pipeline: run.pipeline,
      pipelineNames: run.pipeline_names,
      benchmark: run.dataset_version,
      benchmarkNames: run.benchmark_names,
      status: { state: 'done' },
      metrics: metrics.length === 0 ? [] : [{ family: null, metrics }],
      latencyMs: null,
      startedAt: run.started_at_ms === null ? null : new Date(run.started_at_ms).toISOString(),
      prefix: null,
    };
  });
}

/**
 * Rows grouped by pipeline — by canonical hash, the pipeline's identity
 * whatever names it goes by — each group in the order its first row appears,
 * each row in the order given. A prefix run joins its parent's group, the
 * parent named by one of the group's names, its pipeline's hash or one of its
 * runs' ids; a parent with no run here gets a group of its own, so the prefix
 * run is still shown.
 */
export function groupRows(rows: readonly RunRow[]): RunGroup[] {
  const groups = new Map<string, RunRow[]>();
  for (const row of rows) if (row.prefix === null) groups.set(row.pipeline, [...(groups.get(row.pipeline) ?? []), row]);
  const parentOf = (parent: string) =>
    [...groups].find(([key, members]) => key === parent || members.some((m) => m.pipelineNames.includes(parent) || m.source.id === parent))?.[0] ?? parent;
  // Keep the listing's order: place every row, prefix runs included, in turn.
  const ordered = new Map<string, RunRow[]>();
  for (const row of rows) {
    const key = row.prefix === null ? row.pipeline : parentOf(row.prefix.parent);
    ordered.set(key, [...(ordered.get(key) ?? []), row]);
  }
  return [...ordered].map(([key, members]) => {
    const own = members.filter((r) => r.prefix === null);
    const head = own[0] ?? (members[0] as RunRow);
    return {
      key,
      names: own.length === 0 ? [] : head.pipelineNames,
      pipeline: head.pipeline,
      shapeKey: own.length === 0 ? null : head.pipeline,
      rows: members,
    };
  });
}

/** A hash as the screen prints it: its first twelve digits. */
export const shortHash = (hash: string) => hash.slice(0, 12);

/** The benchmark in words: every name pinned to it, else its short dataset digest. */
export const benchmarkLabel = (row: Pick<RunRow, 'benchmark' | 'benchmarkNames'>) =>
  row.benchmarkNames.length === 0 ? `dataset ${shortHash(row.benchmark)}` : row.benchmarkNames.join(', ');

/**
 * A metric's value as its chip prints it (design/'s MetricChip): four
 * decimals for a ranking metric, a percentage to one decimal for an answer
 * metric; four decimals while the family is not known.
 */
export function formatMetric(family: MetricFamily | null, value: number): string {
  return family === 'answers' ? (value * 100).toFixed(1) : value.toFixed(4);
}

/** One node of a pipeline's shape: its family's tile, or its family as a word when no tile draws it. */
export type ShapeNode = { node: string; family: Family; word?: never } | { node: string; family: null; word: string };

/**
 * A pipeline's nodes in pipeline order — every node after the nodes it
 * reads — ties kept in the graph's canonical order, each as its family's
 * tile (design/'s `familyOfComponent`).
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
    const tile = familyOfComponent(family);
    return tile === null ? { node: id, family: null, word: family } : { node: id, family: tile };
  });
}

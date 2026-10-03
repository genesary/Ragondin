// What the Runs screen shows of a run, apart from where it was read. The
// screen renders `RunRow`s and `RunGroup`s only; `rowsFromListing` fills them
// from `GET /runs`, and later sources — the job queue's queued, running,
// failed and cancelled rows, the prefix relation — fill the same fields rather
// than add a second row shape. A field the listing does not carry stays null
// and its column or label is not drawn: nothing here is invented to fill a
// cell. ARCHITECTURE.md § The Runs screen.
import { familyOfComponent, type Family } from '../../design/index.ts';
import type { Graph, MetricFamily, RunListing, RunSummary } from '../api/types.ts';
import type { Route } from '../routes.ts';

/**
 * Which ground truth a metric reads, as the API's catalogue says: qrels score
 * `ranking`, reference answers `answers` (ADR-008, ADR-C30 § 1), and a name
 * the catalogue does not know is `unknown` — shown, never hidden.
 */
export type { MetricFamily };

/** The order families are drawn in on a row. */
const FAMILY_ORDER: readonly MetricFamily[] = ['ranking', 'answers', 'unknown'];

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
  /**
   * The run's median query latency — the lower median over its queries of
   * each one's summed node durations — in milliseconds, when the source
   * reports one. Not the run's wall time.
   */
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
 * A run's metrics grouped by the family the listing gives each — ranking,
 * then answers, then unknown — each group in the order the run recorded them.
 * A name the listing gives no family for is `unknown`: shown, never dropped.
 */
function metricGroups(run: RunSummary): MetricGroup[] {
  const metrics = Object.entries(run.metrics).map(([name, value]) => ({ name, value, family: run.metric_families[name] ?? 'unknown' }));
  return FAMILY_ORDER.flatMap((family) => {
    const own = metrics.filter((m) => m.family === family).map(({ name, value }) => ({ name, value }));
    return own.length === 0 ? [] : [{ family, metrics: own }];
  });
}

/**
 * The listing's runs as rows, most recent first (`byMostRecent`). Every run
 * the store holds is finished, so each is `done`. The names are every one the
 * listing gives; the start time is the run's own record, or null; the metrics
 * are grouped by the family the listing gives each; the latency is the
 * listing's median query latency, or null. The listing carries no prefix
 * relation (#357), so that is null.
 */
export function rowsFromListing(listing: RunListing): RunRow[] {
  return [...listing.runs].sort(byMostRecent).map((run) => {
    return {
      source: { kind: 'run', id: run.id },
      pipeline: run.pipeline,
      pipelineNames: run.pipeline_names,
      benchmark: run.dataset_version,
      benchmarkNames: run.benchmark_names,
      status: { state: 'done' },
      metrics: metricGroups(run),
      latencyMs: run.median_query_latency_nanos === null ? null : run.median_query_latency_nanos / 1e6,
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
 * metric, the stored value as it is for a metric of unknown family — nothing
 * says how to round it — and four decimals while a source says no family.
 */
export function formatMetric(family: MetricFamily | null, value: number): string {
  if (family === 'answers') return (value * 100).toFixed(1);
  if (family === 'unknown') return String(value);
  return value.toFixed(4);
}

/** What the design calls an answer metric (`EM 41.2`); every other metric goes by its stored name. */
const METRIC_LABEL: Readonly<Record<string, string>> = { exact_match: 'EM', token_f1: 'F1' };

/** A metric's name as its chip prints it. */
export const metricLabel = (name: string) => METRIC_LABEL[name] ?? name;

/** A latency in milliseconds as a cell prints it: whole milliseconds, or two significant figures under ten, so a fast query does not read as 0 ms. */
export const formatLatency = (ms: number) => `${ms >= 10 ? Math.round(ms).toLocaleString('en-US') : Number(ms.toPrecision(2)).toString()} ms`;

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

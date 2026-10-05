// What the Runs screen shows of a run, apart from where it was read. The
// screen renders `RunRow`s and `RunGroup`s only; `rowsFromListing` fills them
// from `GET /runs`, and later sources — the job queue's queued, running,
// failed and cancelled rows, the prefix relation — fill the same fields rather
// than add a second row shape. A field the listing does not carry stays null
// and its column or label is not drawn: nothing here is invented to fill a
// cell. ARCHITECTURE.md § The Runs screen.
import { familyOfComponent, prefixWords, type Family } from '../../design/index.ts';
import type { Graph, MetricFamily, NameHeld, RunListing, RunRequest, RunSummary } from '../api/types.ts';
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
 * Where a row stands. A run of the store is `done`; a job of the queue is any
 * of the five, as the stream last said.
 */
export type RowStatus =
  | { state: 'done' }
  | { state: 'failed'; /** The node that failed, when the failure names one. */ node: string | null; error: string }
  | { state: 'queued' }
  | { state: 'running'; /** Queries done, out of `total`, as the queue reports them; `total` null until the first query. */ done: number; total: number | null }
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
  /**
   * The names among `pipelineNames` the listing says the API refuses to read
   * (`refused_pipeline_names`): another stored name differs from each only in
   * case. Empty when it refuses none, and for a job, which has no hash match.
   */
  refusedNames: string[];
  /**
   * The workspace pipeline name the run's launch record says it was launched
   * as — a prefix run's parent's — or null when it has no record naming one.
   * Recorded at launch, so it may name a pipeline whose content has changed
   * since, or that is gone. A fact beside `pipelineNames`, never resolved
   * with it into one name (ADR-C39 § 4).
   */
  launchedAs: string | null;
  /**
   * Whether the workspace holds `launchedAs` now, as the listing says it
   * (`launched_as.held`, from the same listing of `pipelines/` as the hash
   * matches): exactly, refused as a case alias of a stored name, or gone;
   * null with no name.
   */
  launchedHeld: NameHeld | null;
  /** Whether the run has a launch record at all — one may name no pipeline. */
  launchRecorded: boolean;
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
   * The pipelines this run is a prefix of, and the node it stops at: the
   * parent its launch record names, when the record carries `prefix_of`;
   * otherwise every current document the listing says it is structurally a
   * prefix of (`prefix_of_documents`) — a prefix written by hand and run from
   * the command line is one too. A job's is the parent it was submitted from.
   */
  prefix: { parents: string[]; upTo: string } | null;
  /**
   * What the structural test says the run's content is a prefix of, when its
   * launch record names a prefix and the test names other documents: ADR-C39
   * § 4's other fact, drawn beside the recorded one and never merged into it.
   * Null otherwise — with no recorded prefix, the structural relation is
   * `prefix` itself.
   */
  contentPrefix: { parents: string[]; upTo: string } | null;
  /** What the queue says of a job's row; null for a run of the store. */
  job: JobFacts | null;
  /**
   * The id the run was announced under, when it was filed under another
   * (`id_mismatch`): what ran differs from what was announced (ADR-C36 § 1).
   * Null for every other run, and for a job.
   */
  announced: string | null;
};

/** A job's own facts, beside what its row shares with a run's. */
export type JobFacts = {
  /** What was submitted, sent again as it is to resubmit the job. */
  submission: RunRequest;
  /** Its place among the queued run jobs, 0 the next taken; null when it is not queued. */
  place: number | null;
  /** How many run jobs are queued, so the last place is known. */
  queued: number;
  /** When the worker took it, in milliseconds since the epoch; null before, or when unknown. */
  startedAtMs: number | null;
  /**
   * The median query latency so far, in milliseconds, as the queue reports
   * it on each tick; null before the first query. Never computed here.
   */
  medianMs: number | null;
  /** The run a done job filed, as the queue says; null before it is done, or when it names none. */
  filed: string | null;
  /** Both ids, when the run was filed under another than the one announced. */
  mismatch: { announced: string; decided: string } | null;
  /** The reason of each fault the queue reported beside the job, in their order: none stopped it. */
  faults: string[];
};

/** A pipeline's runs, under one heading. */
export type RunGroup = {
  /** The group's identity among the groups: its recorded name, its runs' hash matches, or its canonical hash, each kept apart from the others. */
  key: string;
  /** The name or names the group is headed by, each linked: a recorded name, or every hash match; empty when the group is its canonical hash. */
  names: string[];
  /**
   * Whether the workspace holds each name the group is headed by, one per
   * name in `names`' order: a recorded name as its runs' records say —
   * `unchecked` when they say nothing, so it fails closed, unlinked — and a
   * hash match `exactly`, a current document of the same listing, unless
   * the listing says the API refuses it (`refusedNames`): `other_case`.
   * Empty for a group headed by its hash.
   */
  held: NameHeld[];
  /** The canonical hash of the group's most recent run of its own — not a prefix — or, with none, of its first run. */
  pipeline: string;
  /** The key of the listing's `shapes` that draws the group: `pipeline`; null when it has no row of its own. */
  shapeKey: string | null;
  rows: RunRow[];
};

/** A row's key among all rows: a run and a job never share one. */
export const rowKey = (row: RunRow) => `${row.source.kind}:${row.source.id}`;

/** The run a row selects, when it is one: a store run, never a job. */
export const runId = (row: RunRow) => (row.source.kind === 'run' ? row.source.id : null);

/**
 * Where opening a row leads: a run to Replay, before a query is chosen; a job
 * to its own address in Runs (`#runs/job/<id>`), never as a run it is not.
 */
export function openRoute(row: RunRow): Route {
  return row.source.kind === 'run' ? { screen: 'replay', run: row.source.id } : { screen: 'runs', job: row.source.id };
}

/** The running chip's words: the real count, as the queue reports it, or `starting` before it knows the total. */
export const runningLabel = (status: Extract<RowStatus, { state: 'running' }>) =>
  status.total === null ? 'starting' : `running ${status.done.toLocaleString('en-US')} / ${status.total.toLocaleString('en-US')}`;

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
 * listing gives; the recorded name is the launch record's, or null; the start
 * time is the run's own record, or null; the metrics are grouped by the
 * family the listing gives each; the latency is the listing's median query
 * latency, or null. A run is a prefix of the parent its launch record names,
 * or, with no such record, of every current document the listing says it is
 * structurally a prefix of.
 */
export function rowsFromListing(listing: RunListing): RunRow[] {
  return [...listing.runs].sort(byMostRecent).map((run) => {
    return {
      source: { kind: 'run', id: run.id },
      pipeline: run.pipeline,
      pipelineNames: run.pipeline_names,
      refusedNames: run.refused_pipeline_names,
      launchedAs: run.launched_as?.name ?? null,
      launchedHeld: run.launched_as?.held ?? null,
      launchRecorded: run.launched_as !== null,
      benchmark: run.dataset_version,
      benchmarkNames: run.benchmark_names,
      status: { state: 'done' },
      metrics: metricGroups(run),
      latencyMs: run.median_query_latency_nanos === null ? null : run.median_query_latency_nanos / 1e6,
      startedAt: run.started_at_ms === null ? null : new Date(run.started_at_ms).toISOString(),
      prefix: prefixOf(run),
      contentPrefix: contentPrefixOf(run),
      job: null,
      announced: null,
    };
  });
}

/**
 * A run's prefix relation: the parent its launch record names, and the node
 * it stops at; else the documents the structural test finds (ADR-C39 § 5),
 * all stopping at the run's one output; null for any other run.
 */
export function prefixOf(run: Pick<RunSummary, 'launched_as' | 'prefix_of_documents'>): RunRow['prefix'] {
  const record = run.launched_as;
  if (record?.name != null && record.prefix_of !== null) return { parents: [record.name], upTo: record.prefix_of.up_to };
  const [first] = run.prefix_of_documents;
  if (first === undefined) return null;
  return { parents: run.prefix_of_documents.map((d) => d.pipeline), upTo: first.up_to };
}

/**
 * The structural relation beside a recorded prefix, when it names other
 * documents than the record's parent; null otherwise.
 */
function contentPrefixOf(run: RunSummary): RunRow['contentPrefix'] {
  const record = run.launched_as;
  const [first] = run.prefix_of_documents;
  if (record?.name == null || record.prefix_of === null || first === undefined) return null;
  const parents = run.prefix_of_documents.map((d) => d.pipeline);
  if (parents.length === 1 && parents[0] === record.name) return null;
  return { parents, upTo: first.up_to };
}

/**
 * A run's prefix relation as one selector option's words — "prefix of
 * hybrid, up to rerank" — or null for a run that is no prefix: Compare's and
 * Replay's run selectors name it as Runs' label does.
 */
export function prefixText(run: Pick<RunSummary, 'launched_as' | 'prefix_of_documents'>): string | null {
  const prefix = prefixOf(run);
  return prefix === null ? null : prefixWords(prefix.parents, prefix.upTo);
}

/**
 * Where a row is grouped (ADR-C39 § 4): under the name its launch record
 * gives, when it has one; otherwise under its hash matches, every one;
 * otherwise, for a prefix the structural test found, under the documents it
 * is a prefix of; and otherwise under its canonical hash. One name is one
 * group whichever fact gave it, so a run without a record whose one match is
 * `hybrid` sits with the runs launched as `hybrid`. A prefix run's record
 * names its parent, so it sits in the parent's group, and so does a prefix
 * run from the command line, which no document holds. The kinds are kept
 * apart in the key, so a document named like a hash is never that hash.
 */
function groupOf(row: RunRow): { key: string; names: string[] } {
  if (row.launchedAs !== null) return { key: `name:${row.launchedAs}`, names: [row.launchedAs] };
  const names = row.pipelineNames.length > 0 ? row.pipelineNames : (row.prefix?.parents ?? []);
  if (names.length === 1) return { key: `name:${names[0]}`, names };
  if (names.length > 1) return { key: `names:${JSON.stringify(names)}`, names };
  return { key: `hash:${row.pipeline}`, names: [] };
}

/**
 * Rows grouped by pipeline, as `groupOf` places each, each group in the order
 * its first row appears, each row in the order given. A group with no run of
 * its own — only prefix runs of a parent with no run here — is still shown,
 * so the prefix run is, and draws no shape.
 */
export function groupRows(rows: readonly RunRow[]): RunGroup[] {
  const ordered = new Map<string, { names: string[]; rows: RunRow[] }>();
  for (const row of rows) {
    const { key, names } = groupOf(row);
    const group = ordered.get(key) ?? { names, rows: [] };
    group.rows.push(row);
    ordered.set(key, group);
  }
  return [...ordered].map(([key, { names, rows: members }]) => {
    // The heading is the store's: a job knows the name it was launched as, but neither its canonical hash nor
    // whether the workspace holds that name now, so it heads a group only when no run of the store is in it.
    const stored = members.filter((r) => r.source.kind === 'run');
    const own = (stored.length === 0 ? members : stored).filter((r) => r.prefix === null);
    const head = own[0] ?? (members[0] as RunRow);
    // Every row's record comes from one listing, so the first that names the heading says it. A heading no record
    // names is hash matches, current documents, so held unless the listing says the API refuses it; one a record
    // names but says nothing of fails closed, unlinked.
    const recorded = (stored.length === 0 ? members : stored).find((r) => r.launchedAs !== null && r.launchedAs === names[0]);
    const held = names.map((name): NameHeld => {
      if (recorded !== undefined) return recorded.launchedHeld ?? 'unchecked';
      return members.some((r) => r.refusedNames.includes(name)) ? 'other_case' : 'exactly';
    });
    return { key, names, held, pipeline: head.pipeline, shapeKey: own.length === 0 || stored.length === 0 ? null : head.pipeline, rows: members };
  });
}

/**
 * The fact a row's group is not headed by, as its secondary label: under a
 * recorded name, the current documents holding the run's content, or that
 * none does — and nothing when the one document holding it is the recorded
 * name itself, which the heading already says; under the hash matches or
 * the hash, that no launch was recorded, or that one was, without a name.
 * Facts only: nothing here says a run is an earlier version of anything
 * (ADR-C39 § 7).
 */
export function otherFact(row: Pick<RunRow, 'launchedAs' | 'launchRecorded' | 'pipelineNames'>): string | null {
  if (row.launchedAs === null) return row.launchRecorded ? 'launch recorded without a name' : 'launch not recorded';
  if (row.pipelineNames.length === 1 && row.pipelineNames[0] === row.launchedAs) return null;
  return row.pipelineNames.length === 0 ? 'no current document has this content' : `content held by ${row.pipelineNames.join(', ')}`;
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

// Generated from runtime/ragondin-api/api/v1.json by `just gen-ui-types`
// (ui/scripts/gen-api-types.mjs). Do not edit: `npm run check` fails when this
// file is not what the description generates (ui/ARCHITECTURE.md § The
// generated types).

/** Which run of a comparison lacks a node a pair drawn by hand names. */
export type AbsentFrom = "baseline" | "run" | "both";

/** One benchmark the registry knows: named by the manifest, or imported. */
export type BenchmarkEntry = {
  /**
   * The format that reads it: `beir`, `beir-qa` or `squad`; `unknown` for
   * an import whose record cannot be read.
   */
  format: string;
  /**
   * The ground truth it carries, read off the loaded dataset; `null` when
   * nothing on disk loaded.
   */
  ground_truth: GroundTruth | null;
  /**
   * The dataset's licence, for a benchmark the manifest names, whatever
   * its state: a downloaded dataset keeps the notice it was obtained
   * under. `null` for an import, whose licence is its owner's.
   */
  licence: string | null;
  /** Where that licence is stated; `null` with it. */
  licence_url: string | null;
  /**
   * Its selector, `<format>/<dir>` — `beir/scifact` — as `ragondin bench
   * --benchmark` takes it; for an import whose record cannot be read, its
   * directory's name alone.
   */
  name: string;
  /** Where it stands against the digest expected of it. */
  state: BenchmarkState;
};

/** `GET /benchmarks`: every benchmark the registry knows. */
export type BenchmarkListing = {
  /** The manifest's entries in manifest order, then the imports by name. */
  benchmarks: BenchmarkEntry[];
};

/**
 * Where a benchmark stands. Every verdict is a statement about digests: the
 * dataset on disk is loaded and its `dataset_version` compared with the one
 * expected — the manifest's, or the one recorded at import.
 */
export type BenchmarkState = {
  /** What it digests to. */
  dataset_version: string;
  kind: "ready";
} | {
  kind: "available";
  /** The snapshot's size. */
  size_bytes: number;
} | {
  /** The digest expected. */
  expected: string;
  /** The digest on disk. */
  found: string;
  kind: "differs";
} | {
  /** The adapter's error. */
  error: string;
  kind: "unreadable";
} | {
  /** What it digests to. */
  dataset_version: string;
  kind: "local";
};

/**
 * What this build can run: the local implementations of each family, and
 * whether it can call a `Remote` one.
 */
export type Capabilities = {
  /** One entry per family, as the launcher lists them. */
  families: FamilyCapabilities[];
  /** Whether this build carries the `remote` feature. */
  remote: boolean;
};

/** The parameters one value of a [`ParameterChoice`]'s key adds. */
export type ChoiceCase = {
  /** The parameters it adds. */
  parameters: Parameter[];
  /** The value; `null` for any other non-empty one, a bound name. */
  value: string | null;
};

/**
 * `POST /compare`: the runs to compare, the baseline among them, and
 * optionally a manual pairing to keep before comparing.
 */
export type CompareRequest = {
  /** The run the others are compared against. */
  baseline: string;
  /**
   * A manual pairing between the baseline's workspace pipeline and
   * another compared run's: applied to this comparison, then kept —
   * replacing any — once the answer is built, so a refused request keeps
   * nothing. With no pairs, it is removed and the two pipelines pair
   * automatically again. Absent changes nothing.
   */
  pairing?: Pairing | null;
  /**
   * The runs, by id: two to five, each once, the baseline among them. The
   * answer puts the baseline first and the others in this order.
   */
  run_ids: string[];
};

/** One run of a comparison. */
export type ComparedRun = {
  /** The run's id. */
  id: string;
  /**
   * The workspace pipeline whose manual pairings apply to it (decided in #402): the
   * name its launch record gives when it has one, whether or not that
   * pipeline's content has changed since; otherwise the one pipeline
   * document whose canonical hash is the run's. `null` when it has no
   * record naming one and no document, or several, holds its hash; such a
   * run is paired automatically only. The lookup's answer, not a claim
   * about which version the run is: a run of earlier content gets the
   * pairs whose nodes it still has, and the rest are in
   * [`Comparison::unplaced_pairs`].
   */
  pipeline: string | null;
  /** The content hash of the canonical logical pipeline it ran. */
  pipeline_hash: string;
};

/**
 * `POST /compare`: runs of one benchmark compared against a baseline.
 *
 * Every list with one entry per run — a metric's values, a stage's cells —
 * is in the order of [`runs`](Self::runs): the baseline first, then the
 * other runs in the order the request gave them.
 */
export type Comparison = {
  /** The baseline's id. */
  baseline: string;
  /**
   * Why derived figures could not be cached under the workspace's
   * `cache/`, one entry per failure; empty otherwise. The response is
   * complete either way.
   */
  cache_errors: string[];
  /** The configuration parameters not identical across the runs. */
  configuration: ConfigurationMatrix;
  /**
   * Whether the benchmark on disk is the one the runs were evaluated on,
   * which the per-query deltas and the per-stage metrics are read
   * against. Without `verified`, `query_deltas` is empty and no stage
   * cell carries a metric; the table, the matrix, the stages and the
   * latency do not depend on it.
   */
  ground_truth: DatasetCheck;
  /** Each run's latency, node by node. */
  latency: RunLatency[];
  /** One row per metric any run recorded, in name order. */
  metrics: MetricRow[];
  /**
   * The manual pairings in use: one per run whose pipeline the workspace
   * holds a pairing for with the baseline's, oriented from the baseline's
   * pipeline.
   */
  pairings: Pairing[];
  /**
   * Each run other than the baseline: its per-query deltas against the
   * baseline, for every ranking metric both recorded.
   */
  query_deltas: RunDeltas[];
  /** The runs compared, the baseline first. */
  runs: ComparedRun[];
  /**
   * The stages the runs are aligned by, in pipeline order: those at least
   * one run has.
   */
  stages: StageRow[];
  /**
   * Every pair of [`pairings`](Self::pairings) a run could not place,
   * with the run and the side lacking its node, in the runs' order; empty
   * when every pair was placed.
   */
  unplaced_pairs: UnplacedPair[];
};

/** How sure an automatic pairing is. */
export type Confidence = "high" | "low";

/**
 * The configuration parameters not identical across the runs, or why that
 * could not be said.
 */
export type ConfigurationMatrix = {
  kind: "compared";
  /**
   * Every parameter — family and `impl:` name included — whose value
   * is not the same in every run, sorted by node and key.
   */
  parameters: ParameterRow[];
  /**
   * Every node some run lacks, sorted by id, with which runs hold
   * it: what lets a node only one pipeline has read as one fact
   * rather than as a row of unset parameters.
   */
  partial_nodes: PartialNode[];
  /**
   * Whether every canonical logical form hashes equal: runs that
   * differ only in their wiring have no row, and are still not one
   * configuration.
   */
  same_logical_form: boolean;
} | {
  kind: "unavailable";
  /** What the parser or the validation pass said. */
  reason: string;
  /** That run's id. */
  run: string;
};

/** What a node family's input edges carry. */
export type ConsumedPorts = {
  /** One kind per port, in port order. */
  kinds: EdgeKind[];
  shape: "fixed";
} | {
  /** The kind every port carries. */
  kind: EdgeKind;
  shape: "variadic";
};

/**
 * A run launched as the matrix's pipeline whose content has since changed:
 * stated as a fact, never guessed to be an earlier version (ADR-C39 § 7).
 */
export type ContentSinceChanged = {
  /**
   * Its parameter difference against the pipeline's current document, by
   * `POST /compare`'s configuration matrix, the current document's column
   * first.
   */
  difference: ConfigurationMatrix;
  /** How its record says it was launched. */
  launched: SinceChangedLaunch;
};

/**
 * Whether the dataset on disk is the one a run was evaluated on: the
 * benchmark pinned to the run's `dataset_version`, digesting to it — and,
 * for passage text, its derived chunk set digesting to the run's
 * `index_version`.
 */
export type DatasetCheck = {
  /**
   * The benchmark the run's `dataset_version` is pinned to, by its
   * selector; `null` when the registry pins no benchmark to it.
   */
  benchmark: string | null;
  /** The verdict in words: what was compared, and what differed. */
  detail: string;
  /** The digests the run recorded. */
  expected: DatasetVersions;
  /**
   * The digests of what is on disk, as far as they were computed; `null`
   * when nothing on disk loaded.
   */
  found: FoundVersions | null;
  /** The verdict. */
  status: DatasetStatus;
};

/** Where the run's dataset stands. */
export type DatasetStatus = "verified" | "dataset_absent" | "dataset_differs" | "dataset_unreadable" | "index_differs";

/** The two dataset digests of a run's identity. */
export type DatasetVersions = {
  /** The dataset's digest. */
  dataset_version: string;
  /** The derived chunk set's digest. */
  index_version: string;
};

/** One bin of the per-query histogram. */
export type DeltaBin = {
  /** Which bin. */
  bin: DeltaBinName;
  /** How many queries it holds. */
  count: number;
  /** Its lower bound; `null` for the lowest. */
  lower: number | null;
  /** Those queries, by id. */
  queries: string[];
  /** Its upper bound; `null` for the highest. */
  upper: number | null;
};

/**
 * The seven bins of a delta `d`: by its sign, and its magnitude against
 * 0.1 and 0.3. A bound belongs to the bin nearer zero.
 */
export type DeltaBinName = "much_worse" | "worse" | "slightly_worse" | "unchanged" | "slightly_better" | "better" | "much_better";

/** `POST /benchmarks/{name}/download`: the download job accepted. */
export type DownloadAccepted = {
  /** The job's id, under `/jobs/{id}`. */
  job_id: string;
};

/** The kind of value travelling along an edge. */
export type EdgeKind = "query" | "chunks" | "context" | "answer" | "opaque";

/** An edge, as a location names it. */
export type EdgeLocation = {
  /** The producing node or declared input. */
  from: string;
  /** The position of the edge among `to`'s inputs, from 0. */
  port: number;
  /** The consuming node. */
  to: string;
};

/** A run job that failed, as a matrix column names it. */
export type FailedAttempt = {
  /** The node that failed, when one did. */
  at_node: string | null;
  /** What failed. */
  error: string;
  /** The job's id, as `GET /jobs/{id}` serves it. */
  job: string;
};

/**
 * One family: its ports, the local implementations this build carries in
 * it with their parameters, those it does not carry and why, and what a
 * bound name takes.
 */
export type FamilyCapabilities = {
  /**
   * The parameters a node takes under a name bound in the family with
   * `--remote` or a service binding; empty for `embedder`.
   */
  bound: Parameter[];
  /**
   * The family, spelled as `--remote` and a service binding spell it: a
   * node family as a configuration's `component:` value, or `embedder`,
   * which no node is and a `dense` node names with `embedder:`.
   */
  family: string;
  /**
   * The names the binary gives a `Local` component of the family in some
   * build and not in this one, each with what a build needs to carry it.
   */
  not_carried: NotCarried[];
  /**
   * The names this build gives a `Local` component in it — `impl:`
   * values, or for `embedder`, `embedder:` values — each with the
   * parameters a node takes under it. An embedder takes none: its keys
   * are a `dense` node's, in that node's choice.
   */
  parameters: ImplementationParameters[];
  /**
   * The ports a node of the family declares, as the pipeline grammar
   * derives them from the family alone; `null` for `embedder`, which no
   * node is.
   */
  ports: FamilyPorts | null;
};

/**
 * The ports of a node family: the kind it puts on its output edge, and
 * the kinds its input edges carry, by position.
 */
export type FamilyPorts = {
  /** The kinds of value it consumes. */
  consumes: ConsumedPorts;
  /** The kind of value a node of the family produces. */
  produces: EdgeKind;
};

/**
 * A run that counts for the matrix's pipeline: one that fills a cell, or
 * one launched as it whose content has since changed.
 */
export type FeedingRun = {
  /** Every benchmark the registry pins to that digest, sorted. */
  benchmark_names: string[];
  /**
   * For a run launched as this pipeline whose content has since changed
   * (ADR-C39 § 7): how it was launched, and its parameter difference
   * against the pipeline's current document; `null` for a run that fills
   * a cell.
   */
  content_since_changed: ContentSinceChanged | null;
  /** The digest of the dataset it ran on. */
  dataset_version: string;
  /** Whether it is the run its column shows. */
  fills_column: boolean;
  /**
   * The run's launch record, as it was written once with the run; `null`
   * when it has none. ADR-C39 § 4's first fact: what the run was launched
   * as, never resolved with `pipeline_names` into one name.
   */
  launched_as: LaunchedAs | null;
  /**
   * Every workspace document whose canonical hash is the run's, sorted —
   * ADR-C39 § 4's content fact.
   */
  pipeline_names: string[];
  /**
   * For a prefix of the matrix's pipeline's current form — by its launch
   * record's parent hash, or by the structural test — the pipeline and the
   * node it stops at; `null` otherwise.
   */
  prefix_of: PrefixOf | null;
  /** The run's id. */
  run: string;
  /** When it started, from its own record; `null` when unknown. */
  started_at_ms: number | null;
};

/** The digests of the dataset on disk. */
export type FoundVersions = {
  /** What the dataset on disk digests to. */
  dataset_version: string;
  /**
   * What its derived chunk set digests to; `null` when it was not
   * compared — the dataset already differs, or the check is the ground
   * truth's, which depends on the dataset alone.
   */
  index_version: string | null;
};

/**
 * A pipeline as a graph: its declared inputs, its nodes and the edges
 * between them.
 */
export type Graph = {
  /**
   * One edge per entry of a node's `inputs`, grouped by consuming node in
   * node order, then in port order.
   */
  edges: GraphEdge[];
  /** The values the pipeline receives from its caller, in declared order. */
  inputs: GraphInput[];
  /** The nodes, sorted by id — the canonical order. */
  nodes: GraphNode[];
};

/** A data edge: `from`'s output feeds `to`'s input at `port`. */
export type GraphEdge = {
  /** The producing node, or a declared input. */
  from: string;
  /** The kind of value the producer puts on it. */
  kind: EdgeKind;
  /** The position of this edge among `to`'s inputs, from 0. */
  port: number;
  /** The consuming node. */
  to: string;
};

/** A declared pipeline input. */
export type GraphInput = {
  /** The id nodes name it by. */
  id: string;
  /** What it carries: the query, for every pipeline this build reads. */
  kind: EdgeKind;
};

/** A node of the graph. */
export type GraphNode = {
  /**
   * Its component family, spelled as a configuration's `component:`
   * value: `retriever`, `fusion`, `reranker`, `context_builder`,
   * `generator` or `extension`.
   */
  family: string;
  /** The node's id. */
  id: string;
  /** Its `impl:` name; an extension node's kind. */
  implementation: string;
  /** Its parameters, in key order. */
  parameters: Record<string, ParameterValue>;
};

/**
 * The ground truth a benchmark carries: which metric families a run over it
 * can compute.
 */
export type GroundTruth = "none" | "qrels" | "reference_answers" | "both";

/**
 * One implementation this build carries, and the parameters a node takes
 * under its name: the keys the executor reads for its family and the keys
 * its constructor reads, as the composition root declares them.
 */
export type ImplementationParameters = {
  /** A key whose value adds further parameters, if it has one. */
  choice: ParameterChoice | null;
  /** The `impl:` name. */
  name: string;
  /** Its parameters, whatever the value of `choice`'s key. */
  parameters: Parameter[];
};

/** `POST /benchmarks/import`: a corpus on disk, and the name to import it as. */
export type ImportRequest = {
  /** The local benchmark's name: one directory name. */
  name: string;
  /**
   * The directory holding the corpus and its ground truth, on the
   * server's disk.
   */
  path: string;
};

/**
 * One event of `GET /jobs/events`: its name, the SSE `event` field, and
 * its data, the SSE `data` field. The stream sends the two as SSE fields,
 * not as this object; the schema is the map from a name to its data's type.
 */
export type JobEvent = {
  data: JobSummary;
  event: "queued";
} | {
  data: JobSummary;
  event: "running";
} | {
  data: JobSummary;
  event: "done";
} | {
  data: JobSummary;
  event: "failed";
} | {
  data: JobSummary;
  event: "cancelled";
} | {
  data: JobSummary;
  event: "reordered";
} | {
  data: JobSummary;
  event: "fault";
} | {
  data: JobListing;
  event: "resync";
};

/**
 * A fault of the queue's record that belongs to no job: a job file the
 * queue could not read, or a directory or store it could not list.
 */
export type JobFault = {
  /** The file or directory. */
  path: string;
  /** What went wrong, and what the queue did about it. */
  reason: string;
};

/**
 * `GET /jobs`: every job, in its place, and every fault of the queue's
 * record.
 */
export type JobListing = {
  /**
   * The faults of the queue's record that belong to no job — a job file
   * that does not read, a directory that cannot be listed, a run store
   * that cannot be listed at start-up — reported, never repaired. A fault
   * beside a job is on the job, in [`JobSummary::faults`].
   */
  faults: JobFault[];
  /**
   * The jobs, by position: the order accepted and as reordered; each
   * lane's queued jobs in the order its worker takes them.
   */
  jobs: JobSummary[];
};

/**
 * Where a job stands. A time is in milliseconds since the epoch, `null`
 * when the clock read before it.
 */
export type JobStatus = {
  kind: "queued";
} | {
  /** Queries executed, or bytes received. */
  done: number;
  kind: "running";
  /**
   * A run's median query latency so far: the lower median of each
   * executed query's latency, the sum of its trace's durations — the
   * figure `GET /runs` lists once it is filed. `null` before the
   * first query, and for a download.
   */
  median_latency_nanos: number | null;
  /** When the worker took it. */
  started_at_ms: number | null;
  /**
   * Queries in the benchmark, or bytes in the snapshot; `null` until
   * the first tick.
   */
  total: number | null;
} | {
  /** When it finished. */
  finished_at_ms: number | null;
  /**
   * Both ids, when the run is filed under another than the one it
   * announced.
   */
  id_mismatch: RunIdMismatch | null;
  kind: "done";
  /**
   * The id the run is filed under — computed from what ran; `null`
   * for a download.
   */
  run_id: string | null;
} | {
  /** The node that failed, when one did. */
  at_node: string | null;
  /** What failed. */
  error: string;
  /** When it failed. */
  finished_at_ms: number | null;
  kind: "failed";
  /**
   * How many queries' traces a run kept when it stopped — every query
   * it executed, the one it failed on included — under
   * `jobs/<id>/partial/`, which `GET /jobs/{id}/queries` serves. `0`
   * for a download, a run that executed none, traces that could not
   * be written, and a job interrupted by a crash, which keeps none it
   * can vouch for.
   */
  partial_traces: number;
} | {
  /** When it was cancelled. */
  finished_at_ms: number | null;
  kind: "cancelled";
  /** How many queries' traces a run kept, as for `failed`. */
  partial_traces: number;
};

/** One job: what it does and where it stands. */
export type JobSummary = {
  /**
   * When it was accepted, in milliseconds since the epoch; `null` when
   * the clock read before it.
   */
  created_at_ms: number | null;
  /**
   * What went wrong beside it without stopping it — a layout not copied
   * at launch, latencies left out of its median, a write of its record
   * that failed — in the order reported. Written into the job's file with
   * it, so a restart reads them back; one that could not be written says
   * it is held in memory only, until a later write of the job carries it.
   */
  faults: ReportedFault[];
  /** Its id. */
  id: string;
  /** Its place: a lane's worker takes the queued job with the lowest. */
  position: number;
  /** Where it stands. */
  state: JobStatus;
  /** What it does. */
  work: JobWork;
};

/** What a job does. */
export type JobWork = {
  /** The benchmark. */
  benchmark: string;
  /** The `Remote` bindings in force at submission. */
  bindings: ServiceBinding[];
  kind: "run";
  /**
   * For a prefix run, the canonical hash of the parent document the
   * cut was made from at submission; `null` for a whole run. Beside
   * `up_to`, the prefix's provenance, never part of the run's
   * identity.
   */
  parent_pipeline_hash: string | null;
  /** The pipeline's name in the workspace. */
  pipeline: string;
  /** The run id announced at submission. */
  run_id: string;
  /**
   * The node a prefix run stops at; `null` for a whole run. The
   * pipeline is then the parent's name, and the job ran the parent's
   * document cut at this node.
   */
  up_to: string | null;
} | {
  /** The benchmark's selector. */
  benchmark: string;
  kind: "download";
};

/** A run's launch record (ADR-C39 § 1). */
export type LaunchedAs = {
  /**
   * Whether the workspace holds [`name`](Self::name) now; `null` when the
   * record names none, and only then. `GET /runs` and the pipeline matrix
   * read it from the same listing of `pipelines/` as their hash matches;
   * `GET /runs/{id}` lists no `pipelines/`, so it stays one run's read
   * that a broken document cannot fail, and sends `unchecked`. A fact
   * about the name, not the content: a document held under it may hold
   * other content since.
   */
  held: NameHeld | null;
  /**
   * The workspace pipeline name it was launched as — for a prefix run,
   * its parent's; `null` when the record names none.
   */
  name: string | null;
  /** For a prefix run, where it was cut from its parent; `null` otherwise. */
  prefix_of: LaunchedPrefix | null;
};

/** Where a launch record says a prefix run was cut from its parent. */
export type LaunchedPrefix = {
  /** The canonical hash of the parent's version it was cut from. */
  parent_pipeline_hash: string;
  /** The node it stops at. */
  up_to: string;
};

/**
 * Where the editor draws each node: UI metadata beside the document, never
 * in its hash. Also the body of `PUT /pipelines/{name}/layout`.
 */
export type Layout = {
  /** Each node's position, by node id. */
  nodes: Record<string, Position>;
  /** The layout format's version: `1`, the only one this build reads. */
  version: number;
};

/** Where in a pipeline a validation failure is. */
export type Location = {
  /** The edge concerned, when there is one. */
  edge: EdgeLocation | null;
  /** The node concerned, when there is one. */
  node: string | null;
};

/** One node on one benchmark: its figure, or why it has none. */
export type MatrixCell = {
  /**
   * The gain over the previous ranking stage, which is what says
   * where a node helps, or why there is none.
   */
  gain: MatrixGain;
  /**
   * How many judged queries a ranking node's means are over; `null`
   * for the generator, whose figures are the run's.
   */
  judged_queries: number | null;
  kind: "measured";
  /** The figures, by metric name. */
  metrics: Record<string, number>;
} | {
  kind: "no_qrels";
} | {
  kind: "no_reference_answers";
} | {
  /** The benchmark to launch. */
  benchmark: string;
  kind: "not_run_yet";
} | {
  kind: "prefix_stops";
  /** The node the prefix run stops at. */
  up_to: string;
} | {
  kind: "not_scored";
} | {
  kind: "unverified";
} | {
  kind: "no_figure";
} | {
  kind: "not_scorable";
} | {
  kind: "not_run_on_this_version";
  /** That run's id, listed among the feeding runs. */
  run: string;
};

/** One benchmark of the matrix, and the run that fills it. */
export type MatrixColumn = {
  /**
   * Every benchmark the registry pins to that digest, sorted; empty when
   * it pins none.
   */
  benchmark_names: string[];
  /** One cell per row, in the rows' order. */
  cells: MatrixCell[];
  /**
   * Whether the dataset on disk is the run's own, which the ranking
   * figures are read against; `null` when no run fills the column.
   */
  dataset_check: DatasetCheck | null;
  /** The digest of the benchmark's dataset, as runs over it record it. */
  dataset_version: string;
  /**
   * The most recent run job of the whole current form on this benchmark,
   * when it failed and no run of the whole current form measured the
   * benchmark; `null` otherwise — no attempt, or a later one that did not
   * fail.
   */
  failed_attempt: FailedAttempt | null;
  /**
   * The ground truth it carries: read off the dataset when it verified,
   * otherwise off the metrics the run recorded; for a benchmark no run of
   * the current content measured, read off its dataset only when the
   * pipeline does not end in an answer — to say whether it can be scored
   * there — and `null` otherwise.
   */
  ground_truth: GroundTruth | null;
  /**
   * The run that fills the column — the most recent run of the whole
   * current form on this benchmark, or, with none, the most recent prefix
   * of it; `null` when neither ran on it.
   */
  run: string | null;
  /** For a prefix run, the node it stops at; `null` otherwise. */
  up_to: string | null;
};

/** A matrix cell's gain over the previous ranking stage, or why it has none. */
export type MatrixGain = {
  kind: "over_previous_stage";
  /** The gains, by metric name. */
  values: Record<string, number>;
} | {
  kind: "first_stage";
} | {
  kind: "ambiguous";
} | {
  kind: "unstaged";
};

/** One node of the matrix's pipeline. */
export type MatrixRow = {
  /** Its component family, as a configuration's `component:` spells it. */
  family: string;
  /** The node's id. */
  node: string;
  /**
   * The kind of value it produces: `chunks` for a ranking node, `answer`
   * for a generator.
   */
  produces: EdgeKind;
};

/** One metric's per-query deltas, and their histogram. */
export type MetricDeltas = {
  /**
   * The seven bins, from the worst to the best: they partition the
   * queries with a delta.
   */
  bins: DeltaBin[];
  /**
   * Each such query's score in the run minus its score in the baseline,
   * by query id.
   */
  deltas: QueryDelta[];
  /**
   * How many queries have a delta: those judged, and scored at both
   * runs' outputs.
   */
  judged_queries: number;
  /** The metric's name. */
  metric: string;
};

/** Which way a metric improves. */
export type MetricDirection = "higher" | "lower";

/** Which ground truth a metric reads, as the listing names it. */
export type MetricFamily = "ranking" | "answers" | "unknown";

/** One metric across the runs compared. */
export type MetricRow = {
  /**
   * The runs holding the best value, by `direction`: every one on a tie;
   * none when `direction` is `null`.
   */
  best: string[];
  /**
   * Each run's value minus the baseline's; `null` where either did not
   * record it. The baseline's own is `0`.
   */
  deltas: (number | null)[];
  /**
   * Which way it improves, from `ragondin-metrics`' catalogue; `null`
   * for a name the catalogue does not know, which then has no best.
   */
  direction: MetricDirection | null;
  /** The metric's name. */
  name: string;
  /** Each run's value; `null` where the run did not record it. */
  values: (number | null)[];
};

/**
 * The nodes of one column no run measured, which a run of the whole
 * pipeline on its benchmark would.
 */
export type MissingCells = {
  /**
   * The benchmark to launch on: the first name pinned to the digest;
   * `null` when the registry pins none, and nothing can be launched.
   */
  benchmark: string | null;
  /** The digest of its dataset. */
  dataset_version: string;
  /** The nodes, in the rows' order. */
  nodes: string[];
};

/** Whether the workspace holds a recorded pipeline name now. */
export type NameHeld = "exactly" | "other_case" | "gone" | "unchecked";

/** One node's latency over a run's queries. */
export type NodeLatency = {
  /** Its component family, as a configuration's `component:` spells it. */
  family: string;
  /**
   * The median of its durations, in nanoseconds: the lower of the two
   * middle values over an even count, so it is a duration that occurred.
   */
  median_nanos: number;
  /** The node's id. */
  node: string;
  /** How many queries it ran for. */
  queries: number;
};

/** One node's ranking metrics over the run. */
export type NodeMetrics = {
  /**
   * How many queries the means are over: the judged queries for which the
   * node produced a ranking. A query without qrels is never in it, as it
   * is never in the harness's means; `0` when the ground truth is not
   * verified.
   */
  judged_queries: number;
  /**
   * The ranking metrics of its ranking, averaged over `judged_queries`;
   * `null` when that is none, or when the ground truth is not verified.
   */
  metrics: Record<string, number> | null;
  /** The node's id. */
  node: string;
  /**
   * Whether the node produced a ranking for at least one query. A context
   * builder, a generator and a node that failed on every query did not.
   */
  produces_ranking: boolean;
};

/**
 * Two nodes compared with each other: the second is shown at the first's
 * stage.
 */
export type NodePair = {
  /**
   * What the pair compares, replacing the stage's name in the row; absent
   * for none.
   */
  label?: string | null;
  /** A node of `pipeline`. */
  node: string;
  /** A node of `other`. */
  other: string;
};

/** A `Local` implementation this build does not carry. */
export type NotCarried = {
  /** Its name: an `impl:` value, or for `embedder`, an `embedder:` value. */
  name: string;
  /**
   * Why this build does not carry it, in words: the feature a build needs
   * to carry it.
   */
  reason: string;
};

/**
 * A manual pairing between two pipelines: a list of node pairs, kept under
 * `pipelines/<pipeline>.pairing/<other>.json` and read in both directions.
 * Also the body's `pairing` in `POST /compare`, which keeps it.
 */
export type Pairing = {
  /** The pipeline the pairs' `other` belongs to. */
  other: string;
  /**
   * The pairs. Empty in a request: "Reset to automatic", which removes
   * the pairing.
   */
  pairs: NodePair[];
  /** The pipeline the pairs' `node` belongs to. */
  pipeline: string;
};

/** Where a stage's pairing comes from. */
export type PairingSource = "automatic" | "manual";

/** One parameter a node takes. */
export type Parameter = {
  /** What it is for, in a sentence. */
  description: string;
  /** The kind of value it holds. */
  kind: ParameterKind;
  /** The key, as a configuration writes it. */
  name: string;
  /**
   * Whether a node must declare it: a node without it is refused before
   * it is stored or run.
   */
  required: boolean;
  /**
   * The value the editor writes under it when a node is placed, or
   * `null`. Only a required key with no component default has one, and
   * nothing applies it to a node that lacks the key: it is written as an
   * ordinary value, or not at all.
   */
  start: ParameterValue | null;
};

/** A key whose value adds parameters: a `dense` node's `embedder:`. */
export type ParameterChoice = {
  /**
   * What each value adds. The first case whose `value` is the key's
   * applies, else the case whose `value` is `null` when the key holds any
   * other, non-empty text.
   */
  cases: ChoiceCase[];
  /** The key. */
  key: string;
};

/** The kind of value a parameter holds. */
export type ParameterKind = "non_negative_integer" | "string" | "float";

/** A node's parameter, as a configuration spells it. */
export type ParameterName = {
  kind: "component";
} | {
  kind: "impl";
} | {
  kind: "param";
  /** The key. */
  name: string;
};

/** One parameter that is not the same in every run. */
export type ParameterRow = {
  /** Which of the node's parameters. */
  key: ParameterName;
  /** The node it belongs to. */
  node: string;
  /** Each run's value; `null` where its configuration does not set it. */
  values: (ParameterValue | null)[];
};

/**
 * A node parameter's value, tagged with its kind: the one parameter type of
 * the API, in a run's graph, a comparison's rows and a pipeline's typed
 * document alike (ADR-C40 § 2). `60` and `60.0` are two configurations
 * (ADR-C22), so an integer and a float of one value are never one value
 * here.
 *
 * An integer travels as decimal text, which the browser's number type
 * cannot round; a float travels as a JSON number and is finite. Read from a
 * request, each refuses what its kind does not carry: a number for an
 * integer, text for a float, an integer written `+1`, `01`, `-0` or wider
 * than 64 bits.
 */
export type ParameterValue = {
  kind: "string";
  value: string;
} | {
  kind: "int";
  value: string;
} | {
  kind: "float";
  value: number;
} | {
  kind: "bool";
  value: boolean;
} | {
  kind: "list";
  value: ParameterValue[];
};

/** A node not every run compared holds. */
export type PartialNode = {
  /** The node's id. */
  node: string;
  /** Whether each run holds it, in the order of the runs compared. */
  present: boolean[];
};

/**
 * `GET /jobs/{id}/queries`: the queries a failed or cancelled run job
 * executed before it stopped — the one a failed run stopped on included,
 * its failing node last — whose traces it kept under
 * `jobs/<id>/partial/` — never in the store, which holds a run complete or
 * not at all.
 *
 * Nothing here is scored, and no query or passage text is read: a run
 * records the digests of the dataset it was evaluated on, and the ground
 * truth and the text are read only against a dataset that digests to them
 * (ADR-C36 § 4); a job's partial traces record none.
 */
export type PartialQueries = {
  /**
   * The query whose trace holds a failed node — the one a failed run
   * stopped on, its failing node last; `null` when no kept trace does.
   */
  failed_query: string | null;
  /**
   * The graph lowered from the pipeline document the job snapshotted at
   * submission.
   */
  graph: Graph;
  /**
   * The job, as `GET /jobs/{id}` answers it: what was submitted, the run
   * id it announced — under which nothing is stored — and how it ended.
   */
  job: JobSummary;
  /**
   * The queries, by id, each with its latency; its `text` is `null` and
   * its `scores` empty.
   */
  queries: QueryScores[];
};

/**
 * `GET /jobs/{id}/trace/{query}`: one query's trace from a failed or
 * cancelled run job's partial traces, node by node, in the shape
 * `GET /runs/{id}/trace/{query}` serves a stored run's — each node's
 * `metrics` and `gold_ranks`, and each passage's `text` and `grade`,
 * `null`, for the reason [`PartialQueries`] gives.
 */
export type PartialTrace = {
  /** The job's id. */
  job: string;
  /** The nodes, in execution order. */
  nodes: TraceNodeView[];
  /** The query's id. */
  query: string;
};

/**
 * `GET /pipelines/{name}`: one pipeline document, verbatim, and as the
 * editor holds it when it can.
 */
export type PipelineDetail = {
  /**
   * Whether the text is, byte for byte, the server's own rendering of the
   * document it reads to — what a write from the editor would store.
   * `false` for a text a person wrote (a comment, another key order,
   * another formatting) and for one that does not read: the editor warns
   * before its first save replaces such a text (ADR-016 § 5).
   */
  canonical: boolean;
  /** The document, byte for byte as the file holds it. */
  document: string;
  /** Why it does not validate, when it does not. */
  error: PipelineError | null;
  /** The digest of those bytes; also the response's `ETag` header, quoted. */
  etag: string;
  /** The content hash of its canonical logical form, when it validates. */
  hash: string | null;
  /** Its name. */
  name: string;
  /**
   * The document as the editor holds it, whenever its text reads into the
   * wire schema, whether or not it validates; `null` when it does not
   * read, or holds a value the typed document cannot carry, such as a
   * non-finite float (ADR-C40 § 4).
   */
  typed: TypedDocument | null;
};

/**
 * `PUT /pipelines/{name}`: a pipeline document, as text or as the editor
 * holds it — one key, naming which (ADR-C40 § 5, § 6).
 */
export type PipelineDocument = {
  document: string;
} | {
  typed: TypedDocument;
};

/**
 * Why a pipeline document does not validate: `pipeline_invalid`'s detail and
 * location, inside a response that still answers.
 */
export type PipelineError = {
  /** What the validation pass said, in the words `ragondin validate` uses. */
  detail: string;
  /** The node and the edge it concerns, when they can be named. */
  location: Location;
};

/**
 * `GET /pipelines/{name}/layout`: the layout beside the document, if it has
 * one. Without one the UI lays the graph out itself, and says so.
 */
export type PipelineLayout = {
  /** The layout, or `null` when the pipeline has none. */
  layout: Layout | null;
};

/** `GET /pipelines`: every pipeline document in the workspace. */
export type PipelineListing = {
  /** One entry per document, by name. */
  pipelines: PipelineSummary[];
};

/**
 * `GET /pipelines/{name}/matrix`: one workspace pipeline's node × benchmark
 * matrix, over the runs of its current canonical form and of its prefixes
 * (ADR-C39 § 6). Derived from the stored runs on every request: it is no
 * object of its own.
 */
export type PipelineMatrix = {
  /**
   * Why derived figures could not be cached under the workspace's
   * `cache/`, one entry per failure; empty otherwise. The response is
   * complete either way.
   */
  cache_errors: string[];
  /**
   * One column per benchmark a feeding run ran on — and, with
   * `include_available`, per benchmark the registry knows that none did —
   * ordered by benchmark name.
   */
  columns: MatrixColumn[];
  /**
   * Every run that counts for this pipeline — of its current canonical
   * form, a prefix of it, or launched as it with content that has since
   * changed — the most recent first, each saying whether it fills its
   * column.
   */
  feeding_runs: FeedingRun[];
  /**
   * Per column, the nodes no run measured that a run of the whole
   * pipeline on that benchmark would: what a launch would fill. Never on a
   * benchmark where such a run exists.
   */
  missing: MissingCells[];
  /** The pipeline's name: its document under `pipelines/`. */
  pipeline: string;
  /**
   * The canonical hash of the document as it is now: the runs whose
   * pipeline hash is this one fill cells, under whatever name they ran.
   */
  pipeline_hash: string;
  /**
   * The pipeline's nodes in topological order, ties by id, so the matrix
   * reads like the pipeline. A judge row is reserved, and absent until the
   * judge exists.
   */
  rows: MatrixRow[];
  /**
   * The runs the store lists and cannot load, with its reason: neither
   * counted nor silently dropped, as `GET /runs` lists them.
   */
  unreadable: UnreadableRun[];
};

/** A pipeline, as the listing shows it. */
export type PipelineSummary = {
  /** Why it does not validate, when it does not. */
  error: PipelineError | null;
  /** The digest of its bytes, the value `If-Match` names to write it. */
  etag: string;
  /** The content hash of its canonical logical form, when it validates. */
  hash: string | null;
  /**
   * When its file was last modified, in milliseconds since the Unix
   * epoch; `null` for a time before the epoch, which is unknown, never
   * `0`.
   */
  modified_ms: number | null;
  /** Its name: the file stem. */
  name: string;
};

/**
 * `POST /pipelines/validate`: the document validates, and this is its hash
 * and its rendering.
 */
export type PipelineValidated = {
  /**
   * The content hash of the canonical logical form, as `ragondin validate`
   * prints it.
   */
  hash: string;
  /**
   * The document as the server renders it: for a typed document, the
   * bytes a write of it stores; for a text, the rendering of the document
   * it reads to, which keeps none of its comments or formatting. What the
   * editor exports. `null` only for a text whose document the renderer
   * cannot write so that it reads back.
   */
  rendering: string | null;
};

/** `PUT /pipelines/{name}`: what was written. */
export type PipelineWritten = {
  /** The etag of the bytes now stored; also the `ETag` header, quoted. */
  etag: string;
  /** The content hash of their canonical logical form. */
  hash: string;
  /** The pipeline's name. */
  name: string;
};

/** A node's position on the editor's canvas. */
export type Position = {
  /** Horizontal, in canvas units. */
  x: number;
  /** Vertical, in canvas units. */
  y: number;
};

/** A prefix run's place in its parent pipeline. */
export type PrefixOf = {
  /** The parent pipeline's name. */
  pipeline: string;
  /** The node the prefix stops at: its output. */
  up_to: string;
};

/**
 * `POST /services/{family}/{name}/probe`: what the identity read needs
 * besides the binding.
 */
export type ProbeRequest = {
  /**
   * The name the service serves the model under — a node's
   * `served_model`. An embedder, a reranker or a generator reports an
   * identity only for one; a context builder takes none. Absent is none.
   */
  served_model?: string | null;
};

/** `POST /services/{family}/{name}/probe`: the identity a run would record. */
export type ProbeResult = {
  /**
   * What the service reported, read as the composition root reads it
   * before a run.
   */
  identity: string;
};

/**
 * An error, as `application/problem+json` (RFC 9457) with this API's own
 * members: a stable `code`, a `hint` naming the action, and a `location` for
 * a validation failure.
 */
export type Problem = {
  /**
   * The stable code a client matches on: one of `ApiError::CODES`, which
   * the schema lists as an enum so a generated client can narrow on it.
   */
  code: "pipeline_invalid" | "impl_not_in_build" | "service_unreachable" | "run_exists" | "run_unreadable" | "run_not_found" | "query_not_found" | "parameter_invalid" | "dataset_absent" | "dataset_differs" | "benchmark_not_found" | "benchmark_exists" | "download_failed" | "download_cancelled" | "import_refused" | "pipeline_not_found" | "precondition_failed" | "binding_refused" | "service_not_found" | "request_invalid" | "backend_failed" | "host_refused" | "origin_refused" | "route_not_found" | "method_not_allowed" | "runs_not_comparable" | "body_too_large" | "job_not_found" | "job_not_queued" | "job_finished" | "job_not_ended" | "no_partial_traces" | "prefix_node_not_found" | "prefix_is_whole_pipeline" | "prefix_ends_in_context" | "prefix_not_scorable";
  /** What happened, in this occurrence's words. */
  detail: string;
  /**
   * The stored document's etag, for `precondition_failed` when one is
   * stored — the value the `ETag` header carries quoted, for a client
   * that reads the body alone. Present only then.
   */
  etag?: string | null;
  /** The action that would resolve it. */
  hint: string;
  /**
   * For `run_exists`, where what already holds the run id is read:
   * `/api/v1/jobs/<id>` for a job queued or running under it,
   * `/api/v1/runs/<id>` for a stored run. Present only then.
   */
  link?: string | null;
  /**
   * Where in a pipeline a validation failure is. Present only for
   * `pipeline_invalid`.
   */
  location?: Location | null;
  /**
   * The parameter, path parameter or header a `parameter_invalid` is
   * about, when it is known. Absent otherwise — never guessed.
   */
  name?: string | null;
  /** The HTTP status. */
  status: number;
  /** A short, fixed summary of the code. */
  title: string;
  /** `urn:ragondin:problem:<code>`. */
  type: string;
};

/** One query's delta. */
export type QueryDelta = {
  /** Its score in the run minus its score in the baseline. */
  delta: number;
  /** The query's id. */
  query: string;
};

/** One query, as the run executed it. */
export type QueryScores = {
  /**
   * Its latency, in nanoseconds: the sum of its nodes' durations — each
   * component's own time, as the trace records it. `null` when that sum
   * overflows, which only a malformed trace can make it do.
   */
  duration_nanos: number | null;
  /** The query's id. */
  id: string;
  /**
   * Its scores at the run's output, by metric name: the ranking metrics
   * when its qrels are non-empty, the answer metrics when it has a
   * reference. Empty when it is judged on neither, or when the ground
   * truth is not verified.
   */
  scores: Record<string, number>;
  /**
   * Its text, the dataset's: present when the ground truth is
   * `verified` and the dataset holds the query; `null` otherwise.
   */
  text: string | null;
};

/**
 * `GET /runs/{id}/trace/{query}`: one query's trace, node by node, in the
 * order the nodes ran, with each named chunk's passage text when the run's
 * own dataset is on disk.
 */
export type QueryTrace = {
  /** The nodes, in execution order. */
  nodes: TraceNodeView[];
  /**
   * Whether passage text could be resolved: only against the dataset the
   * run was evaluated on, digests compared (ADR-C36 § 4).
   */
  passages: DatasetCheck;
  /** The query's id. */
  query: string;
  /** The run's id. */
  run: string;
  /**
   * The query's scores at the run's output, as `GET /runs/{id}/queries`
   * reports them: present when the dataset on disk is the run's, whatever
   * the chunk set.
   */
  scores: Record<string, number>;
  /**
   * The query's text, the dataset's: present when the dataset on disk is
   * the run's, whatever the chunk set — the gate `scores` is under — and
   * it holds the query; `null` otherwise, and `passages` says why.
   */
  text: string | null;
};

/** `PATCH /jobs/{id}`: where to move a queued job. */
export type ReorderRequest = {
  /**
   * Its place among its lane's queued jobs, from 0 — the next taken. A
   * place past the last moves it last.
   */
  position: number;
};

/** A fault beside a job, which did not change its state. */
export type ReportedFault = {
  /**
   * When it was reported, in milliseconds since the epoch; `null` when
   * the clock read before it.
   */
  at_ms: number | null;
  /** What went wrong, and what the queue did about it. */
  reason: string;
};

/** `POST /runs`: the job accepted, and the run id it announced. */
export type RunAccepted = {
  /** The job's id, under `/jobs/{id}`. */
  job_id: string;
  /**
   * The run id the launcher announced: the one the run is filed under,
   * unless what ran differs from what was announced, which the job's
   * terminal state then reports.
   */
  run_id: string;
};

/** One run's per-query deltas against the baseline. */
export type RunDeltas = {
  /**
   * One entry per ranking metric both it and the baseline recorded, in
   * name order.
   */
  metrics: MetricDeltas[];
  /** The run's id. */
  run: string;
};

/** `GET /runs/{id}`: one run, whole. */
export type RunDetail = {
  /** The `Remote` bindings it used, outside its identity. */
  bindings: ServiceBinding[];
  /** The configuration document that produced it, verbatim. */
  configuration: string;
  /**
   * When its evaluation finished, in milliseconds since the Unix epoch,
   * outside its identity; `null` when unknown.
   */
  finished_at_ms: number | null;
  /**
   * The graph lowered from [`configuration`](Self::configuration) by the
   * pipeline grammar's one implementation — never by the browser.
   */
  graph: Graph;
  /** The run's content address. */
  id: string;
  /** The components its identity digests. */
  inputs: RunInputs;
  /**
   * The run's launch record, as [`RunSummary::launched_as`] serves it;
   * `null` for a run stored without one. A prefix run says what it was
   * cut from here, in the record's `prefix_of`: the parent's name, the
   * node it stops at and the parent's canonical hash. A prefix written by
   * hand, with no such record, shows only through `GET /runs`'
   * `prefix_of_documents`.
   */
  launched_as: LaunchedAs | null;
  /** What it scored, by metric name. */
  metrics: Record<string, number>;
  /**
   * When it started, in milliseconds since the Unix epoch, outside its
   * identity; `null` when unknown.
   */
  started_at_ms: number | null;
};

/** A run filed under another id than it announced: both. */
export type RunIdMismatch = {
  /** The id announced at submission. */
  announced: string;
  /** The id computed from what ran, which the run is filed under. */
  decided: string;
};

/** The components of a run's identity tuple. */
export type RunInputs = {
  /** The benchmark dataset's version. */
  dataset_version: string;
  /** The engine's version. */
  engine_version: string;
  /** The index's version. */
  index_version: string;
  /** The model hashes, by role. */
  model_hashes: Record<string, string>;
  /** The content hash of the canonical logical pipeline. */
  pipeline: string;
};

/** One run's latency, node by node. */
export type RunLatency = {
  /** Every node that ran, by id. */
  nodes: NodeLatency[];
  /** The run's id. */
  run: string;
};

/**
 * `GET /runs`: every run the store holds, and every one it holds but cannot
 * read.
 */
export type RunListing = {
  /**
   * Why a run's median query latency could not be read from or written
   * to the workspace's `cache/`, the first such reason; absent when the
   * cache served or took every run, so a listing whose cache works reads
   * as it always has. The listing is complete either way: the cache is
   * never a truth, so its failure fails nothing.
   */
  cache_error?: string;
  /** The readable runs, in the store's listing order. */
  runs: RunSummary[];
  /**
   * The shape of every pipeline a readable run ran, once per pipeline,
   * keyed by its canonical hash ([`RunSummary::pipeline`]): the graph
   * `GET /runs/{id}` serves for a run of it, by the same conversion. A
   * pipeline whose every run's stored document no longer lowers has no
   * entry, as `GET /runs/{id}` has no graph for it.
   */
  shapes: Record<string, Graph>;
  /**
   * The runs the store lists and cannot load — reported, never dropped
   * and never repaired.
   */
  unreadable: UnreadableRun[];
};

/**
 * `GET /runs/{id}/queries`: every query the run executed, its scores read
 * from the trace against the run's own ground truth, and the per-node
 * ranking metrics.
 *
 * Derived data: computed on read and cached under the workspace's `cache/`,
 * never stored in the run, and identical whether the cache was there or not.
 */
export type RunQueries = {
  /**
   * The node whose answer the answer metrics read, when the pipeline ends
   * in one.
   */
  answer_node: string | null;
  /**
   * Why the figures could not be cached under the workspace's `cache/`,
   * when they could not; `null` otherwise. The response is complete
   * either way: the cache is never a truth, so its failure fails nothing.
   */
  cache_error: string | null;
  /**
   * Whether the ground truth the scores are read against is the run's
   * own: the dataset on disk digests to the run's `dataset_version`. The
   * chunk set is not compared — a score depends on the qrels and the
   * reference answers, not on passage text. Scores and per-node metrics
   * are present only when it is `verified`.
   */
  ground_truth: DatasetCheck;
  /**
   * The metrics a query can be scored on: those the run recorded that are
   * read per query, by name.
   */
  metrics: string[];
  /**
   * Every node of the pipeline, in the canonical order, with its ranking
   * metrics averaged over the run's judged queries.
   */
  nodes: NodeMetrics[];
  /**
   * The queries, in the run's order (by id) — all of them, or those the
   * `missing_gold_at` filter kept.
   */
  queries: QueryScores[];
  /**
   * The node whose ranking the ranking metrics read (ADR-C30 § 3), when the
   * pipeline has one.
   */
  ranking_node: string | null;
  /** The run's id. */
  run: string;
};

/**
 * `POST /runs`: what to run. The bindings are not sent: the workspace's
 * bindings in force — those `GET /services` lists — are snapshotted into
 * the job at submission.
 */
export type RunRequest = {
  /** The benchmark's selector, `<format>/<name>`. */
  benchmark: string;
  /**
   * The workspace pipeline's name; its document is snapshotted into the
   * job at submission, so an edit afterwards changes nothing queued.
   */
  pipeline: string;
  /** The node a prefix run stops after; absent for the whole pipeline. */
  up_to?: string | null;
};

/** One run, as the listing shows it. */
export type RunSummary = {
  /**
   * Every registry entry pinned to [`dataset_version`](Self::dataset_version)
   * — a manifest entry or an import — sorted; empty when none is. The
   * pinning `GET /runs/{id}/queries` locates the run's dataset by, never
   * resolved by closeness (ADR-C36 § 4); naming a benchmark here loads and
   * verifies nothing.
   */
  benchmark_names: string[];
  /** The benchmark dataset's version. */
  dataset_version: string;
  /** The engine's version. */
  engine_version: string;
  /**
   * When the run's evaluation finished, in milliseconds since the Unix
   * epoch, as the process that ran it recorded; `null` when unknown.
   */
  finished_at_ms: number | null;
  /** The run's content address: 64 lowercase hex digits. */
  id: string;
  /** The index's version. */
  index_version: string;
  /**
   * The run's launch record, as it was written once with the run, read
   * and never computed or inferred; `null` for a run stored without one.
   * Recorded at launch, so its name may be a pipeline whose content has
   * since changed, or that no longer exists. ADR-C39 § 4's other fact,
   * never resolved with [`pipeline_names`](Self::pipeline_names) into one
   * name.
   */
  launched_as: LaunchedAs | null;
  /**
   * The lower median, over the run's queries, of each query's latency —
   * the sum of its trace's node durations — in nanoseconds.
   * Derived, never stored in the run: read from the traces alone, so a
   * run whose dataset is not on disk has it too, and cached under the
   * workspace's `cache/`. Not the run's wall time, which
   * `finished_at_ms − started_at_ms` gives, preparation included. `null`
   * when no trace of the run reads.
   */
  median_query_latency_nanos: number | null;
  /**
   * The family of each metric, keyed exactly like
   * [`metrics`](Self::metrics): from `ragondin-metrics`' catalogue, and
   * `unknown` for a name it does not know — kept, never dropped.
   */
  metric_families: Record<string, MetricFamily>;
  /** What the run scored, by metric name. */
  metrics: Record<string, number>;
  /** The content hash of the canonical logical pipeline it ran. */
  pipeline: string;
  /**
   * Every workspace pipeline document whose canonical hash is the run's,
   * sorted; empty when none is. The current hash match, found by content
   * when the listing is asked for, so a document edited since the run no
   * longer names it. Of the two facts ADR-C39 § 4 exposes about a run's
   * pipeline, this is the content one; it is not a resolution of
   * [`launched_as`](Self::launched_as), nor that of this.
   */
  pipeline_names: string[];
  /**
   * Every current workspace document the run's pipeline is a structural
   * prefix of — its declared inputs the same, each of its nodes one of the
   * document's, equal in canonical form, and fewer of them — each with the
   * node the run stops at, sorted by name; empty when it is a prefix of
   * none, or its stored document no longer lowers. A content fact like
   * [`pipeline_names`](Self::pipeline_names), asked of every run whatever
   * [`launched_as`](Self::launched_as) records (ADR-C39 § 5), so a prefix
   * written by hand and run from the command line is one too; the pipeline
   * matrix counts a prefix by the same test.
   */
  prefix_of_documents: PrefixOf[];
  /**
   * The names among [`pipeline_names`](Self::pipeline_names) the backend
   * refuses to read, sorted; empty when it refuses none. A name is refused
   * when another stored name is a case alias of it — the same name in
   * another ASCII case, `request_invalid` on a read — as two documents can
   * be on a filesystem that keeps case; each of the two is then refused.
   * Read from the same listing of `pipelines/` as `pipeline_names`, by the
   * rule [`NameHeld::OtherCase`] reads a recorded name by.
   */
  refused_pipeline_names: string[];
  /**
   * When the run started, in milliseconds since the Unix epoch, as the
   * process that ran it recorded; `null` when unknown.
   */
  started_at_ms: number | null;
};

/** `PUT /services/{family}/{name}`: the address to bind the name to. */
export type ServiceAddress = {
  /**
   * The service's address — the scheme `http`, a host and an optional
   * port, nothing else — as `ragondin bench --remote` takes it.
   */
  uri: string;
};

/**
 * A `Remote` component bound by family and name to the address it answers
 * at — in the workspace's settings, and as a run recorded it.
 */
export type ServiceBinding = {
  /** The family the name is bound in: `generator`, `embedder`, …. */
  family: string;
  /** The implementation name a node uses. */
  name: string;
  /** The service's address, as written. */
  uri: string;
};

/**
 * `GET /services`, and the answer of every write to a service: the bindings
 * `workspace.toml` holds.
 */
export type ServiceListing = {
  /** Every binding, in the file's order. */
  services: ServiceStatus[];
};

/** One binding, and what this server last learnt by probing it. */
export type ServiceStatus = {
  /**
   * Whether this server's last probe of it, at this address, read an
   * identity. `false` before any probe.
   */
  connected: boolean;
  /** The family the name is bound in. */
  family: string;
  /** The identity last read at this address, if any was. */
  identity: string | null;
  /** The implementation name a node uses. */
  name: string;
  /** The service's address, as written. */
  uri: string;
};

/** The workspace's settings: deployment data, never hashed into a run. */
export type SettingsSummary = {
  /** The directory benchmarks are read from. */
  datasets: string;
  /** The `Remote` bindings the workspace names. */
  services: ServiceBinding[];
};

/** How a run of since-changed content was launched. */
export type SinceChangedLaunch = "as_pipeline" | "as_prefix";

/** One run at one stage. */
export type StageCell = {
  kind: "absent";
} | {
  /**
   * Per metric, the best value among the nodes and the node it is
   * from — for the legs, the best leg. Empty without a verified
   * ground truth.
   */
  best: Record<string, StageValue>;
  kind: "present";
  /**
   * Each node, in the pipeline's canonical order — several only for
   * the retrieval legs, or where a pair drawn by hand joined one.
   */
  nodes: StageNode[];
};

/** A stage of a pipeline, in the order a ranking travels through them. */
export type StageName = "retrieval_legs" | "after_fusion" | "after_rerank" | "final_ranking" | "answer";

/** A node at a stage. */
export type StageNode = {
  /**
   * Its ranking metrics averaged over the judged queries, as
   * `GET /runs/{id}/queries` reports them; `null` without a verified
   * ground truth or when it ranked no judged query.
   */
  metrics: Record<string, number> | null;
  /** The node's id. */
  node: string;
  /** Whether a pair drawn by hand placed it here. */
  paired_by_hand: boolean;
};

/** One stage across the runs compared. */
export type StageRow = {
  /** Each run's cell. */
  cells: StageCell[];
  /**
   * `low` when a pipeline's graph was ambiguous at this stage — two
   * fusions, two rerankers, a reranker upstream of the fusion — and the
   * automatic pairing is a guess.
   */
  confidence: Confidence;
  /**
   * What the row compares, in the words of the pair drawn by hand into
   * it; `null` when no pair named it.
   */
  label: string | null;
  /** `manual` when a pair drawn by hand placed a node in it. */
  source: PairingSource;
  /** Which stage. */
  stage: StageName;
};

/** A metric's value at a stage, and the node it is read at. */
export type StageValue = {
  /** The node. */
  node: string;
  /** Its value. */
  value: number;
};

/** One node of a query's trace. */
export type TraceNodeView = {
  /** How long its component took, in nanoseconds. */
  duration_nanos: number;
  /** The failure it reported; `null` when it succeeded. */
  error: string | null;
  /**
   * The 1-based ranks of the gold documents — graded above 0 — in its
   * ranking, counted over documents folded from its chunks by first
   * occurrence, the ranking its `metrics` score: present when it produced
   * a ranking, the query is judged and the dataset on disk is the run's;
   * empty when it ranked no gold document; `null` otherwise.
   */
  gold_ranks: number[] | null;
  /** What it received, one summary per input port, in port order. */
  inputs: TraceValue[];
  /**
   * The ranking metrics of what it produced for this query: present when
   * it produced a ranking, the query is judged and the dataset on disk is
   * the run's; `null` otherwise.
   */
  metrics: Record<string, number> | null;
  /** The node's id. */
  node: string;
  /** What it produced; `null` when it failed. */
  output: TraceValue | null;
};

/** One named chunk, with its passage text when it could be resolved. */
export type TracePassage = {
  /** The chunk's id. */
  chunk: string;
  /** The document it was derived from. */
  document: string;
  /**
   * Its document's grade in the query's qrels, `0` when they do not judge
   * it: present when the query is judged and the dataset on disk is the
   * run's, whatever the chunk set; `null` otherwise.
   */
  grade: number | null;
  /** The score the node gave it, on the node's own scale. */
  score: number;
  /**
   * Its text: present only when the passages are `verified` and the chunk
   * set derived from the dataset holds this id; `null` otherwise.
   */
  text: string | null;
};

/**
 * A value along an edge, as the trace records it: sized on an input port,
 * named where a node produced it.
 */
export type TraceValue = {
  /** The query's id. */
  id: string;
  kind: "query";
} | {
  /** How many chunks it held. */
  count: number;
  kind: "chunk_count";
} | {
  /** The chunks, in the order the node returned them. */
  chunks: TracePassage[];
  kind: "ranking";
} | {
  /** How many chunks it held. */
  count: number;
  kind: "context_size";
  /** The length of its text, in bytes of UTF-8. */
  text_bytes: number;
} | {
  /** Its chunks, in the order the builder placed them. */
  chunks: TracePassage[];
  kind: "context";
  /** Its rendered text, whole, as the trace records it. */
  text: string;
} | {
  kind: "answer_size";
  /** The length of its text, in bytes of UTF-8. */
  text_bytes: number;
} | {
  kind: "answer";
  /** Its text, as the generator returned it. */
  text: string;
};

/**
 * A pipeline document as the editor holds it (ADR-C40 § 1): the wire
 * schema's shape — an optional schema version, the declared inputs in
 * order, the nodes in order — with every parameter value tagged with its
 * kind. The API's own type, converted by hand to and from the wire schema
 * in `convert.rs`, never derived from it: the configuration format on disk
 * is unchanged, and the browser never reads or writes it.
 */
export type TypedDocument = {
  /** The pipeline itself. */
  pipeline: TypedGraph;
  /**
   * The schema version the document is written in. Absent, the version
   * this build reads.
   */
  version?: number | null;
};

/**
 * The graph of a `TypedDocument`, as a configuration nests it under
 * `pipeline:`.
 */
export type TypedGraph = {
  /**
   * The ids of the values the pipeline receives from its caller, in
   * declared order.
   */
  inputs: string[];
  /** The nodes, in the order the document lists them. */
  nodes: TypedNode[];
};

/** One node of a `TypedDocument`, keyed as a configuration writes it. */
export type TypedNode = {
  /**
   * Its family, a configuration's `component:` value; not yet known to
   * name one.
   */
  component: string;
  /** The node's id; not yet known to be unique. */
  id: string;
  /** Its `impl:` value. */
  impl: string;
  /** The ids it consumes, in port order; not yet known to exist. */
  inputs: string[];
  /** Its parameters, in key order. */
  params: Record<string, ParameterValue>;
};

/**
 * A pair drawn by hand that a comparison could not apply to one of its
 * runs: a node it names is not a retriever, fusion or reranker of the run
 * it would be read in. A pairing is kept against two pipelines' current
 * documents, and applies to a run launched under a pipeline's name whose
 * content has since changed for the nodes that run still has (decided in #402); every
 * other pair is reported here, never skipped and never guessed onto
 * another node.
 */
export type UnplacedPair = {
  /** Which run lacks its node. */
  absent_from: AbsentFrom;
  /**
   * The pair, oriented from the baseline's pipeline as
   * [`Comparison::pairings`] holds it.
   */
  pair: NodePair;
  /**
   * The compared run, other than the baseline, the pair was not applied
   * to.
   */
  run: string;
};

/** A run the store lists but cannot load, and why. */
export type UnreadableRun = {
  /** The id the store lists it under. */
  id: string;
  /** What loading it reported. */
  reason: string;
};

/**
 * `POST /pipelines/validate`: a pipeline document, as text or as the editor
 * holds it — one key, naming which (ADR-C40 § 5, § 6).
 */
export type ValidationRequest = {
  document: string;
} | {
  typed: TypedDocument;
};

/**
 * `GET /workspace`: where the server works, how it is set up, which build is
 * answering and what that build can run.
 */
export type Workspace = {
  /**
   * The build's identity. The UI compares it with its own and reloads when
   * they differ (ADR-C36 § 1); the same value is on every response in the
   * `x-ragondin-build` header.
   */
  build: string;
  /** What this build can run, as the launcher reports it. */
  capabilities: Capabilities;
  /** What the workspace holds, counted on this request. */
  counts: WorkspaceCounts;
  /** The workspace's root directory, as the binary resolved it. */
  path: string;
  /** The deployment settings the workspace holds. */
  settings: SettingsSummary;
};

/** What a workspace holds, counted when asked: nothing here is cached. */
export type WorkspaceCounts = {
  /**
   * The benchmarks on disk whose digest is the one expected of them:
   * `ready` or `local`.
   */
  benchmarks_ready: number;
  /** The pipeline documents under `pipelines/`, valid or not. */
  pipelines: number;
  /** The runs the store lists, readable or not. */
  runs: number;
  /**
   * The bound services whose last probe, by this server, at their current
   * address, read an identity.
   */
  services_connected: number;
};

/** Every path under the API's base address: per method, its path parameters, its request body and its success response. */
export type Paths = {
  "/benchmarks": {
    /** Every benchmark the registry knows, with its state and licence. */
    get: {
      params: Record<string, never>;
      response: BenchmarkListing;
    };
  };
  "/benchmarks/import": {
    /** Imports a corpus on the server's disk as a local benchmark. */
    post: {
      params: Record<string, never>;
      body: ImportRequest;
      response: BenchmarkEntry;
    };
  };
  "/benchmarks/{name}/download": {
    /** Queues a download of a benchmark the manifest names, verified against its digests. */
    post: {
      params: {
        name: string;
      };
      response: DownloadAccepted;
    };
  };
  "/compare": {
    /** Runs of one benchmark against a baseline: the metric table, the parameter matrix, the stages with their pairing, the per-query deltas and their bins, and the latency per node. */
    post: {
      params: Record<string, never>;
      body: CompareRequest;
      response: Comparison;
    };
  };
  "/jobs": {
    /** Every job, by position, and every fault of the queue's record. */
    get: {
      params: Record<string, never>;
      response: JobListing;
    };
  };
  "/jobs/events": {
    /** Every job transition and progress tick, as server-sent events. */
    get: {
      params: Record<string, never>;
      headers: {
        /**
         * The id of the last event the client received, to resume after it.
         * Absent, or one this server no longer holds the events after, and the
         * stream begins with `resync`.
         */
        "Last-Event-ID"?: string;
      };
      response: never;
    };
  };
  "/jobs/{id}": {
    /** Cancels a job: a queued one at once, never executed; the running one between two queries. */
    delete: {
      params: {
        id: string;
      };
      response: JobSummary;
    };
    /** One job: what it does and where it stands. */
    get: {
      params: {
        id: string;
      };
      response: JobSummary;
    };
    /** Moves a queued job among its lane's queued jobs, and answers the queue in its new order. */
    patch: {
      params: {
        id: string;
      };
      body: ReorderRequest;
      response: JobListing;
    };
  };
  "/jobs/{id}/queries": {
    /** A failed or cancelled run job's partial traces: the queries it executed before it stopped, the one a failed run stopped on included, and the graph of the pipeline it snapshotted. */
    get: {
      params: {
        id: string;
      };
      response: PartialQueries;
    };
  };
  "/jobs/{id}/trace/{query}": {
    /** One query's trace from a failed or cancelled run job's partial traces, node by node, in the shape a stored run's trace is served in. */
    get: {
      params: {
        id: string;
        query: string;
      };
      response: PartialTrace;
    };
  };
  "/pipelines": {
    /** Every pipeline document: its etag, its hash or why it does not validate. */
    get: {
      params: Record<string, never>;
      response: PipelineListing;
    };
  };
  "/pipelines/validate": {
    /** The canonical hash `ragondin validate` prints for a document, or `pipeline_invalid`, located. */
    post: {
      params: Record<string, never>;
      body: ValidationRequest;
      response: PipelineValidated;
    };
  };
  "/pipelines/{name}": {
    /** One pipeline document, verbatim, with its etag, its hash or why it does not validate, its typed document when it reads, and whether its text is the server's own rendering. */
    get: {
      params: {
        name: string;
      };
      response: PipelineDetail;
    };
    /** Stores a document when it validates and its precondition holds: a text byte for byte, a typed document as the server renders it. */
    put: {
      params: {
        name: string;
      };
      headers: {
        /**
         * `"<etag>"` to replace the stored document with that etag — a weak
         * `W/` tag reads as its strong form — or `*` to replace whatever is
         * stored.
         */
        "If-Match"?: string;
        /** `*`, to create the document: nothing may be stored under the name. */
        "If-None-Match"?: string;
      };
      body: PipelineDocument;
      response: PipelineWritten;
    };
  };
  "/pipelines/{name}/layout": {
    /** The layout beside a pipeline document, or `null`. */
    get: {
      params: {
        name: string;
      };
      response: PipelineLayout;
    };
    /** Replaces the layout beside a pipeline document; never changes its etag or hash. */
    put: {
      params: {
        name: string;
      };
      body: Layout;
      response: PipelineLayout;
    };
  };
  "/pipelines/{name}/matrix": {
    /** A pipeline's node × benchmark matrix over its runs: per benchmark the most recent run of its current form or of a prefix of it, each node's figure with its gain over the previous stage, or why the cell is empty. */
    get: {
      params: {
        name: string;
      };
      query: {
        /**
         * Also give a column to every benchmark the registry knows that no
         * counted run ran on, each of its cells not_run_yet. Absent is false.
         */
        include_available?: boolean;
      };
      response: PipelineMatrix;
    };
  };
  "/runs": {
    /** Every run the store holds, and every one it cannot read. */
    get: {
      params: Record<string, never>;
      response: RunListing;
    };
    /** Queues a run of a workspace pipeline on a benchmark, under the run id the launcher announces for it. */
    post: {
      params: Record<string, never>;
      body: RunRequest;
      response: RunAccepted;
    };
  };
  "/runs/{id}": {
    /** One run: its inputs, metrics, configuration, bindings and lowered graph. */
    get: {
      params: {
        id: string;
      };
      response: RunDetail;
    };
  };
  "/runs/{id}/layout": {
    /** The layout copied at launch for a run's pipeline, `layouts/<hash>.json` by its canonical hash, or `null`: what a fork from the run copies beside its new document. */
    get: {
      params: {
        id: string;
      };
      response: PipelineLayout;
    };
  };
  "/runs/{id}/queries": {
    /** A run's queries with their scores read from the trace, and its per-node ranking metrics. */
    get: {
      params: {
        id: string;
      };
      query: {
        /**
         * Keep only the judged queries with no gold document (grade above 0) in
         * the top k of the output ranking. It needs the run's own dataset, and
         * answers dataset_absent or dataset_differs without it.
         */
        missing_gold_at?: number;
      };
      response: RunQueries;
    };
  };
  "/runs/{id}/trace/{query}": {
    /** One query's trace, node by node, with passage text when the run's own dataset is on disk. */
    get: {
      params: {
        id: string;
        query: string;
      };
      response: QueryTrace;
    };
  };
  "/services": {
    /** The `Remote` bindings the workspace holds, and what their last probe read. */
    get: {
      params: Record<string, never>;
      response: ServiceListing;
    };
  };
  "/services/{family}/{name}": {
    /** Unbinds a name. */
    delete: {
      params: {
        family: string;
        name: string;
      };
      response: ServiceListing;
    };
    /** Binds a name to an address, refused in the words `ragondin bench --remote` uses. */
    put: {
      params: {
        family: string;
        name: string;
      };
      body: ServiceAddress;
      response: ServiceListing;
    };
  };
  "/services/{family}/{name}/probe": {
    /** Reads a bound service's identity as a run would, or `service_unreachable`. */
    post: {
      params: {
        family: string;
        name: string;
      };
      body: ProbeRequest;
      response: ProbeResult;
    };
  };
  "/workspace": {
    /** The workspace: its path, its settings, this build, its capabilities and its counts. */
    get: {
      params: Record<string, never>;
      response: Workspace;
    };
  };
};

/** The operations whose success response has no body: the client accepts an empty answer from these, and from a 204. */
export const EMPTY_ANSWERS: readonly string[] = [];

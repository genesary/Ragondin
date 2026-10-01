// Generated from runtime/ragondin-api/api/v1.json by `just gen-ui-types`
// (ui/scripts/gen-api-types.mjs). Do not edit: `npm run check` fails when this
// file is not what the description generates (ui/ARCHITECTURE.md § The
// generated types).

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

/** One family's local implementations in this build. */
export type FamilyCapabilities = {
  /** The family, spelled as a configuration's `component:` value. */
  family: string;
  /** The `impl:` names this build registers in it. */
  local: string[];
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

/** Where in a pipeline a validation failure is. */
export type Location = {
  /** The edge concerned, when there is one. */
  edge: EdgeLocation | null;
  /** The node concerned, when there is one. */
  node: string | null;
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

/** A node parameter's value. */
export type ParameterValue = boolean | number | string | ParameterValue[];

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
  code: "pipeline_invalid" | "impl_not_in_build" | "service_unreachable" | "run_exists" | "run_unreadable" | "run_not_found" | "query_not_found" | "parameter_invalid" | "dataset_absent" | "dataset_differs" | "benchmark_not_found" | "benchmark_exists" | "download_failed" | "download_cancelled" | "import_refused" | "backend_failed" | "host_refused" | "origin_refused" | "route_not_found" | "method_not_allowed";
  /** What happened, in this occurrence's words. */
  detail: string;
  /** The action that would resolve it. */
  hint: string;
  /**
   * Where in a pipeline a validation failure is. Present only for
   * `pipeline_invalid`.
   */
  location?: Location | null;
  /** The HTTP status. */
  status: number;
  /** A short, fixed summary of the code. */
  title: string;
  /** `urn:ragondin:problem:<code>`. */
  type: string;
};

/** One query, as the run executed it. */
export type QueryScores = {
  /**
   * The sum of its nodes' durations, in nanoseconds: each component's own
   * time, as the trace records it.
   */
  duration_nanos: number;
  /** The query's id. */
  id: string;
  /**
   * Its scores at the run's output, by metric name: the ranking metrics
   * when its qrels are non-empty, the answer metrics when it has a
   * reference. Empty when it is judged on neither, or when the ground
   * truth is not verified.
   */
  scores: Record<string, number>;
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
};

/** `GET /runs/{id}`: one run, whole. */
export type RunDetail = {
  /** The `Remote` bindings it used, outside its identity. */
  bindings: ServiceBinding[];
  /** The configuration document that produced it, verbatim. */
  configuration: string;
  /**
   * The graph lowered from [`configuration`](Self::configuration) by the
   * pipeline grammar's one implementation — never by the browser.
   */
  graph: Graph;
  /** The run's content address. */
  id: string;
  /** The components its identity digests. */
  inputs: RunInputs;
  /** What it scored, by metric name. */
  metrics: Record<string, number>;
  /**
   * The run this one is a prefix of. Always absent today: no run is
   * recorded as a prefix yet.
   */
  prefix_of: string | null;
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

/**
 * `GET /runs`: every run the store holds, and every one it holds but cannot
 * read.
 */
export type RunListing = {
  /** The readable runs, in the store's listing order. */
  runs: RunSummary[];
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

/** One run, as the listing shows it. */
export type RunSummary = {
  /** The benchmark dataset's version. */
  dataset_version: string;
  /** The engine's version. */
  engine_version: string;
  /** The run's content address: 64 lowercase hex digits. */
  id: string;
  /** The index's version. */
  index_version: string;
  /** What the run scored, by metric name. */
  metrics: Record<string, number>;
  /** The content hash of the canonical logical pipeline it ran. */
  pipeline: string;
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

/** The workspace's settings: deployment data, never hashed into a run. */
export type SettingsSummary = {
  /** The directory benchmarks are read from. */
  datasets: string;
  /** The `Remote` bindings the workspace names. */
  services: ServiceBinding[];
};

/** One node of a query's trace. */
export type TraceNodeView = {
  /** How long its component took, in nanoseconds. */
  duration_nanos: number;
  /** The failure it reported; `null` when it succeeded. */
  error: string | null;
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

/** A run the store lists but cannot load, and why. */
export type UnreadableRun = {
  /** The id the store lists it under. */
  id: string;
  /** What loading it reported. */
  reason: string;
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
  /** The workspace directory, as the binary was given it. */
  path: string;
  /** The deployment settings the workspace holds. */
  settings: SettingsSummary;
};

/** Every path under the API's base address: per method, its path parameters, its request body and its success response. */
export type Paths = {
  "/runs": {
    /** Every run the store holds, and every one it cannot read. */
    get: {
      params: Record<string, never>;
      response: RunListing;
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
  "/runs/{id}/queries": {
    /** A run's queries with their scores read from the trace, and its per-node ranking metrics. */
    get: {
      params: {
        id: string;
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
  "/workspace": {
    /** The workspace: its path, its settings, this build and its capabilities. */
    get: {
      params: Record<string, never>;
      response: Workspace;
    };
  };
};

/** The operations whose success response has no body: the client accepts an empty answer from these, and from a 204. */
export const EMPTY_ANSWERS: readonly string[] = [];

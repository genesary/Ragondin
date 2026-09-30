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
  code: "pipeline_invalid" | "impl_not_in_build" | "service_unreachable" | "run_exists" | "run_unreadable" | "run_not_found" | "dataset_absent" | "dataset_differs" | "benchmark_not_found" | "benchmark_exists" | "download_failed" | "download_cancelled" | "import_refused" | "backend_failed" | "host_refused" | "origin_refused" | "route_not_found" | "method_not_allowed";
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

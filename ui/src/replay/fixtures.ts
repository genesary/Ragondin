// The runs, graphs, query listings and traces the Replay screen's tests read,
// shaped as runtime/ragondin-api builds them (ARCHITECTURE.md there, § Derived
// data): a hybrid run with a reranker, a context builder and a generator; a
// dense-only run on the same benchmark; the hybrid failing at its reranker;
// and a run on another benchmark. Test data only; no application module
// imports it.
import type { DatasetCheck, Graph, QueryTrace, RunDetail, RunListing, RunQueries, RunSummary, TraceNodeView, TracePassage } from '../api/types.ts';

const hex = (c: string) => c.repeat(64);
export const HYBRID = hex('b');
export const DENSE = hex('d');
export const FAILED = hex('f');
export const ELSEWHERE = hex('9');
export const SCIFACT = hex('5');
const NFCORPUS = hex('6');
const INDEX = hex('7');

const edge = (from: string, to: string, port: number, kind: Graph['edges'][number]['kind']) => ({ from, to, port, kind });

/** `hybrid-rerank-gen`, lowered: nodes by id, edges by consuming node, then port. */
export const HYBRID_GRAPH: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [
    { id: 'answer', family: 'generator', implementation: 'answerer', parameters: { temperature: 0 } },
    { id: 'bm25', family: 'retriever', implementation: 'bm25', parameters: { top_k: 5 } },
    { id: 'context', family: 'context_builder', implementation: 'concat', parameters: { max_chunks: 3 } },
    { id: 'dense', family: 'retriever', implementation: 'dense', parameters: { top_k: 5 } },
    { id: 'rerank', family: 'reranker', implementation: 'cross_encoder', parameters: { top_k: 4 } },
    { id: 'rrf', family: 'fusion', implementation: 'rrf', parameters: { k: 60 } },
  ],
  edges: [
    edge('question', 'answer', 0, 'query'),
    edge('context', 'answer', 1, 'context'),
    edge('question', 'bm25', 0, 'query'),
    edge('question', 'context', 0, 'query'),
    edge('rerank', 'context', 1, 'chunks'),
    edge('question', 'dense', 0, 'query'),
    edge('question', 'rerank', 0, 'query'),
    edge('rrf', 'rerank', 1, 'chunks'),
    edge('bm25', 'rrf', 0, 'chunks'),
    edge('dense', 'rrf', 1, 'chunks'),
  ],
};

/** `dense-only`: one retriever. */
export const DENSE_GRAPH: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [{ id: 'dense', family: 'retriever', implementation: 'dense', parameters: { top_k: 5 } }],
  edges: [edge('question', 'dense', 0, 'query')],
};

/** Gold for q1: documents d2 (grade 2) and d5 (grade 1). */
const GRADE: Record<string, number> = { d2: 2, d5: 1 };
const passage = (n: number, score: number): TracePassage => ({ chunk: `c${n}`, document: `d${n}`, grade: GRADE[`d${n}`] ?? 0, score, text: `Passage ${n} of the corpus.` });
const ranking = (...ns: number[]) => ({ kind: 'ranking' as const, chunks: ns.map((n, i) => passage(n, 1 - i / 10)) });
const ms = (n: number) => n * 1_000_000;

const node = (id: string, duration: number, extra: Partial<TraceNodeView>): TraceNodeView => ({
  node: id,
  duration_nanos: ms(duration),
  error: null,
  inputs: [{ kind: 'query', id: 'q1' }],
  metrics: null,
  gold_ranks: null,
  output: null,
  ...extra,
});

export const VERIFIED: DatasetCheck = {
  status: 'verified',
  benchmark: 'beir/scifact',
  expected: { dataset_version: SCIFACT, index_version: INDEX },
  found: { dataset_version: SCIFACT, index_version: INDEX },
  detail: 'the dataset on disk digests to the run’s dataset_version, and its chunk set to its index_version',
};

/**
 * Query q1 through the hybrid, in execution order. bm25 and dense each find
 * one gold document; rrf lifts both to the top; the reranker keeps four,
 * swapping the first two and the next two, and discards four.
 */
export const HYBRID_TRACE: QueryTrace = {
  run: HYBRID,
  query: 'q1',
  text: 'What do tides depend on?',
  passages: VERIFIED,
  scores: { 'ndcg@10': 0.861, 'recall@10': 1, mrr: 1, token_f1: 0.5 },
  nodes: [
    node('bm25', 9, { output: ranking(3, 2, 7, 5, 9), gold_ranks: [2, 4], metrics: { 'ndcg@10': 0.5, 'recall@10': 1, mrr: 0.5 } }),
    node('dense', 40, { output: ranking(2, 4, 5, 6, 8), gold_ranks: [1, 3], metrics: { 'ndcg@10': 0.81, 'recall@10': 1, mrr: 1 } }),
    node('rrf', 1, {
      inputs: [{ kind: 'chunk_count', count: 5 }, { kind: 'chunk_count', count: 5 }],
      output: ranking(2, 5, 3, 4, 7, 6, 9, 8),
      gold_ranks: [1, 2],
      metrics: { 'ndcg@10': 0.95, 'recall@10': 1, mrr: 1 },
    }),
    node('rerank', 349, {
      inputs: [{ kind: 'query', id: 'q1' }, { kind: 'chunk_count', count: 8 }],
      output: ranking(5, 2, 4, 3),
      gold_ranks: [1, 2],
      metrics: { 'ndcg@10': 0.861, 'recall@10': 1, mrr: 1 },
    }),
    node('context', 1, {
      inputs: [{ kind: 'query', id: 'q1' }, { kind: 'chunk_count', count: 4 }],
      output: { kind: 'context', chunks: [passage(5, 0.9), passage(2, 0.8), passage(4, 0.7)], text: 'Passage 5 of the corpus.\nPassage 2 of the corpus.\nPassage 4 of the corpus.' },
    }),
    node('answer', 600, {
      inputs: [{ kind: 'query', id: 'q1' }, { kind: 'context_size', count: 3, text_bytes: 72 }],
      output: { kind: 'answer', text: 'Tides depend on the moon.' },
    }),
  ],
};

/** Query q1 through the dense-only run: its one retriever, a gold document at rank 3. */
export const DENSE_TRACE: QueryTrace = {
  run: DENSE,
  query: 'q1',
  text: 'What do tides depend on?',
  passages: VERIFIED,
  scores: { 'ndcg@10': 0.6131, 'recall@10': 0.5, mrr: 0.3333 },
  nodes: [node('dense', 38, { output: ranking(4, 6, 5, 8, 1), gold_ranks: [3], metrics: { 'ndcg@10': 0.6131, 'recall@10': 0.5, mrr: 0.3333 } })],
};

/** Query q1 through the hybrid, failing at the reranker: the context builder and the generator never ran. */
export const FAILED_TRACE: QueryTrace = {
  ...HYBRID_TRACE,
  run: FAILED,
  nodes: [...HYBRID_TRACE.nodes.slice(0, 3), node('rerank', 30_000, { error: 'The service at 127.0.0.1:7001 did not answer within 30 s.' })],
};

/**
 * The trace with its passages checked as `status`: no text, and the digests
 * found. Under `index_differs` the dataset is the run's, so the scores, the
 * grades, the gold ranks and the query's text stay; only the text goes.
 */
export function withPassages(trace: QueryTrace, status: 'dataset_absent' | 'dataset_differs' | 'index_differs'): QueryTrace {
  if (status === 'index_differs') {
    const found = { dataset_version: SCIFACT, index_version: hex('8') };
    return {
      ...trace,
      passages: { ...VERIFIED, status, found, detail: 'the dataset digests to the run’s, and its chunk set to another' },
      nodes: trace.nodes.map((n) =>
        n.output !== null && (n.output.kind === 'ranking' || n.output.kind === 'context') ? { ...n, output: { ...n.output, chunks: n.output.chunks.map((p) => ({ ...p, text: null })) } } : n,
      ),
    };
  }
  const passages: DatasetCheck =
    status === 'dataset_absent'
      ? { ...VERIFIED, status, found: null, detail: 'no dataset on disk digests to the run’s dataset_version' }
      : { ...VERIFIED, status, found: { dataset_version: NFCORPUS, index_version: null }, detail: 'the dataset on disk digests to another version' };
  const strip = (p: TracePassage): TracePassage => ({ ...p, text: null, grade: null });
  return {
    ...trace,
    passages,
    text: null,
    scores: {},
    nodes: trace.nodes.map((n) => ({
      ...n,
      metrics: null,
      gold_ranks: null,
      output: n.output !== null && (n.output.kind === 'ranking' || n.output.kind === 'context') ? { ...n.output, chunks: n.output.chunks.map(strip) } : n.output,
    })),
  };
}

const scored = (id: string, text: string | null, scores: Record<string, number>, ms_: number) => ({ id, text, scores, duration_nanos: ms(ms_) });

/** The hybrid's queries: q1 and q2 judged, q3 not; q2 is the one with no gold in the top 10. */
export const HYBRID_QUERIES: RunQueries = {
  run: HYBRID,
  answer_node: 'answer',
  ranking_node: 'rerank',
  cache_error: null,
  ground_truth: { ...VERIFIED, found: { dataset_version: SCIFACT, index_version: null } },
  metrics: ['mrr', 'ndcg@10', 'recall@10', 'token_f1'],
  nodes: [
    { node: 'answer', produces_ranking: false, judged_queries: 0, metrics: null },
    { node: 'bm25', produces_ranking: true, judged_queries: 2, metrics: { 'ndcg@10': 0.41, 'recall@10': 0.6, mrr: 0.4 } },
    { node: 'context', produces_ranking: false, judged_queries: 0, metrics: null },
    { node: 'dense', produces_ranking: true, judged_queries: 2, metrics: { 'ndcg@10': 0.52, 'recall@10': 0.7, mrr: 0.5 } },
    { node: 'rerank', produces_ranking: true, judged_queries: 2, metrics: { 'ndcg@10': 0.7217, 'recall@10': 0.8, mrr: 0.6 } },
    { node: 'rrf', produces_ranking: true, judged_queries: 2, metrics: { 'ndcg@10': 0.6, 'recall@10': 0.8, mrr: 0.55 } },
  ],
  queries: [
    scored('q1', 'What do tides depend on?', { 'ndcg@10': 0.861, 'recall@10': 1, mrr: 1 }, 1000),
    scored('q2', 'Which enzyme breaks down starch?', { 'ndcg@10': 0, 'recall@10': 0, mrr: 0 }, 900),
    scored('q3', 'Is the sky blue at noon?', {}, 950),
  ],
};

/** The hybrid's queries under `missing_gold_at=10`: q2 alone. */
export const HYBRID_MISSING: RunQueries = { ...HYBRID_QUERIES, queries: HYBRID_QUERIES.queries.filter((q) => q.id === 'q2') };

export const DENSE_QUERIES: RunQueries = {
  ...HYBRID_QUERIES,
  run: DENSE,
  answer_node: null,
  ranking_node: 'dense',
  nodes: [{ node: 'dense', produces_ranking: true, judged_queries: 2, metrics: { 'ndcg@10': 0.5, 'recall@10': 0.6, mrr: 0.45 } }],
};

export const FAILED_QUERIES: RunQueries = { ...HYBRID_QUERIES, run: FAILED };

const detail = (id: string, graph: Graph): RunDetail => ({
  id,
  graph,
  configuration: '',
  bindings: [],
  metrics: {},
  inputs: { dataset_version: SCIFACT, index_version: INDEX, engine_version: '0.1.0', model_hashes: {}, pipeline: id },
  prefix_of: null,
  started_at_ms: null,
  finished_at_ms: null,
});
export const HYBRID_DETAIL = detail(HYBRID, HYBRID_GRAPH);
export const DENSE_DETAIL = detail(DENSE, DENSE_GRAPH);
export const FAILED_DETAIL = detail(FAILED, HYBRID_GRAPH);

const summary = (id: string, name: string, dataset: string): RunSummary => ({
  id,
  pipeline: id,
  pipeline_names: [name],
  dataset_version: dataset,
  index_version: INDEX,
  benchmark_names: [dataset === SCIFACT ? 'beir/scifact' : 'beir/nfcorpus'],
  engine_version: '0.1.0',
  metrics: {},
  metric_families: { mrr: 'ranking', 'ndcg@10': 'ranking', 'recall@10': 'ranking', token_f1: 'answers' },
  median_query_latency_nanos: null,
  started_at_ms: null,
  finished_at_ms: null,
});

/** Four runs: three on SciFact, one on NFCorpus, which Replay never offers beside a SciFact run. */
export const LISTING: RunListing = {
  runs: [summary(HYBRID, 'hybrid-rerank-gen', SCIFACT), summary(DENSE, 'dense-only', SCIFACT), summary(FAILED, 'hybrid-broken', SCIFACT), summary(ELSEWHERE, 'dense-nfcorpus', NFCORPUS)],
  shapes: {},
  unreadable: [],
};

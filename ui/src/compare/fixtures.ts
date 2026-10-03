// A `POST /compare` answer the Compare screen's tests read: a dense-only
// baseline, hybrid and hybrid-rerank on beir/scifact, shaped as
// runtime/ragondin-api builds it (ARCHITECTURE.md there, § Compare). Test
// data only; no application module imports it.
import type { Comparison, DeltaBin, DeltaBinName, MetricDeltas, StageCell, StageNode } from '../api/types.ts';

const hex = (c: string) => c.repeat(64);
export const DENSE = hex('d');
export const HYBRID = hex('b');
export const RERANK = hex('e');
export const SCIFACT = hex('5');

/** The bounds each bin carries in the API's answer — the product's bins (the front-end design, § 3) — read here as data. */
const BOUNDS: [DeltaBinName, number | null, number | null][] = [
  ['much_worse', null, -0.3],
  ['worse', -0.3, -0.1],
  ['slightly_worse', -0.1, 0],
  ['unchanged', 0, 0],
  ['slightly_better', 0, 0.1],
  ['better', 0.1, 0.3],
  ['much_better', 0.3, null],
];

/** A delta inside each bin, for the fixture's queries. */
const SAMPLE: Record<DeltaBinName, number> = {
  much_worse: -0.42,
  worse: -0.2,
  slightly_worse: -0.05,
  unchanged: 0,
  slightly_better: 0.05,
  better: 0.2,
  much_better: 0.4,
};

/**
 * One metric's histogram: `counts` queries per bin, worst first, the queries
 * numbered in bin order, and within the worst bin the first query the worst.
 */
export function metricDeltas(metric: string, counts: readonly number[]): MetricDeltas {
  let next = 1;
  const deltas: MetricDeltas['deltas'] = [];
  const bins: DeltaBin[] = BOUNDS.map(([bin, lower, upper], i) => {
    const queries: string[] = [];
    for (let k = 0; k < (counts[i] ?? 0); k++) {
      const query = `q${next++}`;
      queries.push(query);
      deltas.push({ query, delta: SAMPLE[bin] - (bin === 'much_worse' && k === 0 ? 0.1 : 0) });
    }
    return { bin, lower, upper, count: queries.length, queries };
  });
  return { metric, judged_queries: deltas.length, bins, deltas };
}

const node = (id: string, metrics: Record<string, number>, paired_by_hand = false): StageNode => ({ node: id, metrics, paired_by_hand });
const present = (nodes: StageNode[]): StageCell => {
  const best: Record<string, { node: string; value: number }> = {};
  for (const n of nodes) {
    for (const [metric, value] of Object.entries(n.metrics ?? {})) {
      const held = best[metric];
      if (held === undefined || value > held.value) best[metric] = { node: n.node, value };
    }
  }
  return { kind: 'present', nodes, best };
};
const ABSENT: StageCell = { kind: 'absent' };

const DENSE_LEG = { 'mrr@10': 0.6012, 'ndcg@10': 0.6483, 'recall@100': 0.902 };
const BM25_LEG = { 'mrr@10': 0.5711, 'ndcg@10': 0.6203, 'recall@100': 0.874 };
const RRF = { 'mrr@10': 0.628, 'ndcg@10': 0.6611, 'recall@100': 0.931 };
const RERANKED = { 'mrr@10': 0.679, 'ndcg@10': 0.7032, 'recall@100': 0.931 };

/** The three runs, the baseline first, as `POST /compare` answers them. */
export const COMPARISON: Comparison = {
  baseline: DENSE,
  runs: [
    { id: DENSE, pipeline: 'dense-only', pipeline_hash: hex('1') },
    { id: HYBRID, pipeline: 'hybrid', pipeline_hash: hex('2') },
    { id: RERANK, pipeline: 'hybrid-rerank', pipeline_hash: hex('3') },
  ],
  metrics: [
    { name: 'latency_p50_ms', direction: 'lower', values: [12, 19, 141], deltas: [0, 7, 129], best: [DENSE] },
    { name: 'mrr@10', direction: 'higher', values: [0.6012, 0.628, 0.679], deltas: [0, 0.0268, 0.0778], best: [RERANK] },
    { name: 'ndcg@10', direction: 'higher', values: [0.6483, 0.6611, 0.7032], deltas: [0, 0.0128, 0.0549], best: [RERANK] },
    { name: 'recall@100', direction: 'higher', values: [0.902, 0.931, 0.931], deltas: [0, 0.029, 0.029], best: [HYBRID, RERANK] },
  ],
  configuration: {
    kind: 'compared',
    same_logical_form: false,
    parameters: [
      { node: 'bm25', key: { kind: 'param', name: 'top_k' }, values: [null, 100, 100] },
      { node: 'dense', key: { kind: 'param', name: 'top_k' }, values: [100, 100, 50] },
      { node: 'rerank', key: { kind: 'component' }, values: [null, null, 'reranker'] },
      { node: 'rerank', key: { kind: 'impl' }, values: [null, null, 'cross_encoder'] },
    ],
  },
  stages: [
    {
      stage: 'retrieval_legs',
      confidence: 'high',
      source: 'automatic',
      label: null,
      cells: [present([node('dense', DENSE_LEG)]), present([node('bm25', BM25_LEG), node('dense', DENSE_LEG)]), present([node('bm25', BM25_LEG), node('dense', DENSE_LEG)])],
    },
    { stage: 'after_fusion', confidence: 'high', source: 'automatic', label: null, cells: [ABSENT, present([node('rrf', RRF)]), present([node('rrf', RRF)])] },
    { stage: 'after_rerank', confidence: 'high', source: 'automatic', label: null, cells: [ABSENT, ABSENT, present([node('rerank', RERANKED)])] },
    {
      stage: 'final_ranking',
      confidence: 'high',
      source: 'automatic',
      label: null,
      cells: [present([node('dense', DENSE_LEG)]), present([node('rrf', RRF)]), present([node('rerank', RERANKED)])],
    },
  ],
  pairings: [],
  unplaced_pairs: [],
  query_deltas: [
    {
      run: HYBRID,
      metrics: [metricDeltas('mrr@10', [2, 9, 30, 190, 40, 21, 8]), metricDeltas('ndcg@10', [1, 8, 33, 186, 45, 20, 7]), metricDeltas('recall@100', [0, 3, 10, 251, 16, 14, 6])],
    },
    {
      run: RERANK,
      metrics: [metricDeltas('mrr@10', [6, 20, 35, 108, 54, 0, 77]), metricDeltas('ndcg@10', [4, 18, 30, 120, 60, 40, 28]), metricDeltas('recall@100', [0, 3, 10, 251, 16, 14, 6])],
    },
  ],
  latency: [
    { run: DENSE, nodes: [{ node: 'dense', family: 'retriever', median_nanos: 11_000_000, queries: 300 }] },
    {
      run: HYBRID,
      nodes: [
        { node: 'bm25', family: 'retriever', median_nanos: 4_000_000, queries: 300 },
        { node: 'dense', family: 'retriever', median_nanos: 11_000_000, queries: 300 },
        { node: 'rrf', family: 'fusion', median_nanos: 500_000, queries: 300 },
      ],
    },
    {
      run: RERANK,
      nodes: [
        { node: 'bm25', family: 'retriever', median_nanos: 4_000_000, queries: 300 },
        { node: 'dense', family: 'retriever', median_nanos: 11_000_000, queries: 300 },
        { node: 'rerank', family: 'reranker', median_nanos: 120_000_000, queries: 300 },
        { node: 'rrf', family: 'fusion', median_nanos: 500_000, queries: 300 },
      ],
    },
  ],
  ground_truth: {
    benchmark: 'beir/scifact',
    status: 'verified',
    detail: 'the dataset on disk digests to the runs’ dataset_version',
    expected: { dataset_version: SCIFACT, index_version: hex('0') },
    found: { dataset_version: SCIFACT, index_version: hex('0') },
  },
  cache_errors: [],
};

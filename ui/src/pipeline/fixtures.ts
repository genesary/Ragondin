// A `GET /pipelines/{name}/matrix` answer the Pipeline screen's tests read:
// hybrid-rerank-gen on beir/nfcorpus, beir/scifact and squad/dev, shaped as
// runtime/ragondin-api builds it (ARCHITECTURE.md there, § The pipeline
// matrix), and the variants each state needs. Test data only; no application
// module imports it.
import type { FeedingRun, MatrixCell, MatrixColumn, MatrixGain, MatrixRow, PipelineMatrix } from '../api/types.ts';

const hex = (c: string) => c.repeat(64);
export const NAME = 'hybrid-rerank-gen';
export const HASH = hex('a');
export const NFCORPUS = hex('1');
export const SCIFACT = hex('2');
export const SQUAD = hex('3');
export const FIQA = hex('4');
export const RUN_NF = hex('5');
export const RUN_SCI = hex('6');
export const RUN_SQ = hex('7');
export const RUN_PREFIX = hex('8');
export const RUN_OLD = hex('9');

export const ROWS: MatrixRow[] = [
  { node: 'bm25', family: 'retriever', produces: 'chunks' },
  { node: 'dense', family: 'retriever', produces: 'chunks' },
  { node: 'rrf', family: 'fusion', produces: 'chunks' },
  { node: 'rerank', family: 'reranker', produces: 'chunks' },
  { node: 'concat', family: 'context_builder', produces: 'context' },
  { node: 'generate', family: 'generator', produces: 'answer' },
];

const ranked = (ndcg: number, mrr: number, gain: MatrixGain): MatrixCell => ({
  kind: 'measured',
  metrics: { mrr, 'ndcg@10': ndcg },
  gain,
  judged_queries: 300,
});
const over = (ndcg: number, mrr: number) => ({ kind: 'over_previous_stage' as const, values: { mrr, 'ndcg@10': ndcg } });
const first = { kind: 'first_stage' as const };

/** One column filled by a run of the whole pipeline, its ranking figures and gains given per node. */
function whole(dataset: string, names: string[], run: string, ranking: MatrixCell[], generate: MatrixCell, ground: MatrixColumn['ground_truth']): MatrixColumn {
  return {
    dataset_version: dataset,
    benchmark_names: names,
    ground_truth: ground,
    run,
    up_to: null,
    dataset_check: { benchmark: names[0] ?? null, detail: 'the dataset on disk digests to the one the run recorded', expected: { dataset_version: dataset, index_version: hex('0') }, found: { dataset_version: dataset, index_version: hex('0') }, status: 'verified' },
    cells: [...ranking, { kind: 'not_scored' }, generate],
  };
}

/**
 * Three runs of the whole pipeline. On nfcorpus the reranker gains most
 * (+0.0412 nDCG@10), on scifact the fusion's gain is the row's best, and the
 * best *value* of the reranker row (scifact, 0.7217) is not its best gain.
 */
export const COLUMNS: MatrixColumn[] = [
  whole(NFCORPUS, ['beir/nfcorpus'], RUN_NF, [ranked(0.3012, 0.5, first), ranked(0.3215, 0.52, first), ranked(0.3398, 0.55, over(0.0183, 0.03)), ranked(0.3810, 0.6, over(0.0412, 0.05))], { kind: 'no_reference_answers' }, 'qrels'),
  whole(SCIFACT, ['beir/scifact'], RUN_SCI, [ranked(0.6650, 0.62, first), ranked(0.6483, 0.6, first), ranked(0.7001, 0.66, over(0.0351, 0.04)), ranked(0.7217, 0.7, over(0.0216, 0.04))], { kind: 'no_reference_answers' }, 'qrels'),
  whole(SQUAD, ['squad/dev'], RUN_SQ, [ranked(0.5010, 0.48, first), ranked(0.5523, 0.51, first), ranked(0.5781, 0.55, over(0.0258, 0.04)), ranked(0.6012, 0.58, over(0.0231, 0.03))], { kind: 'measured', metrics: { exact_match: 0.412, token_f1: 0.491 }, gain: { kind: 'unstaged' }, judged_queries: null }, 'both'),
];

const feeding = (run: string, dataset: string, names: string[], at: number): FeedingRun => ({
  run,
  dataset_version: dataset,
  benchmark_names: names,
  started_at_ms: at,
  fills_column: true,
  launched_as: { name: NAME, prefix_of: null, held: 'exactly' },
  pipeline_names: [NAME],
  prefix_of: null,
  content_since_changed: null,
});

export const MATRIX: PipelineMatrix = {
  pipeline: NAME,
  pipeline_hash: HASH,
  rows: ROWS,
  columns: COLUMNS,
  feeding_runs: [feeding(RUN_SQ, SQUAD, ['squad/dev'], 3_000), feeding(RUN_SCI, SCIFACT, ['beir/scifact'], 2_000), feeding(RUN_NF, NFCORPUS, ['beir/nfcorpus'], 1_000)],
  missing: [],
  unreadable: [],
  cache_errors: [],
};

/** nfcorpus filled by a run up to rerank instead: the concat and generate cells read `prefix_stops`. */
export const PREFIXED: PipelineMatrix = {
  ...MATRIX,
  columns: [
    { ...(COLUMNS[0] as MatrixColumn), run: RUN_PREFIX, up_to: 'rerank', cells: [...(COLUMNS[0] as MatrixColumn).cells.slice(0, 4), { kind: 'prefix_stops', up_to: 'rerank' }, { kind: 'prefix_stops', up_to: 'rerank' }] },
    COLUMNS[1] as MatrixColumn,
    COLUMNS[2] as MatrixColumn,
  ],
  feeding_runs: [
    feeding(RUN_SQ, SQUAD, ['squad/dev'], 3_000),
    feeding(RUN_SCI, SCIFACT, ['beir/scifact'], 2_000),
    { ...feeding(RUN_PREFIX, NFCORPUS, ['beir/nfcorpus'], 1_000), launched_as: { name: NAME, prefix_of: { parent_pipeline_hash: HASH, up_to: 'rerank' }, held: 'exactly' }, pipeline_names: [], prefix_of: { pipeline: NAME, up_to: 'rerank' } },
  ],
  missing: [{ benchmark: 'beir/nfcorpus', dataset_version: NFCORPUS, nodes: ['concat', 'generate'] }],
};

/** A fourth benchmark, beir/fiqa, that no run measured: every cell `not_run_yet`. */
export const WITH_FIQA: PipelineMatrix = {
  ...MATRIX,
  columns: [
    { dataset_version: FIQA, benchmark_names: ['beir/fiqa'], ground_truth: null, run: null, up_to: null, dataset_check: null, cells: ROWS.map(() => ({ kind: 'not_run_yet', benchmark: 'beir/fiqa' })) },
    ...COLUMNS,
  ],
  missing: [{ benchmark: 'beir/fiqa', dataset_version: FIQA, nodes: ROWS.map((r) => r.node) }],
};

/** A run launched as the pipeline before its dense retriever's `top_k` changed: a feeding run that fills nothing. */
export const SINCE_CHANGED: FeedingRun = {
  run: RUN_OLD,
  dataset_version: SCIFACT,
  benchmark_names: ['beir/scifact'],
  started_at_ms: 500,
  fills_column: false,
  launched_as: { name: NAME, prefix_of: null, held: 'exactly' },
  pipeline_names: ['dense-50'],
  prefix_of: null,
  content_since_changed: {
    launched: 'as_pipeline',
    difference: { kind: 'compared', same_logical_form: false, parameters: [{ node: 'dense', key: { kind: 'param', name: 'top_k' }, values: [50, 100] }] },
  },
};

/** A pipeline with no run at all. */
export const NO_RUNS: PipelineMatrix = { ...MATRIX, columns: [], feeding_runs: [], missing: [] };

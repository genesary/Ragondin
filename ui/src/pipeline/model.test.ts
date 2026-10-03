import { describe, expect, it } from 'vitest';
import type { FeedingRun, PipelineMatrix } from '../api/types.ts';
import { MATRIX, NAME, NO_RUNS, PREFIXED, SINCE_CHANGED, WITH_FIQA } from './fixtures.ts';
import {
  answerMetrics,
  bestGainColumns,
  columnLabel,
  contentFact,
  groundTruthLabel,
  launchFact,
  missingCount,
  rankingMetrics,
  sinceChangedLabel,
  signedGain,
  spansRow,
  verdict,
} from './model.ts';

describe('the metrics a row can read', () => {
  it('lists the ranking metrics the ranking cells carry, in name order', () => {
    expect(rankingMetrics(MATRIX)).toEqual(['mrr', 'ndcg@10']);
  });

  it('lists the answer metrics the generator cells carry, in name order', () => {
    expect(answerMetrics(MATRIX)).toEqual(['exact_match', 'token_f1']);
  });

  it('lists none when no cell is measured', () => {
    expect(rankingMetrics(NO_RUNS)).toEqual([]);
    expect(answerMetrics(NO_RUNS)).toEqual([]);
  });
});

describe('the best gain of a row', () => {
  it('is the column with the greatest gain over the previous stage, never the greatest value', () => {
    // rerank: nfcorpus gains +0.0412, scifact holds the best value (0.7217) with +0.0216.
    expect(bestGainColumns(MATRIX, 3, 'ndcg@10')).toEqual([0]);
    // rrf: scifact's +0.0351 is the best gain.
    expect(bestGainColumns(MATRIX, 2, 'ndcg@10')).toEqual([1]);
  });

  it('is every column of a tie', () => {
    expect(bestGainColumns(MATRIX, 3, 'mrr')).toEqual([0]);
    expect(bestGainColumns(MATRIX, 2, 'mrr')).toEqual([1, 2]);
  });

  it('is none for a retrieval leg, whose value stands alone', () => {
    expect(bestGainColumns(MATRIX, 0, 'ndcg@10')).toEqual([]);
  });

  it('is none when only one cell has a gain: there is nothing to be best against', () => {
    const one: PipelineMatrix = { ...MATRIX, columns: [MATRIX.columns[0]!] };
    expect(bestGainColumns(one, 3, 'ndcg@10')).toEqual([]);
  });
});

describe('a gain as the cell prints it', () => {
  it('is signed, to four decimals, with a true minus', () => {
    expect(signedGain(0.0412)).toBe('+0.0412');
    expect(signedGain(-0.01)).toBe('−0.0100');
  });

  it('reads as no change when it prints as zero', () => {
    expect(signedGain(0.00001)).toBe('0.0000');
  });
});

describe('the rows and the columns in words', () => {
  it('spans a row whose every cell is not scored, the context builder', () => {
    expect(spansRow(MATRIX, 4)).toBe(true);
    expect(spansRow(MATRIX, 5)).toBe(false);
    expect(spansRow(PREFIXED, 4)).toBe(false);
  });

  it('spans no row of a matrix with no columns', () => {
    expect(spansRow(NO_RUNS, 4)).toBe(false);
  });

  it('labels a column by every benchmark pinned to it, or by its short digest', () => {
    expect(columnLabel(MATRIX.columns[0]!)).toBe('beir/nfcorpus');
    expect(columnLabel({ ...MATRIX.columns[0]!, benchmark_names: ['beir/scifact', 'scifact-local'] })).toBe('beir/scifact, scifact-local');
    expect(columnLabel({ ...MATRIX.columns[0]!, benchmark_names: [] })).toBe(`dataset ${'1'.repeat(12)}`);
  });

  it('names what a ground truth carries', () => {
    expect(groundTruthLabel('qrels')).toBe('qrels');
    expect(groundTruthLabel('reference_answers')).toBe('reference answers');
    expect(groundTruthLabel('both')).toBe('qrels, reference answers');
    expect(groundTruthLabel('none')).toBe('no ground truth');
    expect(groundTruthLabel(null)).toBe('ground truth read once run');
  });
});

describe('the missing cells', () => {
  it('counts every node the API lists as missing, over every column', () => {
    expect(missingCount(MATRIX)).toBe(0);
    expect(missingCount(PREFIXED)).toBe(2);
    expect(missingCount(WITH_FIQA)).toBe(6);
  });
});

describe('the verdict', () => {
  it('counts the benchmarks measured and the cells missing, never an adjective', () => {
    expect(verdict(MATRIX)).toBe('Measured on 3 of 3 benchmarks. No cell waits for a run.');
    expect(verdict(PREFIXED)).toBe('Measured on 3 of 3 benchmarks, 1 of them only up to rerank. 2 cells wait for a run of the whole pipeline.');
    expect(verdict(WITH_FIQA)).toBe('Measured on 3 of 4 benchmarks. 6 cells wait for a run of the whole pipeline.');
  });

  it('says one cell in the singular', () => {
    const one: PipelineMatrix = { ...PREFIXED, missing: [{ benchmark: 'beir/nfcorpus', dataset_version: '1', nodes: ['generate'] }] };
    expect(verdict(one)).toBe('Measured on 3 of 3 benchmarks, 1 of them only up to rerank. 1 cell waits for a run of the whole pipeline.');
  });

  it('says prefix runs stopping at different nodes by their count', () => {
    const two: PipelineMatrix = { ...PREFIXED, columns: PREFIXED.columns.map((c, i) => (i === 1 ? { ...c, up_to: 'rrf' } : c)) };
    expect(verdict(two)).toBe('Measured on 3 of 3 benchmarks, 2 of them by a prefix run. 2 cells wait for a run of the whole pipeline.');
  });
});

describe('a feeding run’s two facts', () => {
  const base = MATRIX.feeding_runs[0] as FeedingRun;

  it('says what the run was launched as, from its record', () => {
    expect(launchFact(base)).toBe(`Launched as ${NAME}`);
    expect(launchFact(PREFIXED.feeding_runs[2]!)).toBe(`Launched as a prefix of ${NAME}, up to rerank`);
    expect(launchFact({ ...base, launched_as: null })).toBe('No launch record');
    expect(launchFact({ ...base, launched_as: { name: null, prefix_of: null } })).toBe('Launched under no name');
  });

  it('says which current documents its content is, apart from the launch', () => {
    expect(contentFact(base)).toBe(`Content: ${NAME}`);
    expect(contentFact(SINCE_CHANGED)).toBe('Content: dense-50');
    expect(contentFact({ ...base, pipeline_names: ['a', 'b'] })).toBe('Content: a, b');
    expect(contentFact({ ...base, pipeline_names: [] })).toBe('Content: no current pipeline document');
  });

  it('says a run whose content changed since its launch for what its record says it was', () => {
    expect(sinceChangedLabel(SINCE_CHANGED, NAME)).toBe(`Launched as ${NAME}; content since changed`);
    const prefix: FeedingRun = { ...SINCE_CHANGED, content_since_changed: { ...SINCE_CHANGED.content_since_changed!, launched: 'as_prefix' } };
    expect(sinceChangedLabel(prefix, NAME)).toBe(`A prefix of an earlier version of ${NAME}`);
    expect(sinceChangedLabel(base, NAME)).toBeNull();
  });
});

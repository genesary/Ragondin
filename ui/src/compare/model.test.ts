import { describe, expect, it } from 'vitest';
import type { Comparison, MetricDeltas } from '../api/types.ts';
import { COMPARISON, DENSE, HYBRID, metricDeltas, RERANK } from './fixtures.ts';
import {
  automaticLinks,
  barMetrics,
  binsOf,
  deltaOf,
  departures,
  formatParameter,
  latencyBars,
  manualPairs,
  noVerdict,
  pairableNodes,
  pairsByHandLabel,
  regressions,
  runSeries,
  stageLine,
  verdict,
} from './model.ts';

const deltasOf = (run: string, metric: string) => COMPARISON.query_deltas.find((d) => d.run === run)?.metrics.find((m) => m.metric === metric) as MetricDeltas;

describe('runSeries', () => {
  it('gives the baseline the neutral slot and the others A to D in the answer\'s order, named by pipeline', () => {
    expect(runSeries(COMPARISON)).toEqual([
      { id: DENSE, ink: 'base', short: 'base', label: 'baseline · dense-only' },
      { id: HYBRID, ink: 'a', short: 'A', label: 'A · hybrid' },
      { id: RERANK, ink: 'b', short: 'B', label: 'B · hybrid-rerank' },
    ]);
  });

  it('names a run its pipeline document does not match by its short pipeline hash, never a guess', () => {
    const c: Comparison = { ...COMPARISON, runs: COMPARISON.runs.map((r, i) => (i === 1 ? { ...r, pipeline: null } : r)) };
    expect(runSeries(c)[1]?.label).toBe('A · pipeline 222222222222');
  });
});

describe('barMetrics', () => {
  it('keeps the metrics read on a 0–1 scale where higher is better, leaving latency to the table', () => {
    expect(barMetrics(COMPARISON.metrics).map((m) => m.name)).toEqual(['mrr@10', 'ndcg@10', 'recall@100']);
  });
});

describe('deltaOf', () => {
  it('signs the delta, and reads better or worse from the metric\'s direction, not the sign alone', () => {
    expect(deltaOf('higher', 0.0549)).toEqual({ text: '+0.0549', meaning: 'better', direction: 'up' });
    expect(deltaOf('higher', -0.03)).toEqual({ text: '−0.0300', meaning: 'worse', direction: 'down' });
    expect(deltaOf('lower', 129)).toEqual({ text: '+129.0', meaning: 'worse', direction: 'up' });
    expect(deltaOf('lower', -2)).toEqual({ text: '−2.0', meaning: 'better', direction: 'down' });
    expect(deltaOf('higher', 0)).toEqual({ text: '0.0000', meaning: 'same', direction: 'none' });
  });

  it('signs the delta of a metric with no direction and calls it neither better nor worse', () => {
    expect(deltaOf(null, 1.5)).toEqual({ text: '+1.5000', meaning: null, direction: 'up' });
    expect(deltaOf(null, -0.25)).toEqual({ text: '−0.2500', meaning: null, direction: 'down' });
    expect(deltaOf(null, 0)).toEqual({ text: '0.0000', meaning: 'same', direction: 'none' });
  });

  it('reads a delta too small to print as unchanged, never as a coloured +0.0000', () => {
    expect(deltaOf('higher', 0.00004)).toEqual({ text: '0.0000', meaning: 'same', direction: 'none' });
    expect(deltaOf('higher', -0.00004)).toEqual({ text: '0.0000', meaning: 'same', direction: 'none' });
    expect(deltaOf('lower', 0.04)).toEqual({ text: '0.0', meaning: 'same', direction: 'none' });
  });
});

describe('departures', () => {
  it('marks each value that differs from the baseline\'s, an unset one included', () => {
    const [bm25, dense, component] = COMPARISON.configuration.kind === 'compared' ? COMPARISON.configuration.parameters : [];
    expect(departures(bm25!)).toEqual([false, true, true]);
    expect(departures(dense!)).toEqual([false, false, true]);
    expect(departures(component!)).toEqual([false, false, true]);
  });

  it('writes a value as a configuration does, and an unset one in words', () => {
    expect(formatParameter(100)).toBe('100');
    expect(formatParameter('cross_encoder')).toBe('cross_encoder');
    expect(formatParameter([1, 'a', true])).toBe('[1, a, true]');
    expect(formatParameter(null)).toBe('not set');
  });
});

describe('stageLine', () => {
  it('reads one metric per stage and run, a gap where the run has no stage, and every leg as its own mark', () => {
    const line = stageLine(COMPARISON, 'ndcg@10');
    expect(line.x.map((x) => x.label)).toEqual(['retrieval legs', 'after fusion', 'after rerank', 'final ranking']);
    expect(line.values).toEqual([
      [0.6483, null, null, 0.6483],
      [0.6483, 0.6611, null, 0.6611],
      [0.6483, 0.6611, 0.7032, 0.7032],
    ]);
    expect(line.dots).toEqual([
      { series: 1, x: 0, value: 0.6203, label: 'bm25' },
      { series: 1, x: 0, value: 0.6483, label: 'dense' },
      { series: 2, x: 0, value: 0.6203, label: 'bm25' },
      { series: 2, x: 0, value: 0.6483, label: 'dense' },
    ]);
    expect(line.gap(0, 1)).toBe('no stage here');
  });

  it('says a present stage that carries no figure is unscored, not absent', () => {
    const c: Comparison = { ...COMPARISON, stages: [{ ...COMPARISON.stages[0]!, cells: [{ kind: 'present', nodes: [{ node: 'dense', metrics: null, paired_by_hand: false }], best: {} }, { kind: 'absent' }, { kind: 'absent' }] }] };
    const line = stageLine(c, 'ndcg@10');
    expect(line.gap(0, 0)).toBe('no figure');
    expect(line.gap(1, 0)).toBe('no stage here');
  });
});

describe('latencyBars', () => {
  it('stacks each run\'s nodes in milliseconds, each by the family its configuration names', () => {
    const bars = latencyBars(COMPARISON);
    expect(bars.bars.map((b) => b.label)).toEqual(['baseline · dense-only', 'A · hybrid', 'B · hybrid-rerank']);
    expect(bars.segments[2]).toEqual([
      { id: 'bm25', label: 'bm25', value: 4, family: 'retriever' },
      { id: 'dense', label: 'dense', value: 11, family: 'retriever' },
      { id: 'rerank', label: 'rerank', value: 120, family: 'reranker' },
      { id: 'rrf', label: 'rrf', value: 0.5, family: 'fusion' },
    ]);
    expect(bars.families).toEqual(['retriever', 'fusion', 'reranker']);
  });

  it('draws an extension node neutral, and lists it under its own word', () => {
    const c: Comparison = { ...COMPARISON, latency: [{ run: DENSE, nodes: [{ node: 'x', family: 'extension', median_nanos: 1_000_000, queries: 1 }] }] };
    expect(latencyBars(c).segments[0]?.[0]?.family).toBeNull();
  });
});

describe('binsOf', () => {
  it('renders the API\'s bins and bounds as they come, never edges of its own', () => {
    const bins = binsOf(deltasOf(RERANK, 'mrr@10'));
    expect(bins.map((b) => [b.id, b.label, b.range, b.count, b.tone])).toEqual([
      ['much_worse', 'much worse', 'below −0.3', 6, 'worse'],
      ['worse', 'worse', '−0.3 to −0.1', 20, 'worse'],
      ['slightly_worse', 'slightly worse', '−0.1 to 0', 35, 'worse'],
      ['unchanged', 'unchanged', '0', 108, 'zero'],
      ['slightly_better', 'slightly better', '0 to 0.1', 54, 'better'],
      ['better', 'better', '0.1 to 0.3', 0, 'better'],
      ['much_better', 'much better', 'above 0.3', 77, 'better'],
    ]);
  });

  it('follows whatever bounds the API sends', () => {
    const md = metricDeltas('ndcg@10', [1, 0, 0, 0, 0, 0, 0]);
    md.bins[0] = { ...md.bins[0]!, upper: -0.25 };
    expect(binsOf(md)[0]?.range).toBe('below −0.25');
  });
});

describe('verdict', () => {
  it('names the counts, never an adjective, and the largest regression bin with its bound', () => {
    expect(verdict(deltasOf(RERANK, 'mrr@10'), 'B · hybrid-rerank')).toBe(
      'On mrr@10, B · hybrid-rerank against the baseline: 131 queries improve, 108 are unchanged, 61 get worse — 6 by more than 0.3.',
    );
  });

  it('names the next bin when the largest holds nothing, and agrees each verb with its count', () => {
    expect(verdict(metricDeltas('ndcg@10', [0, 1, 0, 1, 1, 0, 0]), 'A')).toBe('On ndcg@10, A against the baseline: 1 query improves, 1 is unchanged, 1 gets worse — 1 by more than 0.1.');
  });

  it('adds no clause when no query gets worse by more than the first bound', () => {
    expect(verdict(metricDeltas('ndcg@10', [0, 0, 2, 0, 3, 0, 0]), 'A')).toBe('On ndcg@10, A against the baseline: 3 queries improve, 0 are unchanged, 2 get worse.');
  });

  it('says why no verdict can be read: no shared ranking metric, or a ground truth not verified', () => {
    expect(noVerdict(COMPARISON, 'A · hybrid')).toBe('A · hybrid and the baseline share no ranking metric, so no query can be compared.');
    const absent: Comparison = { ...COMPARISON, ground_truth: { ...COMPARISON.ground_truth, status: 'dataset_absent', detail: 'no dataset on disk is pinned to dataset 5555' } };
    expect(noVerdict(absent, 'A · hybrid')).toBe('No query of A · hybrid can be compared with the baseline: no dataset on disk is pinned to dataset 5555.');
  });

  it('says when no query could be compared', () => {
    expect(verdict(metricDeltas('ndcg@10', [0, 0, 0, 0, 0, 0, 0]), 'A')).toBe('On ndcg@10, no query of A could be compared with the baseline.');
  });
});

describe('regressions', () => {
  it('counts the queries that get worse and names the worst of them', () => {
    expect(regressions(deltasOf(RERANK, 'mrr@10'))).toEqual({ count: 61, worst: 'q1' });
    expect(regressions(metricDeltas('ndcg@10', [0, 0, 0, 1, 1, 0, 0]))).toEqual({ count: 0, worst: null });
  });
});

describe('pairing', () => {
  it('offers each run\'s retrievers, fusion and reranker, from the stages the API derived', () => {
    expect(pairableNodes(COMPARISON, 0)).toEqual([{ node: 'dense', stage: 'retrieval_legs', family: 'retriever' }]);
    expect(pairableNodes(COMPARISON, 2).map((n) => n.node)).toEqual(['bm25', 'dense', 'rrf', 'rerank']);
  });

  it('draws the automatic pairs a stage makes, and none across a stage only one run has', () => {
    expect(automaticLinks(COMPARISON, 1)).toEqual([
      { node: 'dense', other: 'bm25' },
      { node: 'dense', other: 'dense' },
    ]);
  });

  it('reads the pairs drawn by hand for the baseline\'s pipeline and the other run\'s', () => {
    const c: Comparison = { ...COMPARISON, pairings: [{ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }] }] };
    expect(manualPairs(c, 2)).toEqual([{ node: 'dense', other: 'rerank' }]);
    expect(manualPairs(c, 1)).toEqual([]);
  });

  it('says how many pairs were drawn by hand', () => {
    expect(pairsByHandLabel(COMPARISON)).toBe('Paired automatically');
    const one: Comparison = { ...COMPARISON, pairings: [{ pipeline: 'dense-only', other: 'hybrid', pairs: [{ node: 'dense', other: 'bm25' }] }] };
    expect(pairsByHandLabel(one)).toBe('1 pair by hand');
    const two: Comparison = { ...one, pairings: [...one.pairings, { pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }] }] };
    expect(pairsByHandLabel(two)).toBe('2 pairs by hand');
  });
});

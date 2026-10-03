import { describe, expect, it } from 'vitest';
import type { Graph, RunListing, RunSummary } from '../api/types.ts';
import { benchmarkLabel, formatLatency, formatMetric, groupRows, metricLabel, openRoute, otherFact, rowKey, rowsFromListing, runningLabel, shapeOf, shortHash, type RunRow } from './model.ts';

const hex = (c: string) => c.repeat(64);

const row = (id: string, over: Partial<RunRow> = {}): RunRow => ({
  source: { kind: 'run', id },
  pipeline: hex('p'),
  pipelineNames: [],
  launchedAs: null,
  launchedHeld: null,
  launchRecorded: false,
  benchmark: hex('b'),
  benchmarkNames: [],
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  ...over,
});

const ids = (rows: RunRow[]) => rows.map((r) => r.source.id);

const summary = (id: string, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline: hex('a'),
  pipeline_names: [],
  launched_as: null,
  dataset_version: hex('d'),
  benchmark_names: [],
  index_version: hex('i'),
  engine_version: '0',
  started_at_ms: null,
  finished_at_ms: null,
  metrics: {},
  metric_families: {},
  median_query_latency_nanos: null,
  ...over,
});

const listingOf = (...runs: RunSummary[]): RunListing => ({ runs, unreadable: [], shapes: {} });

describe('rowsFromListing', () => {
  it('reads every summary as a done run row keyed by its pipeline hash and its dataset version, in listing order', () => {
    const listing = listingOf(summary(hex('1'), { metrics: { 'ndcg@10': 0.5, mrr: 0.25 } }), summary(hex('2'), { pipeline: hex('c'), dataset_version: hex('e') }));
    const rows = rowsFromListing(listing);
    expect(rows.map((r) => [r.source, r.pipeline, r.benchmark, r.status.state])).toEqual([
      [{ kind: 'run', id: hex('1') }, hex('a'), hex('d'), 'done'],
      [{ kind: 'run', id: hex('2') }, hex('c'), hex('e'), 'done'],
    ]);
  });

  it('groups the metrics by the family the listing sends, ranking, then answers, then unknown, each as the run recorded it', () => {
    const listing = listingOf(
      summary(hex('1'), {
        metrics: { exact_match: 0.412, foo_score: 7, mrr: 0.25, 'ndcg@10': 0.5 },
        metric_families: { exact_match: 'answers', foo_score: 'unknown', mrr: 'ranking', 'ndcg@10': 'ranking' },
      }),
    );
    expect(rowsFromListing(listing)[0]?.metrics).toEqual([
      { family: 'ranking', metrics: [{ name: 'mrr', value: 0.25 }, { name: 'ndcg@10', value: 0.5 }] },
      { family: 'answers', metrics: [{ name: 'exact_match', value: 0.412 }] },
      { family: 'unknown', metrics: [{ name: 'foo_score', value: 7 }] },
    ]);
  });

  it('shows a metric the listing gives no family for as unknown, never drops it', () => {
    const listing = listingOf(summary(hex('1'), { metrics: { mrr: 0.25 } }));
    expect(rowsFromListing(listing)[0]?.metrics).toEqual([{ family: 'unknown', metrics: [{ name: 'mrr', value: 0.25 }] }]);
  });

  it('reads the median query latency, in milliseconds', () => {
    expect(rowsFromListing(listingOf(summary(hex('1'), { median_query_latency_nanos: 17_249_000 })))[0]?.latencyMs).toBe(17.249);
  });

  it('gives a run that recorded no metric no group at all, so no empty cell is drawn', () => {
    expect(rowsFromListing(listingOf(summary(hex('1'))))[0]?.metrics).toEqual([]);
  });

  it('leaves what the listing does not carry empty rather than invented', () => {
    expect(rowsFromListing(listingOf(summary(hex('1'))))[0]).toMatchObject({ pipelineNames: [], launchedAs: null, benchmarkNames: [], latencyMs: null, startedAt: null, prefix: null });
  });

  it('reads the launch record’s name beside the hash matches, never one in place of the other', () => {
    const [read] = rowsFromListing(listingOf(summary(hex('1'), { launched_as: { name: 'hybrid', prefix_of: null, held: 'exactly' }, pipeline_names: ['hybrid-fork'] })));
    expect(read).toMatchObject({ launchedAs: 'hybrid', pipelineNames: ['hybrid-fork'], prefix: null });
  });

  it('reads whether the workspace still holds the recorded name, as the listing says it', () => {
    const [held] = rowsFromListing(listingOf(summary(hex('1'), { launched_as: { name: 'hybrid', prefix_of: null, held: 'other_case' } })));
    expect(held).toMatchObject({ launchedAs: 'hybrid', launchedHeld: 'other_case' });
    expect(rowsFromListing(listingOf(summary(hex('1'))))[0]).toMatchObject({ launchedHeld: null });
  });

  it('reads a recorded prefix as a prefix of the parent its record names, up to its node', () => {
    const record = { name: 'hybrid', prefix_of: { up_to: 'rerank', parent_pipeline_hash: hex('a') }, held: 'exactly' as const };
    const [read] = rowsFromListing(listingOf(summary(hex('1'), { launched_as: record })));
    expect(read).toMatchObject({ launchedAs: 'hybrid', prefix: { parent: 'hybrid', upTo: 'rerank' } });
  });

  it('carries every pipeline and benchmark name the listing gives, and the start time as an instant', () => {
    const [read] = rowsFromListing(listingOf(summary(hex('1'), { pipeline_names: ['hybrid', 'hybrid-copy'], benchmark_names: ['beir/scifact', 'scifact-local'], started_at_ms: Date.UTC(2026, 8, 30, 14, 3) })));
    expect(read).toMatchObject({ pipelineNames: ['hybrid', 'hybrid-copy'], benchmarkNames: ['beir/scifact', 'scifact-local'], startedAt: '2026-09-30T14:03:00.000Z' });
  });

  it('runs sort most recent first, unknown last, ties by id', () => {
    const sorted = rowsFromListing(
      listingOf(
        summary(hex('3'), { started_at_ms: null }),
        summary(hex('2'), { started_at_ms: 1000 }),
        summary(hex('9'), { started_at_ms: 2000 }),
        summary(hex('1'), { started_at_ms: null }),
        summary(hex('4'), { started_at_ms: 2000 }),
      ),
    );
    expect(ids(sorted)).toEqual([hex('4'), hex('9'), hex('2'), hex('1'), hex('3')]);
  });
});

describe('a row’s source', () => {
  it('keys a run and a job apart even when their ids are equal', () => {
    expect(rowKey(row('x'))).toBe('run:x');
    expect(rowKey(row('x', { source: { kind: 'job', id: 'x', runId: null } }))).toBe('job:x');
  });

  it('opens a run in Replay, before a query is chosen', () => {
    expect(openRoute(row('r1'))).toEqual({ screen: 'replay', run: 'r1' });
  });

  it('opens a job nowhere yet, whatever its status: the job view has no address in this build', () => {
    const job = (status: RunRow['status']) => row('j1', { source: { kind: 'job', id: 'j1', runId: 'r9' }, status });
    expect(openRoute(job({ state: 'failed', node: 'rerank', error: 'boom' }))).toBeNull();
    expect(openRoute(job({ state: 'queued' }))).toBeNull();
    expect(openRoute(job({ state: 'cancelled' }))).toBeNull();
  });
});

describe('the running state', () => {
  it('reads the queries done out of the total, never a percentage the client computed', () => {
    expect(runningLabel({ state: 'running', done: 1234, total: 10570 })).toBe('running 1,234 / 10,570');
  });
});

describe('groupRows', () => {
  it('runs group by the recorded name, then the hash matches, then the short hash', () => {
    const groups = groupRows([
      row('1', { pipeline: hex('a'), launchedAs: 'hybrid', pipelineNames: ['hybrid-fork'] }),
      row('2', { pipeline: hex('c'), pipelineNames: ['dense', 'dense-copy'] }),
      row('3', { pipeline: hex('e') }),
      // Launched as `hybrid` too, of content no current document holds: the
      // recorded name groups it, whatever its hash.
      row('4', { pipeline: hex('f'), launchedAs: 'hybrid' }),
    ]);
    expect(groups.map((g) => [g.names, ids(g.rows)])).toEqual([
      [['hybrid'], ['1', '4']],
      [['dense', 'dense-copy'], ['2']],
      [[], ['3']],
    ]);
    expect(groups[2]).toMatchObject({ pipeline: hex('e'), shapeKey: hex('e') });
  });

  it('says whether the group’s recorded name is held: as its rows’ records say, and held when a hash match names it', () => {
    const groups = groupRows([
      row('1', { pipeline: hex('a'), launchedAs: 'hybrid-old', launchedHeld: 'gone' }),
      row('2', { pipeline: hex('c'), launchedAs: 'Hybrid', launchedHeld: 'other_case' }),
      row('3', { pipeline: hex('e'), launchedAs: 'dense', launchedHeld: 'exactly' }),
      row('4', { pipeline: hex('f'), pipelineNames: ['bm25', 'bm25-copy'] }),
      row('5', { pipeline: hex('9') }),
    ]);
    expect(groups.map((g) => [g.names, g.held])).toEqual([
      [['hybrid-old'], 'gone'],
      [['Hybrid'], 'other_case'],
      [['dense'], 'exactly'],
      // Hash matches are current documents, from the same listing.
      [['bm25', 'bm25-copy'], 'exactly'],
      [[], 'exactly'],
    ]);
  });

  it('puts a run without a record whose one hash match is a recorded name in that name’s group', () => {
    const groups = groupRows([row('1', { pipeline: hex('a'), launchedAs: 'hybrid' }), row('2', { pipeline: hex('a'), pipelineNames: ['hybrid'] })]);
    expect(groups.map((g) => [g.names, ids(g.rows)])).toEqual([[['hybrid'], ['1', '2']]]);
  });

  it('keeps the order each group first appears in, every run in its order', () => {
    const groups = groupRows([
      row('1', { pipeline: hex('a'), pipelineNames: ['hybrid', 'hybrid-copy'] }),
      row('2', { pipeline: hex('c'), pipelineNames: ['dense'] }),
      row('3', { pipeline: hex('a'), pipelineNames: ['hybrid', 'hybrid-copy'] }),
    ]);
    expect(groups.map((g) => [g.names, ids(g.rows)])).toEqual([
      [['hybrid', 'hybrid-copy'], ['1', '3']],
      [['dense'], ['2']],
    ]);
  });

  it('groups runs no name reaches by canonical hash, naming no pipeline', () => {
    const groups = groupRows([row('1', { pipeline: hex('a') }), row('2', { pipeline: hex('a') }), row('3', { pipeline: hex('c') })]);
    expect(groups.map((g) => [g.pipeline, g.names, g.rows.length])).toEqual([
      [hex('a'), [], 2],
      [hex('c'), [], 1],
    ]);
  });

  it('a prefix run sits in its parent’s group', () => {
    const groups = groupRows([
      row('1', { pipeline: hex('a'), launchedAs: 'hybrid' }),
      row('2', { pipeline: hex('f'), launchedAs: 'hybrid', prefix: { parent: 'hybrid', upTo: 'rerank' } }),
    ]);
    expect(groups.map((g) => [g.names, ids(g.rows)])).toEqual([[['hybrid'], ['1', '2']]]);
  });

  it('gives a prefix run whose parent has no run here a group under the parent’s name, so it is still shown, with no shape', () => {
    const groups = groupRows([row('2', { pipeline: hex('f'), launchedAs: 'hybrid', prefix: { parent: 'hybrid', upTo: 'rerank' } })]);
    expect(groups.map((g) => [g.names, ids(g.rows), g.shapeKey])).toEqual([[['hybrid'], ['2'], null]]);
  });

  it('takes a group’s pipeline — for its shape and its link — from its most recent run of its own, not a prefix', () => {
    const groups = groupRows([
      row('2', { pipeline: hex('f'), launchedAs: 'hybrid', prefix: { parent: 'hybrid', upTo: 'rerank' } }),
      row('1', { pipeline: hex('a'), launchedAs: 'hybrid' }),
      row('3', { pipeline: hex('c'), launchedAs: 'hybrid' }),
    ]);
    expect(groups[0]).toMatchObject({ names: ['hybrid'], pipeline: hex('a'), shapeKey: hex('a') });
  });

  it('keys groups apart that a name and a hash could otherwise share', () => {
    // A document named like a hash is a name, never that hash.
    const groups = groupRows([row('1', { pipeline: hex('a'), launchedAs: hex('a') }), row('2', { pipeline: hex('a') })]);
    expect(groups).toHaveLength(2);
    expect(new Set(groups.map((g) => g.key)).size).toBe(2);
  });
});

describe('the other fact', () => {
  it('the other fact is a secondary label', () => {
    // Under a recorded name: the current documents holding the run's content.
    expect(otherFact(row('1', { launchedAs: 'hybrid', pipelineNames: ['hybrid', 'hybrid-fork'] }))).toBe('content held by hybrid, hybrid-fork');
    expect(otherFact(row('1', { launchedAs: 'hybrid' }))).toBe('no current document has this content');
    // Under the hash matches, or the hash: that no launch was recorded.
    expect(otherFact(row('1', { pipelineNames: ['hybrid'] }))).toBe('launch not recorded');
    expect(otherFact(row('1'))).toBe('launch not recorded');
  });

  it('says a record without a name was recorded, never that nothing was', () => {
    expect(otherFact(row('1', { launchRecorded: true, pipelineNames: ['hybrid'] }))).toBe('launch recorded without a name');
  });

  it('draws no secondary label when the only document holding the content is the recorded name', () => {
    expect(otherFact(row('1', { launchedAs: 'hybrid', launchRecorded: true, pipelineNames: ['hybrid'] }))).toBeNull();
  });

  it('reads whether the run has a launch record at all', () => {
    const rows = rowsFromListing(listingOf(summary(hex('1'), { launched_as: { name: null, prefix_of: null, held: null } }), summary(hex('2'))));
    expect(rows.map((r) => [r.source.id, r.launchedAs, r.launchRecorded])).toEqual([
      [hex('1'), null, true],
      [hex('2'), null, false],
    ]);
  });
});

describe('labels', () => {
  it('shortens a hash to twelve digits', () => {
    expect(shortHash(hex('a'))).toBe('aaaaaaaaaaaa');
  });

  it('names a benchmark by every name pinned to its digest, or by its short dataset digest when it has none', () => {
    expect(benchmarkLabel(row('1', { benchmarkNames: ['beir/scifact'] }))).toBe('beir/scifact');
    expect(benchmarkLabel(row('1', { benchmarkNames: ['beir/scifact', 'scifact-local'] }))).toBe('beir/scifact, scifact-local');
    expect(benchmarkLabel(row('1', { benchmark: hex('d') }))).toBe('dataset dddddddddddd');
  });

  it('formats a ranking metric to four decimals and an answer metric as a percentage to one', () => {
    expect(formatMetric('ranking', 0.54364)).toBe('0.5436');
    expect(formatMetric('answers', 0.33333)).toBe('33.3');
    expect(formatMetric('answers', 0.412)).toBe('41.2');
    expect(formatMetric(null, 0.54364)).toBe('0.5436');
    // A metric of no known family is printed as stored: nothing says how to round it.
    expect(formatMetric('unknown', 0.123456789)).toBe('0.123456789');
  });

  it('names an answer metric as the design writes it, every other by its stored name', () => {
    expect(metricLabel('exact_match')).toBe('EM');
    expect(metricLabel('token_f1')).toBe('F1');
    expect(metricLabel('ndcg@10')).toBe('ndcg@10');
    expect(metricLabel('foo_score')).toBe('foo_score');
  });

  it('writes a latency in whole milliseconds, and to two figures under ten', () => {
    expect(formatLatency(412.4)).toBe('412 ms');
    expect(formatLatency(12.6)).toBe('13 ms');
    expect(formatLatency(4.25)).toBe('4.3 ms');
    expect(formatLatency(0.017249)).toBe('0.017 ms');
  });
});

describe('shapeOf', () => {
  const node = (id: string, family: string) => ({ id, family, implementation: id, parameters: {} });

  it('lists the nodes’ families in pipeline order — inputs before what consumes them — not in id order', () => {
    const graph: Graph = {
      inputs: [{ id: 'question', kind: 'query' }],
      nodes: [node('answer', 'generator'), node('context', 'context_builder'), node('fused', 'fusion'), node('leg_a', 'retriever'), node('leg_b', 'retriever')],
      edges: [
        { from: 'question', to: 'answer', port: 0, kind: 'query' },
        { from: 'context', to: 'answer', port: 1, kind: 'context' },
        { from: 'question', to: 'context', port: 0, kind: 'query' },
        { from: 'fused', to: 'context', port: 1, kind: 'chunks' },
        { from: 'leg_a', to: 'fused', port: 0, kind: 'chunks' },
        { from: 'leg_b', to: 'fused', port: 1, kind: 'chunks' },
        { from: 'question', to: 'leg_a', port: 0, kind: 'query' },
        { from: 'question', to: 'leg_b', port: 0, kind: 'query' },
      ],
    };
    expect(shapeOf(graph)).toEqual([
      { node: 'leg_a', family: 'retriever' },
      { node: 'leg_b', family: 'retriever' },
      { node: 'fused', family: 'fusion' },
      { node: 'context', family: 'context' },
      { node: 'answer', family: 'generator' },
    ]);
  });

  it('keeps a family the design system draws no tile for as its own word', () => {
    const graph: Graph = { inputs: [], nodes: [node('x', 'extension')], edges: [] };
    expect(shapeOf(graph)).toEqual([{ node: 'x', family: null, word: 'extension' }]);
  });
});

import { describe, expect, it } from 'vitest';
import type { Graph, RunListing } from '../api/types.ts';
import { benchmarkLabel, formatMetric, groupRows, openRoute, rowKey, rowsFromListing, runningLabel, shapeOf, shortHash, type RunRow } from './model.ts';

const hex = (c: string) => c.repeat(64);

const row = (id: string, over: Partial<RunRow> = {}): RunRow => ({
  source: { kind: 'run', id },
  pipeline: hex('p'),
  pipelineName: null,
  benchmark: hex('b'),
  benchmarkName: null,
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  ...over,
});

const ids = (rows: RunRow[]) => rows.map((r) => r.source.id);

describe('rowsFromListing', () => {
  it('reads every summary as a done run row keyed by its pipeline hash and its dataset version, in listing order', () => {
    const listing: RunListing = {
      runs: [
        { id: hex('1'), pipeline: hex('a'), dataset_version: hex('d'), index_version: hex('i'), engine_version: '0.0.0', metrics: { 'ndcg@10': 0.5, mrr: 0.25 } },
        { id: hex('2'), pipeline: hex('c'), dataset_version: hex('e'), index_version: hex('i'), engine_version: '0.0.0', metrics: {} },
      ],
      unreadable: [],
    };
    const rows = rowsFromListing(listing);
    expect(rows.map((r) => [r.source, r.pipeline, r.benchmark, r.status.state])).toEqual([
      [{ kind: 'run', id: hex('1') }, hex('a'), hex('d'), 'done'],
      [{ kind: 'run', id: hex('2') }, hex('c'), hex('e'), 'done'],
    ]);
  });

  it('keeps the metrics as the run recorded them, in one group whose family the listing does not say', () => {
    const listing: RunListing = {
      runs: [{ id: hex('1'), pipeline: hex('a'), dataset_version: hex('d'), index_version: hex('i'), engine_version: '0', metrics: { mrr: 0.25, 'ndcg@10': 0.5 } }],
      unreadable: [],
    };
    expect(rowsFromListing(listing)[0]?.metrics).toEqual([
      { family: null, metrics: [{ name: 'mrr', value: 0.25 }, { name: 'ndcg@10', value: 0.5 }] },
    ]);
  });

  it('gives a run that recorded no metric no group at all, so no empty cell is drawn', () => {
    const listing: RunListing = {
      runs: [{ id: hex('1'), pipeline: hex('a'), dataset_version: hex('d'), index_version: hex('i'), engine_version: '0', metrics: {} }],
      unreadable: [],
    };
    expect(rowsFromListing(listing)[0]?.metrics).toEqual([]);
  });

  it('leaves what the listing does not carry empty rather than invented', () => {
    const listing: RunListing = {
      runs: [{ id: hex('1'), pipeline: hex('a'), dataset_version: hex('d'), index_version: hex('i'), engine_version: '0', metrics: {} }],
      unreadable: [],
    };
    expect(rowsFromListing(listing)[0]).toMatchObject({ pipelineName: null, benchmarkName: null, latencyMs: null, startedAt: null, prefix: null });
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
  it('groups by pipeline name when there is one, keeping the order each group first appears in', () => {
    const groups = groupRows([
      row('1', { pipeline: hex('a'), pipelineName: 'hybrid' }),
      row('2', { pipeline: hex('c'), pipelineName: 'dense' }),
      row('3', { pipeline: hex('a'), pipelineName: 'hybrid' }),
    ]);
    expect(groups.map((g) => [g.key, g.name, ids(g.rows)])).toEqual([
      ['hybrid', 'hybrid', ['1', '3']],
      ['dense', 'dense', ['2']],
    ]);
  });

  it('groups by canonical hash when the pipeline has no name', () => {
    const groups = groupRows([row('1', { pipeline: hex('a') }), row('2', { pipeline: hex('a') }), row('3', { pipeline: hex('c') })]);
    expect(groups.map((g) => [g.key, g.name, g.rows.length])).toEqual([
      [hex('a'), null, 2],
      [hex('c'), null, 1],
    ]);
  });

  it('puts a prefix run inside its parent pipeline’s group when the parent is named by its name', () => {
    const groups = groupRows([row('1', { pipeline: hex('a'), pipelineName: 'hybrid' }), row('2', { pipeline: hex('f'), prefix: { parent: 'hybrid', upTo: 'rerank' } })]);
    expect(groups).toHaveLength(1);
    expect(ids(groups[0]?.rows ?? [])).toEqual(['1', '2']);
  });

  it('puts a prefix run inside its parent pipeline’s group when the parent is named by its hash, though the group is keyed by its name', () => {
    const groups = groupRows([row('1', { pipeline: hex('a'), pipelineName: 'hybrid' }), row('2', { pipeline: hex('f'), prefix: { parent: hex('a'), upTo: 'rerank' } })]);
    expect(groups.map((g) => [g.key, ids(g.rows)])).toEqual([['hybrid', ['1', '2']]]);
  });

  it('puts a prefix run inside the group of the run its parent is named by', () => {
    const groups = groupRows([row('r1', { pipeline: hex('a') }), row('2', { pipeline: hex('f'), prefix: { parent: 'r1', upTo: null } })]);
    expect(groups.map((g) => ids(g.rows))).toEqual([['r1', '2']]);
  });

  it('gives a prefix run whose parent has no run here a group of the parent’s own, so it is still shown', () => {
    const groups = groupRows([row('2', { pipeline: hex('f'), prefix: { parent: 'hybrid', upTo: 'rerank' } })]);
    expect(groups.map((g) => [g.key, g.name, ids(g.rows), g.shapeFrom])).toEqual([['hybrid', null, ['2'], null]]);
  });

  it('takes a group’s pipeline — for its shape and its link — from a run of its own, not a prefix and not a job', () => {
    const groups = groupRows([
      row('2', { pipeline: hex('f'), prefix: { parent: 'hybrid', upTo: 'rerank' } }),
      row('j', { pipeline: hex('a'), pipelineName: 'hybrid', source: { kind: 'job', id: 'j', runId: null }, status: { state: 'queued' } }),
      row('1', { pipeline: hex('a'), pipelineName: 'hybrid' }),
    ]);
    expect(groups[0]).toMatchObject({ key: 'hybrid', name: 'hybrid', pipeline: hex('a'), shapeFrom: '1' });
  });
});

describe('labels', () => {
  it('shortens a hash to twelve digits', () => {
    expect(shortHash(hex('a'))).toBe('aaaaaaaaaaaa');
  });

  it('names a benchmark by its name, or by its short dataset digest when it has none', () => {
    expect(benchmarkLabel(row('1', { benchmarkName: 'beir/scifact' }))).toBe('beir/scifact');
    expect(benchmarkLabel(row('1', { benchmark: hex('d') }))).toBe('dataset dddddddddddd');
  });

  it('formats a ranking metric to four decimals and an answer metric as a percentage to one', () => {
    expect(formatMetric('ranking', 0.54364)).toBe('0.5436');
    expect(formatMetric('answers', 0.33333)).toBe('33.3');
    expect(formatMetric(null, 0.54364)).toBe('0.5436');
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

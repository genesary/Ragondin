import { describe, expect, it } from 'vitest';
import type { Jobs } from '../api/jobs.ts';
import type { JobSummary, RunListing, RunSummary } from '../api/types.ts';
import { CANCELLED, doneAs, failedAt, hex, QUEUED, runJob, running } from '../jobs/fixtures.ts';
import { groupRows, rowsFromListing } from './model.ts';
import { queuedOrder, rowsFromJobs, withAnnounced } from './jobRows.ts';

const SCIFACT = hex('5');
const jobs = (...list: JobSummary[]): Jobs => new Map(list.map((j) => [j.id, j]));

const run = (id: string, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline: hex('c'),
  pipeline_names: ['hybrid'],
  refused_pipeline_names: [], prefix_of_documents: [],
  launched_as: { name: 'hybrid', held: 'exactly', prefix_of: null },
  dataset_version: SCIFACT,
  benchmark_names: ['beir/scifact'],
  index_version: hex('0'),
  engine_version: '0.0.0',
  started_at_ms: 1,
  finished_at_ms: 2,
  metrics: {},
  metric_families: {},
  median_query_latency_nanos: null,
  ...over,
});
const listing = (...runs: RunSummary[]): RunListing => ({ runs, unreadable: [], shapes: {} });

describe('rowsFromJobs', () => {
  it('leaves a dismissed job out', () => {
    const dismissed = { ...runJob('f', failedAt('rerank', 'boom')), dismissed_at_ms: 1_700_000_200_000 };
    const rows = rowsFromJobs(jobs(dismissed, runJob('c', CANCELLED, { runId: hex('b') })), []);
    expect(rows.map((r) => r.source.id)).toEqual(['c']);
  });

  it('draws a queued and a running run job as rows of the job queue, live work first, the queued in the order the worker takes them', () => {
    const rows = rowsFromJobs(jobs(runJob('q2', QUEUED, { position: 5 }), runJob('q1', QUEUED, { position: 2 }), runJob('r', running(3, 10, 2_500_000))), []);
    expect(rows.map((r) => r.source.id)).toEqual(['r', 'q1', 'q2']);
    expect(rows[0]?.status).toEqual({ state: 'running', done: 3, total: 10 });
    // A job's live figures are its row's own, never the store's columns, so a first tick adds no column to the table.
    expect(rows[0]?.job?.medianMs).toBe(2.5);
    expect(rows[0]?.latencyMs).toBeNull();
    expect(rows[0]?.startedAt).toBeNull();
    expect(rows[0]?.job?.startedAtMs).toBe(1_700_000_000_000);
    expect(rows[1]?.status).toEqual({ state: 'queued' });
    expect(rows[1]?.job?.place).toBe(0);
    expect(rows[2]?.job?.place).toBe(1);
  });

  it('says a running job whose total is not known yet as such, never as a fraction of nothing', () => {
    const [row] = rowsFromJobs(jobs(runJob('r', running(0, null))), []);
    expect(row?.status).toEqual({ state: 'running', done: 0, total: null });
  });

  it('carries the announced id and the submission, so a row can be resubmitted as it was', () => {
    const [row] = rowsFromJobs(jobs(runJob('c', CANCELLED, { pipeline: 'dense', benchmark: 'beir/fiqa', runId: hex('b'), upTo: 'rrf' })), []);
    expect(row?.source).toEqual({ kind: 'job', id: 'c', runId: hex('b') });
    expect(row?.job?.submission).toEqual({ pipeline: 'dense', benchmark: 'beir/fiqa', up_to: 'rrf' });
    expect(row?.prefix).toEqual({ parents: ['dense'], upTo: 'rrf' });
    expect(row?.status).toEqual({ state: 'cancelled' });
  });

  it('carries the reasons of the faults the queue reported beside the job, in their order', () => {
    const [row] = rowsFromJobs(jobs(runJob('r', running(3, 10), { faults: ['first', 'second'] })), []);
    expect(row?.job?.faults).toEqual(['first', 'second']);
    expect(row?.status).toEqual({ state: 'running', done: 3, total: 10 });
  });

  it('names the failed node and the error', () => {
    const [row] = rowsFromJobs(jobs(runJob('f', failedAt('rerank', 'the reranker answered 503'))), []);
    expect(row?.status).toEqual({ state: 'failed', node: 'rerank', error: 'the reranker answered 503' });
  });

  it('is grouped under the pipeline it was launched as, beside the runs launched as it', () => {
    const runRows = rowsFromListing(listing(run(hex('1'))));
    const groups = groupRows([...rowsFromJobs(jobs(runJob('r', running(1, 10))), runRows), ...runRows]);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.names).toEqual(['hybrid']);
    // The heading is the runs' own: their hash draws the shape, their record says the name is held.
    expect(groups[0]?.pipeline).toBe(hex('c'));
    expect(groups[0]?.held).toEqual(['exactly']);
  });

  it('labels the benchmark as the store does when a run there pins the same name, so one filter chip holds both', () => {
    const runRows = rowsFromListing(listing(run(hex('1'))));
    const [row] = rowsFromJobs(jobs(runJob('r', running(1, 10))), runRows);
    expect(row?.benchmark).toBe(SCIFACT);
    expect(row?.benchmarkNames).toEqual(['beir/scifact']);
  });

  it('keeps a done job until the store holds the run it filed, then hands over to the run row', () => {
    const done = jobs(runJob('d', doneAs(hex('1'))));
    expect(rowsFromJobs(done, []).map((r) => r.status.state)).toEqual(['done']);
    expect(rowsFromJobs(done, rowsFromListing(listing(run(hex('1')))))).toEqual([]);
  });

  it('drops an ended job that a later submission of the same identity, or the store, has taken over', () => {
    const relaunched = jobs(runJob('c', CANCELLED, { runId: hex('1') }), runJob('q', QUEUED, { runId: hex('1') }));
    expect(rowsFromJobs(relaunched, []).map((r) => r.source.id)).toEqual(['q']);
    const filed = jobs(runJob('f', failedAt(null, 'interrupted'), { runId: hex('1') }));
    expect(rowsFromJobs(filed, rowsFromListing(listing(run(hex('1')))))).toEqual([]);
  });

  it('draws no download', () => {
    const download: JobSummary = { id: 'd', created_at_ms: 1, position: 0, state: QUEUED, work: { kind: 'download', benchmark: 'beir/fiqa' }, dismissed_at_ms: null, faults: [] };
    expect(rowsFromJobs(jobs(download), [])).toEqual([]);
  });
});

describe('queuedOrder', () => {
  it('is the queued run jobs in the order the worker takes them', () => {
    expect(queuedOrder(jobs(runJob('b', QUEUED, { position: 3 }), runJob('a', QUEUED, { position: 1 }), runJob('r', running(1, 2))))).toEqual(['a', 'b']);
  });
});

describe('withAnnounced', () => {
  it('gives a run the id its job announced, when the job reports it filed the run under another', () => {
    const runRows = rowsFromListing(listing(run(hex('d')), run(hex('1'))));
    const marked = withAnnounced(runRows, jobs(runJob('j', doneAs(hex('d'), { announced: hex('a'), decided: hex('d') }))));
    expect(marked.map((r) => [r.source.id, r.announced])).toEqual([
      [hex('1'), null],
      [hex('d'), hex('a')],
    ]);
  });
});

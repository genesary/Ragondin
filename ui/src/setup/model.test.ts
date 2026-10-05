import { describe, expect, it } from 'vitest';
import type { ApiProblem } from '../api/client.ts';
import type { BenchmarkEntry, JobSummary } from '../api/types.ts';
import { downloadView, finishedDownloads, formatSize, groundTruthLabel, isFirstLaunch, shortDigest, smallestAvailable, splitBuild } from './model.ts';

const entry = (name: string, state: BenchmarkEntry['state']): BenchmarkEntry => ({ name, format: 'beir', ground_truth: null, licence: null, licence_url: null, state });

describe('formatSize', () => {
  it.each([
    [512, '512 B'],
    [5_200_000, '5.2 MB'],
    [17_149_999, '17.1 MB'],
    [148_000_000, '148 MB'],
    [2_340_000_000, '2.3 GB'],
    [1_000, '1 kB'],
  ])('writes %d bytes as %s, in decimal units', (bytes, text) => {
    expect(formatSize(bytes)).toBe(text);
  });
});

describe('shortDigest', () => {
  it('is the first twelve characters, as the rest of the UI shortens a hash', () => {
    expect(shortDigest('0123456789abcdef')).toBe('0123456789ab');
  });
});

describe('groundTruthLabel', () => {
  it('says what a benchmark carries in words', () => {
    expect(groundTruthLabel('qrels')).toBe('qrels');
    expect(groundTruthLabel('reference_answers')).toBe('reference answers');
    expect(groundTruthLabel('both')).toBe('qrels and reference answers');
    expect(groundTruthLabel('none')).toBe('no ground truth');
  });
});

describe('isFirstLaunch', () => {
  it('is a workspace with no benchmark on disk and no service', () => {
    expect(isFirstLaunch([], [])).toBe(true);
    // What the manifest offers is not on disk yet.
    expect(isFirstLaunch([entry('beir/scifact', { kind: 'available', size_bytes: 1 })], [])).toBe(true);
  });

  it('is over once a benchmark is on disk in any state, or a service is bound', () => {
    expect(isFirstLaunch([entry('a', { kind: 'ready', dataset_version: 'x' })], [])).toBe(false);
    expect(isFirstLaunch([entry('a', { kind: 'differs', expected: 'x', found: 'y' })], [])).toBe(false);
    expect(isFirstLaunch([], [{ family: 'generator', name: 'qwen', uri: 'u', connected: false, identity: null }])).toBe(false);
  });
});

describe('smallestAvailable', () => {
  it('is the available benchmark with the fewest bytes, or none', () => {
    const big = entry('big', { kind: 'available', size_bytes: 9 });
    const small = entry('small', { kind: 'available', size_bytes: 3 });
    expect(smallestAvailable([big, entry('ready', { kind: 'ready', dataset_version: 'x' }), small])).toBe(small);
    expect(smallestAvailable([entry('ready', { kind: 'ready', dataset_version: 'x' })])).toBeNull();
  });
});

describe('splitBuild', () => {
  it('reads the version and the commit out of the build identity', () => {
    expect(splitBuild('0.1.0+aaaaaaaaaaaa')).toEqual({ version: '0.1.0', commit: 'aaaaaaaaaaaa' });
    expect(splitBuild('0.1.0')).toEqual({ version: '0.1.0', commit: null });
  });
});

const job = (id: string, state: JobSummary['state'], benchmark = 'beir/fiqa'): JobSummary => ({ id, created_at_ms: 1, position: 0, state, work: { kind: 'download', benchmark }, dismissed_at_ms: null, faults: [] });
const jobs = (...list: JobSummary[]) => new Map(list.map((j) => [j.id, j]));
const RUNNING: JobSummary['state'] = { kind: 'running', done: 5, total: 10, started_at_ms: 1, median_latency_nanos: null };
const FAILED: JobSummary['state'] = { kind: 'failed', error: 'reset', at_node: null, finished_at_ms: 2, partial_traces: 0 };
const DONE: JobSummary['state'] = { kind: 'done', run_id: null, id_mismatch: null, finished_at_ms: 2 };
const REFUSAL: ApiProblem = { code: 'backend_failed', message: 'disk full', hint: 'Free space.', location: null, status: 500 };

describe('downloadView', () => {
  it('is idle with no submission and no job of the benchmark', () => {
    expect(downloadView('beir/fiqa', undefined, jobs(job('1-0', RUNNING, 'beir/scifact')))).toEqual({ kind: 'idle' });
  });

  it('follows the submission until its job is known: asking, refused, then queued', () => {
    expect(downloadView('beir/fiqa', { kind: 'submitting' }, jobs())).toEqual({ kind: 'submitting' });
    expect(downloadView('beir/fiqa', { kind: 'refused', problem: REFUSAL }, jobs())).toEqual({ kind: 'refused', problem: REFUSAL });
    expect(downloadView('beir/fiqa', { kind: 'accepted', jobId: '2-0' }, jobs())).toEqual({ kind: 'queued' });
  });

  it('reads the submitted job, never an older one of the same benchmark', () => {
    const older = job('1-0', FAILED);
    expect(downloadView('beir/fiqa', { kind: 'accepted', jobId: '2-0' }, jobs(older, job('2-0', RUNNING)))).toEqual({ kind: 'running', done: 5, total: 10 });
    expect(downloadView('beir/fiqa', { kind: 'accepted', jobId: '2-0' }, jobs(older))).toEqual({ kind: 'queued' });
  });

  it('a submitted job that is done is being verified until the listing is read again', () => {
    expect(downloadView('beir/fiqa', { kind: 'accepted', jobId: '2-0' }, jobs(job('2-0', DONE)))).toEqual({ kind: 'verifying' });
  });

  it('without a submission, reads the benchmark’s last job — one started elsewhere, or before this page — but a done one, which the listing already says', () => {
    expect(downloadView('beir/fiqa', undefined, jobs(job('1-0', FAILED), job('2-0', RUNNING)))).toEqual({ kind: 'running', done: 5, total: 10 });
    expect(downloadView('beir/fiqa', undefined, jobs(job('1-0', RUNNING), job('2-0', FAILED)))).toEqual({ kind: 'failed', error: 'reset' });
    expect(downloadView('beir/fiqa', undefined, jobs(job('1-0', { kind: 'cancelled', finished_at_ms: 2, partial_traces: 0 })))).toEqual({ kind: 'cancelled' });
    expect(downloadView('beir/fiqa', undefined, jobs(job('1-0', DONE)))).toEqual({ kind: 'idle' });
  });

  it('ignores a run of the same benchmark', () => {
    const run: JobSummary = { ...job('1-0', RUNNING), work: { kind: 'run', benchmark: 'beir/fiqa', bindings: [], pipeline: 'p', run_id: 'r', up_to: null, parent_pipeline_hash: null } };
    expect(downloadView('beir/fiqa', undefined, jobs(run))).toEqual({ kind: 'idle' });
  });
});

describe('finishedDownloads', () => {
  it('names each benchmark whose download ended done between two readings of the queue', () => {
    const before = jobs(job('1-0', RUNNING), job('2-0', RUNNING, 'beir/scifact'), job('3-0', DONE, 'mine'));
    const after = jobs(job('1-0', DONE), job('2-0', FAILED, 'beir/scifact'), job('3-0', DONE, 'mine'));
    expect(finishedDownloads(before, after, new Set())).toEqual(['beir/fiqa']);
  });

  it('counts a job done at its first sight only when this page submitted it: an old done job is not news', () => {
    const after = jobs(job('1-0', DONE), job('2-0', DONE, 'beir/scifact'));
    expect(finishedDownloads(jobs(), after, new Set(['2-0']))).toEqual(['beir/scifact']);
  });
});

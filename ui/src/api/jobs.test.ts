import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { applyJobEvent, openJobStream, type Jobs } from './jobs.ts';
import { FakeEventSource, installFakeEventSource } from './testing.ts';
import type { JobEvent, JobSummary } from './types.ts';

const job = (id: string, state: JobSummary['state'], benchmark = 'beir/scifact'): JobSummary => ({
  id,
  created_at_ms: 1,
  position: 0,
  state,
  work: { kind: 'download', benchmark },
});

const QUEUED = job('1-0', { kind: 'queued' });
const RUNNING = job('1-0', { kind: 'running', done: 400, total: 1000, started_at_ms: 2, median_latency_nanos: null });

beforeEach(() => {
  installFakeEventSource();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('openJobStream', () => {
  const open = () => {
    const events: JobEvent[] = [];
    const stream = openJobStream({ onEvent: (e) => events.push(e) });
    return { events, stream };
  };

  it('follows GET /jobs/events on the relative base address', () => {
    open();
    expect(FakeEventSource.latest().url).toBe('/api/v1/jobs/events');
  });

  it('hands on every event the description names, typed by JobEvent', () => {
    const { events } = open();
    const source = FakeEventSource.latest();
    source.open();
    source.emit(JSON.stringify({ jobs: [QUEUED], faults: [] }), 'resync');
    for (const name of ['queued', 'running', 'done', 'failed', 'cancelled', 'reordered'] as const) source.emit(JSON.stringify(QUEUED), name);
    expect(events.map((e) => e.event)).toEqual(['resync', 'queued', 'running', 'done', 'failed', 'cancelled', 'reordered']);
    expect(events[0]).toEqual({ event: 'resync', data: { jobs: [QUEUED], faults: [] } });
    expect(events[1]).toEqual({ event: 'queued', data: QUEUED });
  });

  it('an event it cannot read is never acted on: the stream starts again, and so begins with resync', () => {
    const { events } = open();
    FakeEventSource.latest().open();
    FakeEventSource.latest().emit('not json', 'running');
    FakeEventSource.latest().emit(JSON.stringify({ id: '1-0' }), 'running');
    FakeEventSource.latest().emit(JSON.stringify({ faults: [] }), 'resync');
    expect(events).toEqual([]);
    expect(FakeEventSource.instances[0]?.closed).toBe(true);
    vi.advanceTimersByTime(1000);
    expect(FakeEventSource.instances).toHaveLength(2);
  });

  it.each([
    ['a state kind the description does not give', { ...QUEUED, state: { kind: 'paused' } }],
    ['a work kind the description does not give', { ...QUEUED, work: { kind: 'upload', benchmark: 'beir/scifact' } }],
    ['a running count that is not a number', { ...RUNNING, state: { ...RUNNING.state, done: '400' } }],
    ['a running total that is neither a number nor null', { ...RUNNING, state: { ...RUNNING.state, total: '1000' } }],
    ['a failure without its error', { ...QUEUED, state: { kind: 'failed', at_node: null, finished_at_ms: 3, partial_traces: 0 } }],
    ['a failure without the count of the traces it kept', { ...QUEUED, state: { kind: 'failed', error: 'boom', at_node: null, finished_at_ms: 3 } }],
    ['a cancellation whose count is not a number', { ...QUEUED, state: { kind: 'cancelled', finished_at_ms: 3, partial_traces: '2' } }],
    ['a download without its benchmark', { ...QUEUED, work: { kind: 'download' } }],
  ])('treats %s as unreadable: never handed on, and the stream starts again', (_, data) => {
    const { events } = open();
    FakeEventSource.latest().open();
    FakeEventSource.latest().emit(JSON.stringify(data), 'running');
    FakeEventSource.latest().emit(JSON.stringify({ jobs: [data], faults: [] }), 'resync');
    expect(events).toEqual([]);
    expect(FakeEventSource.instances[0]?.closed).toBe(true);
  });

  it('reports its connection state, and every reconnection after the first', () => {
    const states: string[] = [];
    const onReconnect = vi.fn();
    openJobStream({ onEvent: () => {}, onState: (s) => states.push(s), onReconnect });
    const source = FakeEventSource.latest();
    source.open();
    expect(onReconnect).not.toHaveBeenCalled();
    source.fail();
    source.open();
    expect(states).toEqual(['connecting', 'connected', 'disconnected', 'connected']);
    expect(onReconnect).toHaveBeenCalledTimes(1);
  });

  it('closes the stream', () => {
    const { stream } = open();
    stream.close();
    expect(FakeEventSource.latest().closed).toBe(true);
  });
});

describe('applyJobEvent', () => {
  const none: Jobs = new Map();

  it('resync replaces every job with the listing, in its order', () => {
    const stale = applyJobEvent(none, { event: 'queued', data: job('0-0', { kind: 'queued' }) });
    const other = job('2-0', { kind: 'queued' }, 'beir/fiqa');
    const jobs = applyJobEvent(stale, { event: 'resync', data: { jobs: [RUNNING, other], faults: [] } });
    expect([...jobs.keys()]).toEqual(['1-0', '2-0']);
    expect(jobs.get('1-0')).toEqual(RUNNING);
  });

  it('any other event sets its job, a new one last, a known one in its place', () => {
    const other = job('2-0', { kind: 'queued' }, 'beir/fiqa');
    let jobs = applyJobEvent(none, { event: 'queued', data: QUEUED });
    jobs = applyJobEvent(jobs, { event: 'queued', data: other });
    jobs = applyJobEvent(jobs, { event: 'running', data: RUNNING });
    expect([...jobs.keys()]).toEqual(['1-0', '2-0']);
    expect(jobs.get('1-0')).toEqual(RUNNING);
  });

  it('never changes the map it is given', () => {
    const before = applyJobEvent(none, { event: 'queued', data: QUEUED });
    applyJobEvent(before, { event: 'running', data: RUNNING });
    expect(before.get('1-0')).toEqual(QUEUED);
  });
});

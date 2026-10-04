// Jobs of the queue, as the stream sends them, for the tests of the screens
// that follow it. Only tests import this file.
import { act } from '@testing-library/react';
import type { JobEvent, JobStatus, JobSummary } from '../api/types.ts';

export const hex = (c: string) => c.repeat(64);

/** A run job: `pipeline` on `benchmark`, announcing `runId`. */
export function runJob(id: string, state: JobStatus, { pipeline = 'hybrid', benchmark = 'beir/scifact', runId = hex('a'), position = 0, upTo = null }: { pipeline?: string; benchmark?: string; runId?: string; position?: number; upTo?: string | null } = {}): JobSummary {
  return {
    id,
    created_at_ms: 1_700_000_000_000,
    position,
    state,
    work: { kind: 'run', pipeline, benchmark, run_id: runId, up_to: upTo, bindings: [] },
  };
}

export const QUEUED: JobStatus = { kind: 'queued' };

export const running = (done: number, total: number | null, median: number | null = null, started = 1_700_000_000_000): JobStatus => ({
  kind: 'running',
  done,
  total,
  median_latency_nanos: median,
  started_at_ms: started,
});

export const doneAs = (runId: string, mismatch: { announced: string; decided: string } | null = null): JobStatus => ({
  kind: 'done',
  run_id: runId,
  id_mismatch: mismatch,
  finished_at_ms: 1_700_000_100_000,
});

/** A failure at `node`, having kept the traces of `partial` queries. */
export const failedAt = (node: string | null, error: string, partial = 0): JobStatus => ({ kind: 'failed', at_node: node, error, finished_at_ms: 1_700_000_100_000, partial_traces: partial });

export const CANCELLED: JobStatus = { kind: 'cancelled', finished_at_ms: 1_700_000_100_000, partial_traces: 0 };

/** The part of a test's fake event stream these helpers drive: `FakeEventSource.latest()`, passed in. */
export type Stream = { open(): void; emit(data: string, name?: string): void };

/** Sends one event on `stream`, inside `act`. */
export function send(stream: Stream, event: JobEvent) {
  act(() => stream.emit(JSON.stringify(event.data), event.event));
}

/** Opens `stream` and sends the queue it begins with. */
export function connect(stream: Stream, jobs: JobSummary[] = []) {
  act(() => stream.open());
  send(stream, { event: 'resync', data: { jobs, faults: [] } });
}

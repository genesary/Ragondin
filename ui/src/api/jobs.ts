// The job queue's event stream, `GET /jobs/events`, read into the generated
// `JobEvent` type, and the queue a screen holds from it. ARCHITECTURE.md
// § The job stream.
import { openEvents, type ConnectionState, type EventStream } from './events.ts';
import type { JobEvent, JobStatus, JobSummary, JobWork } from './types.ts';

/** The jobs a screen knows, by id, in the queue's order. */
export type Jobs = ReadonlyMap<string, JobSummary>;

// Every event name the description gives the stream, listed once: a record
// over the generated union, so a name added to `JobEvent` does not compile
// until it is here.
const NAMES: Record<JobEvent['event'], true> = { queued: true, running: true, done: true, failed: true, cancelled: true, reordered: true, fault: true, dismissed: true, resync: true };
const isName = (name: string): name is JobEvent['event'] => Object.hasOwn(NAMES, name);

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const isCount = (value: unknown) => typeof value === 'number' && Number.isFinite(value);

// What a screen reads of each kind, one entry per kind of the generated
// unions: a kind added to the description does not compile until it is
// checked here, and a kind the description does not give is unreadable.
const STATES: Record<JobStatus['kind'], (s: Record<string, unknown>) => boolean> = {
  queued: () => true,
  running: (s) => isCount(s.done) && (s.total === null || isCount(s.total)),
  done: () => true,
  failed: (s) => typeof s.error === 'string' && isCount(s.partial_traces),
  cancelled: (s) => isCount(s.partial_traces),
};
const WORKS: Record<JobWork['kind'], (w: Record<string, unknown>) => boolean> = {
  run: (w) => typeof w.benchmark === 'string',
  download: (w) => typeof w.benchmark === 'string',
};
const known = <K extends string>(checks: Record<K, (v: Record<string, unknown>) => boolean>, value: unknown) =>
  isObject(value) && typeof value.kind === 'string' && Object.hasOwn(checks, value.kind) && checks[value.kind as K](value);
const isFaults = (value: unknown) => Array.isArray(value) && value.every((f) => isObject(f) && typeof f.reason === 'string');
const isSummary = (value: unknown) => isObject(value) && typeof value.id === 'string' && known(STATES, value.state) && known(WORKS, value.work) && isFaults(value.faults);

/**
 * One event as `JobEvent`, or null when its data is not JSON or lacks what
 * the screens read: a listing's jobs; a job's id, a state and a work of a
 * kind the description gives, a running job's counts, a failure's error, how
 * many traces a failed or cancelled job kept, a work's benchmark, and the
 * reason of each fault reported beside the job.
 */
function readJobEvent(name: string, data: string): JobEvent | null {
  if (!isName(name)) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    return null;
  }
  if (name === 'resync') return isObject(parsed) && Array.isArray(parsed.jobs) && parsed.jobs.every(isSummary) ? ({ event: name, data: parsed } as JobEvent) : null;
  return isSummary(parsed) ? ({ event: name, data: parsed } as JobEvent) : null;
}

export type JobStreamHandlers = {
  onEvent: (event: JobEvent) => void;
  /** The connection: a screen that shows the queue says when it is not current (the front-end design, § 8). */
  onState?: (state: ConnectionState) => void;
  /** Every connection after the first, so the screen re-checks the build identity, as the shell does for its stream. */
  onReconnect?: () => void;
};

/**
 * Follows the job stream. Every event is handed on as a `JobEvent`, the
 * first one `resync` with the whole queue unless the browser's own retry
 * resumed after the last id it saw. One that does not read is never handed
 * on: the stream starts again, and a new connection begins with `resync`, so
 * no screen acts across the gap.
 */
export function openJobStream({ onEvent, onState, onReconnect }: JobStreamHandlers): EventStream {
  const stream: EventStream = openEvents('/jobs/events', {
    ...(onState === undefined ? {} : { onState }),
    ...(onReconnect === undefined ? {} : { onReconnect }),
    events: {
      names: Object.keys(NAMES),
      on: (name, data) => {
        const event = readJobEvent(name, data);
        if (event === null) stream.restart();
        else onEvent(event);
      },
    },
  });
  return stream;
}

/**
 * The jobs after one event: `resync` replaces them all with the listing;
 * any other sets its job — in its place when known, last when new; a
 * `fault` among them, which carries the job with its faults. The map given
 * is never changed.
 */
export function applyJobEvent(jobs: Jobs, event: JobEvent): Jobs {
  if (event.event === 'resync') return new Map(event.data.jobs.map((j) => [j.id, j]));
  return new Map(jobs).set(event.data.id, event.data);
}

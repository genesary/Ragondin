// The job queue's event stream, `GET /jobs/events`, read into the generated
// `JobEvent` type, and the queue a screen holds from it. ARCHITECTURE.md
// § The job stream.
import { openEvents, type ConnectionState, type EventStream } from './events.ts';
import type { JobEvent, JobSummary } from './types.ts';

/** The jobs a screen knows, by id, in the queue's order. */
export type Jobs = ReadonlyMap<string, JobSummary>;

// Every event name the description gives the stream, listed once: a record
// over the generated union, so a name added to `JobEvent` does not compile
// until it is here.
const NAMES: Record<JobEvent['event'], true> = { queued: true, running: true, done: true, failed: true, cancelled: true, reordered: true, resync: true };
const isName = (name: string): name is JobEvent['event'] => Object.hasOwn(NAMES, name);

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const hasKind = (value: unknown) => isObject(value) && typeof value.kind === 'string';
const isSummary = (value: unknown) => isObject(value) && typeof value.id === 'string' && hasKind(value.state) && hasKind(value.work);

/**
 * One event as `JobEvent`, or null when its data is not JSON or lacks what
 * the screens read: a listing's jobs, a job's id, state and work.
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
  onState?: (state: ConnectionState) => void;
};

/**
 * Follows the job stream. Every event is handed on as a `JobEvent`, the
 * first one `resync` with the whole queue unless the browser's own retry
 * resumed after the last id it saw. One that does not read is never handed
 * on: the stream starts again, and a new connection begins with `resync`, so
 * no screen acts across the gap.
 */
export function openJobStream({ onEvent, onState }: JobStreamHandlers): EventStream {
  const stream: EventStream = openEvents('/jobs/events', {
    ...(onState === undefined ? {} : { onState }),
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
 * any other sets its job — in its place when known, last when new. The map
 * given is never changed.
 */
export function applyJobEvent(jobs: Jobs, event: JobEvent): Jobs {
  if (event.event === 'resync') return new Map(event.data.jobs.map((j) => [j.id, j]));
  return new Map(jobs).set(event.data.id, event.data);
}

// The job queue the page follows: one subscription to `GET /jobs/events`,
// opened by the shell, which every screen and the toasts read. No component
// opens its own stream. ARCHITECTURE.md § The job stream.
import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
import type { ConnectionState } from '../api/events.ts';
import { applyJobEvent, openJobStream, type Jobs } from '../api/jobs.ts';
import type { JobEvent } from '../api/types.ts';

/** A consumer of the events themselves: the queue before and after each, in the order they came. */
export type JobListener = (before: Jobs, after: Jobs, event: JobEvent) => void;

/** What a consumer reads of the queue. */
export type JobQueue = {
  /** The jobs, by id, in the queue's order: the last the stream gave. */
  jobs(): Jobs;
  /** The stream's connection; null when the page follows no stream. */
  connection(): ConnectionState | null;
  /** For `useSyncExternalStore`: called after every change of either. */
  subscribe(onChange: () => void): () => void;
  /** Called on every event, with the queue before and after it. */
  listen(listener: JobListener): () => void;
};

const NONE: Jobs = new Map();

/** The queue of a page that follows no stream: nothing, and no connection to show. */
const IDLE: JobQueue = { jobs: () => NONE, connection: () => null, subscribe: () => () => {}, listen: () => () => {} };

const Context = createContext<JobQueue>(IDLE);

/**
 * The queue a provider holds, made once with it, so every consumer
 * subscribes to the one object from the first render; `start` opens the
 * stream into it and returns what closes it.
 */
function makeQueue(): JobQueue & { start(onReconnect: () => void): () => void } {
  let jobs: Jobs = NONE;
  let connection: ConnectionState = 'connecting';
  const watchers = new Set<() => void>();
  const listeners = new Set<JobListener>();
  const changed = () => {
    for (const watch of watchers) watch();
  };
  return {
    jobs: () => jobs,
    connection: () => connection,
    subscribe: (watch) => {
      watchers.add(watch);
      return () => watchers.delete(watch);
    },
    listen: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    start: (onReconnect) => {
      const stream = openJobStream({
        onEvent: (event) => {
          const before = jobs;
          jobs = applyJobEvent(before, event);
          for (const listener of listeners) listener(before, jobs, event);
          changed();
        },
        onState: (state) => {
          connection = state;
          changed();
        },
        onReconnect,
      });
      return () => stream.close();
    },
  };
}

export type JobQueueProviderProps = {
  /** Every reconnection after the first: the server may be another build now, so the shell re-checks it. */
  onReconnect?: () => void;
  children: ReactNode;
};

/** Follows the job stream while mounted, for everything under it. */
export function JobQueueProvider({ onReconnect, children }: JobQueueProviderProps) {
  const [queue] = useState(makeQueue);
  // Kept in a ref: a new callback from a re-rendering parent is no reason to reopen the stream.
  const reconnect = useRef(onReconnect);
  useEffect(() => {
    reconnect.current = onReconnect;
  }, [onReconnect]);
  useEffect(() => queue.start(() => reconnect.current?.()), [queue]);
  return <Context.Provider value={queue}>{children}</Context.Provider>;
}

/** The jobs and the connection, re-rendering on every change. */
export function useJobs(): { jobs: Jobs; connection: ConnectionState | null } {
  const queue = useContext(Context);
  const jobs = useSyncExternalStore(queue.subscribe, queue.jobs);
  const connection = useSyncExternalStore(queue.subscribe, queue.connection);
  return { jobs, connection };
}

/** The queue itself, for a consumer that reads it outside a render. */
export function useJobQueue(): JobQueue {
  return useContext(Context);
}

/**
 * Calls `listener` on every event while mounted; the latest `listener` given
 * is the one called. It subscribes during the commit — a layout effect — so
 * an event that arrives between a consumer's commit and its passive effects
 * still reaches it.
 */
export function useJobEvents(listener: JobListener) {
  const queue = useContext(Context);
  const latest = useRef(listener);
  useLayoutEffect(() => {
    latest.current = listener;
  });
  useLayoutEffect(() => queue.listen((before, after, event) => latest.current(before, after, event)), [queue]);
}

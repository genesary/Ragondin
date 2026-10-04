/** @vitest-environment happy-dom */
import { act, render, screen } from '@testing-library/react';
import { StrictMode, useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Jobs } from '../api/jobs.ts';
import { FakeEventSource, installFakeEventSource } from '../api/testing.ts';
import { connect, QUEUED, runJob, running, send } from './fixtures.ts';
import { JobQueueProvider, useJobEvents, useJobs } from './queue.tsx';

/** Prints what a consumer reads of the queue. */
function Reader({ name }: { name: string }) {
  const { jobs, connection } = useJobs();
  return <output aria-label={name}>{`${connection ?? 'none'}|${[...jobs.values()].map((j) => `${j.id}:${j.state.kind}`).join(',')}`}</output>;
}

/** Records every event's queue before and after, as a consumer of the events sees them. */
function Listener({ seen }: { seen: [Jobs, Jobs][] }) {
  useJobEvents((before, after) => seen.push([before, after]));
  return null;
}

const read = (name: string) => screen.getByRole('status', { name }).textContent;

beforeEach(() => installFakeEventSource());
afterEach(() => vi.unstubAllGlobals());

describe('the job queue', () => {
  it('follows GET /jobs/events once for every consumer under it', () => {
    render(
      <JobQueueProvider>
        <Reader name="a" />
        <Reader name="b" />
      </JobQueueProvider>,
    );
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.latest().url).toBe('/api/v1/jobs/events');
    connect(FakeEventSource.latest(), [runJob('j1', QUEUED)]);
    send(FakeEventSource.latest(), { event: 'running', data: runJob('j1', running(3, 10)) });
    expect(read('a')).toBe('connected|j1:running');
    expect(read('b')).toBe('connected|j1:running');
  });

  it('says it is connecting, then disconnected while the stream is down, so no consumer shows the queue as current', () => {
    render(
      <JobQueueProvider>
        <Reader name="a" />
      </JobQueueProvider>,
    );
    expect(read('a')).toBe('connecting|');
    connect(FakeEventSource.latest());
    act(() => FakeEventSource.latest().fail());
    expect(read('a')).toBe('disconnected|');
  });

  it('hands each event to its listeners with the queue before and after it', () => {
    const seen: [Jobs, Jobs][] = [];
    render(
      <JobQueueProvider>
        <Listener seen={seen} />
      </JobQueueProvider>,
    );
    connect(FakeEventSource.latest(), [runJob('j1', QUEUED)]);
    send(FakeEventSource.latest(), { event: 'running', data: runJob('j1', running(1, 10)) });
    expect(seen).toHaveLength(2);
    expect(seen[1]?.[0].get('j1')?.state.kind).toBe('queued');
    expect(seen[1]?.[1].get('j1')?.state.kind).toBe('running');
  });

  it('calls back on every reconnection after the first, so the shell re-checks the build', () => {
    const onReconnect = vi.fn();
    render(
      <JobQueueProvider onReconnect={onReconnect}>
        <Reader name="a" />
      </JobQueueProvider>,
    );
    act(() => FakeEventSource.latest().open());
    act(() => FakeEventSource.latest().fail());
    act(() => FakeEventSource.latest().open());
    expect(onReconnect).toHaveBeenCalledTimes(1);
  });

  it('closes its stream when it unmounts, and opens one under StrictMode too', () => {
    const { unmount } = render(
      <StrictMode>
        <JobQueueProvider>
          <Reader name="a" />
        </JobQueueProvider>
      </StrictMode>,
    );
    connect(FakeEventSource.latest(), [runJob('j1', QUEUED)]);
    expect(read('a')).toBe('connected|j1:queued');
    expect(FakeEventSource.instances.filter((s) => !s.closed)).toHaveLength(1);
    unmount();
    expect(FakeEventSource.instances.every((s) => s.closed)).toBe(true);
  });

  it('without a provider, follows nothing: no stream, no connection state, no jobs', () => {
    render(<Reader name="a" />);
    expect(FakeEventSource.instances).toHaveLength(0);
    expect(read('a')).toBe('none|');
  });

  it('keeps its stream when a consumer re-renders', () => {
    function Rerender() {
      const [n, setN] = useState(0);
      return (
        <button type="button" onClick={() => setN(n + 1)}>
          {n}
        </button>
      );
    }
    render(
      <JobQueueProvider>
        <Rerender />
      </JobQueueProvider>,
    );
    act(() => screen.getByRole('button').click());
    expect(FakeEventSource.instances).toHaveLength(1);
  });
});

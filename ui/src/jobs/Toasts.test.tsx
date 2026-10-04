/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeEventSource, installFakeEventSource } from '../api/testing.ts';
import type { JobSummary } from '../api/types.ts';
import { CANCELLED, connect, doneAs, failedAt, hex, QUEUED, runJob, running, send } from './fixtures.ts';
import { JobQueueProvider } from './queue.tsx';
import { JobToasts } from './Toasts.tsx';

const RUN = hex('a');

function show() {
  render(
    <JobQueueProvider>
      <JobToasts />
    </JobQueueProvider>,
  );
  return FakeEventSource.latest();
}

const region = () => screen.getByRole('region', { name: 'Notifications' });
const toasts = () => [...region().querySelectorAll('.rg-toast')];

beforeEach(() => {
  installFakeEventSource();
  window.history.replaceState(null, '', '/#runs');
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the outcome of a run, as a toast', () => {
  it('says a run is done, politely, and its Open lands on the run in Replay', () => {
    const stream = show();
    connect(stream, [runJob('j1', running(9, 10))]);
    send(stream, { event: 'done', data: runJob('j1', doneAs(RUN)) });
    const toast = within(region()).getByRole('status');
    expect(toast.textContent).toContain('Run done');
    expect(toast.textContent).toContain('hybrid on beir/scifact');
    fireEvent.click(within(toast).getByRole('button', { name: 'Open' }));
    expect(window.location.hash).toBe(`#replay/${RUN}`);
    expect(toasts()).toHaveLength(0);
  });

  it('opens the run filed, when what ran was filed under another id than it announced', () => {
    const stream = show();
    connect(stream, [runJob('j1', running(9, 10))]);
    send(stream, { event: 'done', data: runJob('j1', doneAs(hex('d'), { announced: RUN, decided: hex('d') })) });
    fireEvent.click(within(region()).getByRole('button', { name: 'Open' }));
    expect(window.location.hash).toBe(`#replay/${hex('d')}`);
  });

  it('says where a run failed, as an alert that stays until acted on, and its Open lands on the job', () => {
    vi.useFakeTimers();
    const stream = show();
    connect(stream, [runJob('j1', running(9, 10))]);
    send(stream, { event: 'failed', data: runJob('j1', failedAt('rerank', 'the reranker answered 503')) });
    const toast = within(region()).getByRole('alert');
    expect(toast.textContent).toContain('Run failed at rerank');
    act(() => vi.advanceTimersByTime(60_000));
    expect(toasts()).toHaveLength(1);
    fireEvent.click(within(toast).getByRole('button', { name: 'Open' }));
    expect(window.location.hash).toBe('#runs/job/j1');
  });

  it('says a run was cancelled, and leaves on its own after a while', () => {
    vi.useFakeTimers();
    const stream = show();
    connect(stream, [runJob('j1', QUEUED)]);
    send(stream, { event: 'cancelled', data: runJob('j1', CANCELLED) });
    expect(within(region()).getByRole('status').textContent).toContain('Run cancelled');
    act(() => vi.advanceTimersByTime(6000));
    expect(toasts()).toHaveLength(0);
  });

  it('raises nothing for what had already ended when the page opened, nor for a download', () => {
    const stream = show();
    connect(stream, [runJob('j1', doneAs(RUN)), runJob('j2', failedAt(null, 'interrupted'))]);
    const download: JobSummary = { id: 'd1', created_at_ms: 1, position: 0, state: QUEUED, work: { kind: 'download', benchmark: 'beir/fiqa' } };
    send(stream, { event: 'queued', data: download });
    send(stream, { event: 'done', data: { ...download, state: doneAs('') } });
    expect(toasts()).toHaveLength(0);
  });

  it('raises one toast for an outcome missed while the stream was down, and never a second one for it', () => {
    vi.useFakeTimers();
    const stream = show();
    connect(stream, [runJob('j1', running(5, 10))]);
    act(() => stream.drop());
    act(() => vi.advanceTimersByTime(1000));
    // The stream opened anew begins with the whole queue, where the job has ended.
    const again = FakeEventSource.latest();
    connect(again, [runJob('j1', doneAs(RUN))]);
    connect(again, [runJob('j1', doneAs(RUN))]);
    send(again, { event: 'done', data: runJob('j1', doneAs(RUN)) });
    expect(toasts()).toHaveLength(1);
  });

  it('Dismiss takes focus to the toast left, so focus is never dropped on the page', () => {
    const stream = show();
    connect(stream, [runJob('j1', running(1, 10)), runJob('j2', running(1, 10))]);
    send(stream, { event: 'failed', data: runJob('j1', failedAt('rerank', 'x')) });
    send(stream, { event: 'failed', data: runJob('j2', failedAt('dense', 'y')) });
    const [first] = within(region()).getAllByRole('button', { name: 'Dismiss' });
    first?.focus();
    fireEvent.click(first as HTMLElement);
    expect(toasts()).toHaveLength(1);
    expect(document.activeElement).toBe(within(region()).getByRole('button', { name: 'Dismiss' }));
    fireEvent.click(within(region()).getByRole('button', { name: 'Dismiss' }));
    expect(document.activeElement).toBe(region());
  });
});

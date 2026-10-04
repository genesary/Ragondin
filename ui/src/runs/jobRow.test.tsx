/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Table } from '../../design/index.ts';
import { jobRow, type JobRowOptions } from './jobRow.tsx';
import type { JobFacts, RunRow } from './model.ts';

const ANNOUNCED = 'a1b2c3d4e5f6'.padEnd(64, '0');

const facts = (over: Partial<JobFacts> = {}): JobFacts => ({
  submission: { pipeline: 'hybrid', benchmark: 'beir/scifact', up_to: null },
  place: null,
  queued: 0,
  startedAtMs: null,
  medianMs: null,
  mismatch: null,
  ...over,
});

const job = (status: RunRow['status'], over: Partial<JobFacts> = {}): RunRow => ({
  source: { kind: 'job', id: 'j1', runId: ANNOUNCED },
  pipeline: '',
  pipelineNames: [],
  launchedAs: 'hybrid',
  launchedHeld: null,
  launchRecorded: true,
  benchmark: 'name:beir/scifact',
  benchmarkNames: ['beir/scifact'],
  status,
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  job: facts(over),
  announced: null,
});

const COLUMNS = [
  { id: 'bench', label: 'Benchmark' },
  { id: 'run', label: 'Run' },
  { id: 'status', label: 'Status' },
  { id: 'metrics', label: 'Metrics' },
];

function show(row: RunRow, over: Partial<JobRowOptions> = {}) {
  const options: JobRowOptions = {
    columns: { latency: false, started: false },
    stale: false,
    cancelling: false,
    onCancel: vi.fn(),
    onMove: vi.fn(),
    onResubmit: vi.fn(),
    ...over,
  };
  const onOpen = vi.fn();
  render(<Table caption="runs" columns={COLUMNS} rows={[jobRow(row, options)]} onOpen={onOpen} onToggle={() => {}} />);
  return { ...options, onOpen, tr: screen.getAllByRole('row')[1] as HTMLElement };
}

const chip = () => document.querySelector('.rg-status');

afterEach(() => vi.useRealTimers());

describe('a job row', () => {
  it('shows the identity its job announced as a link to the job — never to a run the store does not hold', () => {
    show(job({ state: 'queued' }, { place: 0, queued: 1 }));
    const link = screen.getByRole('link', { name: 'a1b2c3d4e5f6' });
    expect(link.getAttribute('href')).toBe('#runs/job/j1');
    expect(link.getAttribute('tabindex')).toBe('-1');
    expect(screen.getByText('announced')).toBeTruthy();
    expect(screen.queryByRole('checkbox')).toBeNull();
  });

  it('is named by its run, its benchmark and where it stands, and Enter opens the job', () => {
    const { tr, onOpen } = show(job({ state: 'queued' }, { place: 1, queued: 2 }));
    expect(tr.getAttribute('aria-label')).toBe('Run a1b2c3d4e5f6 on beir/scifact, queued, 1 ahead');
    fireEvent.keyDown(tr, { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledWith('job:j1');
  });
});

describe('a queued row', () => {
  it('says its place, and offers Move up, Move down and Cancel, named for the row', () => {
    const { onMove, onCancel } = show(job({ state: 'queued' }, { place: 1, queued: 3 }));
    expect(chip()?.textContent).toBe('queued, 1 ahead');
    fireEvent.click(screen.getByRole('button', { name: 'Move run a1b2c3d4e5f6 up' }));
    expect(onMove).toHaveBeenCalledWith(0);
    fireEvent.click(screen.getByRole('button', { name: 'Move run a1b2c3d4e5f6 down' }));
    expect(onMove).toHaveBeenCalledWith(2);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel run a1b2c3d4e5f6' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('refuses to move past either end, saying why, and stays reachable by keyboard', () => {
    const { onMove } = show(job({ state: 'queued' }, { place: 0, queued: 1 }));
    expect(chip()?.textContent).toBe('queued, next');
    const up = screen.getByRole('button', { name: 'Move run a1b2c3d4e5f6 up' });
    expect(up.getAttribute('aria-disabled')).toBe('true');
    expect(up.hasAttribute('disabled')).toBe(false);
    fireEvent.click(up);
    fireEvent.click(screen.getByRole('button', { name: 'Move run a1b2c3d4e5f6 down' }));
    expect(onMove).not.toHaveBeenCalled();
  });
});

describe('a running row', () => {
  it('shows the real count on its meter, the median the queue reported and the time elapsed', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_083_000);
    show(job({ state: 'running', done: 412, total: 1000 }, { startedAtMs: 1_700_000_000_000, medianMs: 1840 }));
    expect(chip()?.textContent).toBe('running 412 / 1,000');
    expect((document.querySelector('.rg-status__meter i') as HTMLElement).style.width).toBe('41%');
    expect(screen.getByText('1,840 ms / query')).toBeTruthy();
    expect(screen.getByText('1:23 elapsed')).toBeTruthy();
    act(() => vi.advanceTimersByTime(2000));
    expect(screen.getByText('1:25 elapsed')).toBeTruthy();
  });

  it('says it is starting before the queue knows its total, with an empty meter', () => {
    show(job({ state: 'running', done: 0, total: null }));
    expect(chip()?.textContent).toBe('starting');
    expect((document.querySelector('.rg-status__meter i') as HTMLElement).style.width).toBe('0%');
  });

  it('marks what it shows as the last known while the stream is down', () => {
    show(job({ state: 'running', done: 412, total: 1000 }), { stale: true });
    expect(chip()?.textContent).toBe('running 412 / 1,000 · last known');
  });

  it('offers Cancel, and says "cancelling…" until the queue says the job ended', () => {
    const { onCancel } = show(job({ state: 'running', done: 1, total: 10 }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel run a1b2c3d4e5f6' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('while cancelling, refuses a second Cancel and says so on the button that kept focus', () => {
    const { onCancel } = show(job({ state: 'running', done: 1, total: 10 }), { cancelling: true });
    const button = screen.getByRole('button', { name: 'Cancelling…' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(button);
    expect(onCancel).not.toHaveBeenCalled();
  });
});

describe('a failed row', () => {
  it('names the failed node on its chip and in a sentence, and leads to the job', () => {
    const { tr } = show(job({ state: 'failed', node: 'rerank', error: 'the reranker service did not answer' }));
    expect(chip()?.getAttribute('data-state')).toBe('failed');
    expect(chip()?.textContent).toBe('failed at rerank');
    expect(screen.getByText('rerank failed: the reranker service did not answer')).toBeTruthy();
    expect(tr.getAttribute('aria-label')).toBe('Run a1b2c3d4e5f6 on beir/scifact, failed at rerank');
  });

  it('says the run failed without a node when the failure names none', () => {
    show(job({ state: 'failed', node: null, error: 'interrupted' }));
    expect(chip()?.textContent).toBe('failed');
    expect(screen.getByText('The run failed: interrupted')).toBeTruthy();
  });

  it('offers Resubmit: the same submission, so the same announced identity', () => {
    const { onResubmit } = show(job({ state: 'failed', node: null, error: 'interrupted' }));
    fireEvent.click(screen.getByRole('button', { name: 'Resubmit run a1b2c3d4e5f6' }));
    expect(onResubmit).toHaveBeenCalledTimes(1);
  });
});

describe('a cancelled row', () => {
  it('shows the cancelled chip and offers Resubmit', () => {
    const { onResubmit } = show(job({ state: 'cancelled' }));
    expect(chip()?.getAttribute('data-state')).toBe('cancelled');
    fireEvent.click(within(screen.getAllByRole('row')[1] as HTMLElement).getByRole('button', { name: 'Resubmit run a1b2c3d4e5f6' }));
    expect(onResubmit).toHaveBeenCalledTimes(1);
  });
});

describe('a done row, before the store lists its run', () => {
  it('says the run was filed and is being read', () => {
    show(job({ state: 'done' }));
    expect(chip()?.getAttribute('data-state')).toBe('done');
    expect(screen.getByText('Filed; reading it from the store…')).toBeTruthy();
  });

  it('shows both ids and why, when the run was filed under another than the one announced', () => {
    show(job({ state: 'done' }, { mismatch: { announced: ANNOUNCED, decided: 'ffeeddccbbaa'.padEnd(64, '9') } }));
    expect(screen.getByText(/Filed under ffeeddccbbaa, not the announced a1b2c3d4e5f6: what ran differs from what was announced/)).toBeTruthy();
  });
});

describe('every live row', () => {
  it('keeps one control high whatever it holds, so a button coming or going moves no row', () => {
    show(job({ state: 'done' }));
    expect(document.querySelector('.rg-runs__job')).toBeTruthy();
  });
});

/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { RunRow } from './model.ts';
import { RunRowView, type RowColumns } from './RunRowView.tsx';

const ID = 'a1b2c3d4e5f6'.padEnd(64, '0');

const done = (over: Partial<RunRow> = {}): RunRow => ({
  id: ID,
  pipeline: 'p'.repeat(64),
  pipelineName: 'hybrid',
  benchmark: 'd'.repeat(64),
  benchmarkName: 'beir/scifact',
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  ...over,
});

const NO_EXTRA: RowColumns = { latency: false, started: false };

function show(row: RunRow, props: { selected?: boolean; refusal?: string | null; columns?: RowColumns } = {}) {
  const onToggle = vi.fn();
  const onOpen = vi.fn();
  render(
    <table>
      <tbody>
        <RunRowView row={row} selected={props.selected ?? false} refusal={props.refusal ?? null} columns={props.columns ?? NO_EXTRA} onToggle={onToggle} onOpen={onOpen} />
      </tbody>
    </table>,
  );
  return { onToggle, onOpen, tr: screen.getAllByRole('row')[0] as HTMLElement };
}

describe('a done run', () => {
  it('shows its checkbox labelled by its benchmark, its short hash as a link to Replay, and the done chip', () => {
    show(done());
    expect(screen.getByRole('checkbox', { name: 'beir/scifact' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'a1b2c3d4e5f6' }).getAttribute('href')).toBe(`#replay/${ID}`);
    expect(screen.getByText('done').closest('.rg-status')?.getAttribute('data-state')).toBe('done');
  });

  it('shows ranking metrics only, under `ranking`, for a benchmark that carries qrels only', () => {
    show(done({ metrics: [{ family: 'ranking', metrics: [{ name: 'ndcg@10', value: 0.54364 }, { name: 'mrr', value: 0.5 }] }] }));
    const group = screen.getByRole('group', { name: 'ranking' });
    expect(within(group).getByText('ndcg@10').nextElementSibling?.textContent).toBe('0.5436');
    expect(within(group).getByText('mrr').nextElementSibling?.textContent).toBe('0.5000');
    expect(screen.queryByRole('group', { name: 'answers' })).toBeNull();
  });

  it('shows answer metrics only, under `answers`, for a benchmark that carries reference answers only', () => {
    show(done({ metrics: [{ family: 'answers', metrics: [{ name: 'exact_match', value: 0.4123 }] }] }));
    const group = screen.getByRole('group', { name: 'answers' });
    expect(within(group).getByText('exact_match').nextElementSibling?.textContent).toBe('41.2');
    expect(screen.queryByRole('group', { name: 'ranking' })).toBeNull();
  });

  it('shows both families, ranking first, for a benchmark that carries both', () => {
    show(
      done({
        metrics: [
          { family: 'ranking', metrics: [{ name: 'recall@10', value: 0.6667 }] },
          { family: 'answers', metrics: [{ name: 'token_f1', value: 0.4907 }] },
        ],
      }),
    );
    expect(screen.getAllByRole('group').map((g) => g.getAttribute('aria-label'))).toEqual(['ranking', 'answers']);
  });

  it('shows metrics whose family the source does not say as chips under no label', () => {
    show(done({ metrics: [{ family: null, metrics: [{ name: 'mrr', value: 0.5 }] }] }));
    expect(screen.getByText('mrr').nextElementSibling?.textContent).toBe('0.5000');
    expect(screen.queryByText('ranking')).toBeNull();
    expect(screen.queryByText('answers')).toBeNull();
  });

  it('never draws a dash for a metric it does not have', () => {
    const { tr } = show(done({ metrics: [{ family: 'ranking', metrics: [{ name: 'mrr', value: 0.5 }] }] }), { columns: { latency: true, started: true } });
    for (const cell of within(tr).getAllByRole('cell')) expect(cell.textContent?.trim()).not.toBe('—');
  });

  it('shows its latency and its start time when the columns are drawn and the source reports them', () => {
    show(done({ latencyMs: 412.4, startedAt: '2026-09-30T14:03:00Z' }), { columns: { latency: true, started: true } });
    expect(screen.getByText('412 ms')).toBeTruthy();
    expect(document.querySelector('time')?.getAttribute('datetime')).toBe('2026-09-30T14:03:00Z');
  });

  it('draws no latency or start cell when the columns are not drawn', () => {
    const { tr } = show(done());
    expect(within(tr).getAllByRole('cell')).toHaveLength(4);
  });

  it('labels a prefix run with the node it stops at', () => {
    show(done({ prefix: { parent: 'hybrid', upTo: 'rerank' } }));
    expect(screen.getByText('prefix up to rerank')).toBeTruthy();
  });
});

describe('a failed run', () => {
  const failed = done({ status: { state: 'failed', node: 'rerank', error: 'the reranker service did not answer' } });

  it('renders its failed chip naming the node, and the error as a sentence', () => {
    show(failed);
    const chip = document.querySelector('.rg-status');
    expect(chip?.getAttribute('data-state')).toBe('failed');
    expect(chip?.textContent).toBe('failed at rerank');
    expect(screen.getByText('rerank failed: the reranker service did not answer')).toBeTruthy();
  });

  it('opens in Replay', () => {
    show(failed);
    expect(screen.getByRole('link', { name: 'a1b2c3d4e5f6' }).getAttribute('href')).toBe(`#replay/${ID}`);
  });

  it('says the run failed without a node when the failure names none', () => {
    show(done({ status: { state: 'failed', node: null, error: 'interrupted' } }));
    expect(document.querySelector('.rg-status')?.textContent).toBe('failed');
    expect(screen.getByText('The run failed: interrupted')).toBeTruthy();
  });
});

describe('a run that has not finished', () => {
  it.each([
    ['queued', { state: 'queued' } as const, 'queued'],
    ['running', { state: 'running', fraction: 0.4 } as const, 'running 40%'],
  ])('renders the placeholder row: the %s chip only', (_, status, word) => {
    const { tr } = show(done({ status, metrics: [{ family: 'ranking', metrics: [{ name: 'mrr', value: 0.5 }] }] }));
    expect(document.querySelector('.rg-status')?.textContent).toBe(word);
    expect(screen.queryByRole('checkbox')).toBeNull();
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.queryByText('mrr')).toBeNull();
    expect(tr.getAttribute('tabindex')).toBeNull();
  });
});

describe('the checkbox', () => {
  it('is checked when the run is selected', () => {
    show(done(), { selected: true });
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
  });

  it('is disabled with the reason describing it when the run is refused', () => {
    show(done(), { refusal: 'The selected runs are on beir/fiqa. Compare takes runs on one benchmark.' });
    const box = screen.getByRole('checkbox') as HTMLInputElement;
    expect(box.disabled).toBe(true);
    const describedBy = box.getAttribute('aria-describedby') ?? '';
    expect(document.getElementById(describedBy)?.textContent).toBe('The selected runs are on beir/fiqa. Compare takes runs on one benchmark.');
  });

  it('toggles the run when clicked', () => {
    const { onToggle } = show(done());
    fireEvent.click(screen.getByRole('checkbox'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});

describe('the keyboard', () => {
  it('reaches the row', () => {
    const { tr } = show(done());
    expect(tr.getAttribute('tabindex')).toBe('0');
  });

  it('toggles the run with space on the row', () => {
    const { tr, onToggle } = show(done());
    fireEvent.keyDown(tr, { key: ' ' });
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('does not toggle a refused run with space', () => {
    const { tr, onToggle } = show(done(), { refusal: 'no' });
    fireEvent.keyDown(tr, { key: ' ' });
    expect(onToggle).not.toHaveBeenCalled();
  });

  it('opens the run with enter on the row', () => {
    const { tr, onOpen } = show(done());
    fireEvent.keyDown(tr, { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it('leaves a key on a control inside the row to that control', () => {
    const { onToggle, onOpen } = show(done());
    fireEvent.keyDown(screen.getByRole('checkbox'), { key: ' ' });
    fireEvent.keyDown(screen.getByRole('link'), { key: 'Enter' });
    expect(onToggle).not.toHaveBeenCalled();
    expect(onOpen).not.toHaveBeenCalled();
  });
});

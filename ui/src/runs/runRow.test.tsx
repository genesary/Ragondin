/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Table } from '../../design/index.ts';
import type { RunRow } from './model.ts';
import { runRow, type RowColumns } from './runRow.tsx';
import type { Refusal } from './selection.ts';

const ID = 'a1b2c3d4e5f6'.padEnd(64, '0');

const done = (over: Partial<RunRow> = {}): RunRow => ({
  source: { kind: 'run', id: ID },
  pipeline: 'p'.repeat(64),
  pipelineNames: ['hybrid'],
  refusedNames: [],
  launchedAs: null,
  launchedHeld: null,
  launchRecorded: false,
  benchmark: 'd'.repeat(64),
  benchmarkNames: ['beir/scifact'],
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  contentPrefix: null,
  job: null,
  announced: null,
  ...over,
});

const NO_EXTRA: RowColumns = { latency: false, started: false };
const COLUMNS = [
  { id: 'bench', label: 'Benchmark' },
  { id: 'run', label: 'Run' },
  { id: 'status', label: 'Status' },
  { id: 'metrics', label: 'Metrics' },
  { id: 'latency', label: 'Latency' },
  { id: 'started', label: 'Started' },
];

function show(row: RunRow, props: { selected?: boolean; refusal?: Refusal | null; columns?: RowColumns } = {}) {
  const onToggle = vi.fn();
  const onOpen = vi.fn();
  const columns = props.columns ?? NO_EXTRA;
  const drawn = COLUMNS.slice(0, 4 + (columns.latency ? 1 : 0) + (columns.started ? 1 : 0));
  render(
    <Table
      caption="runs"
      columns={drawn}
      rows={[runRow(row, { selected: props.selected ?? false, refusal: props.refusal ?? null, columns, onToggle })]}
      onOpen={onOpen}
      onToggle={() => {}}
    />,
  );
  return { onToggle, onOpen, tr: screen.getAllByRole('row')[1] as HTMLElement };
}

describe('a done run', () => {
  it('is a row named by its run and its benchmark', () => {
    const { tr } = show(done());
    expect(tr.getAttribute('aria-label')).toBe('Run a1b2c3d4e5f6 on beir/scifact');
  });

  it('says in its name that it is selected, so Space on the row is announced', () => {
    const { tr } = show(done(), { selected: true });
    expect(tr.getAttribute('aria-label')).toBe('Run a1b2c3d4e5f6 on beir/scifact, selected');
  });

  it('says in its name why it cannot be selected', () => {
    const { tr } = show(done(), { refusal: { short: 'Other benchmark', full: 'f' } });
    expect(tr.getAttribute('aria-label')).toBe('Run a1b2c3d4e5f6 on beir/scifact, cannot be selected: Other benchmark');
  });

  it('shows its checkbox, labelled by its benchmark and named by its run, and out of the tab order', () => {
    show(done());
    const box = screen.getByRole('checkbox', { name: 'Select run a1b2c3d4e5f6 on beir/scifact' });
    expect(box.closest('label')?.textContent).toBe('beir/scifact');
    expect(box.getAttribute('tabindex')).toBe('-1');
  });

  it('shows its short hash as a link to Replay, out of the tab order: the row opens it', () => {
    show(done());
    const link = screen.getByRole('link', { name: 'a1b2c3d4e5f6' });
    expect(link.getAttribute('href')).toBe(`#replay/${ID}`);
    expect(link.getAttribute('tabindex')).toBe('-1');
  });

  it('shows the done chip', () => {
    show(done());
    expect(document.querySelector('.rg-status')?.getAttribute('data-state')).toBe('done');
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
    expect(within(group).getByText('EM').nextElementSibling?.textContent).toBe('41.2');
    expect(screen.queryByRole('group', { name: 'ranking' })).toBeNull();
  });

  it('answers metrics render as a percentage with one decimal', () => {
    show(done({ metrics: [{ family: 'answers', metrics: [{ name: 'exact_match', value: 0.412 }, { name: 'token_f1', value: 0.49074 }] }] }));
    const group = screen.getByRole('group', { name: 'answers' });
    expect(within(group).getByText('EM').parentElement?.textContent).toBe('EM41.2');
    expect(within(group).getByText('F1').nextElementSibling?.textContent).toBe('49.1');
  });

  it('an unknown metric family is shown, not hidden', () => {
    show(done({ metrics: [{ family: 'ranking', metrics: [{ name: 'mrr', value: 0.5 }] }, { family: 'unknown', metrics: [{ name: 'foo_score', value: 0.123456789 }] }] }));
    const group = screen.getByRole('group', { name: 'unknown' });
    expect(within(group).getByText('unknown')).toBeTruthy();
    expect(within(group).getByText('foo_score').nextElementSibling?.textContent).toBe('0.123456789');
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
    show(done({ latencyMs: 0.017249 }), { columns: { latency: true, started: false } });
    expect(screen.getByText('0.017 ms')).toBeTruthy();
    expect(document.querySelector('time')?.getAttribute('datetime')).toBe('2026-09-30T14:03:00Z');
  });

  it('draws no latency or start cell when the columns are not drawn', () => {
    const { tr } = show(done());
    expect(within(tr).getAllByRole('cell')).toHaveLength(4);
  });

  it('labels a prefix run with its parent and the node it stops at', () => {
    show(done({ launchedAs: 'hybrid', prefix: { parents: ['hybrid'], upTo: 'rerank' } }));
    expect(screen.getByText('prefix of hybrid, up to rerank')).toBeTruthy();
  });

  it('says beside the recorded prefix what its content is a prefix of, when that differs', () => {
    show(done({ launchedAs: 'hybrid', prefix: { parents: ['hybrid'], upTo: 'rerank' }, contentPrefix: { parents: ['fork'], upTo: 'rerank' } }));
    expect(screen.getByText('prefix of hybrid, up to rerank')).toBeTruthy();
    expect(screen.getByText('content: prefix of fork, up to rerank')).toBeTruthy();
  });

  it('writes the fact its group is not headed by beside its hash, in words', () => {
    const { tr } = show(done({ launchedAs: 'hybrid', pipelineNames: ['hybrid-fork'] }));
    const run = within(tr).getAllByRole('cell')[1] as HTMLElement;
    expect(run.textContent).toContain('content held by hybrid-fork');
  });

  it('keeps its words apart from its hash when the text is read or copied', () => {
    const { tr } = show(done({ launchedAs: 'hybrid', pipelineNames: [] }));
    expect((within(tr).getAllByRole('cell')[1] as HTMLElement).textContent).toBe('a1b2c3d4e5f6 no pipeline in the workspace matches this run now');
  });

  it('says a run without a record has none, rather than nothing', () => {
    const { tr } = show(done());
    expect((within(tr).getAllByRole('cell')[1] as HTMLElement).textContent).toContain('launch not recorded');
  });
});

describe('a run filed under another id than its job announced', () => {
  it('says both, and why, beside its hash', () => {
    show(done({ announced: 'ffeeddccbbaa'.padEnd(64, '9') }));
    expect(screen.getByText('announced as ffeeddccbbaa; filed under this id because what ran differs from what was announced')).toBeTruthy();
  });
});

describe('the checkbox', () => {
  it('is checked when the run is selected', () => {
    show(done(), { selected: true });
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
  });

  it('is disabled with the short reason describing it when the run is refused', () => {
    show(done(), { refusal: { short: 'Other benchmark', full: 'The selected runs are on beir/fiqa. Compare takes runs on one benchmark.' } });
    const box = screen.getByRole('checkbox') as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(document.getElementById(box.getAttribute('aria-describedby') ?? '')?.textContent).toBe('Other benchmark');
    expect(screen.queryByText(/Compare takes runs on one benchmark/)).toBeNull();
  });

  it('toggles the run when clicked', () => {
    const { onToggle } = show(done());
    fireEvent.click(screen.getByRole('checkbox'));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });
});

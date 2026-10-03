import { describe, expect, it } from 'vitest';
import type { RunRow } from './model.ts';
import { compareRefusal, refusal, sanitize, toggle, unknownIds } from './selection.ts';

const row = (id: string, benchmark: string, over: Partial<RunRow> = {}): RunRow => ({
  source: { kind: 'run', id },
  pipeline: 'p',
  pipelineNames: [],
  launchedAs: null,
  launchRecorded: false,
  benchmark,
  benchmarkNames: benchmark === 'sci' ? ['beir/scifact'] : benchmark === 'fiqa' ? ['beir/fiqa'] : [],
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  ...over,
});

const ROWS = [
  row('a', 'sci'),
  row('b', 'sci'),
  row('c', 'fiqa'),
  row('f', 'sci', { status: { state: 'failed', node: 'rerank', error: 'boom' } }),
  row('q', 'sci', { source: { kind: 'job', id: 'q', runId: null }, status: { state: 'queued' } }),
  row('d', 'sci'),
  row('e', 'sci'),
  row('g', 'sci'),
  row('h', 'sci'),
];
const at = (id: string) => ROWS.find((r) => r.source.id === id) as RunRow;
const NONE = new Set<string>();

describe('toggle', () => {
  it('appends a run, so the selection keeps the order the runs were checked in', () => {
    expect(toggle(['b'], 'a')).toEqual(['b', 'a']);
  });

  it('removes a run already selected, keeping the others in order', () => {
    expect(toggle(['b', 'a', 'd'], 'a')).toEqual(['b', 'd']);
  });
});

describe('refusal', () => {
  it('refuses nothing while nothing is selected', () => {
    expect(refusal(at('a'), [], ROWS)).toBeNull();
    expect(refusal(at('c'), [], ROWS)).toBeNull();
  });

  it('refuses a run on another benchmark once one is checked: a short reason for the row, the sentence naming the selected benchmark for the table', () => {
    expect(refusal(at('c'), ['a'], ROWS)).toEqual({
      short: 'Other benchmark',
      full: 'The selected runs are on beir/scifact. Compare takes runs on one benchmark.',
    });
  });

  it('allows the other runs of the same benchmark, and the selected run itself', () => {
    expect(refusal(at('b'), ['a'], ROWS)).toBeNull();
    expect(refusal(at('a'), ['a'], ROWS)).toBeNull();
  });

  it('allows every benchmark again once the selection is cleared', () => {
    expect(refusal(at('c'), toggle(toggle([], 'a'), 'a'), ROWS)).toBeNull();
  });

  it('names a benchmark without a name by its short dataset digest', () => {
    const rows = [row('x', 'd'.repeat(64)), row('y', 'e'.repeat(64))];
    expect(refusal(rows[1] as RunRow, ['x'], rows)?.full).toBe('The selected runs are on dataset dddddddddddd. Compare takes runs on one benchmark.');
  });

  it('reads the benchmark from the first selected run the listing holds, past an id it does not hold yet', () => {
    expect(refusal(at('c'), ['unknown', 'a'], ROWS)?.short).toBe('Other benchmark');
  });

  it('refuses a sixth run once five are selected: a baseline and four', () => {
    expect(refusal(at('h'), ['a', 'b', 'd', 'e', 'g'], ROWS)).toEqual({
      short: 'Five selected',
      full: 'Compare takes a baseline and up to four runs. Clear one to choose another.',
    });
    expect(refusal(at('g'), ['a', 'b', 'd', 'e', 'g'], ROWS)).toBeNull();
  });

  it('refuses a failed run, which has no metrics to compare', () => {
    expect(refusal(at('f'), [], ROWS)).toEqual({ short: 'Failed', full: 'A failed run has no metrics to compare.' });
  });

  it('refuses a job, which is not a run in the store', () => {
    expect(refusal(at('q'), [], ROWS)).toEqual({ short: 'Not finished', full: 'Only a finished run can be compared.' });
  });
});

describe('unknownIds', () => {
  it('lists the selected ids the listing does not hold, in order', () => {
    expect(unknownIds(['x', 'a', 'y'], ROWS)).toEqual(['x', 'y']);
  });
});

describe('sanitize', () => {
  it('keeps a valid selection as it is', () => {
    expect(sanitize(['b', 'a'], ROWS, NONE)).toEqual(['b', 'a']);
  });

  it('drops duplicates, and runs that cannot be selected', () => {
    expect(sanitize(['a', 'a', 'f', 'b'], ROWS, NONE)).toEqual(['a', 'b']);
  });

  it('keeps an id the listing does not hold until a fresh listing confirms it is gone', () => {
    expect(sanitize(['a', 'new', 'b'], ROWS, NONE)).toEqual(['a', 'new', 'b']);
    expect(sanitize(['a', 'new', 'b'], ROWS, new Set(['new']))).toEqual(['a', 'b']);
  });

  it('keeps the first run’s benchmark and drops runs on another, as a click would have refused them', () => {
    expect(sanitize(['c', 'a', 'b'], ROWS, NONE)).toEqual(['c']);
    expect(sanitize(['a', 'c', 'b'], ROWS, NONE)).toEqual(['a', 'b']);
  });

  it('keeps at most five, the first five in order', () => {
    expect(sanitize(['a', 'b', 'd', 'e', 'g', 'h'], ROWS, NONE)).toEqual(['a', 'b', 'd', 'e', 'g']);
  });
});

describe('compareRefusal', () => {
  it('refuses fewer than two runs', () => {
    expect(compareRefusal([])).toBe('Select at least two runs on one benchmark to compare.');
    expect(compareRefusal(['a'])).toBe('Select at least two runs on one benchmark to compare.');
  });

  it('allows two runs or more', () => {
    expect(compareRefusal(['a', 'b'])).toBeNull();
  });
});

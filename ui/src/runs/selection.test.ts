import { describe, expect, it } from 'vitest';
import type { RunRow } from './model.ts';
import { compareRefusal, refusal, sanitize, toggle } from './selection.ts';

const row = (id: string, benchmark: string, over: Partial<RunRow> = {}): RunRow => ({
  id,
  pipeline: 'p',
  pipelineName: null,
  benchmark,
  benchmarkName: benchmark === 'sci' ? 'beir/scifact' : benchmark === 'fiqa' ? 'beir/fiqa' : null,
  status: { state: 'done' },
  metrics: [],
  latencyMs: null,
  startedAt: null,
  prefix: null,
  ...over,
});

const ROWS = [row('a', 'sci'), row('b', 'sci'), row('c', 'fiqa'), row('f', 'sci', { status: { state: 'failed', node: 'rerank', error: 'boom' } }), row('q', 'sci', { status: { state: 'queued' } })];

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
    expect(refusal(ROWS[0] as RunRow, [], ROWS)).toBeNull();
    expect(refusal(ROWS[2] as RunRow, [], ROWS)).toBeNull();
  });

  it('refuses a run on another benchmark once one is checked, naming the selected benchmark', () => {
    expect(refusal(ROWS[2] as RunRow, ['a'], ROWS)).toBe('The selected runs are on beir/scifact. Compare takes runs on one benchmark.');
  });

  it('allows the other runs of the same benchmark, and the selected run itself', () => {
    expect(refusal(ROWS[1] as RunRow, ['a'], ROWS)).toBeNull();
    expect(refusal(ROWS[0] as RunRow, ['a'], ROWS)).toBeNull();
  });

  it('allows every benchmark again once the selection is cleared', () => {
    const sel = toggle(toggle([], 'a'), 'a');
    expect(refusal(ROWS[2] as RunRow, sel, ROWS)).toBeNull();
  });

  it('names a benchmark without a name by its short dataset digest', () => {
    const rows = [row('x', 'd'.repeat(64)), row('y', 'e'.repeat(64))];
    expect(refusal(rows[1] as RunRow, ['x'], rows)).toBe('The selected runs are on dataset dddddddddddd. Compare takes runs on one benchmark.');
  });

  it('refuses a failed run, which has no metrics to compare', () => {
    expect(refusal(ROWS[3] as RunRow, [], ROWS)).toBe('A failed run has no metrics to compare.');
  });

  it('refuses a run that has not finished', () => {
    expect(refusal(ROWS[4] as RunRow, [], ROWS)).toBe('This run has not finished.');
  });
});

describe('sanitize', () => {
  it('keeps a valid selection as it is', () => {
    expect(sanitize(['b', 'a'], ROWS)).toEqual(['b', 'a']);
  });

  it('drops ids the listing does not hold, duplicates, and runs that cannot be selected', () => {
    expect(sanitize(['a', 'gone', 'a', 'f', 'q', 'b'], ROWS)).toEqual(['a', 'b']);
  });

  it('keeps the first run’s benchmark and drops runs on another, as a click would have refused them', () => {
    expect(sanitize(['c', 'a', 'b'], ROWS)).toEqual(['c']);
    expect(sanitize(['a', 'c', 'b'], ROWS)).toEqual(['a', 'b']);
  });
});

describe('compareRefusal', () => {
  it('refuses fewer than two runs', () => {
    expect(compareRefusal([])).toBe('Select at least two runs on one benchmark to compare.');
    expect(compareRefusal(['a'])).toBe('Select at least two runs on one benchmark to compare.');
  });

  it('allows two to five runs: a baseline and up to four', () => {
    expect(compareRefusal(['a', 'b'])).toBeNull();
    expect(compareRefusal(['a', 'b', 'c', 'd', 'e'])).toBeNull();
  });

  it('refuses a sixth run rather than invent a colour for it, saying how many to clear', () => {
    expect(compareRefusal(['a', 'b', 'c', 'd', 'e', 'f', 'g'])).toBe('Compare takes a baseline and up to four runs. Clear 2 to compare.');
  });
});

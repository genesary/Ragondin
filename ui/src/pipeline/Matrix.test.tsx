/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { MatrixCell, MatrixColumn, PipelineMatrix } from '../api/types.ts';
import { declared } from '../../design/testing/css.ts';
import css from './Pipeline.css?raw';
import { MATRIX, NAME, PREFIXED, RUN_OLD, WITH_FIQA } from './fixtures.ts';
import { Matrix } from './Matrix.tsx';

const show = (matrix: PipelineMatrix = MATRIX, metric = 'ndcg@10', launch?: (pair: { pipeline: string; benchmark: string }) => void) =>
  render(<Matrix matrix={matrix} metric={metric} {...(launch === undefined ? {} : { launch })} />);

const names = (role: 'rowheader' | 'columnheader') => screen.getAllByRole(role).map((h) => h.textContent?.replace(/\s+/g, ' ').trim());

/** The cell at a row and a column, by the header names a screen reader announces with it. */
function cellAt(node: string, benchmark: string): HTMLElement {
  const table = screen.getByRole('table');
  const row = within(table).getAllByRole('row').find((r) => r.querySelector('th[scope="row"]')?.textContent?.includes(node));
  const headers = [...table.querySelectorAll('thead th')];
  const column = headers.findIndex((h) => h.textContent?.includes(benchmark));
  const cell = row?.children[column];
  if (!(cell instanceof HTMLElement)) throw new Error(`no cell at ${node} × ${benchmark}`);
  return cell;
}

/** A matrix of one ranking row and one column, holding `cell`. */
function single(cell: MatrixCell, column: Partial<MatrixColumn> = {}): PipelineMatrix {
  return {
    ...MATRIX,
    rows: [{ node: 'rerank', family: 'reranker', produces: 'chunks' }],
    columns: [{ ...(MATRIX.columns[1] as MatrixColumn), ...column, cells: [cell] }],
  };
}

describe('the rows', () => {
  it('are the pipeline’s nodes in order, each a row header with its family tile, its name and the row’s metric', () => {
    show();
    expect(names('rowheader')).toEqual(['bm25 ndcg@10', 'dense ndcg@10', 'rrf ndcg@10', 'rerank ndcg@10', 'concat', 'generate EM, F1']);
    const rrf = screen.getByRole('rowheader', { name: /rrf/ });
    expect(rrf.getAttribute('scope')).toBe('row');
    expect(rrf.querySelector('.rg-tile[data-family="fusion"]')).toBeTruthy();
    // The pigment is never alone: the glyph is named, beside the node's name.
    expect(within(rrf).getByRole('img', { name: 'fusion' })).toBeTruthy();
  });

  it('reads the metric chosen for every ranking row', () => {
    show(MATRIX, 'mrr');
    expect(names('rowheader')[3]).toBe('rerank mrr');
  });

  it('writes the family as a word when no tile draws it', () => {
    show({ ...MATRIX, rows: MATRIX.rows.map((r, i) => (i === 4 ? { ...r, family: 'extension' } : r)) });
    expect(screen.getByRole('rowheader', { name: /concat/ }).textContent).toContain('extension');
  });

  it('says the context builder’s row in one sentence across every benchmark', () => {
    show();
    const row = screen.getByRole('rowheader', { name: /concat/ }).closest('tr') as HTMLElement;
    const cells = within(row).getAllByRole('cell');
    expect(cells).toHaveLength(1);
    expect(cells[0]?.getAttribute('colspan')).toBe('3');
    expect(cells[0]?.textContent).toBe('Not scored on any benchmark: no metric reads a context builder’s output.');
  });
});

describe('the columns', () => {
  it('are the benchmarks in the API’s order, each with its status and what its ground truth carries', () => {
    show();
    const headers = names('columnheader');
    expect(headers[0]).toBe('Node');
    expect(headers.slice(1).map((h) => h?.split(' ')[0])).toEqual(['beir/nfcorpus', 'beir/scifact', 'squad/dev']);
    const squad = screen.getByRole('columnheader', { name: /squad\/dev/ });
    expect(squad.getAttribute('scope')).toBe('col');
    expect(squad.querySelector('.rg-status[data-state="done"]')?.textContent).toContain('measured');
    expect(squad.textContent).toContain('qrels, reference answers');
  });

  it('reads “up to rerank” on a column a prefix run fills', () => {
    show(PREFIXED);
    expect(screen.getByRole('columnheader', { name: /beir\/nfcorpus/ }).textContent).toContain('up to rerank');
  });

  it('says a benchmark no run measured is not run yet, its ground truth unknown', () => {
    show(WITH_FIQA);
    const fiqa = screen.getByRole('columnheader', { name: /beir\/fiqa/ });
    expect(fiqa.textContent).toContain('not run yet');
    expect(fiqa.textContent).toContain('ground truth read once run');
    expect(fiqa.querySelector('.rg-status')).toBeNull();
  });

  it('warns on a column whose dataset is not the run’s own', () => {
    show(single({ kind: 'unverified' }, { dataset_check: { ...(MATRIX.columns[1]!.dataset_check as NonNullable<MatrixColumn['dataset_check']>), status: 'dataset_differs', detail: 'the dataset on disk digests to another version' } }));
    expect(screen.getByRole('columnheader', { name: /beir\/scifact/ }).querySelector('.rg-status[data-state="warning"]')?.textContent).toContain('dataset differs');
  });

  it('says a benchmark measured only by earlier content', () => {
    show(single({ kind: 'not_run_on_this_version', run: RUN_OLD }, { run: null, dataset_check: null }));
    expect(screen.getByRole('columnheader', { name: /beir\/scifact/ }).querySelector('.rg-status[data-state="warning"]')?.textContent).toContain('earlier version only');
  });
});

describe('a ranking cell', () => {
  it('shows the value, and on its second line the gain over the previous stage, which its name carries', () => {
    show();
    const cell = cellAt('rerank', 'beir/scifact');
    expect(cell.textContent).toBe('0.7217 +0.0216 gain over the previous stage');
    expect(cell.querySelector('.rg-matrix__value')?.textContent).toBe('0.7217');
    expect(cell.querySelector('.rg-matrix__gain')?.textContent).toContain('+0.0216');
  });

  it('emphasises the gain, never the value', () => {
    expect(declared(css, '.rg-matrix__gain', 'color')).toBe('var(--ink)');
    expect(declared(css, '.rg-matrix__value', 'color')).toBe('var(--ink-3)');
  });

  it('sets the best gain of the row in bold and says so, and never the best value', () => {
    show();
    const best = cellAt('rerank', 'beir/nfcorpus');
    expect(best.querySelector('.rg-matrix__gain')?.hasAttribute('data-best')).toBe(true);
    expect(best.textContent).toContain('(best gain in this row)');
    // scifact holds the row's best value, 0.7217, and is not bold anywhere.
    const value = cellAt('rerank', 'beir/scifact');
    expect(value.querySelector('[data-best]')).toBeNull();
    expect(value.hasAttribute('data-best')).toBe(false);
    expect(declared(css, '.rg-matrix__gain[data-best]', 'font-weight')).toBe('700');
  });

  it('marks no gain on a retrieval leg, whose value stands alone', () => {
    show();
    expect(cellAt('bm25', 'beir/scifact').textContent).toBe('0.6650 first stage — nothing before it to gain over');
    expect(cellAt('bm25', 'beir/scifact').querySelector('[data-best]')).toBeNull();
  });

  it('gives no number where the stages are a guess', () => {
    show(single({ kind: 'measured', metrics: { 'ndcg@10': 0.7 }, gain: { kind: 'ambiguous' }, judged_queries: 300 }));
    expect(cellAt('rerank', 'beir/scifact').textContent).toBe('0.7000 stages guessed — which stage came before is a guess, so no gain is given');
  });

  it('says a value with no stage', () => {
    show(single({ kind: 'measured', metrics: { 'ndcg@10': 0.7 }, gain: { kind: 'unstaged' }, judged_queries: 300 }));
    expect(cellAt('rerank', 'beir/scifact').textContent).toBe('0.7000 not a ranking stage');
  });

  it('says when the chosen metric has no figure or no gain here', () => {
    show(single({ kind: 'measured', metrics: { mrr: 0.5 }, gain: { kind: 'over_previous_stage', values: { mrr: 0.1 } }, judged_queries: 300 }));
    expect(cellAt('rerank', 'beir/scifact').textContent).toBe('no ndcg@10 figure no gain on ndcg@10');
  });
});

describe('the generator’s cells', () => {
  it('read EM and F1 where the benchmark carries reference answers', () => {
    show();
    expect(cellAt('generate', 'squad/dev').textContent).toBe('EM 41.2 · F1 49.1 not a ranking stage');
  });

  it('read “no reference answers”, hatched, elsewhere: never measurable there', () => {
    show();
    const cell = cellAt('generate', 'beir/scifact');
    expect(cell.textContent).toBe('no reference answers — never measurable on this benchmark');
    expect(cell.querySelector('[data-never]')).toBeTruthy();
    expect(declared(css, '.rg-matrix td:has(> [data-never])', 'background')).toMatch(/^repeating-linear-gradient\(-45deg, var\(--surface\) 0 4px, var\(--line\) 4px 8px\)/);
  });
});

describe('an empty cell explains itself', () => {
  it('no qrels: hatched, never measurable', () => {
    show(single({ kind: 'no_qrels' }));
    const cell = cellAt('rerank', 'beir/scifact');
    expect(cell.textContent).toBe('no qrels — never measurable on this benchmark');
    expect(cell.querySelector('[data-never]')).toBeTruthy();
  });

  it('not run yet: a Run button, refused with its reason until the launcher exists', () => {
    show(WITH_FIQA);
    const cell = cellAt('rerank', 'beir/fiqa');
    expect(cell.textContent).toContain('not run yet');
    const run = within(cell).getByRole('button', { name: 'Run on beir/fiqa' });
    expect(run.getAttribute('aria-disabled')).toBe('true');
    expect(run.getAttribute('aria-describedby')).not.toBeNull();
    expect(document.getElementById(run.getAttribute('aria-describedby') as string)?.textContent).toBe('Launching arrives with the launcher.');
    expect(cell.querySelector('[data-never]')).toBeNull();
  });

  it('not run yet: with the launcher, the Run button hands it the pipeline and the benchmark', () => {
    const launch = vi.fn();
    show(WITH_FIQA, 'ndcg@10', launch);
    const run = within(cellAt('rerank', 'beir/fiqa')).getByRole('button', { name: 'Run on beir/fiqa' });
    expect(run.hasAttribute('aria-disabled')).toBe(false);
    fireEvent.click(run);
    expect(launch).toHaveBeenCalledWith({ pipeline: NAME, benchmark: 'beir/fiqa' });
  });

  it('prefix stops: the node the prefix run stops at', () => {
    show(PREFIXED);
    expect(cellAt('generate', 'beir/nfcorpus').textContent).toBe('not run: the prefix run stops at rerank');
  });

  it('not run on this version: the run of earlier content, linked', () => {
    show(single({ kind: 'not_run_on_this_version', run: RUN_OLD }, { run: null, dataset_check: null }));
    const cell = cellAt('rerank', 'beir/scifact');
    expect(cell.textContent).toContain('not run on this version');
    expect(within(cell).getByRole('link', { name: `run ${RUN_OLD.slice(0, 12)}` }).getAttribute('href')).toBe(`#replay/${RUN_OLD}`);
  });

  it('unverified, not scored, no figure: each in its words', () => {
    show(single({ kind: 'unverified' }));
    expect(cellAt('rerank', 'beir/scifact').textContent).toBe('unverified — the dataset on disk is not the run’s own');
  });

  it('not scored and no figure, side by side in a row that is not all unscored', () => {
    const base = single({ kind: 'not_scored' });
    show({ ...base, columns: [...base.columns, { ...(MATRIX.columns[0] as MatrixColumn), cells: [{ kind: 'no_figure' }] }] });
    expect(cellAt('rerank', 'beir/scifact').textContent).toBe('not scored — no metric reads this node’s output');
    expect(cellAt('rerank', 'beir/nfcorpus').textContent).toBe('no figure — run here, and no figure came out');
  });
});

describe('the table for assistive technology', () => {
  it('is one named table with column and row headers, every data cell under both', () => {
    show();
    const table = screen.getByRole('table', { name: `${NAME}: each node on each benchmark` });
    expect(within(table).getAllByRole('columnheader')).toHaveLength(4);
    expect(within(table).getAllByRole('rowheader')).toHaveLength(6);
    for (const row of within(table).getAllByRole('row').slice(1)) {
      expect(row.firstElementChild?.getAttribute('scope')).toBe('row');
    }
  });

  it('is a named region in the tab order, so the keyboard can scroll it sideways', () => {
    show();
    expect(screen.getByRole('region', { name: `${NAME}: each node on each benchmark` }).getAttribute('tabindex')).toBe('0');
  });

  it('keeps every cell two lines tall, whatever its state, so nothing moves when a metric is chosen', () => {
    expect(declared(css, '.rg-matrix__cell', 'min-height')).toBe('calc(2 * var(--space-5))');
  });
});

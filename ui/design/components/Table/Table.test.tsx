/** @vitest-environment happy-dom */
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Table.css?raw';
import { Table, type TableColumn, type TableRow } from './Table.tsx';

const columns: TableColumn[] = [
  { id: 'metric', label: 'Metric' },
  { id: 'base', label: 'dense-only', numeric: true },
  { id: 'a', label: 'hybrid-rerank', numeric: true },
];
const rows: TableRow[] = [
  { kind: 'group', id: 'ranking', label: 'Ranking' },
  { id: 'ndcg', cells: ['nDCG@10', '0.6483', '0.7217'], bestColumn: 2 },
  { id: 'recall', cells: ['Recall@100', '0.9120', '0.9050'], bestColumn: 1, selected: true },
];

describe('Table dense', () => {
  it('is a real table at the working size, with tabular figures and a sticky header', () => {
    render(<Table caption="Metrics of three runs" columns={columns} rows={rows} />);
    const table = screen.getByRole('table', { name: 'Metrics of three runs' });
    expect(table.classList.contains('rg-table')).toBe(true);
    expect(declared(css, '.rg-table', 'font')).toBe('var(--type-dense)');
    expect(declared(css, '.rg-table', 'font-variant-numeric')).toBe('tabular-nums');
    expect(declared(css, '.rg-table th', 'position')).toBe('sticky');
    expect(within(table).getAllByRole('columnheader')).toHaveLength(3);
  });

  it('scrolls sideways inside its own wrapper, never the page', () => {
    const { container } = render(<Table caption="m" columns={columns} rows={rows} />);
    expect(container.querySelector('.rg-tablewrap > table')).toBeTruthy();
    expect(declared(css, '.rg-tablewrap', 'overflow-x')).toBe('auto');
  });
});

describe('Table numeric alignment', () => {
  it('right-aligns numeric columns, header and cells, and leaves labels left', () => {
    render(<Table caption="m" columns={columns} rows={rows} />);
    const header = screen.getByRole('columnheader', { name: 'hybrid-rerank' });
    expect(header.classList.contains('num')).toBe(true);
    expect(screen.getByRole('columnheader', { name: 'Metric' }).classList.contains('num')).toBe(false);
    expect(screen.getByText('0.6483').closest('td')?.classList.contains('num')).toBe(true);
    expect(screen.getByText('nDCG@10').closest('th,td')?.classList.contains('num')).toBe(false);
    expect(declared(css, '.rg-table .num', 'text-align')).toBe('right');
  });
});

describe('Table best per row', () => {
  it('sets exactly the best value of each row in bold, and says so in words', () => {
    const { container } = render(<Table caption="m" columns={columns} rows={rows} />);
    const best = [...container.querySelectorAll('td[data-best]')];
    expect(best.map((td) => td.textContent)).toEqual(['0.7217 (best)', '0.9120 (best)']);
    expect(declared(css, '.rg-table td[data-best]', 'font-weight')).toBe('700');
    expect(container.querySelectorAll('td[data-best] .rg-visually-hidden')).toHaveLength(2);
    expect(css).not.toMatch(/\.is-[a-z]/);
  });
});

describe('Table groups and selection', () => {
  it('heads a group of rows with its name', () => {
    render(<Table caption="m" columns={columns} rows={rows} />);
    expect(screen.getByRole('rowheader', { name: 'Ranking' }).getAttribute('colspan')).toBe('3');
  });

  it('marks the selected row with aria-current, which a table row honours, and a bar beside it, not a tint alone', () => {
    render(<Table caption="m" columns={columns} rows={rows} />);
    const selected = screen.getByText('Recall@100').closest('tr');
    expect(selected?.getAttribute('aria-current')).toBe('true');
    expect(selected?.getAttribute('aria-selected')).toBeNull();
    expect(screen.getByText('nDCG@10').closest('tr')?.getAttribute('aria-current')).toBeNull();
    expect(declared(css, '.rg-table tr[aria-current="true"] td', 'background')).toBe('var(--accent-wash)');
    expect(declared(css, '.rg-table tr[aria-current="true"] > :first-child', 'box-shadow')).toBe('inset 2px 0 0 var(--accent)');
  });
});

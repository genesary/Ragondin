/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
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

  it('marks every column of a tie, since the best can be held by several', () => {
    const tie: TableRow[] = [{ id: 'recall', cells: ['Recall@100', '0.9310', '0.9310'], bestColumn: [1, 2] }];
    const { container } = render(<Table caption="m" columns={columns} rows={tie} />);
    expect([...container.querySelectorAll('td[data-best]')].map((td) => td.textContent)).toEqual(['0.9310 (best)', '0.9310 (best)']);
  });
});

describe('Table row groups', () => {
  const grouped: TableRow[] = [
    { id: 'loose', cells: ['Loose', '1', '2'] },
    { kind: 'group', id: 'hybrid', label: <a href="#pipeline/hybrid">hybrid</a> },
    { id: 'r1', cells: ['r1', '1', '2'] },
    { kind: 'group', id: 'dense', label: 'dense' },
    { id: 'r2', cells: ['r2', '1', '2'] },
  ];

  it('takes any content as a group’s heading, a link included', () => {
    render(<Table caption="m" columns={columns} rows={grouped} />);
    const header = screen.getByRole('rowheader', { name: 'hybrid' });
    expect(within(header).getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
  });

  it('gives each group its own tbody, headed by its row-group header, and rows before the first group a tbody of their own', () => {
    const { container } = render(<Table caption="m" columns={columns} rows={grouped} />);
    const bodies = [...container.querySelectorAll('tbody')];
    expect(bodies.map((b) => [...b.querySelectorAll('tr')].length)).toEqual([1, 2, 2]);
    expect(bodies[1]?.querySelector('tr:first-child th')?.getAttribute('scope')).toBe('rowgroup');
    expect(bodies[2]?.querySelector('tr:first-child th')?.textContent).toBe('dense');
  });
});

describe('Table rows that take the keyboard', () => {
  const live: TableRow[] = [
    { kind: 'group', id: 'g', label: 'hybrid' },
    { id: 'r1', label: 'Run r1', cells: ['r1', <input key="c" type="checkbox" aria-label="pick r1" />, '2'] },
    { id: 'wait', passive: true, cells: ['queued', '', ''] },
    { id: 'r2', label: 'Run r2', cells: ['r2', '1', '2'] },
    { id: 'r3', label: 'Run r3', cells: ['r3', '1', '2'] },
  ];
  const rowOf = (name: string) => screen.getByRole('row', { name });

  function show() {
    const onOpen = vi.fn();
    const onToggle = vi.fn();
    render(<Table caption="m" columns={columns} rows={live} onOpen={onOpen} onToggle={onToggle} />);
    return { onOpen, onToggle };
  }

  it('names each row by its label', () => {
    show();
    expect(rowOf('Run r1').tagName).toBe('TR');
  });

  it('is one tab stop: the first row that takes the keyboard, the others reached by arrows', () => {
    show();
    expect(rowOf('Run r1').getAttribute('tabindex')).toBe('0');
    expect(rowOf('Run r2').getAttribute('tabindex')).toBe('-1');
    expect(screen.getByText('queued').closest('tr')?.hasAttribute('tabindex')).toBe(false);
  });

  it('moves focus and the tab stop with the up and down arrows, skipping a passive row, and to the ends with Home and End', () => {
    show();
    rowOf('Run r1').focus();
    fireEvent.keyDown(rowOf('Run r1'), { key: 'ArrowDown' });
    expect(document.activeElement).toBe(rowOf('Run r2'));
    expect(rowOf('Run r2').getAttribute('tabindex')).toBe('0');
    expect(rowOf('Run r1').getAttribute('tabindex')).toBe('-1');
    fireEvent.keyDown(rowOf('Run r2'), { key: 'End' });
    expect(document.activeElement).toBe(rowOf('Run r3'));
    fireEvent.keyDown(rowOf('Run r3'), { key: 'ArrowUp' });
    expect(document.activeElement).toBe(rowOf('Run r2'));
    fireEvent.keyDown(rowOf('Run r2'), { key: 'Home' });
    expect(document.activeElement).toBe(rowOf('Run r1'));
  });

  it('stops at the ends rather than wrapping round', () => {
    show();
    fireEvent.keyDown(rowOf('Run r3'), { key: 'ArrowDown' });
    expect(document.activeElement).toBe(rowOf('Run r3'));
    fireEvent.keyDown(rowOf('Run r1'), { key: 'ArrowUp' });
    expect(document.activeElement).toBe(rowOf('Run r1'));
  });

  it('takes the keyboard with only one of the two actions', () => {
    render(<Table caption="m" columns={columns} rows={live} onToggle={() => {}} />);
    expect(rowOf('Run r1').getAttribute('tabindex')).toBe('0');
  });

  it('opens a row with Enter and toggles it with Space', () => {
    const { onOpen, onToggle } = show();
    fireEvent.keyDown(rowOf('Run r2'), { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledWith('r2');
    const space = fireEvent.keyDown(rowOf('Run r2'), { key: ' ' });
    expect(onToggle).toHaveBeenCalledWith('r2');
    // The page does not scroll as well.
    expect(space).toBe(false);
  });

  it('leaves a key pressed on a control inside a row to that control', () => {
    const { onOpen, onToggle } = show();
    fireEvent.keyDown(screen.getByRole('checkbox', { name: 'pick r1' }), { key: ' ' });
    fireEvent.keyDown(screen.getByRole('checkbox', { name: 'pick r1' }), { key: 'Enter' });
    expect(onToggle).not.toHaveBeenCalled();
    expect(onOpen).not.toHaveBeenCalled();
  });

  it('makes a row the tab stop when focus enters it, by a click on a control inside included', () => {
    show();
    fireEvent.focus(screen.getByRole('checkbox', { name: 'pick r1' }));
    fireEvent.focus(rowOf('Run r3'));
    expect(rowOf('Run r3').getAttribute('tabindex')).toBe('0');
    expect(rowOf('Run r1').getAttribute('tabindex')).toBe('-1');
  });

  it('takes no keyboard at all without an action', () => {
    render(<Table caption="m" columns={columns} rows={live} />);
    expect(screen.getByText('r1').closest('tr')?.hasAttribute('tabindex')).toBe(false);
  });

  it('draws the focus ring inside the row, so the table’s scroll box does not clip it', () => {
    expect(declared(css, '.rg-table tbody tr:focus-visible', 'outline-offset')).toBe('-2px');
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

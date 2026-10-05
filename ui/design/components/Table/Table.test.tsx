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

  it('holds its cells’ visually hidden text inside that wrapper, so text heard and not seen never widens the page', () => {
    // .rg-visually-hidden is absolutely positioned: without a positioned
    // wrapper its containing block lies outside the scroll box, and a hidden
    // span in a far column stretches the page sideways.
    expect(declared(css, '.rg-tablewrap', 'position')).toBe('relative');
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

describe('Table row headers', () => {
  const matrix: TableRow[] = [
    { id: 'rrf', cells: ['rrf', '0.6100', '0.3300'] },
    { id: 'concat', span: true, cells: ['concat', 'Not scored: no metric reads this node.'] },
  ];

  it('makes each row’s first cell its row header, so a cell is announced with its row and its column', () => {
    render(<Table caption="m" columns={columns} rows={matrix} rowHeaders />);
    const header = screen.getByRole('rowheader', { name: 'rrf' });
    expect(header.tagName).toBe('TH');
    expect(header.getAttribute('scope')).toBe('row');
    expect(screen.getAllByRole('cell').map((c) => c.textContent)).toEqual(['0.6100', '0.3300', 'Not scored: no metric reads this node.']);
  });

  it('keeps the row headers in view while the table scrolls sideways, the corner above every other header', () => {
    expect(declared(css, '.rg-table th[scope="row"]', 'position')).toBe('sticky');
    expect(declared(css, '.rg-table th[scope="row"]', 'left')).toBe('0');
    expect(declared(css, '.rg-table th[scope="row"]', 'top')).toBe('auto');
    expect(declared(css, '.rg-table[data-row-headers] thead th:first-child', 'left')).toBe('0');
    expect(declared(css, '.rg-table[data-row-headers] thead th:first-child', 'z-index')).toBe('2');
  });

  it('leaves the first cell a data cell without the option', () => {
    render(<Table caption="m" columns={columns} rows={matrix} />);
    expect(screen.queryByRole('rowheader', { name: 'rrf' })).toBeNull();
  });

  it('spans a row without row headers too, its label then a data cell', () => {
    render(<Table caption="m" columns={columns} rows={matrix} />);
    expect(screen.getByRole('cell', { name: 'concat' })).toBeTruthy();
    expect(screen.getByRole('cell', { name: 'Not scored: no metric reads this node.' }).getAttribute('colspan')).toBe('2');
  });

  it('spans a row’s one sentence across every column after its label', () => {
    render(<Table caption="m" columns={columns} rows={matrix} rowHeaders />);
    const sentence = screen.getByRole('cell', { name: 'Not scored: no metric reads this node.' });
    expect(sentence.getAttribute('colspan')).toBe('2');
  });
});

describe('Table as a scroll region', () => {
  it('names its scroll box and puts it in the tab order, so a keyboard can scroll a table wider than the screen', () => {
    render(<Table caption="Matrix" columns={columns} rows={rows} region />);
    const region = screen.getByRole('region', { name: 'Matrix' });
    expect(region.classList.contains('rg-tablewrap')).toBe(true);
    expect(region.getAttribute('tabindex')).toBe('0');
  });

  it('is no region and no tab stop without the option', () => {
    const { container } = render(<Table caption="Matrix" columns={columns} rows={rows} />);
    expect(screen.queryByRole('region')).toBeNull();
    expect(container.querySelector('.rg-tablewrap')?.hasAttribute('tabindex')).toBe(false);
  });
});

describe('Table wider than its box', () => {
  // happy-dom lays nothing out: the box's widths are set by hand, and a scroll event asks the table to look again.
  const sized = (box: HTMLElement, scrollWidth: number, clientWidth: number) => {
    Object.defineProperty(box, 'scrollWidth', { configurable: true, value: scrollWidth });
    Object.defineProperty(box, 'clientWidth', { configurable: true, value: clientWidth });
  };

  it('shows which sides hold more, a fade on each edge it can still scroll towards', () => {
    const { container } = render(<Table caption="m" columns={columns} rows={rows} />);
    const box = container.querySelector('.rg-tablewrap') as HTMLElement;
    const frame = box.parentElement as HTMLElement;
    expect(frame.classList.contains('rg-tableframe')).toBe(true);
    expect(frame.hasAttribute('data-more-end')).toBe(false);
    sized(box, 900, 300);
    fireEvent.scroll(box);
    expect(frame.getAttribute('data-more-end')).toBe('true');
    expect(frame.hasAttribute('data-more-start')).toBe(false);
    box.scrollLeft = 600;
    fireEvent.scroll(box);
    expect(frame.getAttribute('data-more-start')).toBe('true');
    expect(frame.hasAttribute('data-more-end')).toBe(false);
  });

  it('draws each fade from the surface the table sits on, above the sticky headers, and lets the pointer through', () => {
    expect(declared(css, '.rg-tableframe', 'position')).toBe('relative');
    expect(declared(css, '.rg-tableframe[data-more-end]::after', 'background')).toBe('linear-gradient(to left, var(--surface), transparent)');
    expect(declared(css, '.rg-tableframe[data-more-start]::before', 'background')).toBe('linear-gradient(to right, var(--surface), transparent)');
    expect(declared(css, '.rg-tableframe::before', 'pointer-events')).toBe('none');
    expect(declared(css, '.rg-tableframe::after', 'pointer-events')).toBe('none');
    expect(declared(css, '.rg-tableframe::after', 'z-index')).toBe('3');
  });
});

import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { tabStop } from '../../roving.ts';
import './Table.css';

export type TableColumn = {
  id: string;
  label: ReactNode;
  /** Right-aligned, header and cells. */
  numeric?: boolean;
};

export type TableRow =
  | {
      kind?: 'row';
      id: string;
      /** One cell per column, in column order; the first is the row's label. */
      cells: readonly ReactNode[];
      /** The column holding this row's best value, set in bold — or every column of a tie. */
      bestColumn?: number | readonly number[];
      /** The row that drives a chart highlight; marked with aria-current, which a table row honours. */
      selected?: boolean;
      /** The row's accessible name, when it takes the keyboard. */
      label?: string;
      /** A row that takes no focus and no key even when the table's rows do: a placeholder. */
      passive?: boolean;
      /**
       * A row that says one thing for every column: two cells, its label and
       * one sentence spanning every column after it.
       */
      span?: boolean;
    }
  | {
      kind: 'group';
      id: string;
      /** The group's heading: a name, or a name with what describes the group — a link, tiles, a count. */
      label: ReactNode;
    };

export type TableProps = {
  /** Names the table for assistive technology. */
  caption: string;
  columns: readonly TableColumn[];
  rows: readonly TableRow[];
  /**
   * Enter on a row. Either this or `onToggle` makes the rows one tab stop,
   * moved by the up and down arrows — stopping at the ends — Home and End.
   */
  onOpen?: (id: string) => void;
  /** Space on a row; either this or `onOpen` makes the rows one tab stop. */
  onToggle?: (id: string) => void;
  /**
   * Each row's first cell is its header (`th scope="row"`), kept in view
   * while the table scrolls sideways, so a cell is announced with its row as
   * well as its column: a matrix rather than a list.
   */
  rowHeaders?: boolean;
  /**
   * The scroll box is a region named by the caption and a tab stop, so a
   * keyboard can scroll a table wider than the screen.
   */
  region?: boolean;
};

type Row = Extract<TableRow, { cells: readonly ReactNode[] }>;
type Group = { head: Extract<TableRow, { kind: 'group' }> | null; rows: Row[] };

/** The rows split at each group heading, so each group becomes its own tbody. */
function bodies(rows: readonly TableRow[]): Group[] {
  const groups: Group[] = [];
  for (const row of rows) {
    if (row.kind === 'group') groups.push({ head: row, rows: [] });
    else if (groups.length === 0) groups.push({ head: null, rows: [row] });
    else (groups[groups.length - 1] as Group).rows.push(row);
  }
  return groups;
}

/**
 * The dense numeric table: tabular figures, numbers right-aligned, the best
 * value per row in bold (typographic, never coloured) and rows grouped by what
 * they measure, each group its own tbody under a row-group header. Wide
 * tables scroll inside their own wrapper. Given an action, its rows take the
 * keyboard as one tab stop: the arrows move between rows, Enter opens one and
 * Space toggles it; a key pressed on a control inside a row stays that
 * control's.
 */
export function Table({ caption, columns, rows, onOpen, onToggle, rowHeaders = false, region = false }: TableProps) {
  const live = onOpen !== undefined || onToggle !== undefined;
  const groups = bodies(rows);
  const keyed = groups.flatMap((g) => g.rows).filter((r) => live && !r.passive);
  const [active, setActive] = useState<string | null>(null);
  const elements = useRef(new Map<string, HTMLTableRowElement>());
  const stop = keyed[tabStop(keyed.map(() => false), keyed.findIndex((r) => r.id === active))]?.id;

  // Which sides of a table wider than its box hold more, so the frame draws a fade on each: a table cut at the
  // screen's edge otherwise reads as whole on a phone, whose scrollbars show only while scrolling.
  const box = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState({ start: false, end: false });
  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const look = () => {
      const start = el.scrollLeft > 1;
      const end = el.scrollLeft < el.scrollWidth - el.clientWidth - 1;
      setMore((m) => (m.start === start && m.end === end ? m : { start, end }));
    };
    look();
    el.addEventListener('scroll', look, { passive: true });
    const resized = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(look);
    resized?.observe(el);
    if (el.firstElementChild !== null) resized?.observe(el.firstElementChild);
    return () => {
      el.removeEventListener('scroll', look);
      resized?.disconnect();
    };
  }, []);

  const move = (to: number) => {
    const target = keyed[to];
    if (target === undefined) return;
    setActive(target.id);
    elements.current.get(target.id)?.focus();
  };

  const onKeyDown = (row: Row) => (event: KeyboardEvent<HTMLTableRowElement>) => {
    if (event.target !== event.currentTarget) return;
    const at = keyed.findIndex((r) => r.id === row.id);
    const handled = () => event.preventDefault();
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp':
        handled();
        // Clamped, not wrapped: a table's ends are its first and last rows, as a list's are.
        move(Math.min(Math.max(at + (event.key === 'ArrowDown' ? 1 : -1), 0), keyed.length - 1));
        break;
      case 'Home':
        handled();
        move(0);
        break;
      case 'End':
        handled();
        move(keyed.length - 1);
        break;
      case 'Enter':
        handled();
        onOpen?.(row.id);
        break;
      case ' ':
        handled();
        onToggle?.(row.id);
        break;
    }
  };

  return (
    <div className="rg-tableframe" data-more-start={more.start ? true : undefined} data-more-end={more.end ? true : undefined}>
      <div ref={box} className="rg-tablewrap" {...(region ? { role: 'region', 'aria-label': caption, tabIndex: 0 } : {})}>
        <table className="rg-table" aria-label={caption} data-row-headers={rowHeaders ? true : undefined}>
          <thead>
            <tr>
              {columns.map((c) => (
                <th key={c.id} scope="col" className={c.numeric ? 'num' : undefined}>
                  {c.label}
                </th>
              ))}
            </tr>
          </thead>
          {groups.map((group, g) => (
            <tbody key={group.head?.id ?? `rows-${g}`}>
              {group.head === null ? null : (
                <tr className="rg-table__group">
                  <th scope="rowgroup" colSpan={columns.length}>
                    {group.head.label}
                  </th>
                </tr>
              )}
              {group.rows.map((row) => {
                const takesKeys = live && !row.passive;
                return (
                  <tr
                    key={row.id}
                    ref={(el) => {
                      if (el === null) elements.current.delete(row.id);
                      else elements.current.set(row.id, el);
                    }}
                    aria-current={row.selected ? true : undefined}
                    aria-label={takesKeys ? row.label : undefined}
                    tabIndex={takesKeys ? (row.id === stop ? 0 : -1) : undefined}
                    onKeyDown={takesKeys ? onKeyDown(row) : undefined}
                    onFocus={takesKeys ? () => setActive(row.id) : undefined}
                  >
                    {row.cells.map((cell, i) => {
                      const key = columns[i]?.id ?? i;
                      if (i === 0 && rowHeaders) {
                        return (
                          <th key={key} scope="row">
                            {cell}
                          </th>
                        );
                      }
                      if (i === 1 && row.span) {
                        return (
                          <td key={key} colSpan={columns.length - 1}>
                            {cell}
                          </td>
                        );
                      }
                      const best = typeof row.bestColumn === 'number' ? i === row.bestColumn : (row.bestColumn?.includes(i) ?? false);
                      return (
                        <td key={key} className={columns[i]?.numeric ? 'num' : undefined} data-best={best ? true : undefined}>
                          {cell}
                          {best ? <span className="rg-visually-hidden"> (best)</span> : null}
                        </td>
                      );
                    })}
                  </tr>
                );
              })}
            </tbody>
          ))}
        </table>
      </div>
    </div>
  );
}

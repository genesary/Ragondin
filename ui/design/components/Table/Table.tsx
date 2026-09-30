import type { ReactNode } from 'react';
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
      /** The column holding this row's best value, set in bold. */
      bestColumn?: number;
      /** The row that drives a chart highlight. */
      selected?: boolean;
    }
  | { kind: 'group'; id: string; label: string };

export type TableProps = {
  /** Names the table for assistive technology. */
  caption: string;
  columns: readonly TableColumn[];
  rows: readonly TableRow[];
};

/**
 * The dense numeric table: tabular figures, numbers right-aligned, the best
 * value per row in bold (typographic, never coloured) and rows grouped by what
 * they measure. Wide tables scroll inside their own wrapper.
 */
export function Table({ caption, columns, rows }: TableProps) {
  return (
    <div className="rg-tablewrap">
      <table className="rg-table" aria-label={caption}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.id} scope="col" className={c.numeric ? 'num' : undefined}>
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) =>
            row.kind === 'group' ? (
              <tr key={row.id} className="rg-table__group">
                <th scope="rowgroup" colSpan={columns.length}>
                  {row.label}
                </th>
              </tr>
            ) : (
              <tr key={row.id} aria-selected={row.selected ? true : undefined}>
                {row.cells.map((cell, i) => {
                  const best = i === row.bestColumn;
                  const classes = [columns[i]?.numeric ? 'num' : '', best ? 'is-best' : ''].filter(Boolean).join(' ');
                  return (
                    <td key={columns[i]?.id ?? i} className={classes || undefined}>
                      {cell}
                      {best ? <span className="rg-visually-hidden"> (best)</span> : null}
                    </td>
                  );
                })}
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

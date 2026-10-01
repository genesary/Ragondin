import { useId, useState, type ReactNode } from 'react';
import { Button } from '../components/Button/Button.tsx';
import './Charts.css';

export type LegendItem = {
  id: string;
  /** What the mark stands for, in words: the legend's text is the carrier colour never is alone. */
  label: string;
  /** The swatch, tile or glyph drawn in the chart for it. */
  mark: ReactNode;
};

export type ChartFrameProps = {
  /** What the chart shows, as a phrase; it names the figure. */
  caption: string;
  /** Always present: every hue on screen is data a legend explains. */
  legend: readonly LegendItem[];
  /** The same data as a table: the chart's equivalent for assistive technology and for anyone who wants the numbers. */
  table: ReactNode;
  /** A line under the caption: how to read the chart. */
  note?: ReactNode;
  children: ReactNode;
};

/**
 * A chart's figure: its caption, its legend, the plot, and a button that
 * shows the same data as a table. The plot is drawn for the eye; the table is
 * its text equivalent, one keyboard stop away.
 */
export function ChartFrame({ caption, legend, table, note, children }: ChartFrameProps) {
  if (legend.length === 0) throw new Error(`ChartFrame "${caption}" has no legend: every chart carries one.`);
  const captionId = useId();
  const tableId = useId();
  const [showTable, setShowTable] = useState(false);
  return (
    <figure className="rg-chart" aria-labelledby={captionId}>
      <figcaption className="rg-chart__head">
        <span id={captionId} className="rg-chart__caption">
          {caption}
        </span>
        {note === undefined ? null : <span className="rg-chart__note">{note}</span>}
      </figcaption>
      <ul className="rg-legend" aria-label="Legend">
        {legend.map((item) => (
          <li key={item.id} className="rg-legend__item">
            {item.mark}
            <span>{item.label}</span>
          </li>
        ))}
      </ul>
      <div className="rg-chart__plot">{children}</div>
      <div className="rg-chart__foot">
        <Button kind="quiet" size="s" aria-describedby={captionId} aria-expanded={showTable} aria-controls={tableId} onClick={() => setShowTable((v) => !v)}>
          {showTable ? 'Hide the table' : 'Show as a table'}
        </Button>
      </div>
      <div id={tableId} className="rg-chart__table" hidden={!showTable}>
        {table}
      </div>
    </figure>
  );
}

/**
 * The hover layer's box, at a point given as a share of the plot (0–1 on each
 * axis). It repeats what the table holds, so it is hidden from assistive
 * technology rather than announced on every pointer move.
 */
export function ChartTooltip({ x, y, children }: { x: number; y: number; children: ReactNode }) {
  return (
    <div className="rg-chart__tip" aria-hidden="true" style={{ left: `${x * 100}%`, top: `${y * 100}%` }}>
      {children}
    </div>
  );
}

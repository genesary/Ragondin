// One run of the store as a row of design/'s Table — a job of the queue is
// `jobRow.tsx`'s. Every part is design/'s — Checkbox, StatusChip, MetricChip,
// Glyph — and this file only fills the cells. A row
// draws what its run has and nothing for what it lacks: no dash stands in for
// a metric the benchmark's ground truth could not produce. The row takes the
// keyboard as one of the table's single tab stop, so the checkbox and the link
// inside it leave the tab order; a click still reaches both.
import type { ReactNode } from 'react';
import { Checkbox, Glyph, MetricChip, StatusChip, type TableRow } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { benchmarkLabel, formatLatency, formatMetric, metricLabel, openRoute, otherFact, rowKey, shortHash, type RunRow } from './model.ts';
import type { Refusal } from './selection.ts';

/** The optional columns, drawn only when some row of the table has their data. */
export type RowColumns = { latency: boolean; started: boolean };

export type RunRowOptions = {
  selected: boolean;
  /** Why the run cannot be checked now; null when it can. */
  refusal: Refusal | null;
  columns: RowColumns;
  onToggle: () => void;
};

const started = (iso: string) => new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

function metrics(row: RunRow): ReactNode {
  return (
    <span className="rg-runs__metrics">
      {row.metrics.map((group) =>
        group.family === null ? (
          <span key="unsaid" className="rg-runs__family">
            {group.metrics.map((m) => (
              <MetricChip key={m.name} name={metricLabel(m.name)} value={formatMetric(null, m.value)} />
            ))}
          </span>
        ) : (
          <span key={group.family} className="rg-runs__family" role="group" aria-label={group.family}>
            <span className="rg-runs__family-label" aria-hidden="true">
              {group.family}
            </span>
            {group.metrics.map((m) => (
              <MetricChip key={m.name} name={group.family === 'unknown' ? m.name : metricLabel(m.name)} value={formatMetric(group.family, m.value)} />
            ))}
          </span>
        ),
      )}
    </span>
  );
}

/** `row`, a run of the store, as a Table row: its key, its name, its cells. */
export function runRow(row: RunRow, { selected, refusal, columns, onToggle }: RunRowOptions): TableRow {
  const extra: ReactNode[] = [
    ...(columns.latency ? [row.latencyMs === null ? null : formatLatency(row.latencyMs)] : []),
    ...(columns.started ? [row.startedAt === null ? null : <time dateTime={row.startedAt}>{started(row.startedAt)}</time>] : []),
  ];

  const bench = benchmarkLabel(row);
  const short = shortHash(row.source.id);
  const name = `Run ${short} on ${bench}`;
  const box =
    refusal === null ? (
      <Checkbox label={bench} accessibleLabel={`Select run ${short} on ${bench}`} checked={selected} onChange={onToggle} tabIndex={-1} />
    ) : (
      <Checkbox
        label={bench}
        accessibleLabel={`Select run ${short} on ${bench}`}
        checked={selected}
        onChange={onToggle}
        tabIndex={-1}
        disabled
        disabledReason={refusal.short}
      />
    );
  const fact = otherFact(row);
  const run = (
    <>
      <a className="rg-runs__hash" href={formatHash(openRoute(row))} tabIndex={-1}>
        {short}
      </a>
      {row.prefix === null ? null : (
        <span className="rg-runs__prefix">
          <Glyph name="prefix" />
          {row.prefix.upTo === null ? 'prefix' : `prefix up to ${row.prefix.upTo}`}
        </span>
      )}
      {/* The fact the group is not headed by: text from the first draw, so nothing moves in later. */}
      {fact === null ? null : <span className="rg-runs__fact">{fact}</span>}
      {row.announced === null ? null : (
        <span className="rg-runs__fact">
          announced as {shortHash(row.announced)}; filed under this id because what ran differs from what was announced
        </span>
      )}
    </>
  );

  // The row keeps focus when Space toggles it, so its name carries the state:
  // a change to it is what a screen reader announces.
  const state = selected ? ', selected' : refusal === null ? '' : `, cannot be selected: ${refusal.short}`;
  return { id: rowKey(row), label: `${name}${state}`, cells: [box, run, <StatusChip state="done" />, metrics(row), ...extra] };
}

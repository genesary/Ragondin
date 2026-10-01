// One run as a row of design/'s Table. Every part is design/'s — Checkbox,
// StatusChip, MetricChip, Glyph — and this file only fills the cells. A row
// draws what its run has and nothing for what it lacks: no dash stands in for
// a metric the benchmark's ground truth could not produce. The row takes the
// keyboard as one of the table's single tab stop, so the checkbox and the link
// inside it leave the tab order; a click still reaches both.
import type { ReactNode } from 'react';
import { Checkbox, Glyph, MetricChip, StatusChip, type TableRow } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { benchmarkLabel, formatMetric, openRoute, rowKey, runningLabel, shortHash, type RunRow } from './model.ts';
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

/** The failure as a sentence, beside the chip: colour is never the only carrier. */
const failure = (node: string | null, error: string) => (node === null ? `The run failed: ${error}` : `${node} failed: ${error}`);

const started = (iso: string) => new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

/** The id a row prints: its run's, else its job's. */
const printed = (row: RunRow) => (row.source.kind === 'run' ? row.source.id : (row.source.runId ?? row.source.id));

function metrics(row: RunRow): ReactNode {
  return (
    <span className="rg-runs__metrics">
      {row.metrics.map((group) =>
        group.family === null ? (
          <span key="unsaid" className="rg-runs__family">
            {group.metrics.map((m) => (
              <MetricChip key={m.name} name={m.name} value={formatMetric(null, m.value)} />
            ))}
          </span>
        ) : (
          <span key={group.family} className="rg-runs__family" role="group" aria-label={group.family}>
            <span className="rg-runs__family-label" aria-hidden="true">
              {group.family}
            </span>
            {group.metrics.map((m) => (
              <MetricChip key={m.name} name={m.name} value={formatMetric(group.family, m.value)} />
            ))}
          </span>
        ),
      )}
    </span>
  );
}

/** `row` as a Table row: its key, its name, its cells. */
export function runRow(row: RunRow, { selected, refusal, columns, onToggle }: RunRowOptions): TableRow {
  const { status } = row;
  const extra: ReactNode[] = [
    ...(columns.latency ? [row.latencyMs === null ? null : `${Math.round(row.latencyMs).toLocaleString('en-US')} ms`] : []),
    ...(columns.started ? [row.startedAt === null ? null : <time dateTime={row.startedAt}>{started(row.startedAt)}</time>] : []),
  ];

  if (status.state === 'queued' || status.state === 'running' || status.state === 'cancelled') {
    // The job queue's slot: the chip alone until the queue fills the row.
    const chip =
      status.state === 'running' ? (
        <StatusChip state="running" fraction={status.total > 0 ? status.done / status.total : 0}>
          {runningLabel(status)}
        </StatusChip>
      ) : (
        <StatusChip state={status.state} />
      );
    return { id: rowKey(row), passive: true, cells: [null, null, chip, null, ...extra] };
  }

  const bench = benchmarkLabel(row);
  const short = shortHash(printed(row));
  const to = openRoute(row);
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
  const run = (
    <>
      {to === null ? (
        <code className="rg-runs__hash">{short}</code>
      ) : (
        <a className="rg-runs__hash" href={formatHash(to)} tabIndex={-1}>
          {short}
        </a>
      )}
      {row.prefix === null ? null : (
        <span className="rg-runs__prefix">
          <Glyph name="prefix" />
          {row.prefix.upTo === null ? 'prefix' : `prefix up to ${row.prefix.upTo}`}
        </span>
      )}
    </>
  );

  if (status.state === 'failed') {
    return {
      id: rowKey(row),
      label: status.node === null ? `${name}, failed` : `${name}, failed at ${status.node}`,
      cells: [
        box,
        run,
        <StatusChip state="failed">
          {status.node === null ? (
            'failed'
          ) : (
            <>
              failed at <code>{status.node}</code>
            </>
          )}
        </StatusChip>,
        <span className="rg-runs__failure">{failure(status.node, status.error)}</span>,
        ...extra,
      ],
    };
  }

  // The row keeps focus when Space toggles it, so its name carries the state:
  // a change to it is what a screen reader announces.
  const state = selected ? ', selected' : refusal === null ? '' : `, cannot be selected: ${refusal.short}`;
  return { id: rowKey(row), label: `${name}${state}`, cells: [box, run, <StatusChip state="done" />, metrics(row), ...extra] };
}

// One run as a row of the Runs table. Every part is design/'s — Checkbox,
// StatusChip, MetricChip, Glyph — and this file only places them. A row draws
// what its run has and nothing for what it lacks: no dash stands in for a
// metric the benchmark's ground truth could not produce.
import type { KeyboardEvent } from 'react';
import { Checkbox, Glyph, MetricChip, StatusChip } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { benchmarkLabel, formatMetric, shortHash, type RunRow } from './model.ts';

/** The optional columns, drawn only when some row of the table has their data. */
export type RowColumns = { latency: boolean; started: boolean };

export type RunRowViewProps = {
  row: RunRow;
  selected: boolean;
  /** Why the run cannot be checked now; null when it can. */
  refusal: string | null;
  columns: RowColumns;
  onToggle: () => void;
  /** Opens the run: Replay, before a query is chosen. */
  onOpen: () => void;
};

/** The failure as a sentence, beside the chip: colour is never the only carrier. */
const failure = (node: string | null, error: string) => (node === null ? `The run failed: ${error}` : `${node} failed: ${error}`);

const started = (iso: string) => new Date(iso).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });

export function RunRowView({ row, selected, refusal, columns, onToggle, onOpen }: RunRowViewProps) {
  const { status } = row;
  const extra = (
    <>
      {columns.latency ? <td className="num">{row.latencyMs === null ? null : `${Math.round(row.latencyMs).toLocaleString('en-US')} ms`}</td> : null}
      {columns.started ? <td>{row.startedAt === null ? null : <time dateTime={row.startedAt}>{started(row.startedAt)}</time>}</td> : null}
    </>
  );

  if (status.state === 'queued' || status.state === 'running') {
    // The job queue's slot: the chip alone until the queue fills the row.
    return (
      <tr className="rg-runs__row" data-placeholder>
        <td />
        <td />
        <td>{status.state === 'running' ? <StatusChip state="running" fraction={status.fraction} /> : <StatusChip state="queued" />}</td>
        <td />
        {extra}
      </tr>
    );
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTableRowElement>) => {
    // A key on the checkbox or the link inside is that control's.
    if (event.target !== event.currentTarget) return;
    if (event.key === ' ') {
      event.preventDefault();
      if (refusal === null) onToggle();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      onOpen();
    }
  };

  const label = benchmarkLabel(row);
  return (
    <tr className="rg-runs__row" tabIndex={0} onKeyDown={onKeyDown}>
      <td>
        {refusal === null ? (
          <Checkbox label={label} checked={selected} onChange={onToggle} />
        ) : (
          <Checkbox label={label} checked={selected} onChange={onToggle} disabled disabledReason={refusal} />
        )}
      </td>
      <td>
        <a className="rg-runs__hash" href={formatHash({ screen: 'replay', run: row.id })}>
          {shortHash(row.id)}
        </a>
        {row.prefix === null ? null : (
          <span className="rg-runs__prefix">
            <Glyph name="prefix" />
            {row.prefix.upTo === null ? 'prefix' : `prefix up to ${row.prefix.upTo}`}
          </span>
        )}
      </td>
      <td>
        {status.state === 'failed' ? (
          <StatusChip state="failed">
            {status.node === null ? (
              'failed'
            ) : (
              <>
                failed at <code>{status.node}</code>
              </>
            )}
          </StatusChip>
        ) : (
          <StatusChip state="done" />
        )}
      </td>
      <td>
        {status.state === 'failed' ? (
          <span className="rg-runs__failure">{failure(status.node, status.error)}</span>
        ) : (
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
        )}
      </td>
      {extra}
    </tr>
  );
}

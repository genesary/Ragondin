// The Compare screen's tables: the metrics with the best of each row and the
// deltas, the parameter × run matrix, the stages, and each chart's table
// alternative. Every number is the API's; these only lay it out.
import type { ReactNode } from 'react';
import { Delta, RunSwatch, Table, type HistogramBin, type RunSeries, type StackSegment, type TableRow } from '../../design/index.ts';
import type { Comparison, ConfigurationMatrix, MetricRow } from '../api/types.ts';
import { deltaOf, departures, formatParameter, formatValue, NO_STAGE, parameterName, stageLabel } from './model.ts';

/** A run's column heading: its swatch, then its letter and pipeline in words. */
const runColumn = (s: RunSeries) => ({
  id: s.id,
  numeric: true,
  label: (
    <span className="rg-compare__run">
      <span aria-hidden="true">
        <RunSwatch slot={s.ink} small />
      </span>
      {s.label}
    </span>
  ),
});

/** The column indices of a row's best runs, by the API's `best`. */
const bestColumns = (c: Comparison, row: MetricRow) => row.best.map((id) => c.runs.findIndex((r) => r.id === id) + 1).filter((i) => i > 0);

/** The metric table: each run's value, the best of each row in bold, each delta against the baseline signed, coloured and worded. */
export function MetricsTable({ comparison, series }: { comparison: Comparison; series: readonly RunSeries[] }) {
  const rows: TableRow[] = comparison.metrics.map((row) => ({
    id: row.name,
    bestColumn: bestColumns(comparison, row),
    cells: [
      row.name,
      ...row.values.map((v, i) => {
        if (v === null) return 'not recorded';
        const d = row.deltas[i] ?? null;
        const delta = i === 0 || d === null ? null : deltaOf(row.direction, d);
        return (
          <>
            {formatValue(row.direction, v)}
            {delta === null ? null : (
              <span className="rg-compare__delta">
                <Delta meaning={delta.meaning} direction={delta.direction}>
                  {delta.text}
                </Delta>
              </span>
            )}
          </>
        );
      }),
    ],
  }));
  return <Table caption={`Metrics of ${comparison.runs.length} runs against the baseline`} columns={[{ id: 'metric', label: 'Metric' }, ...series.map(runColumn)]} rows={rows} />;
}

/** The parameter × run matrix: only what differs across the runs, as the API lists it, each departure from the baseline marked. */
export function ParameterMatrix({ configuration, series }: { configuration: ConfigurationMatrix; series: readonly RunSeries[] }) {
  if (configuration.kind === 'unavailable') {
    return (
      <p className="rg-compare__note">
        The parameters of run {configuration.run.slice(0, 12)} cannot be read: {configuration.reason}
      </p>
    );
  }
  if (configuration.parameters.length === 0) {
    return <p className="rg-compare__note">{configuration.same_logical_form ? 'Every run has the same configuration.' : 'No parameter differs: the runs differ only in how their nodes are wired.'}</p>;
  }
  const rows: TableRow[] = configuration.parameters.map((row) => {
    const off = departures(row);
    return {
      id: `${row.node}/${parameterName(row.key)}`,
      cells: [
        row.node,
        parameterName(row.key),
        ...row.values.map((v, i): ReactNode =>
          off[i] ? (
            <mark className="rg-compare__departure">
              {formatParameter(v)}
              <span className="rg-visually-hidden"> (differs from the baseline)</span>
            </mark>
          ) : (
            formatParameter(v)
          ),
        ),
      ],
    };
  });
  return (
    <Table
      caption="Parameters that differ across the runs"
      columns={[{ id: 'node', label: 'Node' }, { id: 'key', label: 'Parameter' }, ...series.map((s) => ({ ...runColumn(s), numeric: false }))]}
      rows={rows}
    />
  );
}

/** The stage table: each stage the API kept, each run's nodes there and its figure for `metric`, or the words for a stage it lacks. */
export function StageTable({ comparison, series, metric }: { comparison: Comparison; series: readonly RunSeries[]; metric: string }) {
  const rows: TableRow[] = comparison.stages.map((row) => {
    const name = [row.label ?? stageLabel(row.stage), row.source === 'manual' ? 'by hand' : null, row.confidence === 'low' ? 'a guess' : null].filter(Boolean).join(' · ');
    return {
      id: row.stage,
      cells: [
        name,
        ...row.cells.map((cell) => {
          if (cell.kind === 'absent') return <span className="rg-compare__absent">{NO_STAGE}</span>;
          const best = cell.best[metric];
          return (
            <>
              <span className="rg-compare__nodes">{cell.nodes.map((n) => (n.paired_by_hand ? `${n.node} (by hand)` : n.node)).join(', ')}</span>
              {best === undefined ? null : <span className="rg-compare__delta">{best.value.toFixed(4)}</span>}
            </>
          );
        }),
      ],
    };
  });
  return <Table caption="Stages of each run" columns={[{ id: 'stage', label: 'Stage' }, ...series.map(runColumn)]} rows={rows} />;
}

/** A chart's values as a table: one row per position, one column per run, a gap in its words. */
export function ValuesTable({
  caption,
  first,
  rows,
  series,
  values,
  format,
  gap,
  best,
}: {
  caption: string;
  first: string;
  rows: readonly { id: string; label: string }[];
  series: readonly RunSeries[];
  /** `values[row][series]`. */
  values: (row: number, series: number) => number | null;
  format: (v: number) => string;
  gap: (row: number, series: number) => string;
  /** Whether a value is its row's best — the chart's star — so the table says "(best)" for it too. */
  best?: (row: number, series: number) => boolean;
}) {
  return (
    <Table
      caption={caption}
      columns={[{ id: 'row', label: first }, ...series.map((s) => ({ id: s.id, label: s.label, numeric: true }))]}
      rows={rows.map((r, i) => ({
        id: r.id,
        bestColumn: best === undefined ? [] : series.flatMap((_, s) => (best(i, s) && values(i, s) !== null ? [s + 1] : [])),
        cells: [
          r.label,
          ...series.map((_, s) => {
            const v = values(i, s);
            return v === null ? <span className="rg-compare__absent">{gap(i, s)}</span> : format(v);
          }),
        ],
      }))}
    />
  );
}

/** The latency chart as a table: each run's nodes with their median and family. */
export function LatencyTable({ bars, segments, format }: { bars: readonly { id: string; label: string }[]; segments: readonly (readonly StackSegment[])[]; format: (v: number) => string }) {
  const rows: TableRow[] = bars.flatMap((bar, b) => [
    { kind: 'group' as const, id: `run-${bar.id}`, label: bar.label },
    ...(segments[b] ?? []).map((s) => ({ id: `${bar.id}/${s.id}`, cells: [s.label, s.family ?? 'extension', format(s.value)] })),
  ]);
  return (
    <Table
      caption="Median latency per node, in milliseconds, as a table"
      columns={[
        { id: 'node', label: 'Node' },
        { id: 'family', label: 'Family' },
        { id: 'median', label: 'Median', numeric: true },
      ]}
      rows={rows}
    />
  );
}

/** The histogram as a table: each bin, its range and its count. */
export function BinsTable({ caption, bins }: { caption: string; bins: readonly HistogramBin[] }) {
  return (
    <Table
      caption={caption}
      columns={[
        { id: 'bin', label: 'Bin' },
        { id: 'range', label: 'Change' },
        { id: 'count', label: 'Queries', numeric: true },
      ]}
      rows={bins.map((b) => ({ id: b.id, cells: [b.label, b.range, b.count.toLocaleString('en-US')] }))}
    />
  );
}

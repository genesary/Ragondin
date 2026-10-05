// The Compare screen's tables: the metrics with the best of each row and the
// deltas, the parameter × run matrix, the stages, and each chart's table
// alternative. Every number is the API's; these only lay it out.
import type { ReactNode } from 'react';
import { Delta, RunSwatch, Table, type HistogramBin, type RunSeries, type StackSegment, type TableRow } from '../../design/index.ts';
import type { Comparison, ConfigurationMatrix, MetricRow, ParameterRow } from '../api/types.ts';
import { configurationRows, deltaOf, departures, formatParameter, formatValue, NO_STAGE, onlyIn, parameterName, stageLabel } from './model.ts';

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

/** The metric table's caption, which the grouped bars name as their table. */
export const metricsCaption = (c: Comparison) => `Metrics of ${c.runs.length} runs against the baseline`;

/** The stage table's caption, which the stage line names as its table. */
export const STAGES_CAPTION = 'Stages of each run';

/**
 * The metric table: each run's value, the best of each row in bold, each
 * delta against the baseline signed, coloured and worded — signed alone for a
 * metric the API gives no direction, which has no best.
 */
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
            {delta === null ? null : delta.meaning === null ? (
              // No direction, so no colour, arrow or word: the sign alone says which way it moved.
              <span className="rg-compare__delta">{delta.text}</span>
            ) : (
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
  return <Table region caption={metricsCaption(comparison)} columns={[{ id: 'metric', label: 'Metric' }, ...series.map(runColumn)]} rows={rows} />;
}

/** The words for a run without a node, in the configuration table. */
const NO_NODE = 'no such node';

/**
 * The parameter × run matrix: only what differs across the runs, as the API
 * lists it, each departure from the baseline marked — and a node some run
 * lacks as one row saying which runs hold it, never as rows of unset
 * parameters.
 */
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
  const departed = (text: string, off: boolean): ReactNode =>
    off ? (
      <mark className="rg-compare__departure">
        {text}
        <span className="rg-visually-hidden"> (differs from the baseline)</span>
      </mark>
    ) : (
      text
    );
  const lacking = <span className="rg-compare__absent">{NO_NODE}</span>;
  // A node some run lacks: what it is where it is held — its family and implementation — and the words where it is not.
  const heldAs = (node: string, run: number) => {
    const value = (key: ParameterRow['key']['kind']) => configuration.parameters.find((r) => r.node === node && r.key.kind === key)?.values[run] ?? null;
    return [value('component'), value('impl')].flatMap((v) => (v === null ? [] : [formatParameter(v)])).join(' · ');
  };
  const rows: TableRow[] = configurationRows(configuration).map((entry) => {
    if (entry.kind === 'node') {
      return {
        id: `${entry.node}/`,
        // Which runs hold the node is said with its name, the row's label; the
        // Parameter column says the row is the node itself, not one of its parameters.
        cells: [
          <>
            {entry.node} <span className="rg-compare__absent">{onlyIn(entry.present, series)}</span>
          </>,
          <span className="rg-compare__absent">whole node</span>,
          ...entry.present.map((held, i) => (held ? departed(heldAs(entry.node, i), i !== 0 && held !== entry.present[0]) : lacking)),
        ],
      };
    }
    const { row, present } = entry;
    const off = departures(row);
    return {
      id: `${row.node}/${parameterName(row.key)}`,
      cells: [row.node, parameterName(row.key), ...row.values.map((v, i): ReactNode => (present !== null && !present[i] ? lacking : departed(formatParameter(v), off[i] === true && (present === null || present[0] === true))))],
    };
  });
  return (
    <Table
      region
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
  return <Table region caption={STAGES_CAPTION} columns={[{ id: 'stage', label: 'Stage' }, ...series.map(runColumn)]} rows={rows} />;
}

/** The latency chart as a table: each run's nodes with their median and family. */
export function LatencyTable({ bars, segments, format }: { bars: readonly { id: string; label: string }[]; segments: readonly (readonly StackSegment[])[]; format: (v: number) => string }) {
  const rows: TableRow[] = bars.flatMap((bar, b) => [
    { kind: 'group' as const, id: `run-${bar.id}`, label: bar.label },
    ...(segments[b] ?? []).map((s) => ({ id: `${bar.id}/${s.id}`, cells: [s.label, s.family ?? 'extension', format(s.value)] })),
  ]);
  return (
    <Table
      region
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
      region
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

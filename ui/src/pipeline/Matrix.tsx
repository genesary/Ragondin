// The node × benchmark matrix, set in design/'s Table with row headers: a row
// per node in pipeline order, a column per benchmark, and in each cell the
// value and, emphasised, the gain over the previous stage — or why the cell is
// empty, in the cell. Every figure and every reason is the API's.
// ARCHITECTURE.md § The Pipeline screen.
import type { ReactNode } from 'react';
import { Button, FAMILY_LABEL, FamilyTile, Glyph, StatusChip, Table, familyOfComponent, type TableRow } from '../../design/index.ts';
import type { DatasetStatus, MatrixCell, MatrixColumn, MatrixRow, PipelineMatrix } from '../api/types.ts';
import { formatHash } from '../routes.ts';
import { formatMetric, metricLabel, shortHash } from '../runs/model.ts';
import './Pipeline.css';
import { answerMetrics, bestGainColumns, columnLabel, gainOn, groundTruthLabel, signedGain, spansRow } from './model.ts';

/** What the launcher is handed: run this pipeline, whole, on each of these benchmarks. */
export type LaunchRequest = { pipeline: string; benchmarks: string[] };

/**
 * The hand-off to the launcher: the screen passes one that opens Runs' launch
 * panel on the request (`launchInPanel`), so the matrix imports no launcher.
 */
export type Launch = (request: LaunchRequest) => void;

export type MatrixProps = {
  matrix: PipelineMatrix;
  /** The ranking metric every ranking row reads. */
  metric: string;
  launch: Launch;
};

/** A cell's two lines; `never` hatches it — never measurable on this benchmark. */
function Cell({ state, never = false, first, second }: { state: MatrixCell['kind']; never?: boolean; first: ReactNode; second?: ReactNode }) {
  return (
    <div className="rg-matrix__cell" data-state={state} data-never={never ? true : undefined}>
      <span className="rg-matrix__line">{first}</span>
      {second === undefined ? null : (
        <>
          {' '}
          <span className="rg-matrix__line">{second}</span>
        </>
      )}
    </div>
  );
}

/** A visible word, and the rest of its sentence for assistive technology. */
const said = (visible: string, rest: string) => (
  <>
    {visible}
    <span className="rg-visually-hidden">{rest}</span>
  </>
);

/** A measured cell's second line: its gain over the previous stage, or why it has none. */
function gainLine(cell: Extract<MatrixCell, { kind: 'measured' }>, metric: string, best: boolean): ReactNode {
  switch (cell.gain.kind) {
    case 'over_previous_stage': {
      const gain = cell.gain.values[metric];
      if (gain === undefined) return <span className="rg-matrix__note">no gain on {metric}</span>;
      const text = signedGain(gain);
      return (
        <span className="rg-matrix__gain" data-best={best ? true : undefined}>
          {text.startsWith('+') ? <Glyph name="up" /> : text.startsWith('−') ? <Glyph name="down" /> : null}
          {text}
          <span className="rg-visually-hidden"> gain over the previous stage</span>
          {best ? <span className="rg-visually-hidden"> (best gain in this row)</span> : null}
        </span>
      );
    }
    case 'first_stage':
      return <span className="rg-matrix__note">{said('first stage', ' — nothing before it to gain over')}</span>;
    case 'ambiguous':
      return <span className="rg-matrix__note">{said('stages guessed', ' — which stage came before is a guess, so no gain is given')}</span>;
    case 'unstaged':
      return <span className="rg-matrix__note">not a ranking stage</span>;
  }
}

function cellContent(m: PipelineMatrix, row: MatrixRow, cell: MatrixCell, metric: string, best: boolean, runHere: boolean, failed: boolean, launch: Launch): ReactNode {
  switch (cell.kind) {
    case 'measured': {
      if (row.produces === 'answer') {
        const figures = answerMetrics(m)
          .filter((name) => cell.metrics[name] !== undefined)
          .map((name) => `${metricLabel(name)} ${formatMetric('answers', cell.metrics[name] as number)}`)
          .join(' · ');
        return <Cell state="measured" first={<span className="rg-matrix__value">{figures}</span>} second={gainLine(cell, metric, false)} />;
      }
      const value = cell.metrics[metric];
      // The value is the quiet line only beside a gain, which is what is read
      // first; with no gain beside it, it is the cell's one figure.
      const quiet = gainOn(cell, metric) !== null;
      return (
        <Cell
          state="measured"
          first={
            <span className="rg-matrix__value" data-quiet={quiet ? true : undefined}>
              {value === undefined ? `no ${metric} figure` : formatMetric('ranking', value)}
            </span>
          }
          second={gainLine(cell, metric, best)}
        />
      );
    }
    case 'no_qrels':
      return <Cell state={cell.kind} never first={said('no qrels', ' — never measurable on this benchmark')} />;
    case 'no_reference_answers':
      return <Cell state={cell.kind} never first={said('no reference answers', ' — never measurable on this benchmark')} />;
    case 'not_run_yet': {
      // A failed attempt fills nothing, so the cell still waits for a run; it
      // says the attempt failed rather than that none was made.
      const words = failed ? 'last attempt failed' : 'not run yet';
      // One Run control per benchmark not run, in its first such cell: the
      // others say it in words, so a dozen benchmarks are not dozens of tab stops.
      if (!runHere) return <Cell state={cell.kind} first={words} />;
      const name = `Run on ${cell.benchmark}`;
      return (
        <Cell
          state={cell.kind}
          first={words}
          second={
            <Button size="s" aria-label={name} onClick={() => launch({ pipeline: m.pipeline, benchmarks: [cell.benchmark] })}>
              Run
            </Button>
          }
        />
      );
    }
    case 'prefix_stops':
      return <Cell state={cell.kind} first="not run:" second={`the prefix run stops at ${cell.up_to}`} />;
    case 'not_run_on_this_version':
      return (
        <Cell
          state={cell.kind}
          first="not run on this version"
          second={
            <a className="rg-matrix__run" href={formatHash({ screen: 'replay', run: cell.run })}>
              run <code>{shortHash(cell.run)}</code>
            </a>
          }
        />
      );
    case 'unverified':
      return <Cell state={cell.kind} first={said('unverified', ' — the run’s dataset is not verified on disk')} />;
    case 'not_scored':
      return <Cell state={cell.kind} first={said('not scored', ' — no metric reads this node’s output')} />;
    case 'not_scorable':
      return <Cell state={cell.kind} never first={said('cannot be scored', ` — ${m.pipeline} does not end in an answer, and this benchmark is scored on its reference answers`)} />;
    case 'no_figure':
      return <Cell state={cell.kind} first={said('no figure', ' — run here, and no figure came out')} />;
  }
}

/** What a dataset check that did not verify found, in a chip's words. */
const NOT_VERIFIED: Record<Exclude<DatasetStatus, 'verified'>, string> = {
  dataset_absent: 'dataset absent',
  dataset_differs: 'dataset differs',
  dataset_unreadable: 'dataset unreadable',
  index_differs: 'index differs',
};

/** A column's status in words: measured, how far a prefix run reached, or why nothing measured it. */
function columnStatus(column: MatrixColumn): ReactNode {
  if (column.run !== null) return <StatusChip state="done">{column.up_to === null ? 'measured' : `up to ${column.up_to}`}</StatusChip>;
  if (column.cells.length > 0 && column.cells.every((c) => c.kind === 'not_scorable')) return <span className="rg-matrix__note">cannot be scored</span>;
  if (column.cells.length > 0 && column.cells.every((c) => c.kind === 'not_run_on_this_version')) return <StatusChip state="warning">earlier version only</StatusChip>;
  return <span className="rg-matrix__note">not run yet</span>;
}

/** The API's most recent attempt of the whole pipeline here, when it failed: said, with the way to the job. */
function failedAttempt(column: MatrixColumn): ReactNode {
  if (column.failed_attempt === null) return null;
  return (
    <>
      {' '}
      <StatusChip state="failed">last attempt failed</StatusChip>{' '}
      <a className="rg-matrix__run" href={formatHash({ screen: 'runs', job: column.failed_attempt.job })}>
        open<span className="rg-visually-hidden"> the failed attempt on {columnLabel(column)}</span>
      </a>
    </>
  );
}

function columnHeader(column: MatrixColumn) {
  const check = column.dataset_check?.status ?? 'verified';
  return (
    <span className="rg-matrix__col">
      <span className="rg-matrix__bench">{columnLabel(column)}</span>{' '}
      <span className="rg-matrix__status">
        {column.run === null && column.failed_attempt !== null ? null : columnStatus(column)}
        {failedAttempt(column)}
        {check === 'verified' ? null : (
          <>
            {' '}
            <StatusChip state="warning">{NOT_VERIFIED[check]}</StatusChip>
          </>
        )}
      </span>{' '}
      <span className="rg-matrix__truth">{groundTruthLabel(column.ground_truth)}</span>
    </span>
  );
}

/** The metric a row reads, as its header names it: the chosen ranking metric, the answer metrics, or none. */
function rowMetric(m: PipelineMatrix, row: MatrixRow, metric: string): string | null {
  if (row.produces === 'chunks') return metric;
  if (row.produces === 'answer') {
    const names = answerMetrics(m);
    return names.length === 0 ? null : names.map(metricLabel).join(', ');
  }
  return null;
}

function rowHeader(m: PipelineMatrix, row: MatrixRow, metric: string) {
  const family = familyOfComponent(row.family);
  const reads = rowMetric(m, row, metric);
  return (
    <span className="rg-matrix__node">
      {family === null ? <span className="rg-matrix__word">{row.family}</span> : <FamilyTile family={family} labelled />}{' '}
      <span className="rg-matrix__name">{row.node}</span>
      {reads === null ? null : (
        <>
          {' '}
          <span className="rg-matrix__metric">{reads}</span>
        </>
      )}
    </span>
  );
}

/** The sentence a row says for every benchmark when none of its cells is scored. */
function unscored(row: MatrixRow): string {
  const family = familyOfComponent(row.family);
  return `Not scored on any benchmark: no metric reads a${family === null ? 'n' : ''} ${family === null ? `${row.family} node` : FAMILY_LABEL[family]}’s output.`;
}

export function Matrix({ matrix, metric, launch }: MatrixProps) {
  const spanned = matrix.rows.map((_, r) => spansRow(matrix, r));
  // Per column, the row whose cell carries the column's one Run control.
  const runRow = matrix.columns.map((column) => column.cells.findIndex((cell, r) => cell.kind === 'not_run_yet' && !spanned[r]));
  const rows: TableRow[] = matrix.rows.map((row, r) => {
    if (spanned[r]) return { id: row.node, span: true, cells: [rowHeader(matrix, row, metric), unscored(row)] };
    const best = row.produces === 'chunks' ? bestGainColumns(matrix, r, metric) : [];
    return {
      id: row.node,
      cells: [rowHeader(matrix, row, metric), ...matrix.columns.map((column, c) => cellContent(matrix, row, column.cells[r] as MatrixCell, metric, best.includes(c), runRow[c] === r, column.failed_attempt !== null, launch))],
    };
  });
  return (
    <div className="rg-matrix">
      <Table
        caption={`${matrix.pipeline}: each node on each benchmark`}
        rowHeaders
        region
        columns={[{ id: 'node', label: 'Node' }, ...matrix.columns.map((column) => ({ id: column.dataset_version, label: columnHeader(column) }))]}
        rows={rows}
      />
    </div>
  );
}

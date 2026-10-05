// One run job of the queue as a row of design/'s Table: the identity its job
// announced, its state as the stream last said, its real progress, and the
// controls the queue allows — Move up and Move down among the queued, Cancel
// on a queued or running job, Resubmit on an ended one — and how many faults
// the queue reported beside it. Every control is a
// native button in the tab order; the row itself takes the table's keys, and
// Enter opens the job (`#runs/job/<id>`). ARCHITECTURE.md § The Runs screen.
import { useEffect, useState, type ReactNode } from 'react';
import { Button, PrefixLabel, StatusChip, type TableRow } from '../../design/index.ts';
import { formatHash } from '../routes.ts';
import { benchmarkLabel, formatLatency, rowKey, runningLabel, shortHash, type RunRow } from './model.ts';
import type { RowColumns } from './runRow.tsx';

export type JobRowOptions = {
  columns: RowColumns;
  /** The stream is down: what the row says of a live job is the last known, and says so. */
  stale: boolean;
  /** A Cancel was asked of the running job and the stream has not said it ended. */
  cancelling: boolean;
  onCancel: () => void;
  /** Moves the queued job to this place among the queued, 0 the next taken. */
  onMove: (place: number) => void;
  onResubmit: () => void;
};

/** The failure as a sentence, beside the chip: colour is never the only carrier. */
const failure = (node: string | null, error: string) => (node === null ? `The run failed: ${error}` : `${node} failed: ${error}`);

/** A duration as a clock: m:ss, or h:mm:ss past the hour. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const [h, m, s] = [Math.floor(total / 3600), Math.floor((total % 3600) / 60), total % 60];
  const two = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}

/** The time since `since`, read from the clock once a second: not a live region, so it is not read aloud each second. */
function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return <span className="rg-runs__elapsed">{formatElapsed(now - since)} elapsed</span>;
}

/** Where a queued job stands, in words: next, or how many are ahead of it. */
const placeWords = (place: number | null) => (place === null || place === 0 ? 'next' : `${place.toLocaleString('en-US')} ahead`);

/** The row's state in words, as its name and its chip say it. */
function stateWords(row: RunRow, cancelling: boolean): string {
  const { status } = row;
  switch (status.state) {
    case 'queued':
      return cancelling ? 'cancelling…' : `queued, ${placeWords(row.job?.place ?? null)}`;
    case 'running':
      return cancelling ? 'cancelling…' : runningLabel(status);
    case 'failed':
      return status.node === null ? 'failed' : `failed at ${status.node}`;
    case 'done':
      return 'done';
    case 'cancelled':
      return 'cancelled';
  }
}

/** `row`, a run job's, as a Table row: its key, its name, its cells. */
export function jobRow(row: RunRow, { columns, stale, cancelling, onCancel, onMove, onResubmit }: JobRowOptions): TableRow {
  const { status, job } = row;
  const id = row.source.id;
  const short = shortHash(row.source.kind === 'job' ? (row.source.runId ?? id) : id);
  const bench = benchmarkLabel(row);
  const words = stateWords(row, cancelling);
  const live = status.state === 'queued' || status.state === 'running';
  const last = stale && live ? ' · last known' : '';
  const control = (name: string) => ({ 'data-job': id, 'data-control': name });
  const faults = job?.faults ?? [];
  const faultCount = faults.length === 0 ? '' : `${faults.length.toLocaleString('en-US')} ${faults.length === 1 ? 'fault' : 'faults'}`;

  const chip =
    status.state === 'running' ? (
      <StatusChip state="running" fraction={status.total !== null && status.total > 0 ? status.done / status.total : 0}>
        {words}
        {last}
      </StatusChip>
    ) : status.state === 'failed' ? (
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
      <StatusChip state={status.state}>
        {words}
        {last}
      </StatusChip>
    );

  const cancel = cancelling ? (
    <Button size="s" disabled disabledReason="The job is being cancelled; the queue says when it ended." {...control('cancel')}>
      Cancelling…
    </Button>
  ) : (
    <Button size="s" kind="destructive" aria-label={`Cancel run ${short}`} onClick={onCancel} {...control('cancel')}>
      Cancel
    </Button>
  );
  const resubmit = (
    <Button size="s" aria-label={`Resubmit run ${short}`} onClick={onResubmit} {...control('resubmit')}>
      Resubmit
    </Button>
  );

  let detail: ReactNode;
  switch (status.state) {
    case 'queued': {
      const place = job?.place ?? 0;
      const lastPlace = (job?.queued ?? 1) - 1;
      const move = (to: number, glyph: 'up' | 'down', label: string, edge: boolean, why: string) =>
        edge ? (
          <Button size="s" kind="quiet" icon={glyph} aria-label={`Move run ${short} ${label}`} disabled disabledReason={why} {...control(label)}>
            {label === 'up' ? 'Up' : 'Down'}
          </Button>
        ) : (
          <Button size="s" kind="quiet" icon={glyph} aria-label={`Move run ${short} ${label}`} onClick={() => onMove(to)} {...control(label)}>
            {label === 'up' ? 'Up' : 'Down'}
          </Button>
        );
      detail = (
        <>
          {move(place - 1, 'up', 'up', place <= 0, 'First in the queue.')}
          {move(place + 1, 'down', 'down', place >= lastPlace, 'Last in the queue.')}
          {cancel}
        </>
      );
      break;
    }
    case 'running':
      detail = (
        <>
          {job?.medianMs == null ? null : <span>{formatLatency(job.medianMs)} / query</span>}
          {job?.startedAtMs == null ? null : <Elapsed since={job.startedAtMs} />}
          {cancel}
        </>
      );
      break;
    case 'failed':
      detail = (
        <>
          <span className="rg-runs__failure">{failure(status.node, status.error)}</span>
          {resubmit}
        </>
      );
      break;
    case 'cancelled':
      detail = resubmit;
      break;
    case 'done':
      detail =
        job?.filed == null ? (
          <span>Done; the queue names no run filed.</span>
        ) : job.mismatch === null ? (
          <span>Filed; reading it from the store…</span>
        ) : (
          <span>
            Filed under {shortHash(job.mismatch.decided)}, not the announced {shortHash(job.mismatch.announced)}: what ran differs from what was announced, so no run is filed under the announced id.
          </span>
        );
      break;
  }

  const run = (
    <>
      <a className="rg-runs__hash" href={formatHash({ screen: 'runs', job: id })} tabIndex={-1}>
        {short}
      </a>
      <span className="rg-runs__fact">announced</span>
      {/* A fault does not stop the job: a count beside its identity, never its chip, each reason in the title and all of them in the job's view. */}
      {faultCount === '' ? null : (
        <span className="rg-runs__fault" title={faults.join('\n')}>
          {faultCount}
        </span>
      )}
      {row.prefix === null ? null : (
        <span className="rg-runs__prefix">
          <PrefixLabel parents={row.prefix.parents} upTo={row.prefix.upTo} />
        </span>
      )}
    </>
  );

  return {
    id: rowKey(row),
    // The name says where the job stands, never its count: a name that changed on every tick would be re-announced each time.
    label: `Run ${short} on ${bench}, ${status.state === 'running' && !cancelling ? 'running' : words}${last}${faultCount === '' ? '' : `, ${faultCount}`}`,
    cells: [
      <span className="rg-runs__bench">{bench}</span>,
      run,
      chip,
      // One control high whatever it holds, so a button coming or going moves no row.
      <span className="rg-runs__job">{detail}</span>,
      ...(columns.latency ? [null] : []),
      ...(columns.started ? [null] : []),
    ],
  };
}

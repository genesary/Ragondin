// The Runs screen: what the store holds, grouped by pipeline, one row per run
// (the front-end design, § 3), most recent first, with the job queue's run
// jobs in their pipeline's group. It reads `GET /runs` — each pipeline's
// shape comes with the listing — and the queue the shell follows; it owns the
// benchmark filter and the launch panel; the selection it hands to Compare,
// and the job it shows, live in the address. ARCHITECTURE.md § The Runs screen.
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, FilterChip, InlineMessage, Sheet, Table, type TableRow } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { Jobs } from '../api/jobs.ts';
import type { RunListing } from '../api/types.ts';
import { useJobEvents, useJobs } from '../jobs/queue.tsx';
import { STREAM_DOWN, STREAM_DOWN_LIVE } from '../jobs/stream.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { GroupLabel } from './GroupLabel.tsx';
import { JobPanel } from './JobPanel.tsx';
import { jobRow } from './jobRow.tsx';
import { rowsFromJobs, withAnnounced } from './jobRows.ts';
import { Conflict, LaunchPanel } from './LaunchPanel.tsx';
import { benchmarkLabel, groupRows, openRoute, rowKey, rowsFromListing, runId, shapeOf, shortHash, type RunRow } from './model.ts';
import { runRow } from './runRow.tsx';
import './Runs.css';
import { compareRefusal, refusal, sanitize, toggle, unknownIds } from './selection.ts';

export type RunsScreenProps = {
  client: ApiClient;
  /** The selection the address carries, in the order the runs were checked. */
  sel: readonly string[];
  /** The job the address shows (`#runs/job/<id>`), if any. */
  job?: string | undefined;
  /** The workspace's root, where the store is, as the shell read it; null before. */
  store?: string | null;
};

/** Writes a selection to the address in place, keeping the job shown: checking a box is state within the view, not a move Back should undo. */
const selectWith = (job: string | undefined) => (sel: string[]) => navigate(job === undefined ? { screen: 'runs', sel } : { screen: 'runs', sel, job }, { replace: true });

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

/**
 * A listing, with the selection the address carried when it was asked for —
 * what it is authoritative about — and the failure of a later re-read, which
 * keeps the listing on screen rather than replacing it.
 */
type Read = { listing: RequestState<RunListing>; askedWith: ReadonlySet<string>; refresh: ApiProblem | null };

export function RunsScreen({ client, sel, job, store = null }: RunsScreenProps) {
  const [read, setRead] = useState<Read>({ listing: { status: 'loading' }, askedWith: new Set(), refresh: null });
  const latest = useRef(0);
  // The listing read in flight, cancelled once a newer read overtakes it.
  const reading = useRef<AbortController | null>(null);
  const selNow = useRef(sel);
  selNow.current = sel;

  /**
   * Reads the listing; only the answer to the last request asked lands,
   * whatever order they arrive in. The read it overtakes is cancelled, which
   * saves the server's work. Two checks drop an overtaken answer: the count,
   * which a newer read moves before it cancels, and the signal, which alone
   * covers the screen going.
   */
  const fetchListing = useCallback(async () => {
    const mine = ++latest.current;
    reading.current?.abort();
    const controller = new AbortController();
    reading.current = controller;
    const askedWith = new Set(selNow.current);
    const result = await client.get('/runs', { signal: controller.signal });
    if (mine !== latest.current || controller.signal.aborted) return;
    setRead((prev) => {
      if (result.ok) return { listing: { status: 'loaded', value: result.value }, askedWith, refresh: null };
      // A re-read that fails leaves the listing already shown in place.
      if (prev.listing.status === 'loaded') return { ...prev, refresh: result.problem };
      return { listing: { status: 'error', problem: result.problem }, askedWith, refresh: null };
    });
  }, [client]);

  useEffect(() => {
    void fetchListing();
    // A new client, or the screen going, cancels the read left behind.
    return () => reading.current?.abort();
  }, [fetchListing]);

  const retry = () => {
    setRead((r) => ({ ...r, listing: { status: 'loading' } }));
    void fetchListing();
  };

  switch (read.listing.status) {
    case 'loading':
      return (
        <Sheet>
          <Loading label="Reading runs" />
        </Sheet>
      );
    case 'error':
      return <ErrorState problem={read.listing.problem} onRetry={retry} />;
    case 'loaded':
      return (
        <Loaded client={client} listing={read.listing.value} askedWith={read.askedWith} refresh={read.refresh} sel={sel} job={job} store={store} reread={() => void fetchListing()} />
      );
  }
}

type LoadedProps = {
  client: ApiClient;
  listing: RunListing;
  /** The selection the address carried when `listing` was asked for. */
  askedWith: ReadonlySet<string>;
  /** Why the last re-read failed, if it did. */
  refresh: ApiProblem | null;
  sel: readonly string[];
  job: string | undefined;
  store: string | null;
  /** Reads the listing again, keeping this one on screen meanwhile. */
  reread: () => void;
};

/** A job row's control, as `jobRow` marks it: the job, and which button. */
type Control = { job: string; control: string };

const controlOf = (el: Element | null): Control | null => {
  const job = el?.getAttribute('data-job');
  const control = el?.getAttribute('data-control');
  return job == null || control == null ? null : { job, control };
};
const findControl = ({ job, control }: Control) => [...document.querySelectorAll<HTMLElement>(`[data-control="${control}"]`)].find((el) => el.getAttribute('data-job') === job) ?? null;

/** The queue with the positions a reorder's answer gave, until the stream says the order itself. */
function withPositions(jobs: Jobs, positions: ReadonlyMap<string, number> | null): Jobs {
  if (positions === null) return jobs;
  return new Map([...jobs].map(([id, j]) => [id, j.state.kind === 'queued' && positions.has(id) ? { ...j, position: positions.get(id) as number } : j]));
}

function Loaded({ client, listing, askedWith, refresh, sel, job, store, reread }: LoadedProps) {
  const select = useMemo(() => selectWith(job), [job]);
  const { jobs, connection } = useJobs();
  const stored = useMemo(() => rowsFromListing(listing), [listing]);
  // A reorder's answer is the queue in its new order; the stream's `reordered` events then say it too, and win.
  const [positions, setPositions] = useState<ReadonlyMap<string, number> | null>(null);
  useJobEvents((_before, _after, event) => {
    if (event.event === 'reordered' || event.event === 'resync') setPositions(null);
  });
  const queue = useMemo(() => withPositions(jobs, positions), [jobs, positions]);
  const rows = useMemo<RunRow[]>(() => [...rowsFromJobs(queue, stored), ...withAnnounced(stored, jobs)], [queue, stored, jobs]);
  const [filter, setFilter] = useState<readonly string[]>([]);
  const shapes = useMemo(() => new Map(Object.entries(listing.shapes).map(([hash, graph]) => [hash, shapeOf(graph)])), [listing]);

  // An id the listing lacks is either gone or newer than the listing. The
  // listing settles it only if it was asked for while the address named the
  // id; otherwise read again, and rewrite nothing meanwhile.
  const unknown = unknownIds(sel, rows);
  const goneKey = JSON.stringify(unknown.filter((id) => askedWith.has(id)));
  const gone = useMemo(() => new Set<string>(JSON.parse(goneKey)), [goneKey]);
  const pending = JSON.stringify(unknown.filter((id) => !askedWith.has(id)));
  const rereadRef = useRef(reread);
  rereadRef.current = reread;
  useEffect(() => {
    if (pending !== '[]') rereadRef.current();
  }, [pending]);
  // A run is filed in the store before its job is said done: read the store, so the run's row takes the job's place.
  const filing = JSON.stringify(
    [...jobs.values()].flatMap(({ work, state }) => (work.kind === 'run' && state.kind === 'done' && state.run_id !== null && !stored.some((r) => runId(r) === state.run_id) ? [state.run_id] : [])),
  );
  useEffect(() => {
    if (filing !== '[]') rereadRef.current();
  }, [filing]);

  const selection = useMemo(() => sanitize(sel, rows, gone), [sel, rows, gone]);
  // An address can carry what no click could have selected — a run the store
  // confirmed gone, one on another benchmark, a sixth: correct it in place.
  useEffect(() => {
    if (selection.join(',') !== sel.join(',')) select(selection);
  }, [selection, sel, select]);

  // The launch panel, opened from the bar; focus moves to it as it opens.
  const [launching, setLaunching] = useState(false);
  const launchId = useId();
  const launchAnchor = useRef<HTMLElement>(null);
  useEffect(() => {
    if (launching) launchAnchor.current?.focus();
  }, [launching]);

  // The job the address shows takes focus when the address moves to it — not on load: a deep link keeps the browser's focus.
  const jobAnchor = useRef<HTMLElement>(null);
  const shownJob = useRef(job);
  useEffect(() => {
    if (job !== undefined && job !== shownJob.current) jobAnchor.current?.focus();
    shownJob.current = job;
  }, [job]);

  // Cancel, reorder and resubmit: each a request to the API, whose outcome the stream then says.
  const [cancelling, setCancelling] = useState<ReadonlySet<string>>(new Set());
  const [refused, setRefused] = useState<ApiProblem | null>(null);
  // Where focus goes once the rows have redrawn, when the control that had it is gone, replaced or moved.
  const focusNext = useRef<{ want: Control; from: Control } | null>(null);
  useLayoutEffect(() => {
    const next = focusNext.current;
    if (next === null) return;
    const target = findControl(next.want);
    if (target === null) return;
    focusNext.current = null;
    const at = document.activeElement;
    // Only if focus is still where the person left it, or was dropped with the control: never taken from elsewhere.
    if (at === null || at === document.body || at === findControl(next.from) || !document.contains(at)) target.focus();
  });
  const holding = (id: string, control: string) => {
    const at = controlOf(document.activeElement);
    return at !== null && at.job === id && at.control === control;
  };

  const cancel = async (id: string) => {
    if (holding(id, 'cancel')) focusNext.current = { want: { job: id, control: 'resubmit' }, from: { job: id, control: 'cancel' } };
    setRefused(null);
    setCancelling((all) => new Set(all).add(id));
    const result = await client.del('/jobs/{id}', { id });
    if (result.ok) return;
    focusNext.current = null;
    setCancelling((all) => new Set([...all].filter((j) => j !== id)));
    setRefused(result.problem);
  };
  const move = async (id: string, place: number, control: string) => {
    setRefused(null);
    const kept = holding(id, control);
    const result = await client.patch('/jobs/{id}', { position: place }, { id });
    if (!result.ok) {
      setRefused(result.problem);
      return;
    }
    // The rows follow the API's answer, not the request: the queue may have placed it elsewhere.
    setPositions(new Map(result.value.jobs.map((j) => [j.id, j.position])));
    if (kept) focusNext.current = { want: { job: id, control }, from: { job: id, control } };
  };
  const resubmit = async (row: RunRow) => {
    if (row.job === null) return;
    setRefused(null);
    const kept = holding(row.source.id, 'resubmit');
    const result = await client.post('/runs', row.job.submission);
    if (!result.ok) {
      setRefused(result.problem);
      return;
    }
    if (kept) focusNext.current = { want: { job: result.value.job_id, control: 'cancel' }, from: { job: row.source.id, control: 'resubmit' } };
  };

  const benchmarks = useMemo(() => {
    const seen = new Map<string, { label: string; count: number }>();
    for (const row of rows) seen.set(row.benchmark, { label: benchmarkLabel(row), count: (seen.get(row.benchmark)?.count ?? 0) + 1 });
    return [...seen];
  }, [rows]);

  const unreadable =
    listing.unreadable.length === 0 ? null : (
      <div className="rg-runs__unreadable">
        <InlineMessage tone="warning" title={`${runs(listing.unreadable.length)} could not be read.`}>
          The store lists {listing.unreadable.length === 1 ? 'it' : 'them'} and cannot load {listing.unreadable.length === 1 ? 'it' : 'them'}; nothing was changed on disk.
        </InlineMessage>
        <ul>
          {listing.unreadable.map((u) => (
            <li key={u.id}>
              <code>{shortHash(u.id)}</code> {u.reason}
            </li>
          ))}
        </ul>
      </div>
    );

  const launchToggle = (
    <Button aria-expanded={launching} aria-controls={launchId} onClick={() => setLaunching(!launching)}>
      Launch…
    </Button>
  );
  const launchPanel = (
    <div id={launchId} hidden={!launching}>
      {launching ? <LaunchPanel client={client} store={store} anchor={launchAnchor} /> : null}
    </div>
  );

  if (rows.length === 0) {
    return (
      <Sheet>
        {unreadable}
        {launchPanel}
        <EmptyState
          heading="No runs yet"
          action={
            <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor' })}>
              Open Editor
            </ButtonLink>
          }
          secondary={launchToggle}
        >
          A run is one pipeline on one benchmark: build a pipeline in the Editor, then launch it.
        </EmptyState>
      </Sheet>
    );
  }

  const stale = connection === 'disconnected';
  const shown = filter.length === 0 ? rows : rows.filter((r) => filter.includes(r.benchmark));
  const groups = groupRows(shown);
  const columns = { latency: rows.some((r) => r.latencyMs !== null), started: rows.some((r) => r.startedAt !== null) };
  const refusals = new Map(rows.map((r) => [rowKey(r), refusal(r, selection, rows)]));
  // A failed run says why on its own row; the selection's rules are said once.
  const rules = [...new Set([...refusals.values()].filter((r) => r !== null && (r.short === 'Other benchmark' || r.short === 'Five selected')).map((r) => r?.full as string))];
  // An id still being read for is kept in the address but is not yet a run Compare can open.
  const comparable = selection.filter((id) => rows.some((r) => runId(r) === id));
  const compareWhy = compareRefusal(comparable);
  const live = rows.some((r) => r.job !== null && (r.status.state === 'queued' || r.status.state === 'running'));

  const byKey = new Map(rows.map((r) => [rowKey(r), r]));
  const onToggle = (key: string) => {
    const row = byKey.get(key);
    const id = row === undefined ? null : runId(row);
    if (id !== null && refusals.get(key) === null) select(toggle(selection, id));
  };
  const onOpen = (key: string) => {
    const row = byKey.get(key);
    if (row !== undefined) navigate(openRoute(row));
  };

  const header = [
    { id: 'benchmark', label: 'Benchmark' },
    { id: 'run', label: 'Run' },
    { id: 'status', label: 'Status' },
    { id: 'metrics', label: 'Metrics' },
    // The median of the queries' own latencies, not the run's wall time: the label says which.
    ...(columns.latency ? [{ id: 'latency', label: 'Median query latency', numeric: true }] : []),
    ...(columns.started ? [{ id: 'started', label: 'Started' }] : []),
  ];
  const tableRows: TableRow[] = groups.flatMap((group) => [
    { kind: 'group' as const, id: `group:${group.key}`, label: <GroupLabel group={group} shape={group.shapeKey === null ? null : (shapes.get(group.shapeKey) ?? null)} /> },
    ...group.rows.map((row) => {
      if (row.job !== null) {
        const id = row.source.id;
        const place = row.job.place ?? 0;
        const drawn = jobRow(row, {
          columns,
          stale,
          cancelling: cancelling.has(id),
          onCancel: () => void cancel(id),
          onMove: (to) => void move(id, to, to < place ? 'up' : 'down'),
          onResubmit: () => void resubmit(row),
        });
        // The job the address shows is marked on its row, which a table row honours as aria-current.
        return id === job ? { ...drawn, selected: true } : drawn;
      }
      const id = runId(row);
      return runRow(row, { selected: id !== null && selection.includes(id), refusal: refusals.get(rowKey(row)) ?? null, columns, onToggle: () => onToggle(rowKey(row)) });
    }),
  ]);

  return (
    <Sheet>
      <div className="rg-runs__bar">
        <div className="rg-runs__filters" role="group" aria-label="Filter by benchmark">
          {benchmarks.map(([key, b]) => (
            <FilterChip
              key={key}
              label={b.label}
              count={b.count}
              pressed={filter.includes(key)}
              onToggle={(pressed) => setFilter(pressed ? [...filter, key] : filter.filter((f) => f !== key))}
            />
          ))}
        </div>
        <div className="rg-runs__actions">
          <ButtonLink href={formatHash({ screen: 'editor' })}>New pipeline</ButtonLink>
          {launchToggle}
          {compareWhy === null ? (
            <Button kind="primary" onClick={() => navigate({ screen: 'compare', ids: comparable })}>
              Compare {comparable.length} selected
            </Button>
          ) : (
            <Button kind="primary" disabled disabledReason={compareWhy}>
              Compare {comparable.length} selected
            </Button>
          )}
        </div>
      </div>
      {launchPanel}
      {job === undefined ? null : <JobPanel client={client} id={job} closeHref={formatHash({ screen: 'runs', sel: [...sel] })} anchor={jobAnchor} />}
      {unreadable}
      {/* Present while the page follows the queue, one line high, so the rows never move as the stream drops and comes back. */}
      {connection === null ? null : (
        <p className="rg-runs__stream" role="status" data-testid="job-stream">
          {stale ? (live ? STREAM_DOWN_LIVE : STREAM_DOWN) : ''}
        </p>
      )}
      <Table caption="Runs, grouped by pipeline" columns={header} rows={tableRows} onOpen={onOpen} onToggle={onToggle} />
      {/* Below the table: what comes and goes as boxes are checked must not move the rows under the pointer. */}
      {refresh === null && rules.length === 0 && refused === null ? null : (
        <div className="rg-runs__rules">
          {refused === null ? null : refused.code === 'run_exists' ? <Conflict problem={refused} /> : <ErrorState problem={refused} />}
          {refresh === null ? null : <ErrorState problem={refresh} onRetry={reread} />}
          {rules.map((rule) => (
            <InlineMessage key={rule} tone="info" title={rule} />
          ))}
        </div>
      )}
    </Sheet>
  );
}

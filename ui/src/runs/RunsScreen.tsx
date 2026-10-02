// The Runs screen: what the store holds, grouped by pipeline, one row per run
// (the front-end design, § 3), most recent first. It reads `GET /runs` alone —
// each pipeline's shape comes with the listing; it owns the benchmark filter;
// the selection it hands to Compare lives in the address. ARCHITECTURE.md
// § The Runs screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, FilterChip, InlineMessage, Sheet, Table, type TableRow } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { RunListing } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { GroupLabel } from './GroupLabel.tsx';
import { benchmarkLabel, groupRows, openRoute, rowKey, rowsFromListing, runId, shapeOf, shortHash } from './model.ts';
import { runRow } from './runRow.tsx';
import './Runs.css';
import { compareRefusal, refusal, sanitize, toggle, unknownIds } from './selection.ts';

export type RunsScreenProps = {
  client: ApiClient;
  /** The selection the address carries, in the order the runs were checked. */
  sel: readonly string[];
};

/** Writes a selection to the address in place: checking a box is state within the view, not a move Back should undo. */
const select = (sel: string[]) => navigate({ screen: 'runs', sel }, { replace: true });

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

/**
 * A listing, with the selection the address carried when it was asked for —
 * what it is authoritative about — and the failure of a later re-read, which
 * keeps the listing on screen rather than replacing it.
 */
type Read = { listing: RequestState<RunListing>; askedWith: ReadonlySet<string>; refresh: ApiProblem | null };

export function RunsScreen({ client, sel }: RunsScreenProps) {
  const [read, setRead] = useState<Read>({ listing: { status: 'loading' }, askedWith: new Set(), refresh: null });
  const latest = useRef(0);
  const selNow = useRef(sel);
  selNow.current = sel;

  /** Reads the listing; only the answer to the last request asked lands, whatever order they arrive in. */
  const fetchListing = useCallback(async () => {
    const mine = ++latest.current;
    const askedWith = new Set(selNow.current);
    const result = await client.get('/runs');
    if (mine !== latest.current) return;
    setRead((prev) => {
      if (result.ok) return { listing: { status: 'loaded', value: result.value }, askedWith, refresh: null };
      // A re-read that fails leaves the listing already shown in place.
      if (prev.listing.status === 'loaded') return { ...prev, refresh: result.problem };
      return { listing: { status: 'error', problem: result.problem }, askedWith, refresh: null };
    });
  }, [client]);

  useEffect(() => {
    void fetchListing();
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
      return <Loaded listing={read.listing.value} askedWith={read.askedWith} refresh={read.refresh} sel={sel} reread={() => void fetchListing()} />;
  }
}

type LoadedProps = {
  listing: RunListing;
  /** The selection the address carried when `listing` was asked for. */
  askedWith: ReadonlySet<string>;
  /** Why the last re-read failed, if it did. */
  refresh: ApiProblem | null;
  sel: readonly string[];
  /** Reads the listing again, keeping this one on screen meanwhile. */
  reread: () => void;
};

function Loaded({ listing, askedWith, refresh, sel, reread }: LoadedProps) {
  const rows = useMemo(() => rowsFromListing(listing), [listing]);
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

  const selection = useMemo(() => sanitize(sel, rows, gone), [sel, rows, gone]);
  // An address can carry what no click could have selected — a run the store
  // confirmed gone, one on another benchmark, a sixth: correct it in place.
  useEffect(() => {
    if (selection.join(',') !== sel.join(',')) select(selection);
  }, [selection, sel]);

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

  if (rows.length === 0) {
    return (
      <Sheet>
        {unreadable}
        <EmptyState
          heading="No runs yet"
          action={
            <ButtonLink kind="primary" size="l" href={formatHash({ screen: 'editor' })}>
              Open Editor
            </ButtonLink>
          }
        >
          A run is one pipeline on one benchmark: build a pipeline in the Editor, then launch it.
        </EmptyState>
      </Sheet>
    );
  }

  const shown = filter.length === 0 ? rows : rows.filter((r) => filter.includes(r.benchmark));
  const groups = groupRows(shown);
  const columns = { latency: rows.some((r) => r.latencyMs !== null), started: rows.some((r) => r.startedAt !== null) };
  const refusals = new Map(rows.map((r) => [rowKey(r), refusal(r, selection, rows)]));
  // A failed run says why on its own row; the selection's rules are said once.
  const rules = [...new Set([...refusals.values()].filter((r) => r !== null && (r.short === 'Other benchmark' || r.short === 'Five selected')).map((r) => r?.full as string))];
  // An id still being read for is kept in the address but is not yet a run Compare can open.
  const comparable = selection.filter((id) => rows.some((r) => runId(r) === id));
  const refused = compareRefusal(comparable);

  const byKey = new Map(rows.map((r) => [rowKey(r), r]));
  const onToggle = (key: string) => {
    const row = byKey.get(key);
    const id = row === undefined ? null : runId(row);
    if (id !== null && refusals.get(key) === null) select(toggle(selection, id));
  };
  const onOpen = (key: string) => {
    const row = byKey.get(key);
    const to = row === undefined ? null : openRoute(row);
    if (to !== null) navigate(to);
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
          {refused === null ? (
            <Button kind="primary" onClick={() => navigate({ screen: 'compare', ids: comparable })}>
              Compare {comparable.length} selected
            </Button>
          ) : (
            <Button kind="primary" disabled disabledReason={refused}>
              Compare {comparable.length} selected
            </Button>
          )}
        </div>
      </div>
      {unreadable}
      <Table caption="Runs, grouped by pipeline" columns={header} rows={tableRows} onOpen={onOpen} onToggle={onToggle} />
      {/* Below the table: what comes and goes as boxes are checked must not move the rows under the pointer. */}
      {refresh === null && rules.length === 0 ? null : (
        <div className="rg-runs__rules">
          {refresh === null ? null : <ErrorState problem={refresh} onRetry={reread} />}
          {rules.map((rule) => (
            <InlineMessage key={rule} tone="info" title={rule} />
          ))}
        </div>
      )}
    </Sheet>
  );
}

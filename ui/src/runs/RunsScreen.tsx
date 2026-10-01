// The Runs screen: what the store holds, grouped by pipeline, one row per run
// (the front-end design, § 3). It reads `GET /runs`, and `GET /runs/{id}` once
// per group for the pipeline's shape; it owns the benchmark filter; the
// selection it hands to Compare lives in the address. ARCHITECTURE.md § The
// Runs screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, FilterChip, InlineMessage, Sheet, Table, type TableRow } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { RunListing } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { GroupLabel } from './GroupLabel.tsx';
import { benchmarkLabel, groupRows, openRoute, rowKey, rowsFromListing, runId, shapeOf, shortHash, type RunRow, type ShapeNode } from './model.ts';
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

/** A listing, with the selection the address carried when it was asked for: what it is authoritative about. */
type Read = { listing: RequestState<RunListing>; askedWith: ReadonlySet<string> };

export function RunsScreen({ client, sel }: RunsScreenProps) {
  const [read, setRead] = useState<Read>({ listing: { status: 'loading' }, askedWith: new Set() });
  const latest = useRef(0);
  const selNow = useRef(sel);
  selNow.current = sel;

  /** Reads the listing; only the answer to the last request asked lands, whatever order they arrive in. */
  const fetchListing = useCallback(async () => {
    const mine = ++latest.current;
    const askedWith = new Set(selNow.current);
    const result = await client.get('/runs');
    if (mine !== latest.current) return;
    setRead({ listing: result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem }, askedWith });
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
      return <Loaded client={client} listing={read.listing.value} askedWith={read.askedWith} sel={sel} reread={() => void fetchListing()} />;
  }
}

type LoadedProps = {
  client: ApiClient;
  listing: RunListing;
  /** The selection the address carried when `listing` was asked for. */
  askedWith: ReadonlySet<string>;
  sel: readonly string[];
  /** Reads the listing again, keeping this one on screen meanwhile. */
  reread: () => void;
};

function Loaded({ client, listing, askedWith, sel, reread }: LoadedProps) {
  const rows = useMemo(() => rowsFromListing(listing), [listing]);
  const [filter, setFilter] = useState<readonly string[]>([]);
  const shapes = useShapes(client, rows);

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
  const refused = compareRefusal(selection);
  const loadingShapes = groups.filter((g) => g.shapeFrom !== null && (shapes.state[g.key]?.status ?? 'loading') === 'loading').length;

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
    ...(columns.latency ? [{ id: 'latency', label: 'Latency', numeric: true }] : []),
    ...(columns.started ? [{ id: 'started', label: 'Started' }] : []),
  ];
  const tableRows: TableRow[] = groups.flatMap((group) => [
    { kind: 'group' as const, id: `group:${group.key}`, label: <GroupLabel group={group} shape={group.shapeFrom === null ? null : (shapes.state[group.key] ?? { status: 'loading' })} onRetry={() => shapes.retry(group.key)} /> },
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
            <Button kind="primary" onClick={() => navigate({ screen: 'compare', ids: selection })}>
              Compare {selection.length} selected
            </Button>
          ) : (
            <Button kind="primary" disabled disabledReason={refused}>
              Compare {selection.length} selected
            </Button>
          )}
        </div>
      </div>
      {unreadable}
      {rules.length === 0 ? null : (
        <div className="rg-runs__rules">
          {rules.map((rule) => (
            <InlineMessage key={rule} tone="info" title={rule} />
          ))}
        </div>
      )}
      {loadingShapes === 0 ? null : (
        <div className="rg-runs__rules">
          <Loading label={`Reading the shapes of ${loadingShapes} pipeline${loadingShapes === 1 ? '' : 's'}`} />
        </div>
      )}
      <Table caption="Runs, grouped by pipeline" columns={header} rows={tableRows} onOpen={onOpen} onToggle={onToggle} />
    </Sheet>
  );
}

/**
 * Each pipeline's shape, read once from one of its own runs — the run that
 * draws its group over the whole listing — and kept by the group's key, so a
 * filter that hides that run does not read the shape again. `retry` reads a
 * failed one again.
 */
function useShapes(client: ApiClient, rows: readonly RunRow[]) {
  const [state, setState] = useState<Record<string, RequestState<ShapeNode[]>>>({});
  const asked = useRef(new Set<string>());
  const wanted = useMemo(() => new Map(groupRows(rows).flatMap((g) => (g.shapeFrom === null ? [] : [[g.key, g.shapeFrom] as const]))), [rows]);

  const load = useCallback(
    (key: string, id: string) => {
      asked.current.add(key);
      setState((current) => ({ ...current, [key]: { status: 'loading' } }));
      void client.get('/runs/{id}', { id }).then((result) => {
        const shape: RequestState<ShapeNode[]> = result.ok ? { status: 'loaded', value: shapeOf(result.value.graph) } : { status: 'error', problem: result.problem };
        setState((current) => ({ ...current, [key]: shape }));
      });
    },
    [client],
  );

  useEffect(() => {
    for (const [key, id] of wanted) if (!asked.current.has(key)) load(key, id);
  }, [wanted, load]);

  const retry = (key: string) => {
    const id = wanted.get(key);
    if (id !== undefined) load(key, id);
  };

  return { state, retry };
}

// The Runs screen: what the store holds, grouped by pipeline, one row per run
// (the front-end design, § 3). It reads `GET /runs`, and `GET /runs/{id}` once
// per group for the pipeline's shape; it owns the benchmark filter; the
// selection it hands to Compare lives in the address. ARCHITECTURE.md § The
// Runs screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, FilterChip, InlineMessage, Sheet } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { RunListing } from '../api/types.ts';
import { formatHash, navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { GroupHeader } from './GroupHeader.tsx';
import { benchmarkLabel, groupRows, rowsFromListing, shapeOf, shortHash, type RunRow, type ShapeNode } from './model.ts';
import { RunRowView } from './RunRowView.tsx';
import './Runs.css';
import { compareRefusal, refusal, sanitize, toggle } from './selection.ts';

export type RunsScreenProps = {
  client: ApiClient;
  /** The selection the address carries, in the order the runs were checked. */
  sel: readonly string[];
};

/** Writes a selection to the address in place: checking a box is not a move Back should undo. */
const select = (sel: string[]) => navigate({ screen: 'runs', sel }, { replace: true });

const runs = (n: number) => `${n.toLocaleString('en-US')} run${n === 1 ? '' : 's'}`;

export function RunsScreen({ client, sel }: RunsScreenProps) {
  const [listing, setListing] = useState<RequestState<RunListing>>({ status: 'loading' });

  const read = useCallback(async () => {
    const result = await client.get('/runs');
    setListing(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
  }, [client]);

  useEffect(() => {
    void read();
  }, [read]);

  const retry = () => {
    setListing({ status: 'loading' });
    void read();
  };

  switch (listing.status) {
    case 'loading':
      return (
        <Sheet>
          <Loading label="Reading runs" />
        </Sheet>
      );
    case 'error':
      return <ErrorState problem={listing.problem} onRetry={retry} />;
    case 'loaded':
      return <Loaded client={client} listing={listing.value} sel={sel} />;
  }
}

function Loaded({ client, listing, sel }: { client: ApiClient; listing: RunListing; sel: readonly string[] }) {
  const rows = useMemo(() => rowsFromListing(listing), [listing]);
  const [filter, setFilter] = useState<readonly string[]>([]);
  const selection = useMemo(() => sanitize(sel, rows), [sel, rows]);
  const shapes = useShapes(client, rows);

  // An address can carry what no click could have selected — a run since
  // removed, one on another benchmark: correct it in place.
  useEffect(() => {
    if (selection.join(',') !== sel.join(',')) select(selection);
  }, [selection, sel]);

  const benchmarks = useMemo(() => {
    const seen = new Map<string, { label: string; count: number }>();
    for (const row of rows) seen.set(row.benchmark, { label: benchmarkLabel(row), count: (seen.get(row.benchmark)?.count ?? 0) + 1 });
    return [...seen];
  }, [rows]);

  const shown = filter.length === 0 ? rows : rows.filter((r) => filter.includes(r.benchmark));
  const groups = groupRows(shown);
  const columns = { latency: rows.some((r) => r.latencyMs !== null), started: rows.some((r) => r.startedAt !== null) };
  const refused = compareRefusal(selection);

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

  const onToggle = (row: RunRow) => select(toggle(selection, row.id));
  const header = ['Benchmark', 'Run', 'Status', 'Metrics', ...(columns.latency ? ['Latency'] : []), ...(columns.started ? ['Started'] : [])];

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
      <div className="rg-tablewrap">
        <table className="rg-table rg-runs__table" aria-label="Runs, grouped by pipeline">
          <thead>
            <tr>
              {header.map((h) => (
                <th key={h} scope="col" className={h === 'Latency' ? 'num' : undefined}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          {groups.map((group) => (
            <tbody key={group.key}>
              <GroupHeader group={group} shape={shapes[group.key] ?? { status: 'loading' }} columns={header.length} />
              {group.rows.map((row) => (
                <RunRowView
                  key={row.id}
                  row={row}
                  selected={selection.includes(row.id)}
                  refusal={refusal(row, selection, rows)}
                  columns={columns}
                  onToggle={() => onToggle(row)}
                  onOpen={() => navigate({ screen: 'replay', run: row.id })}
                />
              ))}
            </tbody>
          ))}
        </table>
      </div>
    </Sheet>
  );
}

/**
 * Each pipeline's shape, read once from one of its own runs — the run that
 * draws its group over the whole listing — and kept by the group's key, so a
 * filter that hides that run does not read the shape again.
 */
function useShapes(client: ApiClient, rows: readonly RunRow[]): Record<string, RequestState<ShapeNode[]>> {
  const [shapes, setShapes] = useState<Record<string, RequestState<ShapeNode[]>>>({});
  const asked = useRef(new Set<string>());
  const wanted = useMemo(() => groupRows(rows).map((g) => [g.key, g.shapeFrom] as const), [rows]);

  useEffect(() => {
    for (const [key, id] of wanted) {
      if (asked.current.has(key)) continue;
      asked.current.add(key);
      void client.get('/runs/{id}', { id }).then((result) => {
        const state: RequestState<ShapeNode[]> = result.ok ? { status: 'loaded', value: shapeOf(result.value.graph) } : { status: 'error', problem: result.problem };
        setShapes((current) => ({ ...current, [key]: state }));
      });
    }
  }, [client, wanted]);

  return shapes;
}

// The Replay screen: one run, one query, node by node — and two runs side by
// side (the front-end design, § 3). Its state is the address,
// `#replay/<run>/q/<query>?with=<run>`; it owns the per-node metric, the
// filter, the search and the selected node, none of them remembered.
// ARCHITECTURE.md § The Replay screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { EmptyState, InlineMessage, RunSwatch, SegmentedControl, Select, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem, ApiResult } from '../api/client.ts';
import type { QueryTrace, RunDetail, RunListing, RunQueries } from '../api/types.ts';
import { Canvas } from '../canvas/index.ts';
import { navigate } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { candidates, defaultMetric, firstJudged, overlayOf, passagesBanner } from './model.ts';
import { NodeInspector, type Side } from './NodeInspector.tsx';
import { QueryList } from './QueryList.tsx';
import './Replay.css';

/** The depth the "where we miss" filter looks for a gold document in: the rank strip's ten cells. */
export const MISS_AT = 10;

export type ReplayScreenProps = {
  client: ApiClient;
  /** The run the address names. */
  run: string;
  /** The query it names, if any. */
  query?: string | undefined;
  /** The run it names beside, if any. */
  with?: string | undefined;
};

type Read<T> = { key: string | null; state: RequestState<T>; shown: T | null; retry: () => void };

/**
 * One request, keyed: asked again whenever the key changes, and only the
 * answer to the last one asked lands. `shown` keeps the last value that
 * loaded while a newer one is read, so a view need not collapse and come
 * back. A null key asks nothing.
 */
function useRead<T>(key: string | null, read: () => Promise<ApiResult<T>>): Read<T> {
  const [value, setValue] = useState<Omit<Read<T>, 'retry'>>({ key: null, state: { status: 'loading' }, shown: null });
  const latest = useRef(0);
  const reader = useRef(read);
  reader.current = read;
  const ask = useCallback(() => {
    const mine = ++latest.current;
    if (key === null) return;
    setValue((prev) => ({ key, state: { status: 'loading' }, shown: prev.state.status === 'loaded' ? prev.state.value : prev.shown }));
    void reader.current().then((result) => {
      if (mine !== latest.current) return;
      setValue((prev) => ({ key, state: result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem }, shown: result.ok ? result.value : prev.shown }));
    });
  }, [key]);
  useEffect(ask, [ask]);
  // Until the first answer for this key lands, the state is its loading.
  const current = value.key === key ? value.state : ({ status: 'loading' } as const);
  return { key, state: current, shown: value.shown, retry: ask };
}

const loaded = <T,>(read: Read<T>): T | null => (read.state.status === 'loaded' ? read.state.value : null);
const short = (id: string) => id.slice(0, 12);
const nameOf = (listing: RunListing | null, id: string) => listing?.runs.find((r) => r.id === id)?.pipeline_names[0] ?? `run ${short(id)}`;
/** Copies a run's full id; a browser that refuses the clipboard leaves the id in the hash's tooltip. */
const copy = (hash: string) => {
  void navigator.clipboard?.writeText(hash).catch(() => {});
};

type Selected = { node: string; from: 'A' | 'B' };

export function ReplayScreen({ client, run, query, with: other }: ReplayScreenProps) {
  const detail = useRead<RunDetail>(`detail ${run}`, () => client.get('/runs/{id}', { id: run }));
  const queries = useRead<RunQueries>(`queries ${run}`, () => client.get('/runs/{id}/queries', { id: run }));
  const listing = useRead<RunListing>('listing', () => client.get('/runs'));
  const [missing, setMissing] = useState(false);
  const missed = useRead<RunQueries>(missing ? `missing ${run}` : null, () => client.get('/runs/{id}/queries', { id: run }, { query: { missing_gold_at: MISS_AT } }));
  const trace = useRead<QueryTrace>(query === undefined ? null : `trace ${run} ${query}`, () => client.get('/runs/{id}/trace/{query}', { id: run, query: query ?? '' }));

  const runs = loaded(listing);
  const offered = useMemo(() => (runs === null ? [] : candidates(runs, run)), [runs, run]);
  // A run beside that is not on this run's benchmark is refused, never drawn.
  const refused = other !== undefined && runs !== null && !offered.some((r) => r.id === other);
  const beside = other !== undefined && !refused ? other : null;
  const otherDetail = useRead<RunDetail>(beside === null ? null : `detail ${beside}`, () => client.get('/runs/{id}', { id: beside ?? '' }));
  const otherQueries = useRead<RunQueries>(beside === null ? null : `queries ${beside}`, () => client.get('/runs/{id}/queries', { id: beside ?? '' }));
  const otherTrace = useRead<QueryTrace>(beside === null || query === undefined ? null : `trace ${beside} ${query}`, () => client.get('/runs/{id}/trace/{query}', { id: beside ?? '', query: query ?? '' }));

  const [chosenMetric, setMetric] = useState<string | null>(null);
  const [selected, setSelected] = useState<Selected | null>(null);
  const listed = loaded(queries);
  const families = loaded(listing)?.runs.find((r) => r.id === run)?.metric_families ?? {};
  const metric = chosenMetric ?? (listed === null ? null : defaultMetric(listed.metrics, families));

  const go = useCallback(
    (q: string, w: string | null) => navigate(w === null ? { screen: 'replay', run, query: q } : { screen: 'replay', run, query: q, with: w }, { replace: true }),
    [run],
  );

  // No query chosen: the first judged one, filled in as a correction.
  const first = listed === null ? null : firstJudged(listed.queries);
  useEffect(() => {
    if (query === undefined && first !== null) go(first.id, null);
  }, [query, first, go]);

  if (detail.state.status === 'error') return <ErrorState problem={detail.state.problem} onRetry={detail.retry} />;
  if (queries.state.status === 'error') return <ErrorState problem={queries.state.problem} onRetry={queries.retry} />;
  const graph = loaded(detail)?.graph;
  if (graph === undefined || listed === null) {
    return (
      <Sheet>
        <Loading label={`Reading run ${short(run)}`} />
      </Sheet>
    );
  }
  if (listed.queries.length === 0) {
    return (
      <Sheet>
        <EmptyState heading="This run executed no query">There is nothing to replay: its traces hold no query.</EmptyState>
      </Sheet>
    );
  }
  if (query === undefined) {
    return (
      <Sheet>
        <Loading label="Opening the first judged query" />
      </Sheet>
    );
  }

  const nameA = nameOf(runs, run);
  const shownTrace = trace.state.status === 'loaded' ? trace.state.value : trace.shown?.run === run ? trace.shown : null;
  const otherGraph = loaded(otherDetail)?.graph;
  // B stands beside A only on the query A shows: an older answer of B's is
  // never drawn beside a newer query of A's, and B's place is held instead.
  const otherAnswer = otherTrace.state.status === 'loaded' ? otherTrace.state.value : null;
  const shownOther = beside !== null && shownTrace !== null && otherAnswer !== null && otherAnswer.run === beside && otherAnswer.query === shownTrace.query ? otherAnswer : null;
  // What keeps B from standing there, said in its place: the first of its reads that failed.
  const otherFailure = [otherDetail, otherQueries, otherTrace].find((r) => r.state.status === 'error');
  const sides: Side[] =
    shownTrace === null
      ? []
      : [
          { letter: 'A', name: nameA, graph, trace: shownTrace, queries: listed },
          ...(beside !== null && otherFailure === undefined && otherGraph !== undefined && shownOther !== null ? [{ letter: 'B' as const, name: nameOf(runs, beside), graph: otherGraph, trace: shownOther, queries: loaded(otherQueries) }] : []),
        ];
  const held: Held | null =
    beside === null || sides.length === 2
      ? null
      : { name: nameOf(runs, beside), failure: otherFailure !== undefined && otherFailure.state.status === 'error' ? { problem: otherFailure.state.problem, retry: otherFailure.retry } : null };
  const filtering = missing && missed.state.status === 'loading';
  const reading =
    trace.state.status === 'loading' && shownTrace !== null
      ? `Reading query ${query}…`
      : beside !== null && otherTrace.state.status === 'loading'
        ? `Reading query ${query} in B…`
        : filtering
          ? `Finding the queries with no gold in the top ${MISS_AT}…`
          : '';
  // While the filter is read, the list on screen stays rather than collapsing.
  const shownQueries = missing ? (loaded(missed)?.queries ?? missed.shown?.queries ?? listed.queries) : listed.queries;
  const verified = listed.ground_truth.status === 'verified';

  return (
    <div className="rg-replay">
      <div className="rg-replay__bar">
        <RunSwatch slot="a" name={nameA} hash={run} onCopyHash={copy} copyLabel="run A" />
        <SegmentedControl
          label="Replay mode"
          value={beside === null ? 'single' : 'side'}
          onChange={(mode) => go(query, mode === 'side' ? (offered[0]?.id ?? null) : null)}
          options={[
            { value: 'single', label: 'One run' },
            { value: 'side', label: 'Side by side', disabled: offered.length === 0, reason: 'No other run on this benchmark' },
          ]}
        />
        {beside === null ? null : (
          <Select id="replay-beside" label="Beside" value={beside} onChange={(e) => go(query, e.target.value)} options={offered.map((r) => ({ value: r.id, label: `${r.pipeline_names[0] ?? 'run'} · ${short(r.id)}` }))} />
        )}
        {listed.metrics.length === 0 ? null : (
          <Select id="replay-metric" label="Metric" value={metric ?? ''} onChange={(e) => setMetric(e.target.value)} options={listed.metrics.map((m) => ({ value: m, label: m }))} />
        )}
      </div>
      <p className="rg-replay__status" role="status">
        {reading}
      </p>
      {refused ? <InlineMessage tone="info" title={`Run ${short(other)} is not on this run’s benchmark, so it cannot stand beside it.`}>Choose a run beside it from the runs on the same benchmark.</InlineMessage> : null}
      <div className="rg-replay__body" data-columns={beside === null ? 1 : 2}>
        <aside className="rg-replay__side" aria-label="Queries of this run">
          {missing && missed.state.status === 'error' ? <ErrorState problem={missed.state.problem} onRetry={missed.retry} /> : null}
          <QueryList
              queries={shownQueries}
              current={query}
              metric={metric}
              onChoose={(q) => go(q, beside)}
              filter={
                verified
                  ? { pressed: missing, onToggle: setMissing, ...(loaded(missed) === null ? {} : { count: loaded(missed)!.queries.length }) }
                  : { pressed: false, onToggle: () => {}, disabled: true, reason: 'Needs the run’s own dataset on disk' }
              }
            />
        </aside>
        <div className="rg-replay__stage" aria-busy={reading !== '' || undefined}>
          {shownTrace === null ? (
            trace.state.status === 'error' ? <ErrorState problem={trace.state.problem} onRetry={trace.retry} /> : <Loading label={`Reading query ${query}…`} />
          ) : (
            <Stage sides={sides} held={held} trace={trace.state.status === 'error' ? trace.state : null} onRetry={trace.retry} metric={metric} selected={selected} onSelect={setSelected} />
          )}
        </div>
        <div className="rg-replay__panel">
          {selected === null || sides.length === 0 ? (
            <p className="rg-replay__placeholder">Select a node to see what it produced for this query.</p>
          ) : (
            <NodeInspector node={selected.node} from={sides.length === 2 ? selected.from : 'A'} sides={sides} metric={metric} onClose={() => setSelected(null)} />
          )}
        </div>
      </div>
    </div>
  );
}

/** B's place while B cannot stand beside A: being read, or failed with what failed. */
type Held = { name: string; failure: { problem: ApiProblem; retry: () => void } | null };

type StageProps = {
  sides: readonly Side[];
  /** The run beside while it cannot be drawn: its place is held, so its canvas arriving moves nothing. */
  held: Held | null;
  /** The newer query's failure, shown above the one still on screen. */
  trace: Extract<RequestState<QueryTrace>, { status: 'error' }> | null;
  onRetry: () => void;
  metric: string | null;
  selected: Selected | null;
  onSelect: (selected: Selected | null) => void;
};

/** The query's head, the banner, and the canvas — or two, stacked, with their run labels. */
function Stage({ sides, held, trace, onRetry, metric, selected, onSelect }: StageProps) {
  const a = sides[0]!;
  const b = sides[1];
  const overlays = useMemo(
    () => sides.map((side, i) => overlayOf({ graph: side.graph, trace: side.trace, metric, letter: side.letter, ...(sides.length === 2 ? { other: { graph: sides[1 - i]!.graph, letter: sides[1 - i]!.letter } } : {}) })),
    [sides, metric],
  );
  const banner = passagesBanner(a.trace.passages);
  const failed = a.trace.nodes.find((n) => n.error !== null);
  const has = (side: Side, id: string) => side.graph.nodes.some((n) => n.id === id) || side.graph.inputs.some((i) => i.id === id);
  return (
    <>
      <h2 className="rg-replay__query">
        <code>{a.trace.query}</code> <span>{a.trace.text ?? ''}</span>
      </h2>
      {trace === null ? null : <ErrorState problem={trace.problem} onRetry={onRetry} />}
      {banner === null ? null : (
        <InlineMessage tone="warning" title={banner.title}>
          <span title={banner.digests}>{banner.detail}</span>
          <span className="rg-visually-hidden"> {banner.digests}</span>
        </InlineMessage>
      )}
      {failed === undefined ? null : <InlineMessage tone="warning" title={`This query failed at ${failed.node}; the nodes after it did not run.`} />}
      <div className="rg-replay__canvases" data-columns={held === null ? sides.length : 2}>
        {sides.map((side, i) => {
          const mine = selected === null ? null : selected.from === side.letter || has(side, selected.node) ? selected.node : null;
          return (
            <div key={side.letter} className="rg-replay__canvas">
              {b === undefined && held === null ? null : <RunSwatch slot={side.letter === 'A' ? 'a' : 'b'} name={side.name} />}
              <Canvas
                graph={side.graph}
                label={`Run ${side.letter}, ${side.name}, query ${side.trace.query}`}
                overlay={overlays[i]}
                selected={mine !== null && has(side, mine) ? mine : null}
                onSelect={(id) => onSelect(id === null ? null : { node: id, from: side.letter })}
              />
            </div>
          );
        })}
        {held === null ? null : (
          <div className="rg-replay__canvas">
            <RunSwatch slot="b" name={held.name} />
            <div className="rg-replay__waiting">{held.failure === null ? <Loading label={`Reading ${held.name}`} /> : <ErrorState problem={held.failure.problem} onRetry={held.failure.retry} />}</div>
          </div>
        )}
      </div>
    </>
  );
}

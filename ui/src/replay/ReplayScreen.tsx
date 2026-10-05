// The Replay screen: one run, one query, node by node — and two runs side by
// side (the front-end design, § 3) — or a failed or cancelled job's partial
// traces, alone and labelled partial. Its state is the address,
// `#replay/<run>/q/<query>/node/<id>?with=<run>` or
// `#replay/job/<id>/q/<query>/node/<id>`, the selected node included; it
// owns the per-node metric, the filter and the search, none of them
// remembered.
// ARCHITECTURE.md § The Replay screen.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Button, ButtonLink, EmptyState, InlineMessage, RunSwatch, SegmentedControl, Select, Sheet } from '../../design/index.ts';
import type { ApiClient, ApiProblem, ApiResult } from '../api/client.ts';
import { ForkButton } from '../editor/Fork.tsx';
import type { Graph, JobSummary, PartialQueries, PartialTrace, QueryTrace, RunDetail, RunListing, RunQueries } from '../api/types.ts';
import { Canvas } from '../canvas/index.ts';
import { formatHash, navigate } from '../routes.ts';
import { prefixText } from '../runs/model.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { defaultMetric } from '../metrics.ts';
import { candidates, editorTarget, firstJudged, fromPartial, overlayOf, passagesBanner, runName, type ReplayTrace } from './model.ts';
import { NodeInspector, type Side } from './NodeInspector.tsx';
import { QueryList } from './QueryList.tsx';
import './Replay.css';

/** The depth the "where we miss" filter looks for a gold document in: the rank strip's ten cells. */
export const MISS_AT = 10;

type RunSource = {
  client: ApiClient;
  /** The run the address names. */
  run: string;
  /** The query it names, if any. */
  query?: string | undefined;
  /** The node it names selected on that query, if any. */
  node?: string | undefined;
  /** The run it names beside, if any. */
  with?: string | undefined;
};

type JobSource = {
  client: ApiClient;
  /** The failed or cancelled run job whose partial traces the address names. */
  job: string;
  /** The query it names, if any. */
  query?: string | undefined;
  /** The node it names selected on that query, if any. */
  node?: string | undefined;
};

/** What Replay reads: a stored run, or a job's partial traces — never one passed off as the other. */
export type ReplayScreenProps = RunSource | JobSource;

type Read<T> = { key: string | null; state: RequestState<T>; shown: T | null; retry: () => void };

/**
 * One request, keyed: asked again whenever the key changes, and only the
 * answer to the last one asked lands. The request a newer one supersedes —
 * by a new key, a retry, or the screen going — is cancelled through its
 * signal, so thirty quick arrow presses leave one request running, not
 * thirty. Two checks drop a superseded answer, either one sufficient: the
 * count of the last one asked, which every cancellation here also moves, and
 * the signal. `shown` keeps the last value that loaded
 * while a newer one is read, so a view need not collapse and come back. A
 * null key asks nothing.
 */
function useRead<T>(key: string | null, read: (signal: AbortSignal) => Promise<ApiResult<T>>): Read<T> {
  const [value, setValue] = useState<Omit<Read<T>, 'retry'>>({ key: null, state: { status: 'loading' }, shown: null });
  const latest = useRef(0);
  const inFlight = useRef<AbortController | null>(null);
  const reader = useRef(read);
  reader.current = read;
  const ask = useCallback(() => {
    const mine = ++latest.current;
    inFlight.current?.abort();
    inFlight.current = null;
    if (key === null) return;
    const controller = new AbortController();
    inFlight.current = controller;
    setValue((prev) => ({ key, state: { status: 'loading' }, shown: prev.state.status === 'loaded' ? prev.state.value : prev.shown }));
    void reader.current(controller.signal).then((result) => {
      // A cancelled request was superseded: its outcome is never shown, an error least of all.
      if (mine !== latest.current || controller.signal.aborted) return;
      setValue((prev) => ({ key, state: result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem }, shown: result.ok ? result.value : prev.shown }));
    });
  }, [key]);
  useEffect(() => {
    ask();
    return () => {
      // The key changed or the screen went: whatever this asked is superseded.
      ++latest.current;
      inFlight.current?.abort();
      inFlight.current = null;
    };
  }, [ask]);
  // Until the first answer for this key lands, the state is its loading.
  const current = value.key === key ? value.state : ({ status: 'loading' } as const);
  return { key, state: current, shown: value.shown, retry: ask };
}

const loaded = <T,>(read: Read<T>): T | null => (read.state.status === 'loaded' ? read.state.value : null);
const short = (id: string) => id.slice(0, 12);
const nameOf = (listing: RunListing | null, id: string) => {
  const run = listing?.runs.find((r) => r.id === id);
  return (run === undefined ? null : runName(run)) ?? `run ${short(id)}`;
};
/** Copies a run's full id; a browser that refuses the clipboard leaves the id in the hash's tooltip. */
const copy = (hash: string) => {
  void navigator.clipboard?.writeText(hash).catch(() => {});
};

type Selected = { node: string; from: 'A' | 'B' };

/** Whether a graph has a node or a declared input of this id. */
const has = (graph: Graph, id: string) => graph.nodes.some((n) => n.id === id) || graph.inputs.some((i) => i.id === id);

/** The B last drawn beside A: kept on screen, stale, while B's answer for a newer query is read. */
type Kept = { run: string; graph: Graph; trace: ReplayTrace };

export function ReplayScreen(props: ReplayScreenProps) {
  return 'job' in props ? <PartialReplay {...props} /> : <RunReplay {...props} />;
}

function RunReplay({ client, run, query, node, with: other }: RunSource) {
  const detail = useRead<RunDetail>(`detail ${run}`, (signal) => client.get('/runs/{id}', { id: run }, { signal }));
  const queries = useRead<RunQueries>(`queries ${run}`, (signal) => client.get('/runs/{id}/queries', { id: run }, { signal }));
  const listing = useRead<RunListing>('listing', (signal) => client.get('/runs', { signal }));
  const [missing, setMissing] = useState(false);
  const missed = useRead<RunQueries>(missing ? `missing ${run}` : null, (signal) => client.get('/runs/{id}/queries', { id: run }, { query: { missing_gold_at: MISS_AT }, signal }));
  const trace = useRead<QueryTrace>(query === undefined ? null : `trace ${run} ${query}`, (signal) => client.get('/runs/{id}/trace/{query}', { id: run, query: query ?? '' }, { signal }));

  const runs = loaded(listing);
  const offered = useMemo(() => (runs === null ? [] : candidates(runs, run)), [runs, run]);
  // A run beside that is not on this run's benchmark is refused, never drawn.
  const refused = other !== undefined && runs !== null && !offered.some((r) => r.id === other);
  const beside = other !== undefined && !refused ? other : null;
  const otherDetail = useRead<RunDetail>(beside === null ? null : `detail ${beside}`, (signal) => client.get('/runs/{id}', { id: beside ?? '' }, { signal }));
  const otherQueries = useRead<RunQueries>(beside === null ? null : `queries ${beside}`, (signal) => client.get('/runs/{id}/queries', { id: beside ?? '' }, { signal }));
  const otherTrace = useRead<QueryTrace>(beside === null || query === undefined ? null : `trace ${beside} ${query}`, (signal) => client.get('/runs/{id}/trace/{query}', { id: beside ?? '', query: query ?? '' }, { signal }));

  const [chosenMetric, setMetric] = useState<string | null>(null);
  // The node selected is the address's; which canvas it was selected on is
  // the screen's, since both canvases select it where both have it.
  const [from, setFrom] = useState<Selected['from']>('A');
  const listed = loaded(queries);
  const families = loaded(listing)?.runs.find((r) => r.id === run)?.metric_families ?? {};
  const metric = chosenMetric ?? (listed === null ? null : defaultMetric(listed.metrics, families));

  // A query, the run beside and the node selected are state within the view:
  // written in place. The node stays selected across queries unless one is given.
  const go = useCallback(
    (q: string, w: string | null, n: string | null | undefined = node) =>
      navigate({ screen: 'replay', run, query: q, ...(n === null || n === undefined ? {} : { node: n }), ...(w === null ? {} : { with: w }) }, { replace: true }),
    [run, node],
  );

  const graph = loaded(detail)?.graph;
  const nameA = nameOf(runs, run);
  const shownTrace = trace.state.status === 'loaded' ? trace.state.value : trace.shown?.run === run ? trace.shown : null;
  const otherGraph = loaded(otherDetail)?.graph;
  // B is drawn as current only on the query A shows: an older answer of B's
  // is never drawn as if it were for a newer query of A's.
  const otherAnswer = otherTrace.state.status === 'loaded' ? otherTrace.state.value : null;
  const shownOther = beside !== null && shownTrace !== null && otherAnswer !== null && otherAnswer.run === beside && otherAnswer.query === shownTrace.query ? otherAnswer : null;
  // What keeps B from standing there, said in its place: the first of its reads that failed.
  const otherFailure = [otherDetail, otherQueries, otherTrace].find((r) => r.state.status === 'error');
  const sides: Side[] =
    shownTrace === null || graph === undefined || listed === null
      ? []
      : [
          { letter: 'A', name: nameA, graph, trace: shownTrace, queries: listed },
          ...(beside !== null && otherFailure === undefined && otherGraph !== undefined && shownOther !== null ? [{ letter: 'B' as const, name: nameOf(runs, beside), graph: otherGraph, trace: shownOther, queries: loaded(otherQueries) }] : []),
        ];
  // The B last drawn is remembered, so that while B reads a newer query its
  // canvas stays where it was — the same canvas, its pan and zoom kept —
  // muted and labelled stale, rather than unmounted and rebuilt.
  const [kept, setKept] = useState<Kept | null>(null);
  const drawnB = sides[1];
  const drawnGraph = drawnB?.graph;
  const drawnTrace = drawnB?.trace;
  useEffect(() => {
    if (beside !== null && drawnGraph !== undefined && drawnTrace !== undefined) setKept({ run: beside, graph: drawnGraph, trace: drawnTrace });
  }, [beside, drawnGraph, drawnTrace]);
  // B put away, or switched to another run, forgets it: B's next first read
  // shows its loading line, never a canvas of another run or an older visit.
  useEffect(() => {
    setKept((k) => (k !== null && k.run === beside ? k : null));
  }, [beside]);
  // Only the same run's canvas is kept, and never over a failure, which is said in its place.
  const stale = sides.length === 1 && beside !== null && otherFailure === undefined && kept?.run === beside ? kept : null;
  const graphB = drawnB?.graph ?? stale?.graph ?? null;

  const selected: Selected | null = node === undefined ? null : { node, from: (from === 'B' && graphB !== null && has(graphB, node)) || (graph !== undefined && !has(graph, node)) ? 'B' : 'A' };
  const select = (s: Selected | null) => {
    if (query === undefined) return;
    if (s !== null) setFrom(s.from);
    go(query, beside, s?.node ?? null);
  };
  // A node selected in B goes with B: in A it would read as "not run".
  useEffect(() => {
    if (beside !== null || from !== 'B') return;
    setFrom('A');
    if (node !== undefined && query !== undefined) go(query, null, null);
  }, [beside, from, node, query, go]);
  // So does a node neither graph on screen has — B switched to a run that lacks it.
  useEffect(() => {
    if (graph === undefined || node === undefined || query === undefined) return;
    if (!has(graph, node) && (graphB === null || !has(graphB, node))) go(query, beside, null);
  }, [graph, graphB, node, query, beside, go]);

  // No query chosen: the first judged one, filled in as a correction.
  const first = listed === null ? null : firstJudged(listed.queries);
  useEffect(() => {
    if (query === undefined && first !== null) go(first.id, null, null);
  }, [query, first, go]);

  if (detail.state.status === 'error') return <ErrorState problem={detail.state.problem} onRetry={detail.retry} />;
  if (queries.state.status === 'error') return <ErrorState problem={queries.state.problem} onRetry={queries.retry} />;
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

  // The node the editor opens on, from either button: one of run A's — a node
  // only B has is no node of the document A ran.
  const nodeA = selected !== null && has(graph, selected.node) ? selected.node : null;
  // What the stage draws: the sides, and B's kept canvas while B is stale.
  const drawn: Side[] = stale === null ? sides : [...sides, { letter: 'B', name: nameOf(runs, stale.run), graph: stale.graph, trace: stale.trace, queries: null }];
  const held: Held | null =
    beside === null || drawn.length === 2
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
        <ForkButton client={client} run={run} size="s" node={nodeA} />
        <OpenInEditor listing={listing} run={run} node={nodeA} />
        <SegmentedControl
          label="Replay mode"
          value={beside === null ? 'single' : 'side'}
          onChange={(mode) => go(query, mode === 'side' ? (offered[0]?.id ?? null) : null)}
          options={[
            { value: 'single', label: 'One run' },
            {
              value: 'side',
              label: 'Side by side',
              disabled: offered.length === 0,
              reason: listing.state.status === 'error' ? `The runs could not be listed: ${listing.state.problem.message}` : 'No other run on this benchmark',
            },
          ]}
        />
        {beside === null ? null : (
          <Select id="replay-beside" label="Beside" value={beside} onChange={(e) => go(query, e.target.value)} options={offered.map((r) => {
              const prefix = prefixText(r);
              return { value: r.id, label: `${runName(r) ?? 'run'} · ${short(r.id)}${prefix === null ? '' : ` · ${prefix}`}` };
            })} />
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
            <Stage sides={drawn} stale={stale === null ? null : { asked: query, reading: otherTrace.state.status === 'loading' }} held={held} trace={trace.state.status === 'error' ? trace.state : null} onRetry={trace.retry} metric={metric} selected={selected} onSelect={select} />
          )}
        </div>
        <div className="rg-replay__panel">
          {selected === null || sides.length === 0 ? (
            <p className="rg-replay__placeholder">Select a node to see what it produced for this query.</p>
          ) : sides.length === 1 && !has(graph, selected.node) ? (
            // A node only B has, selected on B's kept canvas while B reads this query.
            <p className="rg-replay__placeholder">No such node in A.</p>
          ) : (
            <NodeInspector node={selected.node} from={sides.length === 2 ? selected.from : 'A'} sides={sides} held={beside !== null && sides.length === 1} metric={metric} onClose={() => select(null)} />
          )}
        </div>
      </div>
    </div>
  );
}

/** How a job ended, in the words the partial banner opens on. */
function ending(job: JobSummary): string {
  const { state } = job;
  if (state.kind === 'failed') return state.at_node === null ? `Job ${job.id} failed: ${state.error}.` : `Job ${job.id} failed at ${state.at_node}: ${state.error}.`;
  if (state.kind === 'cancelled') return `Job ${job.id} was cancelled.`;
  return `Job ${job.id} is ${state.kind}.`;
}

/**
 * Replay over a failed or cancelled run job's partial traces
 * (`#replay/job/<id>/q/<query>`): the traces it kept, read query by query
 * as a run's are, alone and labelled partial — never as a stored run. The API
 * reads them against no dataset, so nothing is scored and no text is shown,
 * and nothing stands beside them.
 */
function PartialReplay({ client, job, query, node }: JobSource) {
  const queries = useRead<PartialQueries>(`partial ${job}`, (signal) => client.get('/jobs/{id}/queries', { id: job }, { signal }));
  const trace = useRead<PartialTrace>(query === undefined ? null : `partial ${job} ${query}`, (signal) => client.get('/jobs/{id}/trace/{query}', { id: job, query: query ?? '' }, { signal }));
  const listed = loaded(queries);
  // The node selected is the address's, kept across queries, as a run's is.
  const go = useCallback(
    (q: string, n: string | null | undefined = node) => navigate({ screen: 'replay', job, query: q, ...(n === null || n === undefined ? {} : { node: n }) }, { replace: true }),
    [job, node],
  );
  const selected: Selected | null = node === undefined ? null : { node, from: 'A' };
  const select = (s: Selected | null) => {
    if (query !== undefined) go(query, s?.node ?? null);
  };
  // A node the job's graph does not have is cleared, never read as "not run".
  useEffect(() => {
    if (listed !== null && node !== undefined && query !== undefined && !has(listed.graph, node)) go(query, null);
  }, [listed, node, query, go]);
  // No query chosen: the one a failed job stopped on, else the first kept one, filled in as a correction.
  const first = listed === null ? null : (listed.failed_query ?? listed.queries[0]?.id ?? null);
  useEffect(() => {
    if (query === undefined && first !== null) go(first, null);
  }, [query, first, go]);

  if (queries.state.status === 'error') return <ErrorState problem={queries.state.problem} onRetry={queries.retry} />;
  if (listed === null) {
    return (
      <Sheet>
        <Loading label={`Reading job ${job}`} />
      </Sheet>
    );
  }
  if (query === undefined) {
    return (
      <Sheet>
        <Loading label="Opening the query it stopped on" />
      </Sheet>
    );
  }
  // A newer query's trace is read while the one on screen stays.
  const answer = trace.state.status === 'loaded' ? trace.state.value : trace.shown;
  const sides: Side[] = answer === null ? [] : [{ letter: 'A', name: `job ${job}`, graph: listed.graph, trace: fromPartial(answer), queries: null }];
  const reading = trace.state.status === 'loading' && answer !== null ? `Reading query ${query}…` : '';
  const kept = listed.queries.length;
  return (
    <div className="rg-replay">
      <div className="rg-replay__bar">
        <p className="rg-replay__source">
          <strong>Partial traces</strong> <code title={job}>{job}</code>
        </p>
        <SegmentedControl
          label="Replay mode"
          value="single"
          onChange={() => {}}
          options={[
            { value: 'single', label: 'Alone' },
            { value: 'side', label: 'Side by side', disabled: true, reason: 'A job’s partial traces are replayed alone' },
          ]}
        />
      </div>
      <p className="rg-replay__status" role="status">
        {reading}
      </p>
      <InlineMessage tone="info" title={`Partial traces of job ${job}`}>
        {ending(listed.job)} These are the traces of the {kept === 1 ? 'query' : `${kept.toLocaleString('en-US')} queries`} it executed before it stopped. No run was stored, so nothing is scored and no passage text is shown: a run records the dataset it was evaluated on, and these traces record none.{' '}
        <ButtonLink size="s" href={formatHash({ screen: 'runs', job })}>
          Open the job
        </ButtonLink>
      </InlineMessage>
      <div className="rg-replay__body" data-columns={1}>
        <aside className="rg-replay__side" aria-label="Queries of this job">
          <QueryList queries={listed.queries} current={query} metric={null} onChoose={(q) => go(q)} filter={{ pressed: false, onToggle: () => {}, disabled: true, reason: 'Partial traces are not scored' }} />
        </aside>
        <div className="rg-replay__stage" aria-busy={reading !== '' || undefined}>
          {sides.length === 0 ? (
            trace.state.status === 'error' ? <ErrorState problem={trace.state.problem} onRetry={trace.retry} /> : <Loading label={`Reading query ${query}…`} />
          ) : (
            <Stage
              sides={sides}
              stale={null}
              held={null}
              trace={trace.state.status === 'error' ? trace.state : null}
              onRetry={trace.retry}
              metric={null}
              selected={selected}
              onSelect={select}
              labelOf={(side) => `Job ${job}, partial traces, query ${side.trace.query}`}
            />
          )}
        </div>
        <div className="rg-replay__panel">
          {selected === null || sides.length === 0 ? (
            <p className="rg-replay__placeholder">Select a node to see what it produced for this query.</p>
          ) : (
            <NodeInspector node={selected.node} from="A" sides={sides} metric={null} onClose={() => select(null)} />
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * "Open in the editor": the stored document holding what the run ran
 * (`editorTarget`), opened on the node selected here, so the selection
 * survives the move from Replay to the editor. Refused, saying why, while the
 * runs are read, when they could not be, and when no document holds the run —
 * "Fork this run" beside it is then the way to edit it.
 */
function OpenInEditor({ listing, run, node }: { listing: Read<RunListing>; run: string; node: string | null }) {
  const summary = loaded(listing)?.runs.find((r) => r.id === run);
  const target =
    listing.state.status === 'error'
      ? { reason: `The runs could not be listed: ${listing.state.problem.message}` }
      : listing.state.status === 'loading'
        ? { reason: 'Reading the runs…' }
        : summary === undefined
          ? { reason: 'This run is not in the listing of runs, so no pipeline document is known to hold it.' }
          : editorTarget(summary);
  if ('reason' in target) {
    return (
      <Button size="s" disabled disabledReason={target.reason}>
        Open in the editor
      </Button>
    );
  }
  return (
    <ButtonLink size="s" href={formatHash(node === null ? { screen: 'editor', name: target.name } : { screen: 'editor', name: target.name, node })}>
      Open in the editor
    </ButtonLink>
  );
}

/** B's place while B has no canvas to show — its first read, a run just chosen, or a failure, with what failed. */
type Held = { name: string; failure: { problem: ApiProblem; retry: () => void } | null };

type StageProps = {
  /** A, then B: drawn current, or kept from an earlier query while `stale`. */
  sides: readonly Side[];
  /**
   * B's canvas is kept from an earlier query: B is muted and labelled stale,
   * with the query asked when B's answer for it is being read.
   */
  stale: { asked: string; reading: boolean } | null;
  /** The run beside while it has no canvas, current or kept: its place is held, so its canvas arriving moves nothing. */
  held: Held | null;
  /** The newer query's failure, shown above the one still on screen. */
  trace: Extract<RequestState<QueryTrace>, { status: 'error' }> | null;
  onRetry: () => void;
  metric: string | null;
  selected: Selected | null;
  onSelect: (selected: Selected | null) => void;
  /** Each canvas's accessible name; a run's by default. */
  labelOf?: (side: Side, stale: boolean) => string;
};

const runLabel = (side: Side, stale: boolean) => `Run ${side.letter}, ${side.name}, query ${side.trace.query}${stale ? ', stale' : ''}`;

/** The query's head, the banner, and the canvas — or two, stacked, with their run labels. */
function Stage({ sides, stale, held, trace, onRetry, metric, selected, onSelect, labelOf = runLabel }: StageProps) {
  const a = sides[0]!;
  const b = sides[1];
  const overlays = useMemo(
    () => sides.map((side, i) => overlayOf({ graph: side.graph, trace: side.trace, metric, letter: side.letter, ...(sides.length === 2 ? { other: { graph: sides[1 - i]!.graph, letter: sides[1 - i]!.letter } } : {}) })),
    [sides, metric],
  );
  // Each run's passages are checked on their own: B's dataset may differ where A's does not.
  const banners = sides.flatMap((side) => {
    const banner = side.trace.passages === null ? null : passagesBanner(side.trace.passages);
    if (banner === null) return [];
    return [{ ...banner, letter: side.letter, title: sides.length === 2 ? `Run ${side.letter}: ${banner.title.charAt(0).toLowerCase()}${banner.title.slice(1)}` : banner.title }];
  });
  const failed = a.trace.nodes.find((n) => n.error !== null);
  return (
    <>
      <h2 className="rg-replay__query" title={a.trace.text ?? undefined}>
        <code>{a.trace.query}</code> <span>{a.trace.text ?? ''}</span>
      </h2>
      {trace === null ? null : <ErrorState problem={trace.problem} onRetry={onRetry} />}
      {banners.map((banner) => (
        <InlineMessage key={banner.letter} tone="warning" title={banner.title}>
          <span title={banner.digests}>{banner.detail}</span>
          <span className="rg-visually-hidden"> {banner.digests}</span>
        </InlineMessage>
      ))}
      {failed === undefined ? null : <InlineMessage tone="warning" title={`This query failed at ${failed.node}; the nodes after it did not run.`} />}
      <div className="rg-replay__canvases" data-columns={held === null ? sides.length : 2}>
        {sides.map((side, i) => {
          const mine = selected !== null && has(side.graph, selected.node) ? selected.node : null;
          const old = side.letter === 'B' && stale !== null;
          // The key is the letter, whether B is current or kept: B's canvas stays mounted across queries.
          return (
            <div key={side.letter} className="rg-replay__canvas" data-stale={old || undefined}>
              {b === undefined && held === null ? null : <RunSwatch slot={side.letter === 'A' ? 'a' : 'b'} name={side.name} />}
              <Canvas
                graph={side.graph}
                label={labelOf(side, old)}
                overlay={overlays[i]}
                selected={mine}
                onSelect={(id) => onSelect(id === null ? null : { node: id, from: side.letter })}
              />
              {old ? (
                <p className="rg-replay__stale">
                  Stale: query {side.trace.query}
                  {stale?.reading === true ? `, reading ${stale.asked}…` : null}
                </p>
              ) : null}
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

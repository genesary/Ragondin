/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockReply } from '../api/testing.ts';
import type { Problem, QueryTrace, RunDetail, RunQueries } from '../api/types.ts';
import { parseHash } from '../routes.ts';
import {
  DENSE,
  DENSE_DETAIL,
  DENSE_QUERIES,
  DENSE_TRACE,
  ELSEWHERE,
  FAILED,
  FAILED_DETAIL,
  FAILED_QUERIES,
  FAILED_TRACE,
  HYBRID,
  HYBRID_DETAIL,
  HYBRID_MISSING,
  HYBRID_QUERIES,
  HYBRID_TRACE,
  LISTING,
  withPassages,
} from './fixtures.ts';
import { declared } from '../../design/testing/css.ts';
import css from './Replay.css?raw';
import { ReplayScreen } from './ReplayScreen.tsx';

const problem = (code: Problem['code'], status: number, detail: string): Problem => ({ type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint: 'Pick another.' });
const segment = (path: string, i: number) => decodeURIComponent(path.split('/')[i] ?? '');

const DETAILS: Record<string, RunDetail> = { [HYBRID]: HYBRID_DETAIL, [DENSE]: DENSE_DETAIL, [FAILED]: FAILED_DETAIL };
const QUERIES: Record<string, RunQueries> = { [HYBRID]: HYBRID_QUERIES, [DENSE]: DENSE_QUERIES, [FAILED]: FAILED_QUERIES };
const TRACES: Record<string, QueryTrace> = { [HYBRID]: HYBRID_TRACE, [DENSE]: DENSE_TRACE, [FAILED]: FAILED_TRACE };

/** The API over the fixtures: each run's detail, queries and q1's trace; another query's trace is q1's renamed. */
type Override<T> = (run: string) => MockReply<T> | Promise<MockReply<T>> | undefined;
function api({
  traces = TRACES,
  trace,
  queries,
  detail,
}: { traces?: Record<string, QueryTrace>; trace?: (run: string, query: string) => MockReply<QueryTrace> | Promise<MockReply<QueryTrace>>; queries?: Override<RunQueries>; detail?: Override<RunDetail> } = {}) {
  return mockApi({
    'GET /runs': { body: LISTING },
    'GET /runs/{id}': (_q, path) => detail?.(segment(path, 4)) ?? (DETAILS[segment(path, 4)] === undefined ? { problem: problem('run_not_found', 404, 'no such run') } : { body: DETAILS[segment(path, 4)]! }),
    'GET /runs/{id}/queries': (query, path) => {
      const run = segment(path, 4);
      const replaced = queries?.(run);
      if (replaced !== undefined) return replaced;
      if (query.get('missing_gold_at') === '10') return { body: HYBRID_MISSING };
      return { body: QUERIES[run]! };
    },
    'GET /runs/{id}/trace/{query}': (_q, path) => {
      const [run, q] = [segment(path, 4), segment(path, 6)];
      if (trace !== undefined) return trace(run, q);
      if (q === 'nope') return { problem: problem('query_not_found', 404, 'The run holds no query nope.') };
      const t = traces[run]!;
      return { body: { ...t, query: q, text: HYBRID_QUERIES.queries.find((x) => x.id === q)?.text ?? null } };
    },
  });
}

const show = (props: { run?: string; query?: string; with?: string } = {}) =>
  render(
    <div style={{ width: 1400 }}>
      <ReplayScreen client={createApiClient()} run={props.run ?? HYBRID} query={props.query} with={props.with} />
    </div>,
  );

const nodeOf = (root: HTMLElement, id: string) => root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement;
/** A node once the canvas has drawn it: the application can be in the page a frame before its nodes. */
const drawn = (root: HTMLElement, id: string) =>
  waitFor(() => {
    const node = nodeOf(root, id);
    if (node === null) throw new Error(`node ${id} is not drawn yet`);
    return node;
  });
const cardOf = (root: HTMLElement, id: string) => nodeOf(root, id).querySelector('.rg-node') as HTMLElement;
const route = () => parseHash(window.location.hash);

beforeEach(() => window.history.replaceState(null, '', '/'));
afterEach(() => vi.unstubAllGlobals());

describe('Replay, one run', () => {
  it('shows every node’s duration and share, its metric, and a rank strip filled at the gold ranks on the ranking nodes', async () => {
    api();
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    // Every node of a canvas is drawn in the same frame: one drawn, all are.
    await drawn(graph.parentElement!, 'rerank');
    const rerank = cardOf(graph.parentElement!, 'rerank');
    expect(rerank.getAttribute('data-replay')).toBe('true');
    expect(within(rerank).getByText('349 ms')).toBeTruthy();
    expect(within(rerank).getByText('0.8610')).toBeTruthy();
    expect(within(rerank).getByRole('img', { name: '2 gold passages in the top 10, at rank 1, 2' })).toBeTruthy();
    expect(within(rerank).getByText('4 discarded')).toBeTruthy();
    expect(within(cardOf(graph.parentElement!, 'answer')).getByText('600 ms')).toBeTruthy();
    expect(cardOf(graph.parentElement!, 'answer').querySelector('.rg-rankstrip')).toBeNull();
  });

  it('carries the metric, the rank sentence and the duration in each node’s accessible description', async () => {
    api();
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    const ids = (await drawn(graph.parentElement!, 'rerank')).getAttribute('aria-describedby')!.split(' ');
    expect(document.getElementById(ids[0]!)?.textContent).toBe("ndcg@10 0.8610. 2 gold passages in the top 10, at rank 1, 2. 4 discarded. 349 ms, 35% of this query's time.");
  });

  it('lists the reranker’s kept chunks in rank order with gold stars, and its discarded ones struck through with their former rank, when it is selected', async () => {
    api();
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    fireEvent.click(await drawn(graph.parentElement!, 'rerank'));
    const kept = await screen.findByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    expect(within(kept).getAllByRole('listitem').map((li) => [li.querySelector('.rg-replay__chunk')?.textContent, li.getAttribute('data-gold')])).toEqual([
      ['c5', 'true'],
      ['c2', 'true'],
      ['c4', null],
      ['c3', null],
    ]);
    expect(within(kept).getByText('Passage 5 of the corpus.')).toBeTruthy();
    const discarded = screen.getByRole('list', { name: 'Discarded, 4 chunks' });
    expect(discarded.querySelectorAll('del')).toHaveLength(4);
    expect(discarded.textContent).toContain('was rank 5');
  });

  it('reserves the inspector’s place before a node is selected, so selecting one moves nothing', async () => {
    api();
    show({ query: 'q1' });
    await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    expect(screen.getByText('Select a node to see what it produced for this query.')).toBeTruthy();
  });

  it('shows the query’s text', async () => {
    api();
    show({ query: 'q1' });
    expect(await screen.findByRole('heading', { name: 'q1 What do tides depend on?' })).toBeTruthy();
  });

  it('gives the query’s text a fixed height of two lines, the whole text on hover and to assistive technology, so another query moves nothing below it', async () => {
    api();
    show({ query: 'q1' });
    const heading = await screen.findByRole('heading', { name: 'q1 What do tides depend on?' });
    expect(heading.getAttribute('title')).toBe('What do tides depend on?');
    expect(declared(css, '.rg-replay__query', 'height')).toBe('40px');
    expect(declared(css, '.rg-replay__query', '-webkit-line-clamp')).toBe('2');
    expect(declared(css, '.rg-replay__query', 'overflow')).toBe('hidden');
  });
});

describe('the passages banner', () => {
  it.each([
    ['dataset_absent', 'Passage text is hidden: the dataset of this run is not on disk.'],
    ['dataset_differs', 'Passage text is hidden: the dataset on disk differs from the one this run was evaluated on.'],
  ] as const)('with %s, names the case, gives the digests on hover, and shows ids instead of text', async (status, title) => {
    api({ traces: { ...TRACES, [HYBRID]: withPassages(HYBRID_TRACE, status) } });
    show({ query: 'q1' });
    const banner = (await screen.findByText(title)).closest('.rg-inline') as HTMLElement;
    expect(banner.querySelector('[title]')?.getAttribute('title')).toMatch(/^The run expects dataset 5{64}/);
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    fireEvent.click(await drawn(graph.parentElement!, 'rerank'));
    const kept = await screen.findByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    expect(within(kept).queryByText('Passage 5 of the corpus.')).toBeNull();
    expect(within(kept).getAllByRole('listitem').every((li) => li.getAttribute('data-text') === 'none')).toBe(true);
  });

  it('is absent when the passages are verified', async () => {
    api();
    show({ query: 'q1' });
    await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    expect(screen.queryByText(/Passage text is hidden/)).toBeNull();
  });
});

describe('Replay side by side', () => {
  it('stacks two canvases with their run labels, and draws the nodes the dense-only run lacks dashed, as absent from it', async () => {
    api();
    show({ query: 'q1', with: DENSE });
    const a = await screen.findByRole('application', { name: /^Run A, hybrid-rerank-gen/ });
    const b = await screen.findByRole('application', { name: /^Run B, dense-only/ });
    await drawn(a.parentElement!, 'dense');
    await drawn(b.parentElement!, 'dense');
    for (const id of ['bm25', 'rrf', 'rerank']) {
      expect(cardOf(a.parentElement!, id).getAttribute('data-only-here'), id).toBe('true');
      expect(within(cardOf(a.parentElement!, id)).getByText('only in A')).toBeTruthy();
    }
    expect(cardOf(a.parentElement!, 'dense').getAttribute('data-only-here')).toBeNull();
    expect(cardOf(b.parentElement!, 'dense').getAttribute('data-only-here')).toBeNull();
  });

  it("shows the hybrid's rerank output beside the dense-only run's final output, labelled as such", async () => {
    api();
    show({ query: 'q1', with: DENSE });
    const a = await screen.findByRole('application', { name: /^Run A/ });
    await screen.findByRole('application', { name: /^Run B/ });
    fireEvent.click(await drawn(a.parentElement!, 'rerank'));
    const columnB = await screen.findByRole('region', { name: 'B, dense-only' });
    const columnA = screen.getByRole('region', { name: 'A, hybrid-rerank-gen' });
    expect(within(columnA).getByRole('list', { name: 'Ranked by rerank, 4 chunks' })).toBeTruthy();
    expect(within(columnB).getByText('No such node in B.')).toBeTruthy();
    expect(within(columnB).getByRole('heading', { name: "B's final output: dense" })).toBeTruthy();
  });

  it('holds the place of the run beside while it is read, labelled, so its canvas arriving moves nothing', async () => {
    api({ trace: (run, q) => (run === DENSE ? new Promise(() => {}) : { body: { ...HYBRID_TRACE, query: q } }) });
    show({ query: 'q1', with: DENSE });
    expect(await screen.findByText('Reading dense-only')).toBeTruthy();
    expect(screen.getByRole('application', { name: /^Run A/ })).toBeTruthy();
    expect(document.querySelector('.rg-replay__canvases')?.getAttribute('data-columns')).toBe('2');
  });

  it('offers beside the run only the runs on its benchmark', async () => {
    api();
    show({ query: 'q1', with: DENSE });
    const beside = (await screen.findByLabelText('Beside')) as HTMLSelectElement;
    const offered = [...beside.options].map((o) => o.value);
    expect(offered).toContain(DENSE);
    expect(offered).toContain(FAILED);
    expect(offered).not.toContain(ELSEWHERE);
    expect(offered).not.toContain(HYBRID);
  });

  it('refuses a run on another benchmark named in the address, says why, and replays the one run', async () => {
    api();
    show({ query: 'q1', with: ELSEWHERE });
    expect(await screen.findByText(/is not on this run’s benchmark/)).toBeTruthy();
    expect(screen.getAllByRole('application')).toHaveLength(1);
  });

  it('turns side by side on and off through the address, beside the first run of the benchmark', async () => {
    api();
    show({ query: 'q1' });
    const mode = await screen.findByRole('radiogroup', { name: 'Replay mode' });
    fireEvent.click(within(mode).getByRole('radio', { name: 'Side by side' }));
    expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q1', with: DENSE });
  });
});

describe('the query selector', () => {
  it('filters to exactly the queries the API flags as missing gold, asked through the generated client', async () => {
    const calls = api();
    show({ query: 'q1' });
    const list = await screen.findByRole('listbox', { name: 'Queries' });
    expect(within(list).getAllByRole('option')).toHaveLength(3);
    fireEvent.click(screen.getByRole('button', { name: /No gold in the top 10/ }));
    await waitFor(() => expect(within(list).getAllByRole('option').map((o) => o.getAttribute('data-query'))).toEqual(['q2']));
    expect(calls.requests).toContain(`GET /api/v1/runs/${HYBRID}/queries?missing_gold_at=10`);
  });

  it('moves through the queries with the arrow keys, and the address follows', async () => {
    api();
    show({ query: 'q1' });
    const list = await screen.findByRole('listbox', { name: 'Queries' });
    fireEvent.keyDown(list, { key: 'ArrowDown' });
    expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q2' });
  });

  it('keeps the query on screen while the next one is read, and says so', async () => {
    let release: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (_run, q) => (q === 'q1' ? { body: HYBRID_TRACE } : new Promise((r) => (release = r))) });
    const view = show({ query: 'q1' });
    await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    view.rerender(
      <div style={{ width: 1400 }}>
        <ReplayScreen client={createApiClient()} run={HYBRID} query="q2" with={undefined} />
      </div>,
    );
    expect(await screen.findByText('Reading query q2…')).toBeTruthy();
    expect(screen.getByRole('application', { name: /hybrid-rerank-gen/ })).toBeTruthy();
    await act(async () => release({ body: { ...HYBRID_TRACE, query: 'q2' } }));
    await waitFor(() => expect(screen.queryByText('Reading query q2…')).toBeNull());
  });
});

describe('Replay of a failed run', () => {
  it('renders up to the node that failed, with its error, and the nodes after it as not run', async () => {
    api();
    show({ run: FAILED, query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-broken/ });
    // Every node of a canvas is drawn in the same frame: one drawn, all are.
    await drawn(graph.parentElement!, 'rerank');
    const rerank = cardOf(graph.parentElement!, 'rerank');
    expect(rerank.getAttribute('data-status')).toBe('failed');
    expect(within(rerank).getByText('The service at 127.0.0.1:7001 did not answer within 30 s.')).toBeTruthy();
    for (const id of ['context', 'answer']) expect(cardOf(graph.parentElement!, id).getAttribute('data-status'), id).toBe('not-run');
    expect(screen.getByText('This query failed at rerank; the nodes after it did not run.')).toBeTruthy();
  });
});

describe('Replay’s states', () => {
  it('opens on the first judged query when none is chosen, correcting the address in place', async () => {
    api();
    show({});
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q1' }));
  });

  it('says a query the run does not hold, inline, with the way on', async () => {
    api();
    show({ query: 'nope' });
    expect(await screen.findByText('The run holds no query nope.')).toBeTruthy();
    expect(screen.getByText('query_not_found')).toBeTruthy();
  });

  it('says a run that does not exist, inline', async () => {
    api();
    show({ run: 'a'.repeat(64), query: 'q1' });
    expect(await screen.findByText('no such run')).toBeTruthy();
  });

  it('says what it is reading while it reads', () => {
    api({ trace: () => new Promise(() => {}) });
    show({ query: 'q1' });
    expect(screen.getByText(/^Reading run/)).toBeTruthy();
  });
});

const rerender = (view: ReturnType<typeof show>, props: { query: string; with?: string }) =>
  view.rerender(
    <div style={{ width: 1400 }}>
      <ReplayScreen client={createApiClient()} run={HYBRID} query={props.query} with={props.with} />
    </div>,
  );
const traceOf = (base: QueryTrace, q: string): MockReply<QueryTrace> => ({ body: { ...base, query: q } });

describe('side by side, never two queries at once', () => {
  it("keeps B's last canvas while B reads the new query, muted and labelled stale, the same canvas, until B's answer lands", async () => {
    let releaseB: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === DENSE && q === 'q2' ? new Promise((r) => (releaseB = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    const b = await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q2', with: DENSE });
    expect(await screen.findByRole('application', { name: 'Run A, hybrid-rerank-gen, query q2' })).toBeTruthy();
    // Never B's old query passed off as A's: the kept canvas says which query it shows, and that it is stale.
    expect(screen.getByRole('application', { name: 'Run B, dense-only, query q1, stale' })).toBe(b);
    const slot = b.closest('.rg-replay__canvas') as HTMLElement;
    expect(slot.getAttribute('data-stale')).toBe('true');
    expect(within(slot).getByText('Stale: query q1, reading q2…')).toBeTruthy();
    expect(screen.queryByText('Reading dense-only')).toBeNull();
    await act(async () => releaseB(traceOf(DENSE_TRACE, 'q2')));
    expect(await screen.findByRole('application', { name: 'Run B, dense-only, query q2' })).toBe(b);
    expect(slot.getAttribute('data-stale')).toBeNull();
    expect(within(slot).queryByText(/^Stale/)).toBeNull();
  });

  it('mutes a stale canvas and lays its label over it, so neither moves anything', () => {
    expect(declared(css, '.rg-replay__canvas[data-stale] .rg-canvas-frame', 'opacity')).toBeTruthy();
    expect(declared(css, '.rg-replay__stale', 'position')).toBe('absolute');
  });

  it("keeps B's canvas, labelled stale, when B's answer for the new query lands while A still shows the old one", async () => {
    let releaseA: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === HYBRID && q === 'q2' ? new Promise((r) => (releaseA = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    const b = await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q2', with: DENSE });
    expect(await screen.findByRole('application', { name: 'Run B, dense-only, query q1, stale' })).toBe(b);
    expect(screen.getByRole('application', { name: 'Run A, hybrid-rerank-gen, query q1' })).toBeTruthy();
    // B's answer for q2 is in: nothing is being read for B, so its label says only that it is stale.
    await waitFor(() => expect(b.closest('.rg-replay__canvas')?.querySelector('.rg-replay__stale')?.textContent).toBe('Stale: query q1'));
    await act(async () => releaseA(traceOf(HYBRID_TRACE, 'q2')));
    expect(await screen.findByRole('application', { name: 'Run B, dense-only, query q2' })).toBe(b);
    expect(screen.getByRole('application', { name: 'Run A, hybrid-rerank-gen, query q2' })).toBeTruthy();
  });

  it("shows B's loading line, never an old canvas, when B is put away, the query changes, and the same B is chosen again", async () => {
    api({ trace: (run, q) => (run === DENSE && q === 'q2' ? new Promise(() => {}) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q1' });
    await waitFor(() => expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull());
    rerender(view, { query: 'q2' });
    await screen.findByRole('application', { name: 'Run A, hybrid-rerank-gen, query q2' });
    rerender(view, { query: 'q2', with: DENSE });
    expect(await screen.findByText('Reading dense-only')).toBeTruthy();
    expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull();
  });

  it('shows the loading line, never an old canvas, when B goes from one run to another and back', async () => {
    let asked = 0;
    // Dense-only answers its first read; the other run never answers; dense-only's second read is held.
    api({ trace: (run, q) => (run === FAILED ? new Promise(() => {}) : run === DENSE && ++asked > 1 ? new Promise(() => {}) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q1', with: FAILED });
    expect(await screen.findByText('Reading hybrid-broken')).toBeTruthy();
    rerender(view, { query: 'q1', with: DENSE });
    expect(await screen.findByText('Reading dense-only')).toBeTruthy();
    expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull();
  });

  it("never hands the inspector B's kept canvas: a node both runs have reads in A alone while B is stale", async () => {
    api({ trace: (run, q) => (run === DENSE && q === 'q2' ? new Promise(() => {}) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    const b = await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q2', with: DENSE });
    await screen.findByRole('application', { name: 'Run B, dense-only, query q1, stale' });
    fireEvent.click(await drawn(b.parentElement!, 'dense'));
    const inspector = await screen.findByRole('complementary', { name: 'dense' });
    expect(inspector.querySelector('.rg-replay__columns')?.getAttribute('data-columns')).toBe('1');
    expect(within(inspector).queryByRole('region', { name: /^B, / })).toBeNull();
  });

  it("holds B's place, labelled, when B is switched to another run: another run's canvas is never kept", async () => {
    let releaseB: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === FAILED ? new Promise((r) => (releaseB = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    await screen.findByRole('application', { name: 'Run B, dense-only, query q1' });
    rerender(view, { query: 'q1', with: FAILED });
    expect(await screen.findByText('Reading hybrid-broken')).toBeTruthy();
    expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull();
    await act(async () => releaseB(traceOf(FAILED_TRACE, 'q1')));
    expect(await screen.findByRole('application', { name: 'Run B, hybrid-broken, query q1' })).toBeTruthy();
  });

  it("says in B's place, with Retry, that B's trace failed — and draws no B", async () => {
    api({ trace: (run, q) => (run === DENSE ? { problem: problem('query_not_found', 404, 'Run B holds no query q1.') } : traceOf(HYBRID_TRACE, q)) });
    show({ query: 'q1', with: DENSE });
    expect(await screen.findByText('Run B holds no query q1.')).toBeTruthy();
    expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull();
    expect(within(document.querySelector('.rg-replay__canvases') as HTMLElement).getByRole('button', { name: 'Retry' })).toBeTruthy();
  });

  it.each([
    ['detail', { detail: (run: string) => (run === DENSE ? { problem: problem('run_unreadable', 500, 'Run B does not load.') } : undefined) }],
    ['queries', { queries: (run: string) => (run === DENSE ? { problem: problem('dataset_differs', 409, 'Run B’s dataset differs.') } : undefined) }],
  ] as const)("says in B's place that B's %s failed", async (_what, options) => {
    api(options);
    show({ query: 'q1', with: DENSE });
    expect(await screen.findByText(/^Run B/, { selector: '.rg-inline b' })).toBeTruthy();
    expect(screen.queryByRole('application', { name: /^Run B/ })).toBeNull();
  });

  it("leaves the verdict out while B's queries are read, rather than saying B ranks none", async () => {
    api({ queries: (run) => (run === DENSE ? new Promise(() => {}) : undefined) });
    show({ query: 'q1', with: DENSE });
    const a = await screen.findByRole('application', { name: /^Run A/ });
    await screen.findByRole('application', { name: /^Run B/ });
    fireEvent.click(await drawn(a.parentElement!, 'answer'));
    await screen.findByRole('region', { name: 'B, dense-only' });
    expect(screen.queryByRole('heading', { name: 'Verdict' })).toBeNull();
    expect(screen.queryByText(/ranks none/)).toBeNull();
  });
});

describe('side by side, while B is held', () => {
  it("leaves the verdict out while B's trace is read, rather than giving A's one-run verdict", async () => {
    api({ trace: (run, q) => (run === DENSE ? new Promise(() => {}) : traceOf(HYBRID_TRACE, q)) });
    show({ query: 'q1', with: DENSE });
    const a = await screen.findByRole('application', { name: /^Run A/ });
    fireEvent.click(await drawn(a.parentElement!, 'answer'));
    expect(await screen.findByRole('complementary', { name: 'answer' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Verdict' })).toBeNull();
    expect(screen.queryByText(/^ndcg@10 is/)).toBeNull();
  });

  it("leaves the verdict out while B's trace for a new query is read, and gives both runs' once it lands", async () => {
    let releaseB: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === DENSE && q === 'q2' ? new Promise((r) => (releaseB = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = show({ query: 'q1', with: DENSE });
    const a = await screen.findByRole('application', { name: /^Run A/ });
    await screen.findByRole('application', { name: /^Run B/ });
    fireEvent.click(await drawn(a.parentElement!, 'answer'));
    expect(await screen.findByRole('heading', { name: 'Verdict' })).toBeTruthy();
    rerender(view, { query: 'q2', with: DENSE });
    await screen.findByRole('application', { name: 'Run A, hybrid-rerank-gen, query q2' });
    expect(screen.queryByRole('heading', { name: 'Verdict' })).toBeNull();
    await act(async () => releaseB(traceOf(DENSE_TRACE, 'q2')));
    expect(await screen.findByRole('heading', { name: 'Verdict' })).toBeTruthy();
  });

  it("says a node only B has is not in A while B reads the new query, rather than calling it not run", async () => {
    let releaseB: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === HYBRID && q === 'q2' ? new Promise((r) => (releaseB = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)) });
    const view = render(<ReplayScreen client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<ReplayScreen client={createApiClient()} run={DENSE} query="q2" with={HYBRID} />);
    await screen.findByRole('application', { name: 'Run A, dense-only, query q2' });
    expect(screen.getByText('No such node in A.')).toBeTruthy();
    expect(screen.queryByText(/Not run/)).toBeNull();
    await act(async () => releaseB(traceOf(HYBRID_TRACE, 'q2')));
    expect(await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' })).toBeTruthy();
  });

  it('clears a node only B has when B is switched to another run — even one that has it — rather than calling it not run in A', async () => {
    // The third run on the benchmark has the reranker too: B has no canvas while it is read, so the node goes.
    api();
    const view = render(<ReplayScreen client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<ReplayScreen client={createApiClient()} run={DENSE} query="q1" with={FAILED} />);
    await screen.findByRole('application', { name: /^Run B, hybrid-broken/ });
    expect(await screen.findByText('Select a node to see what it produced for this query.')).toBeTruthy();
    // The new B's own cards may say "not run"; the inspector's place says nothing of the node.
    const panel = document.querySelector('.rg-replay__panel') as HTMLElement;
    expect(within(panel).queryByText(/Not run/)).toBeNull();
    expect(within(panel).queryByText('No such node in A.')).toBeNull();
  });
});

describe('only the last query asked lands', () => {
  it('keeps q3 on screen when the answer for q2, asked before it, arrives after', async () => {
    let releaseQ2: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (_run, q) => (q === 'q2' ? new Promise((r) => (releaseQ2 = r)) : traceOf(HYBRID_TRACE, q)) });
    const view = show({ query: 'q1' });
    await screen.findByRole('application', { name: /query q1$/ });
    rerender(view, { query: 'q2' });
    rerender(view, { query: 'q3' });
    expect(await screen.findByRole('application', { name: /query q3$/ })).toBeTruthy();
    await act(async () => releaseQ2(traceOf(HYBRID_TRACE, 'q2')));
    expect(screen.getByRole('application', { name: /query q3$/ })).toBeTruthy();
    expect(screen.queryByRole('application', { name: /query q2$/ })).toBeNull();
  });

  it("cancels q2's request when q3 is chosen, and shows no error for it", async () => {
    const mock = api({ trace: (_run, q) => (q === 'q2' ? new Promise(() => {}) : traceOf(HYBRID_TRACE, q)) });
    const view = show({ query: 'q1' });
    await screen.findByRole('application', { name: /query q1$/ });
    rerender(view, { query: 'q2' });
    await screen.findByText('Reading query q2…');
    const q2 = mock.requests.findIndex((r) => r.endsWith(`/trace/q2`));
    expect(mock.signals[q2]?.aborted).toBe(false);
    rerender(view, { query: 'q3' });
    expect(await screen.findByRole('application', { name: /query q3$/ })).toBeTruthy();
    expect(mock.signals[q2]?.aborted).toBe(true);
    expect(screen.queryByText('request_aborted')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(screen.getByRole('status').textContent).toBe('');
  });
});

describe('what is busy', () => {
  it('marks only the stage busy while a query is read, so the status line and the list still announce', async () => {
    api({ trace: (_run, q) => (q === 'q1' ? traceOf(HYBRID_TRACE, q) : new Promise(() => {})) });
    const view = show({ query: 'q1' });
    await screen.findByRole('application', { name: /query q1$/ });
    rerender(view, { query: 'q2' });
    await screen.findByText('Reading query q2…');
    expect(document.querySelector('.rg-replay')?.getAttribute('aria-busy')).toBeNull();
    expect(document.querySelector('.rg-replay__stage')?.getAttribute('aria-busy')).toBe('true');
  });
});

describe('side by side, the rest', () => {
  it("gives B its own passages banner, named, when B's passages are not verified", async () => {
    api({ traces: { ...TRACES, [DENSE]: withPassages(DENSE_TRACE, 'dataset_absent') } });
    show({ query: 'q1', with: DENSE });
    expect(await screen.findByText('Run B: passage text is hidden: the dataset of this run is not on disk.')).toBeTruthy();
    expect(screen.queryByText(/^Run A: passage text is hidden/)).toBeNull();
  });

  it('clears a node selected in B when B is put away, rather than calling it not run in A', async () => {
    api();
    const view = render(<ReplayScreen client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<ReplayScreen client={createApiClient()} run={DENSE} query="q1" with={undefined} />);
    expect(await screen.findByText('Select a node to see what it produced for this query.')).toBeTruthy();
    expect(screen.queryByText(/Not run/)).toBeNull();
  });

  it('says side by side is unavailable because the runs could not be listed, not because none shares the benchmark', async () => {
    mockApi({
      'GET /runs': { problem: problem('backend_failed', 500, 'The store did not answer.') },
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
    });
    show({ query: 'q1' });
    const side = await screen.findByRole('radio', { name: 'Side by side' });
    await waitFor(() => expect(side.getAttribute('title')).toBe('The runs could not be listed: The store did not answer.'));
  });
});

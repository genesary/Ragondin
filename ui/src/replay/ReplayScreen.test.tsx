/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockReply } from '../api/testing.ts';
import type { PartialQueries, Problem, QueryTrace, RunDetail, RunListing, RunQueries } from '../api/types.ts';
import { formatHash, parseHash, useRoute } from '../routes.ts';
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
  JOB,
  LISTING,
  PARTIAL_QUERIES,
  partialTrace,
  withPassages,
} from './fixtures.ts';
import { declared } from '../../design/testing/css.ts';
import css from './Replay.css?raw';
import { ReplayScreen, type ReplayScreenProps } from './ReplayScreen.tsx';

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
  listing = LISTING,
}: { listing?: RunListing; traces?: Record<string, QueryTrace>; trace?: (run: string, query: string) => MockReply<QueryTrace> | Promise<MockReply<QueryTrace>>; queries?: Override<RunQueries>; detail?: Override<RunDetail> } = {}) {
  return mockApi({
    'GET /runs': { body: listing },
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

/**
 * The screen as the shell renders it for the node selected: read from the
 * address, which the screen writes, so a selection made on the canvas comes
 * back to it as the shell would pass it. The rest is the props given.
 */
function Routed(props: ReplayScreenProps extends infer P ? (P extends unknown ? Omit<P, 'node'> : never) : never) {
  const route = useRoute();
  const node = route?.screen === 'replay' && 'node' in route ? route.node : undefined;
  return <ReplayScreen {...props} node={node} />;
}

const show = (props: { run?: string; query?: string; node?: string; with?: string } = {}) => {
  const run = props.run ?? HYBRID;
  if (props.query !== undefined) window.history.replaceState(null, '', `/${formatHash({ screen: 'replay', run, query: props.query, ...(props.node === undefined ? {} : { node: props.node }), ...(props.with === undefined ? {} : { with: props.with }) })}`);
  return render(
    <div style={{ width: 1400 }}>
      <Routed client={createApiClient()} run={run} query={props.query} with={props.with} />
    </div>,
  );
};

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

  it('labels a prefix run among the runs offered beside it', async () => {
    const prefixed = LISTING.runs.map((r) => (r.id === FAILED ? { ...r, prefix_of_documents: [{ pipeline: 'hybrid-rerank-gen', up_to: 'rrf' }] } : r));
    api({ listing: { ...LISTING, runs: prefixed } });
    show({ query: 'q1', with: DENSE });
    const beside = (await screen.findByLabelText('Beside')) as HTMLSelectElement;
    const label = (id: string) => [...beside.options].find((o) => o.value === id)?.textContent;
    expect(label(FAILED)).toContain('prefix of hybrid-rerank-gen, up to rrf');
    expect(label(DENSE)).not.toContain('prefix');
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

describe('the node selected, in the address', () => {
  it('writes a node selected on the canvas into the address, in place', async () => {
    api();
    window.history.replaceState(null, '', `/#replay/${HYBRID}/q/q1`);
    const entries = window.history.length;
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    fireEvent.click(await drawn(graph.parentElement!, 'rerank'));
    expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q1', node: 'rerank' });
    expect(window.history.length).toBe(entries);
  });

  it('restores the node the address names when it is reopened', async () => {
    api();
    show({ query: 'q1', node: 'rerank' });
    expect(await screen.findByRole('list', { name: 'Ranked by rerank, 4 chunks' })).toBeTruthy();
  });

  it('keeps the node selected when the query changes', async () => {
    api();
    window.history.replaceState(null, '', `/#replay/${HYBRID}/q/q1/node/rerank`);
    show({ query: 'q1', node: 'rerank' });
    const list = await screen.findByRole('listbox', { name: 'Queries' });
    fireEvent.keyDown(list, { key: 'ArrowDown' });
    expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q2', node: 'rerank' });
  });

  it('takes the node out of the address when the inspector is closed', async () => {
    api();
    window.history.replaceState(null, '', `/#replay/${HYBRID}/q/q1/node/rerank`);
    show({ query: 'q1', node: 'rerank' });
    await screen.findByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    fireEvent.click(screen.getByRole('button', { name: 'Close inspector' }));
    expect(route()).toEqual({ screen: 'replay', run: HYBRID, query: 'q1' });
  });
});

describe('Open in the editor', () => {
  const editorLink = () => screen.findByRole('link', { name: 'Open in the editor' });
  const listingWith = (changes: Partial<RunListing['runs'][number]>): RunListing => ({ ...LISTING, runs: LISTING.runs.map((r) => (r.id === HYBRID ? { ...r, ...changes } : r)) });

  it('opens the stored document holding what the run ran, on the node selected in Replay', async () => {
    api();
    show({ query: 'q1', node: 'rerank' });
    expect((await editorLink()).getAttribute('href')).toBe('#editor/hybrid-rerank-gen/node/rerank');
  });

  it('opens it with no node when none is selected', async () => {
    api();
    show({ query: 'q1' });
    expect((await editorLink()).getAttribute('href')).toBe('#editor/hybrid-rerank-gen');
  });

  it('opens the name the run was launched as when that document still holds what it ran', async () => {
    api({ listing: listingWith({ pipeline_names: ['a-copy', 'hybrid'], launched_as: { name: 'hybrid', held: 'exactly', prefix_of: null } }) });
    show({ query: 'q1' });
    expect((await editorLink()).getAttribute('href')).toBe('#editor/hybrid');
  });

  it('never opens a name the API refuses to read', async () => {
    api({ listing: listingWith({ pipeline_names: ['Hybrid', 'hybrid', 'z-copy'], refused_pipeline_names: ['Hybrid', 'hybrid'] }) });
    show({ query: 'q1' });
    expect((await editorLink()).getAttribute('href')).toBe('#editor/z-copy');
  });

  it('is refused, saying why and offering the fork, when no stored document holds what the run ran', async () => {
    api({ listing: listingWith({ pipeline_names: [] }) });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.getAttribute('aria-describedby')).toBeTruthy();
    expect(document.getElementById(button.getAttribute('aria-describedby')!)?.textContent).toBe('No pipeline document in the workspace holds what this run ran. Fork this run to edit it.');
    expect(screen.queryByRole('link', { name: 'Open in the editor' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Fork this run' })).toBeTruthy();
  });

  it('names the document the run was launched as when it has changed since', async () => {
    api({ listing: listingWith({ pipeline_names: [], launched_as: { name: 'hybrid', held: 'exactly', prefix_of: null } }) });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    expect(document.getElementById(button.getAttribute('aria-describedby')!)?.textContent).toBe('hybrid has changed since this run, and no pipeline document holds what it ran. Fork this run to edit it.');
  });

  it('names the documents the API refuses to read when they are the only ones', async () => {
    api({ listing: listingWith({ pipeline_names: ['Hybrid', 'hybrid'], refused_pipeline_names: ['Hybrid', 'hybrid'] }) });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    expect(document.getElementById(button.getAttribute('aria-describedby')!)?.textContent).toBe('Hybrid and hybrid hold what this run ran, but each differs from another stored name only in case, so neither can be read. Fork this run to edit it.');
  });

  it('is refused while the runs are being read, saying so', async () => {
    mockApi({
      'GET /runs': () => new Promise(() => {}),
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
    });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(button.getAttribute('aria-describedby')!)?.textContent).toBe('Reading the runs…');
  });

  it('keeps focus on the control when the runs land and it becomes a link', async () => {
    let release: (reply: MockReply<RunListing>) => void = () => {};
    mockApi({
      'GET /runs': () => new Promise((r) => (release = r)),
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
    });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    button.focus();
    expect(document.activeElement).toBe(button);
    await act(async () => release({ body: LISTING }));
    const link = await editorLink();
    expect(document.activeElement).toBe(link);
  });

  it('takes no focus when the runs land after focus moved elsewhere', async () => {
    let release: (reply: MockReply<RunListing>) => void = () => {};
    mockApi({
      'GET /runs': () => new Promise((r) => (release = r)),
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
    });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    button.focus();
    const list = screen.getByRole('listbox', { name: 'Queries' });
    act(() => list.focus());
    await act(async () => new Promise((r) => setTimeout(r, 10)));
    await act(async () => release({ body: LISTING }));
    await editorLink();
    expect(document.activeElement).toBe(list);
  });

  it('opens the fork on the node selected in Replay', async () => {
    mockApi({
      'GET /runs': { body: LISTING },
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
      'GET /pipelines': { body: { pipelines: [] } },
      'PUT /pipelines/{name}': { body: { name: 'run-x-fork', etag: 'f'.repeat(64), hash: HYBRID } },
      'GET /runs/{id}/layout': { body: { layout: null } },
    });
    show({ query: 'q1', node: 'rerank' });
    await screen.findByRole('list', { name: 'Ranked by rerank, 4 chunks' });
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    await waitFor(() => expect(window.location.hash).toBe('#editor/run-x-fork/node/rerank'));
  });

  it('is refused while the runs are being read, and says the listing failed when it did', async () => {
    mockApi({
      'GET /runs': { problem: problem('backend_failed', 500, 'the store is down') },
      'GET /runs/{id}': { body: HYBRID_DETAIL },
      'GET /runs/{id}/queries': { body: HYBRID_QUERIES },
      'GET /runs/{id}/trace/{query}': { body: HYBRID_TRACE },
    });
    show({ query: 'q1' });
    const button = await screen.findByRole('button', { name: 'Open in the editor' });
    await waitFor(() => expect(document.getElementById(button.getAttribute('aria-describedby')!)?.textContent).toBe('The runs could not be listed: the store is down'));
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
        <Routed client={createApiClient()} run={HYBRID} query="q2" with={undefined} />
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

describe('Replay over a job’s partial traces', () => {
  /** The API over the job: its partial traces, and a refusal for any other job. */
  const jobApi = (queries: MockReply<PartialQueries> = { body: PARTIAL_QUERIES }) =>
    mockApi({
      'GET /jobs/{id}/queries': (_q, path) => (segment(path, 4) === JOB ? queries : { problem: problem('job_not_found', 404, 'no such job') }),
      'GET /jobs/{id}/trace/{query}': (_q, path) => ({ body: partialTrace(segment(path, 6)) }),
    });
  const showJob = (query?: string) =>
    render(
      <div style={{ width: 1400 }}>
        <Routed client={createApiClient()} job={JOB} query={query} />
      </div>,
    );

  it("writes the node selected on a job's query in place, never as a new entry", async () => {
    jobApi();
    window.history.replaceState(null, '', `/${formatHash({ screen: 'replay', job: JOB, query: 'q1' })}`);
    const entries = window.history.length;
    showJob('q1');
    const graph = await screen.findByRole('application', { name: /partial traces/ });
    fireEvent.click(await drawn(graph.parentElement!, 'rerank'));
    expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q1', node: 'rerank' });
    expect(window.history.length).toBe(entries);
  });

  it("clears a node the job's graph does not have", async () => {
    jobApi();
    window.history.replaceState(null, '', `/${formatHash({ screen: 'replay', job: JOB, query: 'q1', node: 'nowhere' })}`);
    showJob('q1');
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q1' }));
  });

  it("restores the node the address names on a job's query", async () => {
    jobApi();
    window.history.replaceState(null, '', `/${formatHash({ screen: 'replay', job: JOB, query: 'q1', node: 'rerank' })}`);
    showJob('q1');
    await waitFor(() => expect(document.querySelector('.rg-replay__panel')?.textContent).toContain('349'));
  });

  it('replays_a_failed_job_s_partial_traces_query_by_query_labelled_partial', async () => {
    const calls = jobApi();
    showJob('q1');
    const graph = await screen.findByRole('application', { name: `Job ${JOB}, partial traces, query q1` });
    await drawn(graph.parentElement!, 'rerank');
    // The nodes of a kept trace, read as a run's are: durations on the canvas, nothing scored.
    expect(within(cardOf(graph.parentElement!, 'rerank')).getByText('349 ms')).toBeTruthy();
    // Labelled as what it is: a job's partial traces, never a stored run.
    const banner = screen.getByText(`Partial traces of job ${JOB}`).closest('.rg-inline') as HTMLElement;
    expect(banner.textContent).toContain('failed at rerank: the reranker answered 503');
    expect(banner.textContent).toContain('These are the traces of the 2 queries it executed before it stopped. No run was stored');
    // The failing query's trace is among them: nothing claims it left none.
    expect(banner.textContent).not.toContain('left no trace');
    expect(within(banner).getByRole('link', { name: 'Open the job' }).getAttribute('href')).toBe(`#runs/job/${JOB}`);
    expect(calls.requests.filter((r) => r.startsWith('GET /api/v1/runs'))).toEqual([]);
    // Its source named in words where a run's swatch stands.
    expect(document.querySelector('.rg-replay__source')?.textContent).toBe(`Partial traces ${JOB}`);
    // One job, alone: nothing stands beside it, and nothing is scored to filter on — each refusal with its reason.
    expect(screen.getByRole('radio', { name: 'Alone' }).getAttribute('aria-checked')).toBe('true');
    expect(screen.queryByRole('radio', { name: 'One run' })).toBeNull();
    const side = screen.getByRole('radio', { name: 'Side by side' }) as HTMLButtonElement;
    expect(side.disabled).toBe(true);
    expect(side.getAttribute('title')).toBe('A job’s partial traces are replayed alone');
    expect((screen.getByRole('button', { name: /No gold in the top 10/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('Partial traces are not scored')).toBeTruthy();
    // The inspector reads a kept trace as a run's: what the node produced, by id, with no text and no metric.
    fireEvent.click(await drawn(graph.parentElement!, 'rerank'));
    const panel = document.querySelector('.rg-replay__panel') as HTMLElement;
    await waitFor(() => expect(within(panel).getByText('rerank')).toBeTruthy());
    expect(panel.textContent).toContain('349');
    expect(panel.querySelector('[data-text="none"]')).not.toBeNull();
    expect(panel.textContent).not.toMatch(/judged quer/);
    // The node selected is written into the address, in place.
    expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q1', node: 'rerank' });
    // Query by query, the address following, the node kept.
    const list = screen.getByRole('listbox', { name: 'Queries' });
    expect(within(list).getAllByRole('option').map((o) => o.id.split('-').at(-1))).toHaveLength(2);
    fireEvent.keyDown(list, { key: 'ArrowDown' });
    expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q2', node: 'rerank' });
  });

  it('opens a failed job on the query it failed on when none is chosen', async () => {
    jobApi();
    showJob();
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q2' }));
  });

  it('opens on the first kept query when no trace failed', async () => {
    jobApi({ body: { ...PARTIAL_QUERIES, failed_query: null } });
    showJob();
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', job: JOB, query: 'q1' }));
  });

  it('says a cancelled job was cancelled, and claims nothing of the query it stopped on', async () => {
    const cancelled: PartialQueries = { ...PARTIAL_QUERIES, failed_query: null, job: { ...PARTIAL_QUERIES.job, state: { kind: 'cancelled', finished_at_ms: 1_700_000_100_000, partial_traces: 2 } } };
    jobApi({ body: cancelled });
    showJob('q1');
    const title = await screen.findByText(`Partial traces of job ${JOB}`);
    expect((title.closest('.rg-inline') as HTMLElement).textContent).toContain(
      `Job ${JOB} was cancelled. These are the traces of the 2 queries it executed before it stopped. No run was stored, so nothing is scored and no passage text is shown`,
    );
  });

  it('says why a job has no partial traces, with the way on', async () => {
    jobApi({ problem: problem('no_partial_traces', 404, `job ${JOB} kept no partial traces: it was interrupted by a crash, which keeps no trace it can vouch for`) });
    showJob('q1');
    expect(await screen.findByText(/interrupted by a crash, which keeps no trace it can vouch for/)).toBeTruthy();
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
      <Routed client={createApiClient()} run={HYBRID} query={props.query} with={props.with} />
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
    const view = render(<Routed client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<Routed client={createApiClient()} run={DENSE} query="q2" with={HYBRID} />);
    await screen.findByRole('application', { name: 'Run A, dense-only, query q2' });
    expect(screen.getByText('No such node in A.')).toBeTruthy();
    expect(screen.queryByText(/Not run/)).toBeNull();
    await act(async () => releaseB(traceOf(HYBRID_TRACE, 'q2')));
    expect(await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' })).toBeTruthy();
  });

  it('opens the editor on A’s document without a node only B has: it is no node of the pipeline A ran', async () => {
    api();
    render(<Routed client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    expect(route()).toMatchObject({ node: 'rerank' });
    expect(screen.getByRole('link', { name: 'Open in the editor' }).getAttribute('href')).toBe('#editor/dense-only');
  });

  it('keeps a node only B has when B is switched to another run that has it, once that run is read', async () => {
    api();
    const view = render(<Routed client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<Routed client={createApiClient()} run={DENSE} query="q1" with={FAILED} />);
    await screen.findByRole('application', { name: /^Run B, hybrid-broken/ });
    expect(await screen.findByRole('region', { name: 'B, hybrid-broken' })).toBeTruthy();
    expect(route()).toMatchObject({ node: 'rerank' });
    // A lacks it, and says so in its column; it is never called not run there.
    const panel = document.querySelector('.rg-replay__panel') as HTMLElement;
    expect(within(within(panel).getByRole('region', { name: 'A, dense-only' })).queryByText(/Not run/)).toBeNull();
  });

  it('clears a node only B has when B is switched to another run that lacks it, once that run is read', async () => {
    // A second dense-only run on the benchmark: it has no reranker.
    const OTHER = '7'.repeat(64);
    const listing: RunListing = { ...LISTING, runs: [...LISTING.runs, { ...LISTING.runs[1]!, id: OTHER, pipeline: OTHER, pipeline_names: ['dense-two'] }] };
    let releaseOther: (reply: MockReply<QueryTrace>) => void = () => {};
    api({
      listing,
      detail: (run) => (run === OTHER ? { body: { ...DENSE_DETAIL, id: OTHER } } : undefined),
      queries: (run) => (run === OTHER ? { body: { ...DENSE_QUERIES, run: OTHER } } : undefined),
      trace: (run, q) => (run === OTHER ? new Promise((r) => (releaseOther = r)) : traceOf(run === DENSE ? DENSE_TRACE : HYBRID_TRACE, q)),
    });
    const view = render(<Routed client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<Routed client={createApiClient()} run={DENSE} query="q1" with={OTHER} />);
    await screen.findByText(/Reading query q1 in B/);
    // While the new B is read, the node waits for it.
    expect(route()).toMatchObject({ node: 'rerank' });
    await act(async () => releaseOther({ body: { ...DENSE_TRACE, run: OTHER, query: 'q1' } }));
    await screen.findByRole('application', { name: /^Run B, dense-two/ });
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', run: DENSE, query: 'q1', with: OTHER }));
    expect(await screen.findByText('Select a node to see what it produced for this query.')).toBeTruthy();
  });

  it('keeps a node only B has, named in a reopened address, while B is first read, and shows it once B lands', async () => {
    let releaseB: (reply: MockReply<QueryTrace>) => void = () => {};
    api({ trace: (run, q) => (run === HYBRID ? new Promise((r) => (releaseB = r)) : traceOf(DENSE_TRACE, q)) });
    show({ run: DENSE, query: 'q1', node: 'rerank', with: HYBRID });
    await screen.findByRole('application', { name: 'Run A, dense-only, query q1' });
    await screen.findByText(/Reading query q1 in B/);
    expect(route()).toMatchObject({ node: 'rerank' });
    await act(async () => releaseB(traceOf(HYBRID_TRACE, 'q1')));
    expect(await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' })).toBeTruthy();
    expect(route()).toMatchObject({ node: 'rerank' });
  });

  it("clears a node only B has, named in a reopened address, when B's read fails", async () => {
    api({ trace: (run, q) => (run === HYBRID ? { problem: problem('query_not_found', 404, 'Run B holds no query q1.') } : traceOf(DENSE_TRACE, q)) });
    show({ run: DENSE, query: 'q1', node: 'rerank', with: HYBRID });
    expect(await screen.findByText('Run B holds no query q1.')).toBeTruthy();
    await waitFor(() => expect(route()).toEqual({ screen: 'replay', run: DENSE, query: 'q1', with: HYBRID }));
  });

  it('reads a node both runs have, selected on B, as B’s: its verdict when it is B’s final node', async () => {
    api();
    show({ query: 'q1', with: DENSE });
    const b = await screen.findByRole('application', { name: /^Run B, dense-only/ });
    fireEvent.click(await drawn(b.parentElement!, 'dense'));
    await screen.findByRole('region', { name: 'B, dense-only' });
    // `dense` ends B's pipeline, not A's: only read as B's does it carry the verdict.
    expect(await screen.findByRole('heading', { name: 'Verdict' })).toBeTruthy();
  });

  it('clears a node both runs have, selected on B, when B is put away', async () => {
    api();
    const view = show({ query: 'q1', with: DENSE });
    const b = await screen.findByRole('application', { name: /^Run B, dense-only/ });
    fireEvent.click(await drawn(b.parentElement!, 'dense'));
    await screen.findByRole('region', { name: 'B, dense-only' });
    expect(route()).toMatchObject({ node: 'dense' });
    view.rerender(
      <div style={{ width: 1400 }}>
        <Routed client={createApiClient()} run={HYBRID} query="q1" with={undefined} />
      </div>,
    );
    await waitFor(() => expect(route()).not.toHaveProperty('node'));
    expect(await screen.findByText('Select a node to see what it produced for this query.')).toBeTruthy();
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
    const view = render(<Routed client={createApiClient()} run={DENSE} query="q1" with={HYBRID} />);
    const b = await screen.findByRole('application', { name: /^Run B, hybrid-rerank-gen/ });
    fireEvent.click(await drawn(b.parentElement!, 'rerank'));
    await screen.findByRole('region', { name: 'B, hybrid-rerank-gen' });
    view.rerender(<Routed client={createApiClient()} run={DENSE} query="q1" with={undefined} />);
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

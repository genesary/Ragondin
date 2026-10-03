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
import { ReplayScreen } from './ReplayScreen.tsx';

const problem = (code: Problem['code'], status: number, detail: string): Problem => ({ type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint: 'Pick another.' });
const segment = (path: string, i: number) => decodeURIComponent(path.split('/')[i] ?? '');

const DETAILS: Record<string, RunDetail> = { [HYBRID]: HYBRID_DETAIL, [DENSE]: DENSE_DETAIL, [FAILED]: FAILED_DETAIL };
const QUERIES: Record<string, RunQueries> = { [HYBRID]: HYBRID_QUERIES, [DENSE]: DENSE_QUERIES, [FAILED]: FAILED_QUERIES };
const TRACES: Record<string, QueryTrace> = { [HYBRID]: HYBRID_TRACE, [DENSE]: DENSE_TRACE, [FAILED]: FAILED_TRACE };

/** The API over the fixtures: each run's detail, queries and q1's trace; another query's trace is q1's renamed. */
function api({ traces = TRACES, trace }: { traces?: Record<string, QueryTrace>; trace?: (run: string, query: string) => MockReply<QueryTrace> | Promise<MockReply<QueryTrace>> } = {}) {
  return mockApi({
    'GET /runs': { body: LISTING },
    'GET /runs/{id}': (_q, path) => (DETAILS[segment(path, 4)] === undefined ? { problem: problem('run_not_found', 404, 'no such run') } : { body: DETAILS[segment(path, 4)]! }),
    'GET /runs/{id}/queries': (query, path) => {
      const run = segment(path, 4);
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

const canvas = (name: RegExp | string) => screen.getByRole('application', { name });
const nodeOf = (root: HTMLElement, id: string) => root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement;
const cardOf = (root: HTMLElement, id: string) => nodeOf(root, id).querySelector('.rg-node') as HTMLElement;
const route = () => parseHash(window.location.hash);

beforeEach(() => window.history.replaceState(null, '', '/'));
afterEach(() => vi.unstubAllGlobals());

describe('Replay, one run', () => {
  it('shows every node’s duration and share, its metric, and a rank strip filled at the gold ranks on the ranking nodes', async () => {
    api();
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
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
    const ids = nodeOf(graph.parentElement!, 'rerank').getAttribute('aria-describedby')!.split(' ');
    expect(document.getElementById(ids[0]!)?.textContent).toBe("ndcg@10 0.8610. 2 gold passages in the top 10, at rank 1, 2. 4 discarded. 349 ms, 35% of this query's time.");
  });

  it('lists the reranker’s kept chunks in rank order with gold stars, and its discarded ones struck through with their former rank, when it is selected', async () => {
    api();
    show({ query: 'q1' });
    const graph = await screen.findByRole('application', { name: /hybrid-rerank-gen/ });
    fireEvent.click(nodeOf(graph.parentElement!, 'rerank'));
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
    const graph = canvas(/hybrid-rerank-gen/);
    fireEvent.click(nodeOf(graph.parentElement!, 'rerank'));
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
    fireEvent.click(nodeOf(a.parentElement!, 'rerank'));
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

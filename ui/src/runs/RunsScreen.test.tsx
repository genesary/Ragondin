/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createApiClient, type ApiClient, type ApiResult } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Graph, Problem, RunListing, RunSummary } from '../api/types.ts';
import { navigate, useRoute } from '../routes.ts';
import { RunsScreen } from './RunsScreen.tsx';

const hex = (c: string) => c.repeat(64);
const SCIFACT = hex('5');
const FIQA = hex('f');
const HYBRID = hex('a');
const DENSE = hex('c');

const summary = (id: string, pipeline: string, dataset: string, metrics: Record<string, number> = {}, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline,
  pipeline_names: [],
  dataset_version: dataset,
  benchmark_names: [],
  index_version: hex('0'),
  engine_version: '0.0.0',
  started_at_ms: null,
  finished_at_ms: null,
  metrics,
  ...over,
});

const R1 = hex('1');
const R2 = hex('2');
const R3 = hex('3');
const R4 = hex('4');
const R5 = hex('6');

const node = (id: string, family: string) => ({ id, family, implementation: id, parameters: {} });

const HYBRID_GRAPH: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [node('bm25', 'retriever'), node('dense', 'retriever'), node('rerank', 'reranker'), node('rrf', 'fusion')],
  edges: [
    { from: 'question', to: 'bm25', port: 0, kind: 'query' },
    { from: 'question', to: 'dense', port: 0, kind: 'query' },
    { from: 'question', to: 'rerank', port: 0, kind: 'query' },
    { from: 'rrf', to: 'rerank', port: 1, kind: 'chunks' },
    { from: 'bm25', to: 'rrf', port: 0, kind: 'chunks' },
    { from: 'dense', to: 'rrf', port: 1, kind: 'chunks' },
  ],
};

const DENSE_GRAPH: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [node('dense', 'retriever')],
  edges: [{ from: 'question', to: 'dense', port: 0, kind: 'query' }],
};

/** Two pipelines over two benchmarks, and one run the store cannot read. */
const LISTING: RunListing = {
  runs: [
    summary(R1, HYBRID, SCIFACT, { 'ndcg@10': 0.6483, mrr: 0.5912 }),
    summary(R2, HYBRID, FIQA, { 'ndcg@10': 0.3301 }),
    summary(R3, DENSE, SCIFACT, { 'ndcg@10': 0.6012, exact_match: 0.412 }),
    summary(R4, HYBRID, SCIFACT, { 'ndcg@10': 0.6611 }),
  ],
  unreadable: [{ id: hex('9'), reason: 'metrics.json is not valid JSON' }],
  shapes: { [HYBRID]: HYBRID_GRAPH, [DENSE]: DENSE_GRAPH },
};
/** The same store after R5 was written to it. */
const LATER: RunListing = { ...LISTING, runs: [...LISTING.runs, summary(R5, DENSE, SCIFACT)] };

const routes = (listing: MockRoutes['GET /runs'] = { body: LISTING }): MockRoutes => ({
  'GET /runs': listing,
});

/** What the shell does: hands the screen the selection the address carries. */
function Shell({ client }: { client: ApiClient }) {
  const route = useRoute();
  return <RunsScreen client={client} sel={route?.screen === 'runs' ? (route.sel ?? []) : []} />;
}

function show(hash: string, mocks: MockRoutes = routes()) {
  window.history.replaceState(null, '', `/${hash}`);
  const api = mockApi(mocks);
  render(<Shell client={createApiClient()} />);
  return api;
}

const short = (id: string) => id.slice(0, 12);
const rowOf = (id: string) => screen.getByRole('row', { name: new RegExp(`^Run ${short(id)} on `) });
const box = (id: string) => within(rowOf(id)).getByRole('checkbox') as HTMLInputElement;
const reasonOf = (input: HTMLElement) => document.getElementById(input.getAttribute('aria-describedby') ?? '')?.textContent;
const compare = () => screen.getByRole('button', { name: /^Compare/ });
const loaded = () => screen.findByRole('row', { name: new RegExp(`^Run ${short(R1)} on `) });

beforeEach(() => {
  window.history.replaceState(null, '', '/');
});

afterEach(() => {
  window.history.replaceState(null, '', '/');
});

describe('the states', () => {
  it('says so while the runs are being read', () => {
    show('#runs');
    expect(screen.getByRole('status').textContent).toBe('Reading runs');
  });

  it('shows the error inline with Retry, and Retry reads the runs again', async () => {
    show('#runs', routes([{ network: 'down' }, { body: LISTING }]));
    const retry = await screen.findByRole('button', { name: 'Retry' });
    expect(screen.getByRole('alert').textContent).toContain('GET /api/v1/runs');
    fireEvent.click(retry);
    await loaded();
  });

  it('says there are no runs yet in one sentence, and leads to the Editor', async () => {
    show('#runs', routes({ body: { runs: [], unreadable: [], shapes: {} } }));
    expect((await screen.findByRole('heading', { name: 'No runs yet' })).tagName).toBe('H3');
    expect(screen.getByRole('link', { name: 'Open Editor' }).getAttribute('href')).toBe('#editor');
  });
});

describe('over a listing with two benchmarks', () => {
  it('groups the runs by pipeline, each group with its shape and its count', async () => {
    show('#runs');
    const hybrid = await screen.findByRole('link', { name: `pipeline ${short(HYBRID)}` });
    expect(hybrid.getAttribute('href')).toBe(`#pipeline/${HYBRID}`);
    expect(screen.getByRole('link', { name: `pipeline ${short(DENSE)}` })).toBeTruthy();
    expect(screen.getByText('3 runs')).toBeTruthy();
    expect(screen.getByText('1 run')).toBeTruthy();
    expect(screen.getAllByRole('list', { name: 'Shape' })).toHaveLength(2);
    const tiles = [...(screen.getAllByRole('list', { name: 'Shape' })[0] as HTMLElement).querySelectorAll('.rg-tile')];
    expect(tiles.map((t) => t.getAttribute('data-family'))).toEqual(['retriever', 'retriever', 'fusion', 'reranker']);
  });

  it('the shape comes from the listing', async () => {
    const api = show('#runs');
    await loaded();
    expect(screen.getAllByRole('list', { name: 'Shape' })).toHaveLength(2);
    expect(api.requests.filter((r) => r.startsWith('GET /api/v1/runs/'))).toEqual([]);
    // Nothing is left to read, so nothing announces a read in progress.
    expect(screen.queryAllByRole('status').filter((s) => s.getAttribute('aria-busy') === 'true')).toEqual([]);
  });

  it('draws no shape for a pipeline the listing carries none for, and still lists its runs', async () => {
    show('#runs', routes({ body: { ...LISTING, shapes: { [HYBRID]: HYBRID_GRAPH } } }));
    await loaded();
    expect(screen.getAllByRole('list', { name: 'Shape' })).toHaveLength(1);
    expect(rowOf(R3)).toBeTruthy();
  });

  it('every benchmark and pipeline name is shown', async () => {
    const named: RunListing = {
      ...LISTING,
      runs: [summary(R1, HYBRID, SCIFACT, { 'ndcg@10': 0.6483 }, { pipeline_names: ['hybrid', 'hybrid-copy'], benchmark_names: ['beir/scifact', 'scifact-local'] })],
    };
    show('#runs', routes({ body: named }));
    const hybrid = await screen.findByRole('link', { name: 'hybrid' });
    expect(hybrid.getAttribute('href')).toBe('#pipeline/hybrid');
    expect(screen.getByRole('link', { name: 'hybrid-copy' }).getAttribute('href')).toBe('#pipeline/hybrid-copy');
    expect(rowOf(R1).getAttribute('aria-label')).toBe(`Run ${short(R1)} on beir/scifact, scifact-local`);
    expect(screen.getByRole('button', { name: /^beir\/scifact, scifact-local/ })).toBeTruthy();
  });

  it('lists the most recent run first, a run of unknown time last, and shows when each started', async () => {
    const timed: RunListing = {
      ...LISTING,
      runs: [
        summary(R1, HYBRID, SCIFACT, {}, { started_at_ms: null }),
        summary(R3, DENSE, SCIFACT, {}, { started_at_ms: Date.UTC(2026, 8, 30, 14, 3) }),
        summary(R4, HYBRID, SCIFACT, {}, { started_at_ms: Date.UTC(2026, 8, 29, 9, 0) }),
      ],
    };
    show('#runs', routes({ body: timed }));
    await loaded();
    const order = screen.getAllByRole('row').map((r) => r.getAttribute('aria-label')).filter((l) => l?.startsWith('Run '));
    expect(order).toEqual([R3, R4, R1].map((id) => `Run ${short(id)} on dataset 555555555555`));
    expect(screen.getByRole('columnheader', { name: 'Started' })).toBeTruthy();
    expect(within(rowOf(R3)).getByText((_, el) => el?.tagName === 'TIME').getAttribute('datetime')).toBe('2026-09-30T14:03:00.000Z');
    expect(within(rowOf(R1)).queryByText((_, el) => el?.tagName === 'TIME')).toBeNull();
  });

  it('keeps the runs in design/’s table, one semantic table, a row group per pipeline', async () => {
    show('#runs');
    const table = await screen.findByRole('table', { name: 'Runs, grouped by pipeline' });
    expect(table.closest('.rg-tablewrap')).toBeTruthy();
    expect(table.querySelectorAll('tbody')).toHaveLength(2);
    expect(table.querySelectorAll('th[scope="rowgroup"]')).toHaveLength(2);
    expect(within(table).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['Benchmark', 'Run', 'Status', 'Metrics']);
  });

  it('shows on each row only the metrics its run recorded, and no cell reads "—"', async () => {
    show('#runs');
    await loaded();
    expect(within(rowOf(R3)).getByText('exact_match')).toBeTruthy();
    expect(within(rowOf(R2)).queryByText('exact_match')).toBeNull();
    expect(within(rowOf(R2)).queryByText('mrr')).toBeNull();
    for (const cell of screen.getAllByRole('cell')) expect(cell.textContent?.trim()).not.toBe('—');
  });

  it('lists the runs the store cannot read, with why, rather than dropping them', async () => {
    show('#runs');
    expect(await screen.findByText('1 run could not be read.')).toBeTruthy();
    expect(screen.getByText(short(hex('9'))).closest('li')?.textContent).toContain('metrics.json is not valid JSON');
  });

  it('filters by benchmark', async () => {
    show('#runs');
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: /^dataset ffffffffffff/ }));
    expect(rowOf(R2)).toBeTruthy();
    expect(screen.queryByRole('row', { name: new RegExp(`^Run ${short(R1)}`) })).toBeNull();
    expect(screen.queryByRole('link', { name: `pipeline ${short(DENSE)}` })).toBeNull();
  });
});

describe('the keyboard', () => {
  it('makes the table one tab stop, whose rows are named by their run and benchmark', async () => {
    show('#runs');
    await loaded();
    const stops = screen.getAllByRole('row').filter((r) => r.getAttribute('tabindex') === '0');
    expect(stops.map((r) => r.getAttribute('aria-label'))).toEqual([`Run ${short(R1)} on dataset 555555555555`]);
    expect(box(R1).getAttribute('tabindex')).toBe('-1');
  });

  it('names each checkbox by its run', async () => {
    show('#runs');
    await loaded();
    expect(screen.getByRole('checkbox', { name: `Select run ${short(R2)} on dataset ffffffffffff` })).toBeTruthy();
  });

  it('opens a run in Replay with enter on its row', async () => {
    show('#runs');
    await loaded();
    fireEvent.keyDown(rowOf(R1), { key: 'Enter' });
    await waitFor(() => expect(window.location.hash).toBe(`#replay/${R1}`));
  });

  it('checks a run with space on its row', async () => {
    show('#runs');
    await loaded();
    fireEvent.keyDown(rowOf(R1), { key: ' ' });
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R1}`));
  });

  it('does not check a refused run with space', async () => {
    show(`#runs?sel=${R1}`);
    await loaded();
    fireEvent.keyDown(rowOf(R2), { key: ' ' });
    await new Promise((r) => setTimeout(r, 20));
    expect(window.location.hash).toBe(`#runs?sel=${R1}`);
  });
});

describe('the selection', () => {
  it('disables every run on another benchmark once a run is checked, and re-enables them when it is unchecked', async () => {
    show('#runs');
    await loaded();
    fireEvent.click(box(R1));
    await waitFor(() => expect(box(R2).disabled).toBe(true));
    expect(box(R3).disabled).toBe(false);
    fireEvent.click(box(R1));
    await waitFor(() => expect(box(R2).disabled).toBe(false));
  });

  it('gives a refused box a short reason, and says the whole sentence once above the table', async () => {
    show(`#runs?sel=${R1}`);
    await loaded();
    expect(reasonOf(box(R2))).toBe('Other benchmark');
    expect(screen.getAllByText('The selected runs are on dataset 555555555555. Compare takes runs on one benchmark.')).toHaveLength(1);
  });

  it('says the rule below the table, so the rows do not move under the pointer when it appears', async () => {
    show('#runs');
    await loaded();
    const table = screen.getByRole('table');
    const before = (el: Element) => {
      const wrap = el.closest('.rg-tablewrap') as Element;
      const out: string[] = [];
      for (let s = wrap.previousElementSibling; s !== null; s = s.previousElementSibling) out.push(s.outerHTML.replace(/ aria-pressed="(true|false)"/g, ''));
      return out;
    };
    const above = before(table);
    fireEvent.click(box(R1));
    const rule = await screen.findByText('The selected runs are on dataset 555555555555. Compare takes runs on one benchmark.');
    // Only the action bar's own text changes above the table ("Compare 1 selected"); nothing is inserted.
    expect(before(screen.getByRole('table')).length).toBe(above.length);
    expect(screen.getByRole('table').compareDocumentPosition(rule) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('refuses a sixth run once five are selected, saying why', async () => {
    const six = [R1, R3, R4, R5, hex('7'), hex('8')];
    show(`#runs?sel=${six.slice(0, 5).join(',')}`, routes({ body: { runs: six.map((id) => summary(id, DENSE, SCIFACT)), unreadable: [], shapes: {} } }));
    await loaded();
    expect(box(hex('8')).disabled).toBe(true);
    expect(reasonOf(box(hex('8')))).toBe('Five selected');
    expect(screen.getByText('Compare takes a baseline and up to four runs. Clear one to choose another.')).toBeTruthy();
  });

  it('is written to the address, in the order the runs were checked', async () => {
    show('#runs');
    await loaded();
    fireEvent.click(box(R3));
    await waitFor(() => expect(box(R3).checked).toBe(true));
    fireEvent.click(box(R1));
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R3},${R1}`));
  });

  it('is restored from the address on load', async () => {
    show(`#runs?sel=${R4},${R1}`);
    await loaded();
    expect(box(R1).checked).toBe(true);
    expect(box(R4).checked).toBe(true);
    expect(box(R2).disabled).toBe(true);
  });

  it('drops, in place, what the listing read for this address does not hold or no click could have selected', async () => {
    show(`#runs?sel=${hex('8')},${R1},${R2}`);
    await loaded();
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R1}`));
  });

  it('reads the runs again when the address names a run the listing does not hold, and keeps it once the store has it', async () => {
    const api = show('#runs', routes([{ body: LISTING }, { body: LATER }]));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R5] }, { replace: true }));
    await waitFor(() => expect(box(R5).checked).toBe(true));
    expect(window.location.hash).toBe(`#runs?sel=${R5}`);
    expect(api.requests.filter((r) => r === 'GET /api/v1/runs')).toHaveLength(2);
  });

  it('drops a run the address names only once a fresh listing confirms the store does not hold it', async () => {
    show('#runs', routes([{ body: LISTING }, { body: LISTING }]));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R1, R5] }, { replace: true }));
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R1}`));
  });

  it('keeps the table when a re-read fails, saying so inline with Retry', async () => {
    const problem: Problem = { type: 'urn:ragondin:problem:internal', title: 'Internal', status: 500, detail: 'the store is busy', code: 'run_unreadable', hint: 'Try again.' };
    show('#runs', routes([{ body: LISTING }, { problem }, { body: LATER }]));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R5] }, { replace: true }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('the store is busy');
    expect(rowOf(R1)).toBeTruthy();
    expect(window.location.hash).toBe(`#runs?sel=${R5}`);
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(box(R5).checked).toBe(true));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('does not count a run it is still reading for toward "Compare N selected"', async () => {
    const answers: ((r: ApiResult<RunListing>) => void)[] = [];
    const client = {
      get: (path: string) =>
        path === '/runs'
          ? new Promise<ApiResult<RunListing>>((resolve) => answers.push(resolve))
          : Promise.reject(new Error(`the screen reads /runs alone, not ${path}`)),
    } as unknown as ApiClient;
    window.history.replaceState(null, '', '/#runs');
    render(<Shell client={client} />);
    await act(async () => answers[0]?.({ ok: true, value: LISTING, build: null }));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R1, R4, R5] }, { replace: true }));
    await waitFor(() => expect(answers).toHaveLength(2));
    expect(compare().textContent).toBe('Compare 2 selected');
    fireEvent.click(compare());
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${R1}+${R4}`));
  });

  it('takes the answer of the last listing asked for, not of the last to arrive', async () => {
    const answers: ((r: ApiResult<RunListing>) => void)[] = [];
    const client = {
      get: (path: string) =>
        path === '/runs'
          ? new Promise<ApiResult<RunListing>>((resolve) => answers.push(resolve))
          : Promise.reject(new Error(`the screen reads /runs alone, not ${path}`)),
    } as unknown as ApiClient;
    window.history.replaceState(null, '', '/#runs');
    render(<Shell client={client} />);
    await act(async () => answers[0]?.({ ok: true, value: LISTING, build: null }));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R5] }, { replace: true }));
    await waitFor(() => expect(answers).toHaveLength(2));
    act(() => navigate({ screen: 'runs', sel: [R5, hex('7')] }, { replace: true }));
    await waitFor(() => expect(answers).toHaveLength(3));
    const both: RunListing = { ...LATER, runs: [...LATER.runs, summary(hex('7'), DENSE, SCIFACT)] };
    // The later request answers first, with both runs; the earlier one, asked before the second run existed, answers last.
    await act(async () => answers[2]?.({ ok: true, value: both, build: null }));
    await act(async () => answers[1]?.({ ok: true, value: LISTING, build: null }));
    await waitFor(() => expect(box(hex('7')).checked).toBe(true));
    expect(box(R5).checked).toBe(true);
    expect(window.location.hash).toBe(`#runs?sel=${R5},${hex('7')}`);
  });
});

describe('the actions', () => {
  it('refuses "Compare" below two selections, saying why', async () => {
    show(`#runs?sel=${R1}`);
    await loaded();
    expect(compare().textContent).toBe('Compare 1 selected');
    expect(compare().getAttribute('aria-disabled')).toBe('true');
    expect(screen.getByText('Select at least two runs on one benchmark to compare.')).toBeTruthy();
  });

  it('opens Compare with the selected runs, in selection order', async () => {
    show(`#runs?sel=${R4},${R1}`);
    await loaded();
    expect(compare().textContent).toBe('Compare 2 selected');
    fireEvent.click(compare());
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${R4}+${R1}`));
  });

  it('offers a new pipeline, in the Editor', async () => {
    show('#runs');
    await loaded();
    expect(screen.getByRole('link', { name: 'New pipeline' }).getAttribute('href')).toBe('#editor');
  });
});

/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Graph, RunDetail, RunListing, RunSummary } from '../api/types.ts';
import { useRoute } from '../routes.ts';
import { RunsScreen } from './RunsScreen.tsx';

const hex = (c: string) => c.repeat(64);
const SCIFACT = hex('5');
const FIQA = hex('f');
const HYBRID = hex('a');
const DENSE = hex('c');

const summary = (id: string, pipeline: string, dataset: string, metrics: Record<string, number>): RunSummary => ({
  id,
  pipeline,
  dataset_version: dataset,
  index_version: hex('0'),
  engine_version: '0.0.0',
  metrics,
});

const R1 = hex('1');
const R2 = hex('2');
const R3 = hex('3');
const R4 = hex('4');

/** Two pipelines over two benchmarks, and one run the store cannot read. */
const LISTING: RunListing = {
  runs: [
    summary(R1, HYBRID, SCIFACT, { 'ndcg@10': 0.6483, mrr: 0.5912 }),
    summary(R2, HYBRID, FIQA, { 'ndcg@10': 0.3301 }),
    summary(R3, DENSE, SCIFACT, { 'ndcg@10': 0.6012, exact_match: 0.412 }),
    summary(R4, HYBRID, SCIFACT, { 'ndcg@10': 0.6611 }),
  ],
  unreadable: [{ id: hex('9'), reason: 'metrics.json is not valid JSON' }],
};

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

const detail = (id: string, pipeline: string, graph: Graph): RunDetail => ({
  id,
  bindings: [],
  configuration: '',
  graph,
  inputs: { pipeline, dataset_version: SCIFACT, index_version: hex('0'), engine_version: '0.0.0', model_hashes: {} },
  metrics: {},
  prefix_of: null,
});

/** `GET /runs/{id}` answers by id, as the server does. */
const routes = (listing: MockRoutes['GET /runs'] = { body: LISTING }): MockRoutes => ({
  'GET /runs': listing,
  'GET /runs/{id}': { body: detail(R1, HYBRID, HYBRID_GRAPH) },
});

/** What the shell does: hands the screen the selection the address carries. */
function Shell() {
  const route = useRoute();
  return <RunsScreen client={createApiClient()} sel={route?.screen === 'runs' ? (route.sel ?? []) : []} />;
}

function show(hash: string, mocks: MockRoutes = routes()) {
  window.history.replaceState(null, '', `/${hash}`);
  const api = mockApi(mocks);
  render(<Shell />);
  return api;
}

const box = (id: string) => {
  const link = screen.getByRole('link', { name: id.slice(0, 12) });
  return within(link.closest('tr') as HTMLElement).getByRole('checkbox') as HTMLInputElement;
};

const compare = () => screen.getByRole('button', { name: /^Compare/ });

beforeEach(() => {
  window.history.replaceState(null, '', '/');
});

afterEach(() => {
  // mockApi stubs fetch for each test.
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
    await screen.findByRole('link', { name: `pipeline ${HYBRID.slice(0, 12)}` });
  });

  it('says there are no runs yet in one sentence, and leads to the Editor', async () => {
    show('#runs', routes({ body: { runs: [], unreadable: [] } }));
    expect((await screen.findByRole('heading', { name: 'No runs yet' })).tagName).toBe('H3');
    expect(screen.getByRole('link', { name: 'Open Editor' }).getAttribute('href')).toBe('#editor');
  });
});

describe('over a listing with two benchmarks', () => {
  it('groups the runs by pipeline, each group with its shape and its count', async () => {
    show('#runs');
    const hybrid = await screen.findByRole('link', { name: `pipeline ${HYBRID.slice(0, 12)}` });
    expect(hybrid.getAttribute('href')).toBe(`#pipeline/${HYBRID}`);
    expect(screen.getByRole('link', { name: `pipeline ${DENSE.slice(0, 12)}` })).toBeTruthy();
    expect(screen.getByText('3 runs')).toBeTruthy();
    expect(screen.getByText('1 run')).toBeTruthy();
    await waitFor(() => expect(screen.getAllByRole('list', { name: 'Shape' })).toHaveLength(2));
    const tiles = [...(screen.getAllByRole('list', { name: 'Shape' })[0] as HTMLElement).querySelectorAll('.rg-tile')];
    expect(tiles.map((t) => t.getAttribute('data-family'))).toEqual(['retriever', 'retriever', 'fusion', 'reranker']);
  });

  it('reads each group’s shape once, from one of its runs', async () => {
    const api = show('#runs');
    await waitFor(() => expect(screen.getAllByRole('list', { name: 'Shape' })).toHaveLength(2));
    expect(api.requests.filter((r) => r.startsWith('GET /api/v1/runs/')).sort()).toEqual([`GET /api/v1/runs/${R1}`, `GET /api/v1/runs/${R3}`]);
  });

  it('keeps the runs in one semantic table, a row group per pipeline', async () => {
    show('#runs');
    const table = await screen.findByRole('table', { name: 'Runs, grouped by pipeline' });
    expect(table.querySelectorAll('tbody')).toHaveLength(2);
    expect(within(table).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['Benchmark', 'Run', 'Status', 'Metrics']);
  });

  it('shows on each row only the metrics its run recorded, and no cell reads "—"', async () => {
    show('#runs');
    const row = (await screen.findByRole('link', { name: R3.slice(0, 12) })).closest('tr') as HTMLElement;
    expect(within(row).getByText('exact_match')).toBeTruthy();
    const other = screen.getByRole('link', { name: R2.slice(0, 12) }).closest('tr') as HTMLElement;
    expect(within(other).queryByText('exact_match')).toBeNull();
    expect(within(other).queryByText('mrr')).toBeNull();
    for (const cell of screen.getAllByRole('cell')) expect(cell.textContent?.trim()).not.toBe('—');
  });

  it('lists the runs the store cannot read, with why, rather than dropping them', async () => {
    show('#runs');
    expect(await screen.findByText('1 run could not be read.')).toBeTruthy();
    expect(screen.getByText(hex('9').slice(0, 12)).closest('li')?.textContent).toContain('metrics.json is not valid JSON');
  });

  it('filters by benchmark', async () => {
    show('#runs');
    await screen.findByRole('link', { name: R2.slice(0, 12) });
    fireEvent.click(screen.getByRole('button', { name: /^dataset ffffffffffff/ }));
    expect(screen.getByRole('link', { name: R2.slice(0, 12) })).toBeTruthy();
    expect(screen.queryByRole('link', { name: R1.slice(0, 12) })).toBeNull();
    expect(screen.queryByRole('link', { name: `pipeline ${DENSE.slice(0, 12)}` })).toBeNull();
  });
});

describe('the selection', () => {
  it('disables every run on another benchmark once a run is checked, with the reason, and re-enables them when it is unchecked', async () => {
    show('#runs');
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    fireEvent.click(box(R1));
    await waitFor(() => expect(box(R2).disabled).toBe(true));
    const reason = document.getElementById(box(R2).getAttribute('aria-describedby') ?? '')?.textContent;
    expect(reason).toBe('The selected runs are on dataset 555555555555. Compare takes runs on one benchmark.');
    expect(box(R3).disabled).toBe(false);
    fireEvent.click(box(R1));
    await waitFor(() => expect(box(R2).disabled).toBe(false));
  });

  it('is written to the address, in the order the runs were checked', async () => {
    show('#runs');
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    fireEvent.click(box(R3));
    await waitFor(() => expect(box(R3).checked).toBe(true));
    fireEvent.click(box(R1));
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R3},${R1}`));
  });

  it('is restored from the address on load', async () => {
    show(`#runs?sel=${R4},${R1}`);
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    expect(box(R1).checked).toBe(true);
    expect(box(R4).checked).toBe(true);
    expect(box(R2).disabled).toBe(true);
  });

  it('drops from the address what no click could have selected, in place', async () => {
    show(`#runs?sel=${hex('8')},${R1},${R2}`);
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R1}`));
  });
});

describe('the actions', () => {
  it('refuses "Compare" below two selections, saying why', async () => {
    show(`#runs?sel=${R1}`);
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    expect(compare().textContent).toBe('Compare 1 selected');
    expect(compare().getAttribute('aria-disabled')).toBe('true');
    expect(screen.getByText('Select at least two runs on one benchmark to compare.')).toBeTruthy();
  });

  it('opens Compare with the selected runs, in selection order', async () => {
    show(`#runs?sel=${R4},${R1}`);
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    expect(compare().textContent).toBe('Compare 2 selected');
    fireEvent.click(compare());
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${R4}+${R1}`));
  });

  it('offers a new pipeline, in the Editor', async () => {
    show('#runs');
    await screen.findByRole('link', { name: R1.slice(0, 12) });
    expect(screen.getByRole('link', { name: 'New pipeline' }).getAttribute('href')).toBe('#editor');
  });

  it('opens a run in Replay with enter on its row', async () => {
    show('#runs');
    const row = (await screen.findByRole('link', { name: R1.slice(0, 12) })).closest('tr') as HTMLElement;
    fireEvent.keyDown(row, { key: 'Enter' });
    await waitFor(() => expect(window.location.hash).toBe(`#replay/${R1}`));
  });

  it('checks a run with space on its row', async () => {
    show('#runs');
    const row = (await screen.findByRole('link', { name: R1.slice(0, 12) })).closest('tr') as HTMLElement;
    fireEvent.keyDown(row, { key: ' ' });
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${R1}`));
  });
});

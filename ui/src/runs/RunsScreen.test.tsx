/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient, type ApiClient, type ApiResult } from '../api/client.ts';
import { mockApi, type MockReply, type MockRoutes } from '../api/testing.ts';
import type { Graph, Problem, RunListing, RunSummary } from '../api/types.ts';
import { navigate, useRoute } from '../routes.ts';
import { RunsScreen } from './RunsScreen.tsx';

const hex = (c: string) => c.repeat(64);
const SCIFACT = hex('5');
const FIQA = hex('f');
const HYBRID = hex('a');
const DENSE = hex('c');

/** The family the API's catalogue gives a name, as `GET /runs` sends it. */
const familyOf = (name: string) => (name === 'exact_match' || name === 'token_f1' ? 'answers' : /^(ndcg@|recall@)\d+$|^mrr$/.test(name) ? 'ranking' : 'unknown');

const summary = (id: string, pipeline: string, dataset: string, metrics: Record<string, number> = {}, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline,
  pipeline_names: [],
  refused_pipeline_names: [], prefix_of_documents: [],
  launched_as: null,
  dataset_version: dataset,
  benchmark_names: [],
  index_version: hex('0'),
  engine_version: '0.0.0',
  started_at_ms: null,
  finished_at_ms: null,
  metrics,
  metric_families: Object.fromEntries(Object.keys(metrics).map((name) => [name, familyOf(name)])),
  median_query_latency_nanos: null,
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
  return <RunsScreen client={client} sel={route?.screen === 'runs' ? (route.sel ?? []) : []} bench={route?.screen === 'runs' ? route.bench : undefined} />;
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

  it('with no runs, lists the three steps to a first one — a benchmark, a pipeline, a launch — ticking those the workspace holds', async () => {
    const refresh = vi.fn();
    window.history.replaceState(null, '', '/#runs');
    mockApi(routes({ body: { runs: [], unreadable: [], shapes: {} } }));
    const counts = { benchmarks_ready: 1, pipelines: 0, runs: 0, services_connected: 0 };
    render(<RunsScreen client={createApiClient()} sel={[]} counts={counts} refreshWorkspace={refresh} />);
    const steps = await screen.findByRole('list', { name: 'Steps to a first run' });
    const items = within(steps).getAllByRole('listitem');
    expect(items.map((i) => i.querySelector('h4, b, strong')?.textContent ?? '')).toEqual(['Add a benchmark', 'Build a pipeline', 'Launch it']);
    expect(within(items[0] as HTMLElement).getByRole('img', { name: 'Done' })).toBeTruthy();
    expect(within(items[1] as HTMLElement).queryByRole('img', { name: 'Done' })).toBeNull();
    expect(within(items[0] as HTMLElement).getByRole('link', { name: 'Open Setup' }).getAttribute('href')).toBe('#setup/benchmarks');
    expect(within(items[1] as HTMLElement).getByRole('link', { name: 'Open Editor' }).getAttribute('href')).toBe('#editor');
    expect(within(items[2] as HTMLElement).getByRole('button', { name: 'Launch…' })).toBeTruthy();
    // The counts are the workspace's as the shell read it: read again, so a pipeline saved since ticks its step.
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('ticks no step before the workspace is read', async () => {
    show('#runs', routes({ body: { runs: [], unreadable: [], shapes: {} } }));
    const steps = await screen.findByRole('list', { name: 'Steps to a first run' });
    expect(within(steps).queryByRole('img', { name: 'Done' })).toBeNull();
  });

  it('says there are no runs yet in one sentence, and leads to the Editor', async () => {
    show('#runs', routes({ body: { runs: [], unreadable: [], shapes: {} } }));
    expect((await screen.findByRole('heading', { name: 'No runs yet' })).tagName).toBe('H3');
    expect(screen.getByRole('link', { name: 'Open Editor' }).getAttribute('href')).toBe('#editor');
  });

  it('draws the empty state’s two actions at one size', async () => {
    show('#runs', routes({ body: { runs: [], unreadable: [], shapes: {} } }));
    const open = await screen.findByRole('link', { name: 'Open Editor' });
    const launch = screen.getByRole('button', { name: 'Launch…' });
    expect(launch.className).toContain('rg-btn--l');
    expect(open.className).toContain('rg-btn--l');
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

  it('heads a group by its recorded name as the listing says it is held: linked, gone, or refused as a case alias', async () => {
    const recorded: RunListing = {
      ...LISTING,
      runs: [
        summary(R1, HYBRID, SCIFACT, {}, { launched_as: { name: 'hybrid-old', prefix_of: null, held: 'gone' } }),
        summary(R3, DENSE, SCIFACT, {}, { launched_as: { name: 'Dense', prefix_of: null, held: 'other_case' } }),
        summary(R4, HYBRID, SCIFACT, {}, { launched_as: { name: 'hybrid', prefix_of: null, held: 'exactly' } }),
      ],
    };
    const api = show('#runs', routes({ body: recorded }));
    await loaded();
    const gone = screen.getByText('hybrid-old').closest('th') as HTMLElement;
    expect(within(gone).queryByRole('link')).toBeNull();
    expect(gone.textContent).toContain('no longer a document in this workspace');
    const cased = screen.getByText('Dense').closest('th') as HTMLElement;
    expect(within(cased).queryByRole('link')).toBeNull();
    expect(cased.textContent).toContain('refused: another spelling differs only in case');
    expect(screen.getByRole('link', { name: 'hybrid' }).getAttribute('href')).toBe('#pipeline/hybrid');
    // The listing says it all: no second read of the workspace's documents.
    expect(api.requests.some((r) => r.startsWith('GET /api/v1/pipelines'))).toBe(false);
  });

  it('links neither of two hash matches that differ only in case, and says why truly, linking the third', async () => {
    // `hybrid.yaml` and `Hybrid.yaml` both stored: the API refuses a read of either.
    const both: RunListing = {
      ...LISTING,
      runs: [summary(R1, HYBRID, SCIFACT, {}, { pipeline_names: ['Hybrid', 'hybrid', 'hybrid-copy'], refused_pipeline_names: ['Hybrid', 'hybrid'] })],
    };
    show('#runs', routes({ body: both }));
    await loaded();
    const heading = screen.getByText('hybrid-copy').closest('th') as HTMLElement;
    expect(within(heading).getAllByRole('link').filter((a) => !a.classList.contains('rg-runs__edit')).map((a) => a.textContent)).toEqual(['hybrid-copy']);
    expect(heading.textContent?.match(/refused: another spelling differs only in case/g)).toHaveLength(2);
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
    // An answer metric as the design writes it: EM 41.2, from 0.412.
    expect(within(rowOf(R3)).getByText('EM').nextElementSibling?.textContent).toBe('41.2');
    expect(within(rowOf(R2)).queryByText('EM')).toBeNull();
    expect(within(rowOf(R2)).queryByText('mrr')).toBeNull();
    for (const cell of screen.getAllByRole('cell')) expect(cell.textContent?.trim()).not.toBe('—');
  });

  it('the latency is labelled as a median query latency', async () => {
    const timed: RunListing = { ...LISTING, runs: LISTING.runs.map((run) => (run.id === R1 ? { ...run, median_query_latency_nanos: 17_249_000 } : run)) };
    show('#runs', routes({ body: timed }));
    const table = await screen.findByRole('table', { name: 'Runs, grouped by pipeline' });
    const headers = within(table).getAllByRole('columnheader').map((h) => h.textContent);
    expect(headers).toContain('Median query latency');
    expect(headers).not.toContain('Latency');
    expect(within(rowOf(R1)).getByText('17 ms')).toBeTruthy();
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

  it('keeps the filter in the address, in place, so a link or a reload shows the same rows', async () => {
    show('#runs');
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: /^dataset ffffffffffff/ }));
    await waitFor(() => expect(window.location.hash).toBe(`#runs?bench=${FIQA}`));
    fireEvent.click(screen.getByRole('button', { name: /^dataset ffffffffffff/ }));
    await waitFor(() => expect(window.location.hash).toBe('#runs'));
  });

  it('filters by the benchmarks the address names', async () => {
    show(`#runs?bench=${FIQA}`);
    await screen.findByRole('row', { name: new RegExp(`^Run ${short(R2)} on `) });
    expect(screen.queryByRole('row', { name: new RegExp(`^Run ${short(R1)}`) })).toBeNull();
    expect(screen.getByRole('button', { name: /^dataset ffffffffffff/ }).getAttribute('aria-pressed')).toBe('true');
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

describe('fork', () => {
  const fork = () => screen.getByRole('button', { name: 'Fork this run' });

  it('is refused, saying why, until exactly one run is selected', async () => {
    show('#runs');
    await loaded();
    expect(fork().getAttribute('aria-disabled')).toBe('true');
    expect(reasonOf(fork())).toBe('Select one run to fork it.');
  });

  it('forks the one run selected and opens the editor on the new pipeline', async () => {
    const api = show(`#runs?sel=${R1}`, {
      ...routes(),
      'GET /runs/{id}': { body: { id: R1, inputs: { pipeline: HYBRID, dataset_version: SCIFACT, index_version: 'i', model_hashes: {}, engine_version: '0.1.0' }, metrics: {}, configuration: 'pipeline: {}\n', bindings: [], started_at_ms: null, finished_at_ms: null, graph: HYBRID_GRAPH, launched_as: null } },
      'GET /pipelines': { body: { pipelines: [] } },
      'PUT /pipelines/{name}': { body: { name: 'run-11111111-fork', etag: 'e'.repeat(64), hash: HYBRID } },
      'GET /runs/{id}/layout': { body: { layout: null } },
    });
    await loaded();
    fireEvent.click(fork());
    await waitFor(() => expect(window.location.hash).toBe('#editor/run-11111111-fork'));
    expect(api.bodies[api.requests.indexOf('PUT /api/v1/pipelines/run-11111111-fork')]).toEqual({ document: 'pipeline: {}\n' });
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
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${R1.slice(0, 12)}+${R4.slice(0, 12)}`));
  });

  it('cancels a re-read a newer one overtakes, and shows no error for it', async () => {
    let asked = 0;
    let release: (reply: MockReply<RunListing>) => void = () => {};
    const both: RunListing = { ...LATER, runs: [...LATER.runs, summary(hex('7'), DENSE, SCIFACT)] };
    // The first listing answers; the re-read for R5 is held; so is the one for R5 and the seventh run, until released.
    const api = show('#runs', routes(() => (++asked === 1 ? { body: LISTING } : asked === 2 ? new Promise(() => {}) : new Promise((r) => (release = r)))));
    // The signals each path was sent with, in order: `/runs` alone, since the listing says which names are held.
    const signalsOf = (path: string) => api.signals.filter((_, i) => api.requests[i]?.startsWith(`GET /api/v1${path}`));
    await loaded();
    act(() => navigate({ screen: 'runs', sel: [R5] }, { replace: true }));
    await waitFor(() => expect(signalsOf('/runs')).toHaveLength(2));
    expect(signalsOf('/runs')[1]?.aborted).toBe(false);
    act(() => navigate({ screen: 'runs', sel: [R5, hex('7')] }, { replace: true }));
    await waitFor(() => expect(signalsOf('/runs')).toHaveLength(3));
    expect(signalsOf('/runs')[1]?.aborted).toBe(true);
    expect(signalsOf('/pipelines')).toHaveLength(0);
    // While the newer re-read is still held, the aborted one has settled: it must show nothing.
    await act(async () => new Promise((r) => setTimeout(r, 0)));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByText('request_aborted')).toBeNull();
    await act(async () => release({ body: both }));
    await waitFor(() => expect(box(hex('7')).checked).toBe(true));
    expect(screen.queryByRole('alert')).toBeNull();
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
    // Each run by its 12-character prefix, which names one run of the listing.
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${R4.slice(0, 12)}+${R1.slice(0, 12)}`));
  });

  it('offers a new pipeline, in the Editor', async () => {
    show('#runs');
    await loaded();
    expect(screen.getByRole('link', { name: 'New pipeline' }).getAttribute('href')).toBe('#editor');
  });
});

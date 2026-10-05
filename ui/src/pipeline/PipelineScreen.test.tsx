/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { MatrixColumn, PipelineListing, PipelineMatrix, PipelineSummary, Problem } from '../api/types.ts';
import { useRoute } from '../routes.ts';
import { readStored } from '../shell/storage.ts';
import { MATRIX, NAME, NO_RUNS, PREFIXED, RUN_OLD, RUN_PREFIX, SINCE_CHANGED, WITH_FIQA } from './fixtures.ts';
import { LAST_PIPELINE } from './last.ts';
import type { LaunchRequest } from './Matrix.tsx';
import { PipelineScreen } from './PipelineScreen.tsx';

const summary = (name: string): PipelineSummary => ({ name, etag: 'e', hash: 'a'.repeat(64), modified_ms: null, ends_in_answer: false, error: null });
const LISTING: PipelineListing = { pipelines: [summary('dense-only'), summary(NAME)] };

const problem = (code: Problem['code'], detail: string, status = 404): Problem => ({ type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint: 'Check the name under pipelines/.' });

const routes = (matrix: MockRoutes['GET /pipelines/{name}/matrix'] = { body: MATRIX }): MockRoutes => ({ 'GET /pipelines/{name}/matrix': matrix, 'GET /pipelines': { body: LISTING } });

/** What the shell does: hands the screen the name its address carries. */
function Shell({ launch }: { launch?: ((request: LaunchRequest) => void) | undefined }) {
  const route = useRoute();
  if (route?.screen !== 'pipeline') return <p>elsewhere {window.location.hash}</p>;
  return <PipelineScreen client={createApiClient()} name={route.name} {...(launch === undefined ? {} : { launch })} />;
}

function show(hash: string, mocks: MockRoutes = routes(), launch?: (request: LaunchRequest) => void) {
  window.history.replaceState(null, '', `/${hash}`);
  const api = mockApi(mocks);
  render(<Shell launch={launch} />);
  return api;
}

const loaded = () => screen.findByRole('table', { name: `${NAME}: each node on each benchmark` });

beforeEach(() => {
  window.history.replaceState(null, '', '/');
  window.localStorage.clear();
});
afterEach(() => {
  window.history.replaceState(null, '', '/');
  vi.unstubAllGlobals();
});

describe('reading the matrix', () => {
  it('offers to edit the pipeline in the editor, from its header', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    expect(screen.getByRole('link', { name: `Edit ${NAME}` }).getAttribute('href')).toBe(`#editor/${NAME}`);
  });

  it('asks the API for the pipeline the address names, every benchmark the registry knows included', async () => {
    const api = show(`#pipeline/${NAME}`);
    await loaded();
    expect(api.requests).toContain(`GET /api/v1/pipelines/${NAME}/matrix?include_available=true`);
  });

  it('says what it is reading while the request is in flight', () => {
    show(`#pipeline/${NAME}`);
    expect(screen.getByRole('status').textContent).toBe(`Reading the matrix of ${NAME}`);
  });

  it('shows a failure inline with its code and a retry that asks again', async () => {
    const api = show(`#pipeline/${NAME}`, routes([{ problem: problem('pipeline_not_found', `No pipeline named ${NAME}.`) }, { body: MATRIX }]));
    expect(await screen.findByText(`No pipeline named ${NAME}.`)).toBeTruthy();
    expect(screen.getByText('pipeline_not_found')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await loaded();
    expect(api.requests.filter((r) => r.includes('/matrix'))).toHaveLength(2);
  });

  it('reads again when the address names another pipeline, and only the last answer lands', async () => {
    const api = show(`#pipeline/${NAME}`);
    await loaded();
    window.location.hash = '#pipeline/dense-only';
    await waitFor(() => expect(api.requests).toContain('GET /api/v1/pipelines/dense-only/matrix?include_available=true'));
  });

  it('remembers the last pipeline viewed, for the top bar', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    expect(readStored('local', LAST_PIPELINE)).toBe(NAME);
  });

  it('remembers nothing for a pipeline the API does not know', async () => {
    show(`#pipeline/${NAME}`, routes({ problem: problem('pipeline_not_found', 'No pipeline named that.') }));
    await screen.findByText('No pipeline named that.');
    expect(readStored('local', LAST_PIPELINE)).toBeNull();
  });
});

describe('the header', () => {
  it('offers every pipeline of the workspace by name, the current one chosen, and moves to the one chosen', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const select = screen.getByRole('combobox', { name: 'Pipeline' }) as HTMLSelectElement;
    await waitFor(() => expect([...select.options].map((o) => o.value)).toEqual(['dense-only', NAME]));
    expect(select.value).toBe(NAME);
    fireEvent.change(select, { target: { value: 'dense-only' } });
    expect(window.location.hash).toBe('#pipeline/dense-only');
  });

  it('draws the pipeline’s shape as family tiles in pipeline order, each with its family in words beside it', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const shape = screen.getByRole('list', { name: 'Shape, node by node' });
    expect(within(shape).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['bm25 retriever', 'dense retriever', 'rrf fusion', 'rerank reranker', 'concat context builder', 'generate generator']);
    // The label is on screen, not only announced.
    expect(screen.getByText('Shape, node by node').tagName).not.toBe('OL');
  });

  it('names the shape by the words on screen, and hides each glyph its family’s words already say', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const shape = screen.getByRole('list', { name: 'Shape, node by node' });
    const label = screen.getByText('Shape, node by node');
    expect(shape.getAttribute('aria-labelledby')).toBe(label.id);
    expect(shape.hasAttribute('aria-label')).toBe(false);
    // Six tiles drawn, none announced: the family is said once, in words.
    expect(shape.querySelectorAll('.rg-tile')).toHaveLength(6);
    expect(within(shape).queryAllByRole('img')).toHaveLength(0);
  });

  it('says the matrix is derived from runs, and from how many', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const subtitle = screen.getByText(/^Derived from 3 runs/);
    expect(subtitle.textContent).toBe(`Derived from 3 runs: each column is the most recent run on its benchmark; nothing here is stored. Canonical hash ${'a'.repeat(12)}`);
  });

  it('reads every ranking row by the ranking metric chosen, ndcg@10 by default as every view opens on', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const select = screen.getByRole('combobox', { name: 'Ranking metric' }) as HTMLSelectElement;
    expect(select.value).toBe('ndcg@10');
    expect(screen.getByRole('rowheader', { name: /rerank/ }).textContent).toContain('ndcg@10');
    fireEvent.change(select, { target: { value: 'mrr' } });
    expect(screen.getByRole('rowheader', { name: /rerank/ }).textContent).toContain('mrr');
  });
});

describe('the states', () => {
  it('shows a pipeline with no run as an empty state with the way to launch it', async () => {
    show(`#pipeline/${NAME}`, routes({ body: NO_RUNS }));
    const empty = await screen.findByRole('heading', { name: `No run of ${NAME} yet` });
    expect(empty).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open in the Editor' }).getAttribute('href')).toBe(`#editor/${NAME}`);
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.queryByText(/Derived from/)).toBeNull();
  });

  it('asks for a pipeline before one is chosen, offering the workspace’s', async () => {
    show('#pipeline');
    expect(await screen.findByRole('heading', { name: 'No pipeline chosen' })).toBeTruthy();
    const select = (await screen.findByRole('combobox', { name: 'Pipeline' })) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(3));
    fireEvent.change(select, { target: { value: NAME } });
    expect(window.location.hash).toBe(`#pipeline/${NAME}`);
  });

  it('leads to the Editor when the workspace holds no pipeline', async () => {
    show('#pipeline', { 'GET /pipelines': { body: { pipelines: [] } } });
    expect(await screen.findByRole('link', { name: 'Open the Editor' })).toBeTruthy();
  });

  it('lists the runs the store cannot read, with its reason', async () => {
    show(`#pipeline/${NAME}`, routes({ body: { ...MATRIX, unreadable: [{ id: 'f'.repeat(64), reason: 'run.json is not JSON' }] } }));
    await loaded();
    expect(screen.getByText('run.json is not JSON')).toBeTruthy();
  });

  it('says why a column’s figures are unverified, in the API’s words', async () => {
    const columns = MATRIX.columns.map((c, i) => (i === 1 ? ({ ...c, dataset_check: { ...c.dataset_check!, status: 'dataset_differs', detail: 'the dataset on disk digests to another version' } } satisfies MatrixColumn) : c));
    show(`#pipeline/${NAME}`, routes({ body: { ...MATRIX, columns } }));
    await loaded();
    expect(screen.getByText('beir/scifact: the dataset on disk digests to another version.')).toBeTruthy();
  });
});

describe('the runs that feed it', () => {
  const runs = () => screen.getByRole('list', { name: 'Runs that feed the matrix' });

  it('lists every feeding run, the most recent first, each the way to its replay', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    const items = within(runs()).getAllByRole('listitem');
    expect(items).toHaveLength(3);
    expect(within(items[0] as HTMLElement).getByRole('link', { name: /run 777777777777/ }).getAttribute('href')).toBe(`#replay/${'7'.repeat(64)}`);
  });

  it('labels a prefix run, and shows its two facts apart', async () => {
    show(`#pipeline/${NAME}`, routes({ body: PREFIXED }));
    await loaded();
    const prefix = within(runs()).getAllByRole('listitem').find((li) => li.textContent?.includes(RUN_PREFIX.slice(0, 12))) as HTMLElement;
    expect(within(prefix).getByText(`prefix of ${NAME}, up to rerank`)).toBeTruthy();
    expect(within(prefix).getByText(`Run as a prefix of ${NAME}, up to rerank`)).toBeTruthy();
    expect(within(prefix).getByText('Configuration matches no pipeline in the workspace now')).toBeTruthy();
  });

  it('says a run whose content changed since its launch, with its parameter difference, and that it fills nothing', async () => {
    show(`#pipeline/${NAME}`, routes({ body: { ...MATRIX, feeding_runs: [...MATRIX.feeding_runs, SINCE_CHANGED] } }));
    await loaded();
    const old = within(runs()).getAllByRole('listitem').find((li) => li.textContent?.includes(RUN_OLD.slice(0, 12))) as HTMLElement;
    expect(old.textContent).toContain(`Launched as ${NAME}; content since changed`);
    expect(old.textContent).toContain('fills no cell');
    const diff = within(old).getByRole('table', { name: `What differs between ${NAME} now and run ${RUN_OLD.slice(0, 12)}` });
    // It scrolls sideways on a phone: its scroll box is a named tab stop, as the matrix's is.
    expect(within(old).getByRole('region', { name: `What differs between ${NAME} now and run ${RUN_OLD.slice(0, 12)}` }).getAttribute('tabindex')).toBe('0');
    const row = within(diff).getByRole('row', { name: /top_k/ });
    expect(within(row).getAllByRole('cell').map((c) => c.textContent)).toEqual(['top_k', '50', '100']);
  });

  it('reads a since-changed prefix as a prefix of an earlier version', async () => {
    const prefix = { ...SINCE_CHANGED, content_since_changed: { ...SINCE_CHANGED.content_since_changed!, launched: 'as_prefix' as const } };
    show(`#pipeline/${NAME}`, routes({ body: { ...MATRIX, feeding_runs: [prefix] } }));
    await loaded();
    expect(runs().textContent).toContain(`A prefix of an earlier version of ${NAME}`);
  });

  it('says why the difference cannot be read when the API could not compare', async () => {
    const unavailable = { ...SINCE_CHANGED, content_since_changed: { launched: 'as_pipeline' as const, difference: { kind: 'unavailable' as const, reason: 'the stored document does not parse', run: RUN_OLD } } };
    show(`#pipeline/${NAME}`, routes({ body: { ...MATRIX, feeding_runs: [unavailable] } }));
    await loaded();
    expect(runs().textContent).toContain('The difference cannot be read: the stored document does not parse');
  });
});

describe('the verdict and the one primary action', () => {
  it('ends on the verdict sentence', async () => {
    show(`#pipeline/${NAME}`, routes({ body: PREFIXED }));
    await loaded();
    expect(screen.getByText('Measured on 3 of 3 benchmarks, 1 of them only up to rerank. 2 cells wait for a run of the whole pipeline.')).toBeTruthy();
  });

  it('offers “Launch the missing run”, counting runs — one per benchmark — not cells, which opens the launch panel on the one column missing', async () => {
    const launch = vi.fn();
    show(`#pipeline/${NAME}`, routes({ body: WITH_FIQA }), launch);
    await loaded();
    const action = screen.getByRole('button', { name: 'Launch the missing run' });
    expect(action.classList.contains('rg-btn--primary')).toBe(true);
    expect(action.hasAttribute('aria-disabled')).toBe(false);
    fireEvent.click(action);
    expect(launch.mock.calls).toEqual([[{ pipeline: NAME, benchmarks: ['beir/fiqa'] }]]);
  });

  it('is absent when no cell is missing', async () => {
    show(`#pipeline/${NAME}`);
    await loaded();
    expect(screen.queryByRole('button', { name: /missing run/ })).toBeNull();
  });

  it('hands “Launch the N missing runs” every launchable column’s benchmark at once, and lists the columns it cannot launch with why', async () => {
    const launch = vi.fn();
    const both: PipelineMatrix = { ...WITH_FIQA, missing: [...WITH_FIQA.missing, ...PREFIXED.missing, { benchmark: null, dataset_version: '0'.repeat(64), nodes: ['bm25'] }] };
    show(`#pipeline/${NAME}`, routes({ body: both }), launch);
    await loaded();
    // The column with no benchmark name cannot be launched, and is not counted in the action.
    const action = screen.getByRole('button', { name: 'Launch the 2 missing runs' });
    expect(action.hasAttribute('aria-disabled')).toBe(false);
    fireEvent.click(action);
    expect(launch.mock.calls).toEqual([[{ pipeline: NAME, benchmarks: ['beir/fiqa', 'beir/nfcorpus'] }]]);
    expect(screen.getByText('Not launched: dataset 000000000000, which no benchmark name is pinned to, so there is nothing to launch it on.')).toBeTruthy();
  });

  it('hands each benchmark once, though two columns of missing cells carry its name', async () => {
    const launch = vi.fn();
    const twice: PipelineMatrix = { ...WITH_FIQA, missing: [...WITH_FIQA.missing, { benchmark: 'beir/fiqa', dataset_version: '1'.repeat(64), nodes: ['bm25'] }] };
    show(`#pipeline/${NAME}`, routes({ body: twice }), launch);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Launch the missing run' }));
    expect(launch.mock.calls).toEqual([[{ pipeline: NAME, benchmarks: ['beir/fiqa'] }]]);
  });

  it('opens Runs’ launch panel on every missing benchmark from “Launch the N missing runs”', async () => {
    const both: PipelineMatrix = { ...WITH_FIQA, missing: [...WITH_FIQA.missing, ...PREFIXED.missing] };
    show(`#pipeline/${NAME}`, routes({ body: both }));
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Launch the 2 missing runs' }));
    await screen.findByText(`elsewhere #runs?launch=${NAME}&benchmark=beir%2Ffiqa&benchmark=beir%2Fnfcorpus`);
  });

  it('opens Runs’ launch panel on the pipeline and the benchmark when a Run is pressed, with no launcher of its own', async () => {
    show(`#pipeline/${NAME}`, routes({ body: WITH_FIQA }));
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Run on beir/fiqa' }));
    await screen.findByText(`elsewhere #runs?launch=${NAME}&benchmark=beir%2Ffiqa`);
  });
});

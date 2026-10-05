/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from './api/client.ts';
import { FakeEventSource, installFakeEventSource, mockApi } from './api/testing.ts';
import type { JobSummary, Workspace } from './api/types.ts';
import { App } from './App.tsx';
import { COMPARISON, DENSE, HYBRID, RERANK } from './compare/fixtures.ts';
import * as replay from './replay/fixtures.ts';
import * as editor from './editor/fixtures.ts';
import { MATRIX, NAME as PIPELINE } from './pipeline/fixtures.ts';

const BUILD = '0.0.0+aaaaaaaaaaaa';

const WORKSPACE: Workspace = {
  path: '/home/ada/ragondin-ws',
  build: BUILD,
  settings: {
    datasets: '/home/ada/ragondin-ws/datasets',
    services: [
      { family: 'generator', name: 'qwen', uri: 'http://127.0.0.1:50051' },
      { family: 'embedder', name: 'bge', uri: 'http://127.0.0.1:50052' },
    ],
  },
  capabilities: { families: [], remote: true },
  counts: { pipelines: 1, runs: 0, benchmarks_ready: 0, services_connected: 0 },
};

function show(hash: string, props: { reload?: () => void; followJobs?: boolean } = {}) {
  window.history.replaceState(null, '', `/${hash}`);
  const reload = props.reload ?? vi.fn();
  const client = createApiClient();
  const view = render(<App client={client} build={BUILD} reload={reload} {...(props.followJobs === undefined ? {} : { followJobs: props.followJobs })} />);
  return { ...view, reload, client };
}

const indicator = () => within(screen.getByRole('banner')).getAllByRole('link').find((l) => l.getAttribute('href') === '#setup' && l.closest('nav') === null);
const main = () => screen.getByRole('main');

beforeEach(() => {
  window.sessionStorage.clear();
  window.localStorage.clear();
  // The shell follows the job stream when a test tells it to (`followJobs`).
  installFakeEventSource();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the shell’s screens', () => {
  it.each([
    ['#runs', 'Runs', 'No runs yet'],
    ['#pipeline', 'Pipeline', 'No pipeline chosen'],
    ['#compare', 'Compare', 'Choose at least two runs'],
    ['#compare/aaa', 'Compare', 'Choose at least two runs'],
    ['#replay', 'Replay', 'No run to replay yet'],
    ['#editor', 'Editor', 'No pipeline open'],
    ['#editor/hybrid-rrf', 'Editor', 'hybrid-rrf cannot be opened on the canvas'],
  ])('%s renders the %s screen, restored from the hash, in its empty state', async (hash, tab, heading) => {
    const pipeline = { name: 'hybrid-rrf', document: 'pipeline: {}\n', etag: 'e'.repeat(64), hash: null, error: null, typed: null, canonical: false };
    mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } }, 'GET /pipelines/{name}': { body: pipeline } }, { build: BUILD });
    show(hash);
    expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe(tab);
    expect((await within(main()).findByRole('heading', { level: 3 })).textContent).toBe(heading);
    const nav = within(screen.getByRole('navigation', { name: 'Screens' }));
    expect(nav.getByRole('link', { name: tab }).getAttribute('aria-current')).toBe('page');
    await screen.findByText(WORKSPACE.path);
  });

  it('hands Compare the runs and the baseline its address carries', async () => {
    const api = mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } }, 'POST /compare': { body: COMPARISON } }, { build: BUILD });
    show(`#compare/${DENSE}+${HYBRID}+${RERANK}?baseline=${DENSE}`);
    expect(await within(main()).findByRole('heading', { name: 'Verdict' })).toBeTruthy();
    expect(api.bodies[api.requests.indexOf('POST /api/v1/compare')]).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: DENSE });
  });

  it('hands Replay the run, the query and the run beside that its address carries, so a reload restores the view', async () => {
    const id = (path: string) => decodeURIComponent(path.split('/')[4] ?? '');
    const details = { [replay.HYBRID]: replay.HYBRID_DETAIL, [replay.DENSE]: replay.DENSE_DETAIL };
    const queries = { [replay.HYBRID]: replay.HYBRID_QUERIES, [replay.DENSE]: replay.DENSE_QUERIES };
    const traces = { [replay.HYBRID]: replay.HYBRID_TRACE, [replay.DENSE]: replay.DENSE_TRACE };
    mockApi(
      {
        'GET /workspace': { body: WORKSPACE },
        'GET /runs': { body: replay.LISTING },
        'GET /runs/{id}': (_q, path) => ({ body: details[id(path)]! }),
        'GET /runs/{id}/queries': (_q, path) => ({ body: queries[id(path)]! }),
        'GET /runs/{id}/trace/{query}': (_q, path) => ({ body: traces[id(path)]! }),
      },
      { build: BUILD },
    );
    show(`#replay/${replay.HYBRID}/q/q1?with=${replay.DENSE}`);
    expect(await within(main()).findByRole('application', { name: 'Run A, hybrid-rerank-gen, query q1' })).toBeTruthy();
    expect(await within(main()).findByRole('application', { name: 'Run B, dense-only, query q1' })).toBeTruthy();
    expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Replay');
  });

  it('hands Replay the set of regressions its address carries, read against the run beside', async () => {
    const id = (path: string) => decodeURIComponent(path.split('/')[4] ?? '');
    const details = { [replay.HYBRID]: replay.HYBRID_DETAIL, [replay.DENSE]: replay.DENSE_DETAIL };
    const queries = { [replay.HYBRID]: replay.HYBRID_QUERIES, [replay.DENSE]: replay.DENSE_QUERIES };
    const traces = { [replay.HYBRID]: replay.HYBRID_TRACE, [replay.DENSE]: replay.DENSE_TRACE };
    const api = mockApi(
      {
        'GET /workspace': { body: WORKSPACE },
        'GET /runs': { body: replay.LISTING },
        'GET /runs/{id}': (_q, path) => ({ body: details[id(path)]! }),
        'GET /runs/{id}/queries': (_q, path) => ({ body: queries[id(path)]! }),
        'GET /runs/{id}/trace/{query}': (_q, path) => ({ body: traces[id(path)]! }),
        'POST /compare': { body: COMPARISON },
      },
      { build: BUILD },
    );
    show(`#replay/${replay.HYBRID}/q/q1?with=${replay.DENSE}&set=regressions&metric=ndcg%4010`);
    expect(await within(main()).findByRole('navigation', { name: 'Step through the regressions' })).toBeTruthy();
    await waitFor(() => expect(api.bodies[api.requests.indexOf('POST /api/v1/compare')]).toEqual({ run_ids: [replay.DENSE, replay.HYBRID], baseline: replay.DENSE }));
  });

  it('keeps the node selected in Replay when Replay opens the editor on the same pipeline', async () => {
    const id = (path: string) => decodeURIComponent(path.split('/')[4] ?? '');
    const hash = replay.HYBRID;
    const listing = { ...replay.LISTING, runs: replay.LISTING.runs.map((r) => (r.id === hash ? { ...r, pipeline_names: ['hybrid'] } : r)) };
    mockApi(
      {
        'GET /workspace': { body: WORKSPACE },
        'GET /runs': { body: listing },
        'GET /runs/{id}': (_q, path) => ({ body: id(path) === hash ? replay.HYBRID_DETAIL : replay.DENSE_DETAIL }),
        'GET /runs/{id}/queries': { body: replay.HYBRID_QUERIES },
        'GET /runs/{id}/trace/{query}': { body: replay.HYBRID_TRACE },
        'GET /pipelines': { body: { pipelines: [{ name: 'hybrid', etag: 'e'.repeat(64), modified_ms: null, hash, error: null }] } },
        'GET /pipelines/{name}': { body: { name: 'hybrid', document: 'pipeline: …\n', etag: 'e'.repeat(64), hash, error: null, typed: editor.HYBRID_RAG, canonical: true } },
        'GET /pipelines/{name}/layout': { body: { layout: null } },
        'GET /services': { body: editor.SERVICES },
        'POST /pipelines/validate': { body: { hash, rendering: null } },
      },
      { build: BUILD },
    );
    show(`#replay/${hash}/q/q1/node/context`);
    expect(await within(main()).findByRole('complementary', { name: 'context' })).toBeTruthy();
    const open = await within(main()).findByRole('link', { name: 'Open in the editor' });
    expect(open.getAttribute('href')).toBe('#editor/hybrid/node/context');
    act(() => {
      window.location.hash = open.getAttribute('href')!;
    });
    expect(await within(main()).findByRole('application', { name: 'Pipeline hybrid' }, { timeout: 5000 })).toBeTruthy();
    expect(await within(main()).findByRole('complementary', { name: 'context' }, { timeout: 5000 })).toBeTruthy();
    expect(window.location.hash).toBe('#editor/hybrid/node/context');
    expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Editor');
  }, 15000);

  it('hands Runs the selection its address carries', async () => {
    const id = (c: string) => c.repeat(64);
    const run = (c: string) => ({ id: id(c), pipeline: id('p'), dataset_version: id('d'), index_version: id('i'), engine_version: '0.0.0', metrics: {}, pipeline_names: [], refused_pipeline_names: [], prefix_of_documents: [], launched_as: null, benchmark_names: [], started_at_ms: null, finished_at_ms: null, metric_families: {}, median_query_latency_nanos: null });
    mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /runs': { body: { runs: [run('1'), run('2'), run('3')], unreadable: [], shapes: {} } } }, { build: BUILD });
    show(`#runs?sel=${id('2')},${id('1')}`);
    await within(main()).findAllByRole('checkbox');
    expect(within(main()).getAllByRole('checkbox').map((b) => (b as HTMLInputElement).checked)).toEqual([true, true, false]);
    expect(within(main()).getByRole('button', { name: /^Compare/ }).textContent).toBe('Compare 2 selected');
  });

  it('hands Pipeline the name its address carries, and draws its matrix', async () => {
    const api = mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /pipelines': { body: { pipelines: [] } }, 'GET /pipelines/{name}/matrix': { body: MATRIX } }, { build: BUILD });
    show(`#pipeline/${PIPELINE}`);
    expect(await within(main()).findByRole('table', { name: `${PIPELINE}: each node on each benchmark` })).toBeTruthy();
    expect(api.requests).toContain(`GET /api/v1/pipelines/${PIPELINE}/matrix?include_available=true`);
  });

  it('opens the last pipeline viewed from the top bar, and the bare screen before any', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /pipelines': { body: { pipelines: [] } }, 'GET /pipelines/{name}/matrix': { body: MATRIX } }, { build: BUILD });
    show('#setup');
    const pipelineLink = () => within(screen.getByRole('navigation', { name: 'Screens' })).getByRole('link', { name: 'Pipeline' });
    expect(pipelineLink().getAttribute('href')).toBe('#pipeline');
    window.location.hash = `#pipeline/${PIPELINE}`;
    await within(main()).findByRole('table', { name: `${PIPELINE}: each node on each benchmark` });
    window.location.hash = '#runs';
    await waitFor(() => expect(pipelineLink().getAttribute('href')).toBe(`#pipeline/${PIPELINE}`));
  });

  it('lists the six screens, widest to narrowest, each a link to its screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    const links = within(screen.getByRole('navigation', { name: 'Screens' })).getAllByRole('link');
    expect(links.map((l) => [l.textContent, l.getAttribute('href')])).toEqual([
      ['Runs', '#runs'],
      ['Pipeline', '#pipeline'],
      ['Compare', '#compare'],
      ['Replay', '#replay'],
      ['Editor', '#editor'],
      ['Setup', '#setup'],
    ]);
    await screen.findByText(WORKSPACE.path);
  });

  it('gives an empty state the action that leads on, as a real link to its screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    const action = within(main()).getByRole('link', { name: 'Open Runs' });
    expect(action.getAttribute('href')).toBe('#runs');
    fireEvent.click(action);
    await waitFor(() => expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Runs'));
  });

  it('moves focus to the new screen’s heading when the route changes, so the change is announced', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    await screen.findByText(WORKSPACE.path);
    // A deep link keeps the browser's own focus: nothing is moved on load.
    expect(document.activeElement).toBe(document.body);
    fireEvent.click(within(main()).getByRole('link', { name: 'Open Runs' }));
    await waitFor(() => expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Runs'));
    const heading = within(main()).getByRole('heading', { level: 1 });
    expect(document.activeElement).toBe(heading);
    expect(heading.getAttribute('tabindex')).toBe('-1');
  });

  it('moves no focus on load under StrictMode either, whose effects run twice on mount', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    window.history.replaceState(null, '', '/#editor');
    render(
      <StrictMode>
        <App client={createApiClient()} build={BUILD} reload={vi.fn()} />
      </StrictMode>,
    );
    await screen.findByText(WORKSPACE.path);
    expect(document.activeElement).toBe(document.body);
    fireEvent.click(within(main()).getByRole('link', { name: 'Open Runs' }));
    await waitFor(() => expect(document.activeElement).toBe(within(main()).getByRole('heading', { level: 1 })));
  });

  describe('state within a screen', () => {
    const id = (c: string) => c.repeat(64);
    const run = (c: string, dataset = id('d')) => ({ id: id(c), pipeline: id('p'), dataset_version: dataset, index_version: id('i'), engine_version: '0.0.0', metrics: {}, pipeline_names: [], refused_pipeline_names: [], prefix_of_documents: [], launched_as: null, benchmark_names: [], started_at_ms: null, finished_at_ms: null, metric_families: {}, median_query_latency_nanos: null });
    const runsRoutes = () =>
      mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /runs': { body: { runs: [run('1'), run('2'), run('3', id('e'))], unreadable: [], shapes: {} } } }, { build: BUILD });
    const row = (c: string) => within(main()).getByRole('row', { name: new RegExp(`^Run ${id(c).slice(0, 12)} on `) });

    it('keeps focus on a row when space selects it, though the selection is written to the address', async () => {
      runsRoutes();
      show('#runs');
      await waitFor(() => row('1'));
      row('1').focus();
      fireEvent.keyDown(row('1'), { key: ' ' });
      await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${id('1')}`));
      await screen.findByText(WORKSPACE.path);
      expect(document.activeElement).toBe(row('1'));
    });

    it('keeps focus on a checkbox when a click selects its run', async () => {
      runsRoutes();
      show('#runs');
      await waitFor(() => row('2'));
      const box = within(row('2')).getByRole('checkbox');
      box.focus();
      fireEvent.click(box);
      await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${id('2')}`));
      expect(document.activeElement).toBe(within(row('2')).getByRole('checkbox'));
    });

    it('moves nothing when a deep link’s selection is corrected in place', async () => {
      runsRoutes();
      show(`#runs?sel=${id('1')},${id('3')}`);
      await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${id('1')}`));
      await screen.findByText(WORKSPACE.path);
      expect(document.activeElement).toBe(document.body);
    });
  });

  it('says so, and offers Runs, for an address that names no screen', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#nowhere');
    expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('No such screen');
    expect(within(main()).getByRole('alert').textContent).toContain('#nowhere');
    expect(within(main()).getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe('#runs');
    await screen.findByText(WORKSPACE.path);
  });
});

describe('Setup', () => {
  it('is handed the workspace the shell read, and shows a failed read in its own sections rather than above the screen', async () => {
    const { requests } = mockApi(
      {
        'GET /workspace': [{ network: 'Failed to fetch' }, { body: WORKSPACE }],
        'GET /benchmarks': { body: { benchmarks: [] } },
        'GET /services': { body: { services: [{ family: 'generator', name: 'qwen', uri: 'http://127.0.0.1:50051', connected: false, identity: null }] } },
      },
      { build: BUILD },
    );
    show('#setup');
    const workspace = await within(main()).findByRole('region', { name: 'Workspace' });
    const alert = await within(workspace).findByRole('alert');
    expect(alert.textContent).toContain('network_failed');
    // Said once per section that needs the workspace, never also above the screen.
    expect(within(main()).getAllByRole('alert').every((a) => a.closest('section') !== null)).toBe(true);
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));
    expect(await within(workspace).findByText(WORKSPACE.path)).toBeTruthy();
    expect(requests.filter((r) => r === 'GET /api/v1/workspace')).toHaveLength(2);
  });
});

describe('the workspace indicator', () => {
  it('says the workspace is being read while GET /workspace is in flight', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    expect(within(screen.getByRole('banner')).getByRole('status').textContent).toBe('Reading the workspace');
    await screen.findByText(WORKSPACE.path);
  });

  it('shows the path, the service count and a reachable dot, from GET /workspace', async () => {
    const { requests } = mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    await screen.findByText(WORKSPACE.path);
    const link = indicator();
    expect(link?.textContent).toBe('/home/ada/ragondin-ws, 2 services');
    expect(link?.getAttribute('data-connected')).toBe('true');
    expect(link?.querySelector('.rg-dot')).toBeTruthy();
    expect(requests).toEqual(['GET /api/v1/workspace']);
  });

  it('counts one service in the singular', async () => {
    mockApi({ 'GET /workspace': { body: { ...WORKSPACE, settings: { ...WORKSPACE.settings, services: WORKSPACE.settings.services.slice(0, 1) } } } }, { build: BUILD });
    show('#editor');
    await screen.findByText(WORKSPACE.path);
    expect(indicator()?.textContent).toBe('/home/ada/ragondin-ws, 1 service');
  });

  it('opens Setup when clicked', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE }, 'GET /runs': { body: { runs: [], unreadable: [], shapes: {} } } }, { build: BUILD });
    show('#runs');
    await screen.findByText(WORKSPACE.path);
    const link = indicator();
    if (link === undefined) throw new Error('no indicator');
    fireEvent.click(link);
    await waitFor(() => expect(within(main()).getByRole('heading', { level: 1 }).textContent).toBe('Setup'));
  });

  it('shows the workspace unreachable, in words, and the failure with a retry, when the request fails', async () => {
    const { requests } = mockApi({ 'GET /workspace': [{ network: 'Failed to fetch' }, { body: WORKSPACE }] }, { build: BUILD });
    show('#editor');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('GET /api/v1/workspace');
    expect(alert.textContent).toContain('network_failed');
    expect(indicator()?.textContent).toBe('Workspace unreachable');
    expect(indicator()?.getAttribute('data-connected')).toBe('false');
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));
    await screen.findByText(WORKSPACE.path);
    expect(within(main()).queryByRole('alert')).toBeNull();
    expect(requests).toHaveLength(2);
  });

  it('renders a problem the API answers, with its code and hint', async () => {
    mockApi(
      {
        'GET /workspace': {
          problem: {
            type: 'urn:ragondin:problem:backend_failed',
            title: 'Backend failed',
            status: 500,
            detail: 'The run store could not be read.',
            code: 'backend_failed',
            hint: 'Check the workspace directory, then retry.',
          },
        },
      },
      { build: BUILD },
    );
    show('#editor');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('The run store could not be read.');
    expect(alert.textContent).toContain('Check the workspace directory, then retry.');
    expect(alert.textContent).toContain('backend_failed');
  });
});

describe('the theme control', () => {
  it('sits in the top bar', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    expect(within(screen.getByRole('banner')).getByRole('radiogroup', { name: 'Theme' })).toBeTruthy();
    await screen.findByText(WORKSPACE.path);
  });
});

describe('the build identity handshake', () => {
  it('continues without reloading when the server is this build', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    const { reload } = show('#editor');
    await screen.findByText(WORKSPACE.path);
    expect(reload).not.toHaveBeenCalled();
  });

  it('judges the identity a problem answer carries too', async () => {
    mockApi(
      {
        'GET /workspace': {
          problem: {
            type: 'urn:ragondin:problem:backend_failed',
            title: 'Backend failed',
            status: 500,
            detail: 'The run store could not be read.',
            code: 'backend_failed',
            hint: 'Check the workspace directory, then retry.',
          },
        },
      },
      { build: '0.0.0+bbbbbbbbbbbb' },
    );
    const { reload } = show('#editor');
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  });

  it('reloads once when another build answers, then refuses, naming both builds', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: '0.0.0+bbbbbbbbbbbb' });
    const first = show('#editor');
    await waitFor(() => expect(first.reload).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(WORKSPACE.path)).toBeNull();
    first.unmount();

    // The reload fetched the same page again, and the same other build answers.
    const second = show('#editor');
    const alert = await within(main()).findByRole('alert');
    expect(alert.textContent).toContain('0.0.0+aaaaaaaaaaaa');
    expect(alert.textContent).toContain('0.0.0+bbbbbbbbbbbb');
    expect(alert.textContent).toContain('build_mismatch');
    expect(second.reload).not.toHaveBeenCalled();
  });
});

describe('the connection state', () => {
  beforeEach(() => {
    installFakeEventSource();
  });

  it('shows the stream connected, then "disconnected — retrying" while it is down, and never current meanwhile', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor', { followJobs: true });
    await screen.findByText(WORKSPACE.path);
    const banner = within(screen.getByRole('banner'));
    expect(banner.getByText('connecting')).toBeTruthy();
    act(() => FakeEventSource.latest().open());
    expect(banner.getByText('connected').closest('[data-connected]')?.getAttribute('data-connected')).toBe('true');
    act(() => FakeEventSource.latest().fail());
    expect(banner.getByText('disconnected — retrying').closest('[data-connected]')?.getAttribute('data-connected')).toBe('false');
    expect(banner.queryByText('connected')).toBeNull();
  });

  it('re-checks the build identity when the stream reconnects, and reloads if the server became another build', async () => {
    const { requests } = mockApi({ 'GET /workspace': [{ body: WORKSPACE }, { body: WORKSPACE, build: '0.0.0+cccccccccccc' }] }, { build: BUILD });
    const { reload } = show('#editor', { followJobs: true });
    await screen.findByText(WORKSPACE.path);
    act(() => FakeEventSource.latest().open());
    act(() => FakeEventSource.latest().fail());
    act(() => FakeEventSource.latest().open());
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
    expect(requests).toEqual(['GET /api/v1/workspace', 'GET /api/v1/workspace']);
  });

  it('closes its stream when the shell unmounts', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    const { unmount } = show('#editor', { followJobs: true });
    await screen.findByText(WORKSPACE.path);
    unmount();
    expect(FakeEventSource.latest().closed).toBe(true);
  });

  it('neither reads the workspace again nor reopens the stream when it re-renders with a new reload function', async () => {
    const { requests } = mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    const { rerender, client } = show('#editor', { followJobs: true });
    await screen.findByText(WORKSPACE.path);
    rerender(<App client={client} build={BUILD} reload={vi.fn()} followJobs />);
    await act(async () => {});
    expect(requests).toEqual(['GET /api/v1/workspace']);
    expect(FakeEventSource.instances).toHaveLength(1);
  });

  it('follows GET /jobs/events, once for the whole page, and raises a run’s outcome as a toast on whatever screen is shown', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor', { followJobs: true });
    await screen.findByText(WORKSPACE.path);
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.latest().url).toBe('/api/v1/jobs/events');
    const job = (state: JobSummary['state']): JobSummary => ({ id: 'j1', created_at_ms: 1, position: 0, state, work: { kind: 'run', pipeline: 'hybrid', benchmark: 'beir/scifact', run_id: 'a'.repeat(64), up_to: null, parent_pipeline_hash: null, bindings: [] }, faults: [] });
    act(() => FakeEventSource.latest().open());
    act(() => FakeEventSource.latest().emit(JSON.stringify({ jobs: [job({ kind: 'running', done: 1, total: 2, started_at_ms: 1, median_latency_nanos: null })], faults: [] }), 'resync'));
    act(() => FakeEventSource.latest().emit(JSON.stringify(job({ kind: 'failed', at_node: 'rerank', error: 'boom', finished_at_ms: 2, partial_traces: 0 })), 'failed'));
    const toast = within(screen.getByRole('region', { name: 'Notifications' })).getByRole('alert');
    expect(toast.textContent).toContain('Run failed at rerank');
  });

  it('shows no connection state when no stream is open', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE } }, { build: BUILD });
    show('#editor');
    await screen.findByText(WORKSPACE.path);
    expect(FakeEventSource.instances).toHaveLength(0);
    expect(screen.queryByText(/connect/)).toBeNull();
  });
});

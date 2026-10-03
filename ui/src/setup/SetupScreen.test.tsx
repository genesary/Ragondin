/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App.tsx';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { BenchmarkEntry, Problem, ServiceListing, ServiceStatus, Workspace } from '../api/types.ts';
import type { SetupSection } from '../routes.ts';
import type { RequestState } from '../shell/states.tsx';
import { SetupScreen } from './SetupScreen.tsx';

const hex = (c: string) => c.repeat(64);
const BUILD = '0.1.0+aaaaaaaaaaaa';

const WORKSPACE: Workspace = {
  path: '/home/ada/ragondin-ws',
  build: BUILD,
  settings: { datasets: '/home/ada/ragondin-ws/datasets', services: [] },
  capabilities: {
    families: [
      { family: 'retriever', local: ['bm25', 'dense'] },
      { family: 'fusion', local: ['rrf'] },
      { family: 'reranker', local: ['cross_encoder'] },
      { family: 'context_builder', local: ['concat'] },
      { family: 'generator', local: [] },
      { family: 'embedder', local: ['onnx'] },
    ],
    remote: true,
  },
  counts: { pipelines: 3, runs: 12, benchmarks_ready: 2, services_connected: 0 },
};

const entry = (name: string, state: BenchmarkEntry['state'], over: Partial<BenchmarkEntry> = {}): BenchmarkEntry => ({
  name,
  format: name.split('/')[0] ?? 'beir',
  ground_truth: null,
  licence: 'CC-BY-SA-4.0',
  licence_url: null,
  state,
  ...over,
});

const READY = entry('beir/scifact', { kind: 'ready', dataset_version: hex('5') }, { ground_truth: 'qrels' });
const AVAILABLE = entry('beir/fiqa', { kind: 'available', size_bytes: 17_100_000 }, { licence_url: 'https://example.org/fiqa-terms' });
const DIFFERS = entry('beir/nfcorpus', { kind: 'differs', expected: hex('a'), found: hex('b') }, { ground_truth: 'qrels' });
const UNREADABLE = entry('squad/squad', { kind: 'unreadable', error: 'corpus.jsonl line 4: expected `_id`' });
const LOCAL = entry('mine', { kind: 'local', dataset_version: hex('c') }, { format: 'beir-qa', ground_truth: 'both', licence: null });
const EVERY_STATE = [READY, AVAILABLE, DIFFERS, UNREADABLE, LOCAL];

const service = (family: string, name: string, uri: string, over: Partial<ServiceStatus> = {}): ServiceStatus => ({ family, name, uri, connected: false, identity: null, ...over });
const QWEN = service('generator', 'qwen', 'http://127.0.0.1:8080');

const problem = (code: Problem['code'], status: number, detail: string, hint = 'Correct it, then try again.'): { problem: Problem } => ({
  problem: { type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint },
});

/** What the shell hands the screen: the workspace it read, and the two ways to read it again. */
function Harness({ workspace = { status: 'loaded', value: WORKSPACE }, section, refresh = () => {} }: { workspace?: RequestState<Workspace>; section?: SetupSection; refresh?: () => void }) {
  const [client] = useState(() => createApiClient());
  return <SetupScreen client={client} workspace={workspace} refreshWorkspace={refresh} retryWorkspace={refresh} section={section} />;
}

const routes = (over: MockRoutes = {}): MockRoutes => ({
  'GET /benchmarks': { body: { benchmarks: EVERY_STATE } },
  'GET /services': { body: { services: [QWEN] } },
  ...over,
});

const region = (name: string) => screen.getByRole('region', { name });
const row = (name: string) => {
  const cell = within(region('Benchmarks')).getByText(name, { selector: 'th *, td *, th, td' });
  const tr = cell.closest('tr');
  if (tr === null) throw new Error(`no row for ${name}`);
  return tr;
};

beforeEach(() => {
  window.history.replaceState(null, '', '/#setup');
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the benchmarks', () => {
  it('benchmarks render each registry state with its label and actions', async () => {
    mockApi(routes());
    render(<Harness />);
    await within(await screen.findByRole('region', { name: 'Benchmarks' })).findByText('beir/scifact');

    const ready = row('beir/scifact');
    expect(within(ready).getByText('ready').closest('.rg-status')?.getAttribute('data-state')).toBe('done');
    expect(ready.textContent).toContain('verified');
    expect(within(ready).getByText(hex('5').slice(0, 12)).getAttribute('title')).toBe(hex('5'));
    expect(ready.textContent).toContain('qrels');
    expect(within(ready).queryByRole('button', { name: 'Download' })).toBeNull();

    const available = row('beir/fiqa');
    expect(within(available).getByText('available')).toBeTruthy();
    expect(available.textContent).toContain('17.1 MB');
    expect(available.textContent).toContain('CC-BY-SA-4.0');
    const download = within(available).getByRole('button', { name: 'Download' });
    expect(download.getAttribute('aria-disabled')).toBe('true');
    expect(available.textContent).toContain('Downloads arrive with the job queue');

    const unreadable = row('squad/squad');
    expect(within(unreadable).getByText('unreadable').closest('.rg-status')?.getAttribute('data-state')).toBe('failed');
    expect(unreadable.textContent).toContain('corpus.jsonl line 4: expected `_id`');

    const local = row('mine');
    expect(within(local).getByText('local')).toBeTruthy();
    expect(local.textContent).toContain(hex('c').slice(0, 12));
    expect(local.textContent).toContain('qrels and reference answers');
  });

  it('an available benchmark shows its licence before its download button', async () => {
    mockApi(routes());
    render(<Harness />);
    await within(await screen.findByRole('region', { name: 'Benchmarks' })).findByText('beir/fiqa');
    const available = row('beir/fiqa');
    const licence = within(available).getByRole('link', { name: 'CC-BY-SA-4.0' });
    expect(licence.getAttribute('href')).toBe('https://example.org/fiqa-terms');
    const download = within(available).getByRole('button', { name: 'Download' });
    expect(licence.compareDocumentPosition(download) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('a differing dataset is not called ready and shows both digests', async () => {
    mockApi(routes());
    render(<Harness />);
    await within(await screen.findByRole('region', { name: 'Benchmarks' })).findByText('beir/nfcorpus');
    const differs = row('beir/nfcorpus');
    expect(within(differs).getByText('differs').closest('.rg-status')?.getAttribute('data-state')).toBe('warning');
    expect(differs.textContent).not.toContain('ready');
    expect(within(differs).getByText(hex('a').slice(0, 12)).getAttribute('title')).toBe(hex('a'));
    expect(within(differs).getByText(hex('b').slice(0, 12)).getAttribute('title')).toBe(hex('b'));
    expect(differs.textContent).toContain('not the one the manifest pins');
    expect(within(differs).queryByRole('button', { name: 'Download' })).toBeNull();
  });

  it('import shows the adapter error inline or adds a local row', async () => {
    const imported = entry('notes', { kind: 'local', dataset_version: hex('d') }, { licence: null, ground_truth: 'qrels' });
    const api = mockApi(
      routes({
        'POST /benchmarks/import': [problem('import_refused', 422, 'qrels/test.tsv: no such file in /data/notes'), { body: imported }],
      }),
    );
    const refresh = vi.fn();
    render(<Harness refresh={refresh} />);
    const benchmarks = within(await screen.findByRole('region', { name: 'Benchmarks' }));
    await benchmarks.findByText('beir/scifact');

    fireEvent.change(benchmarks.getByLabelText('Corpus directory'), { target: { value: '/data/notes' } });
    fireEvent.change(benchmarks.getByLabelText('Import as'), { target: { value: 'notes' } });
    fireEvent.click(benchmarks.getByRole('button', { name: 'Import' }));

    const path = benchmarks.getByLabelText('Corpus directory');
    await waitFor(() => expect(path.getAttribute('aria-invalid')).toBe('true'));
    const help = document.getElementById(path.getAttribute('aria-describedby') ?? '');
    expect(help?.textContent).toContain('qrels/test.tsv: no such file in /data/notes');
    expect(benchmarks.queryByText('notes', { selector: 'td, td *' })).toBeNull();

    fireEvent.click(benchmarks.getByRole('button', { name: 'Import' }));
    expect(await benchmarks.findByText('notes', { selector: 'td, td *' })).toBeTruthy();
    expect(within(row('notes')).getByText('local')).toBeTruthy();
    expect(path.getAttribute('aria-invalid')).toBeNull();
    expect(api.bodies[api.requests.indexOf('POST /api/v1/benchmarks/import')]).toEqual({ path: '/data/notes', name: 'notes' });
    expect(refresh).toHaveBeenCalled();
  });
});

describe('the first launch', () => {
  const empty = routes({ 'GET /benchmarks': { body: { benchmarks: [] } }, 'GET /services': { body: { services: [] } } });

  it('the first launch state shows two steps on an empty workspace', async () => {
    mockApi(empty);
    render(<Harness />);
    const start = await screen.findByRole('region', { name: 'Get started' });
    const steps = within(start).getAllByRole('listitem').filter((li) => li.parentElement?.tagName === 'OL');
    expect(steps.map((s) => within(s).getByRole('heading').textContent)).toEqual(['Add a benchmark', 'Connect a generator, if your pipeline ends in an answer']);
    expect(start.textContent).toContain('A retrieval-only pipeline needs no service');
    // The workspace line stays, with how to change it.
    expect(within(region('Workspace')).getByText(WORKSPACE.path)).toBeTruthy();
    expect(region('Workspace').textContent).toContain('ragondin ui --workspace');
    expect(screen.queryByRole('region', { name: 'Benchmarks' })).toBeNull();
    expect(screen.queryByRole('region', { name: 'Services' })).toBeNull();
  });

  it('points at the smallest benchmark the manifest pins, read from its size', async () => {
    const big = entry('beir/trec-covid', { kind: 'available', size_bytes: 230_000_000 });
    const small = entry('beir/scifact', { kind: 'available', size_bytes: 5_200_000 });
    mockApi(routes({ 'GET /benchmarks': { body: { benchmarks: [big, small] } }, 'GET /services': { body: { services: [] } } }));
    render(<Harness />);
    const start = await screen.findByRole('region', { name: 'Get started' });
    const first = within(start).getAllByRole('listitem')[0] as HTMLElement;
    expect(first.textContent).toContain('beir/scifact');
    expect(first.textContent).toContain('5.2 MB');
    expect(first.textContent).toContain('the smallest, a good first run');
    expect(first.textContent).not.toContain('beir/trec-covid');
    expect(within(first).getByRole('button', { name: 'Download' }).getAttribute('aria-disabled')).toBe('true');
  });

  it('adding a benchmark leaves the first launch state', async () => {
    const imported = entry('notes', { kind: 'local', dataset_version: hex('d') }, { licence: null });
    mockApi({ ...empty, 'POST /benchmarks/import': { body: imported } });
    render(<Harness />);
    const start = within(await screen.findByRole('region', { name: 'Get started' }));
    fireEvent.change(start.getByLabelText('Corpus directory'), { target: { value: '/data/notes' } });
    fireEvent.change(start.getByLabelText('Import as'), { target: { value: 'notes' } });
    fireEvent.click(start.getByRole('button', { name: 'Import' }));

    await screen.findByRole('region', { name: 'Benchmarks' });
    expect(screen.queryByRole('region', { name: 'Get started' })).toBeNull();
    expect(within(region('Benchmarks')).getByText('notes')).toBeTruthy();
    // The form that had focus is gone with the invitation: focus lands on the section the benchmark joined.
    await waitFor(() => expect(document.activeElement).toBe(region('Benchmarks')));
    expect(region('Services')).toBeTruthy();
    expect(region('This build')).toBeTruthy();
  });
});

describe('the services', () => {
  it('connect stores the service then probes it and shows the identity', async () => {
    const stored: ServiceListing = { services: [QWEN, service('reranker', 'bge', 'http://127.0.0.1:9090')] };
    const api = mockApi(
      routes({
        'PUT /services/{family}/{name}': { body: stored },
        'POST /services/{family}/{name}/probe': { body: { identity: 'bge-reranker-v2-m3@sha256:1f2e' } },
      }),
    );
    const refresh = vi.fn();
    render(<Harness refresh={refresh} />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    await services.findByRole('listitem', { name: 'generator/qwen' });

    const form = within(services.getByRole('form', { name: 'Connect a service' }));
    fireEvent.change(form.getByLabelText('Family'), { target: { value: 'reranker' } });
    fireEvent.change(form.getByLabelText('Name'), { target: { value: 'bge' } });
    fireEvent.change(form.getByLabelText('Address'), { target: { value: 'http://127.0.0.1:9090' } });
    fireEvent.change(form.getByLabelText('Served model'), { target: { value: 'bge-reranker-v2-m3' } });
    fireEvent.click(form.getByRole('button', { name: 'Connect' }));

    const bge = await services.findByRole('listitem', { name: 'reranker/bge' });
    await waitFor(() => expect(within(bge).getByText('connected')).toBeTruthy());
    expect(bge.textContent).toContain('bge-reranker-v2-m3@sha256:1f2e');
    expect(within(bge).getByText('http://127.0.0.1:9090').tagName).toBe('CODE');
    expect(bge.textContent).toMatch(/read at \d{1,2}:\d{2}/);
    expect(api.requests.filter((r) => !r.startsWith('GET'))).toEqual(['PUT /api/v1/services/reranker/bge', 'POST /api/v1/services/reranker/bge/probe']);
    expect(api.bodies[api.requests.indexOf('PUT /api/v1/services/reranker/bge')]).toEqual({ uri: 'http://127.0.0.1:9090' });
    expect(api.bodies[api.requests.indexOf('POST /api/v1/services/reranker/bge/probe')]).toEqual({ served_model: 'bge-reranker-v2-m3' });
    // Focus comes back to the row the probe answered for.
    expect(document.activeElement).toBe(bge);
    expect(refresh).toHaveBeenCalled();
  });

  it('an unreachable probe is an inline message on the row not a toast', async () => {
    mockApi(
      routes({
        'POST /services/{family}/{name}/probe': problem('service_unreachable', 502, 'http://127.0.0.1:8080 did not answer: connection refused', 'Start the service, or bind the name to the address it answers at.'),
      }),
    );
    render(<Harness />);
    const qwen = await within(await screen.findByRole('region', { name: 'Services' })).findByRole('listitem', { name: 'generator/qwen' });
    const test = within(qwen).getByRole('button', { name: 'Test' });
    test.focus();
    fireEvent.click(test);

    const alert = await within(qwen).findByRole('alert');
    expect(alert.textContent).toContain('http://127.0.0.1:8080 did not answer: connection refused');
    expect(alert.textContent).toContain('Start the service');
    expect(alert.textContent).toContain('service_unreachable');
    expect(within(qwen).getByText('unreachable').closest('.rg-status')?.getAttribute('data-state')).toBe('failed');
    expect(document.querySelector('.rg-toast')).toBeNull();
    expect(document.activeElement).toBe(test);
  });

  it('a refused service write shows the server message under the field', async () => {
    const words = '`--remote generator/qwen=ftp://host` has a uri whose scheme is not `http`';
    mockApi(routes({ 'PUT /services/{family}/{name}': problem('binding_refused', 422, words) }));
    render(<Harness />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    await services.findByRole('listitem', { name: 'generator/qwen' });
    const form = within(services.getByRole('form', { name: 'Connect a service' }));
    fireEvent.change(form.getByLabelText('Family'), { target: { value: 'generator' } });
    fireEvent.change(form.getByLabelText('Name'), { target: { value: 'qwen' } });
    fireEvent.change(form.getByLabelText('Address'), { target: { value: 'ftp://host' } });
    fireEvent.click(form.getByRole('button', { name: 'Connect' }));

    const address = form.getByLabelText('Address');
    await waitFor(() => expect(address.getAttribute('aria-invalid')).toBe('true'));
    expect(document.getElementById(address.getAttribute('aria-describedby') ?? '')?.textContent).toContain(words);
  });

  it('marks a binding the server has read before as connected, and says the time is this page’s only', async () => {
    mockApi(routes({ 'GET /services': { body: { services: [service('generator', 'qwen', 'http://127.0.0.1:8080', { connected: true, identity: 'qwen2.5-7b-instruct' })] } } }));
    render(<Harness />);
    const services = await screen.findByRole('region', { name: 'Services' });
    const qwen = await within(services).findByRole('listitem', { name: 'generator/qwen' });
    expect(within(qwen).getByText('connected')).toBeTruthy();
    expect(qwen.textContent).toContain('qwen2.5-7b-instruct');
    expect(qwen.textContent).toContain('read before this page was opened');
    expect(services.textContent).toContain('The address never enters a pipeline. A run records which address answered, as provenance — two runs with different addresses and the same identity are one experiment run twice.');
    expect(services.textContent).toContain('since this page was opened');
  });

  it('removes a binding at once, and offers to undo it', async () => {
    const api = mockApi(
      routes({
        'DELETE /services/{family}/{name}': { body: { services: [] } },
        'PUT /services/{family}/{name}': (body) => ({ body: { services: [service('generator', 'qwen', body.uri)] } }),
      }),
    );
    render(<Harness />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    const qwen = await services.findByRole('listitem', { name: 'generator/qwen' });
    fireEvent.click(within(qwen).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(services.queryByRole('listitem', { name: 'generator/qwen' })).toBeNull());
    // The message that offers Undo takes the row's place in the list, so nothing below it moves.
    const slot = services.getByRole('listitem', { name: 'generator/qwen, removed' });
    const undo = within(slot).getByRole('button', { name: 'Undo' });
    await waitFor(() => expect(document.activeElement).toBe(undo));
    fireEvent.click(undo);
    await services.findByRole('listitem', { name: 'generator/qwen' });
    expect(api.bodies[api.requests.lastIndexOf('PUT /api/v1/services/generator/qwen')]).toEqual({ uri: 'http://127.0.0.1:8080' });
  });

  it('says, beside the Connect form, that a build without remote stores a binding and refuses to test it', async () => {
    mockApi(routes());
    render(<Harness workspace={{ status: 'loaded', value: { ...WORKSPACE, capabilities: { ...WORKSPACE.capabilities, remote: false } } }} />);
    const services = await screen.findByRole('region', { name: 'Services' });
    expect(services.textContent).toContain('This build has no remote feature');
  });
});

describe('this build', () => {
  it('shows the version, the commit, whether remote is on, and the Local implementations per family', async () => {
    mockApi(routes());
    render(<Harness />);
    const build = within(await screen.findByRole('region', { name: 'This build' }));
    expect(build.getByText('0.1.0')).toBeTruthy();
    expect(build.getByText('aaaaaaaaaaaa')).toBeTruthy();
    expect(build.getByText('on')).toBeTruthy();
    const retriever = build.getByText('retriever').closest('tr');
    expect(retriever?.textContent).toContain('bm25, dense');
    expect(build.getByText('generator').closest('tr')?.textContent).toContain('none in this build');
    expect(region('This build').textContent).toContain('Adding a Local component is a rebuild, not a setting.');
  });
});

describe('the states', () => {
  it('a failed section shows retry while the others render', async () => {
    const api = mockApi(routes({ 'GET /benchmarks': [problem('backend_failed', 500, 'The datasets directory could not be listed.'), { body: { benchmarks: EVERY_STATE } }] }));
    render(<Harness />);
    const benchmarks = within(await screen.findByRole('region', { name: 'Benchmarks' }));
    const alert = await benchmarks.findByRole('alert');
    expect(alert.textContent).toContain('The datasets directory could not be listed.');
    expect(within(region('Services')).getByRole('listitem', { name: 'generator/qwen' })).toBeTruthy();
    expect(within(region('Workspace')).getByText(WORKSPACE.path)).toBeTruthy();
    expect(within(region('This build')).getByText('0.1.0')).toBeTruthy();

    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));
    expect(await benchmarks.findByText('beir/scifact')).toBeTruthy();
    expect(api.requests.filter((r) => r === 'GET /api/v1/benchmarks')).toHaveLength(2);
  });

  it('says what it is reading while it reads, in words', () => {
    mockApi(routes());
    render(<Harness />);
    expect(screen.getByText('Reading benchmarks and services').getAttribute('role')).toBe('status');
  });

  it('shows the workspace failure in the sections that need it, with Retry', async () => {
    mockApi(routes());
    const retry = vi.fn();
    render(<Harness workspace={{ status: 'error', problem: { code: 'network_failed', message: 'GET /api/v1/workspace got no answer.', hint: 'Check that ragondin ui is running.', location: null, status: null } }} refresh={retry} />);
    const workspace = within(region('Workspace'));
    fireEvent.click(within(workspace.getByRole('alert')).getByRole('button', { name: 'Retry' }));
    expect(retry).toHaveBeenCalled();
    await within(await screen.findByRole('region', { name: 'Services' })).findByRole('listitem', { name: 'generator/qwen' });
  });
});

describe('the address', () => {
  it('the services anchor focuses the services section', async () => {
    mockApi({ 'GET /workspace': { body: WORKSPACE }, ...routes() }, { build: BUILD });
    window.history.replaceState(null, '', '/#setup/services');
    render(<App client={createApiClient()} build={BUILD} reload={() => {}} />);
    const services = await screen.findByRole('region', { name: 'Services' });
    await waitFor(() => expect(document.activeElement).toBe(services));

    // Moving to another section is a move within the page: that section takes focus, not the heading.
    act(() => {
      window.location.hash = '#setup/benchmarks';
    });
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('region', { name: 'Benchmarks' })));
  });
});

describe('the page contract', () => {
  it('the screen uses no browser dialog', () => {
    const sources = import.meta.glob(['./*.tsx', './*.ts', '!./*.test.tsx'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    expect(Object.keys(sources).length).toBeGreaterThan(0);
    for (const [file, text] of Object.entries(sources)) {
      expect(text, file).not.toMatch(/\b(prompt|confirm|alert)\s*\(/);
    }
  });
});

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
import { SetupScreen, UNDO_WINDOW_MS } from './SetupScreen.tsx';

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
    expect(help?.textContent).toContain('Correct it, then try again.');
    // Focus moves to the field the refusal describes, so its words are read.
    expect(document.activeElement).toBe(path);
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
    expect(within(bge).getByText('http://127.0.0.1:9090', { selector: '.rg-setup__service-head code' }).tagName).toBe('CODE');
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
    const test = within(qwen).getByRole('button', { name: 'Test generator/qwen' });
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
    const help = document.getElementById(address.getAttribute('aria-describedby') ?? '');
    expect(help?.textContent).toContain(words);
    expect(help?.textContent).toContain('Correct it, then try again.');
    expect(document.activeElement).toBe(address);
  });

  it('sends the family the form shows, though the workspace answered after the form was drawn', async () => {
    const api = mockApi(routes({ 'PUT /services/{family}/{name}': { body: { services: [QWEN] } }, 'POST /services/{family}/{name}/probe': { body: { identity: 'x' } } }));
    const view = render(<Harness workspace={{ status: 'loading' }} />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    await services.findByRole('listitem', { name: 'generator/qwen' });
    view.rerender(<Harness workspace={{ status: 'loaded', value: WORKSPACE }} />);
    const form = within(services.getByRole('form', { name: 'Connect a service' }));
    expect((form.getByLabelText('Family') as HTMLSelectElement).value).toBe('retriever');
    fireEvent.change(form.getByLabelText('Name'), { target: { value: 'q2' } });
    fireEvent.change(form.getByLabelText('Address'), { target: { value: 'http://127.0.0.1:7070' } });
    fireEvent.click(form.getByRole('button', { name: 'Connect' }));
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/services/retriever/q2'));
  });

  it('shows the binding as its row at once, while it is stored and tested', async () => {
    let release: (reply: { body: ServiceListing }) => void = () => {};
    mockApi(routes({ 'PUT /services/{family}/{name}': () => new Promise((resolve) => (release = resolve)), 'POST /services/{family}/{name}/probe': { body: { identity: 'bge@1' } } }));
    render(<Harness />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    await services.findByRole('listitem', { name: 'generator/qwen' });
    const form = within(services.getByRole('form', { name: 'Connect a service' }));
    fireEvent.change(form.getByLabelText('Family'), { target: { value: 'reranker' } });
    fireEvent.change(form.getByLabelText('Name'), { target: { value: 'bge' } });
    fireEvent.change(form.getByLabelText('Address'), { target: { value: 'http://127.0.0.1:9090' } });
    fireEvent.click(form.getByRole('button', { name: 'Connect' }));
    // Drawn with the click, so the answer arriving later moves nothing.
    const pending = await services.findByRole('listitem', { name: 'reranker/bge' });
    // Pending, and saying so: not yet a stored binding.
    expect(within(pending).getByText('connecting').closest('.rg-status')).toBeTruthy();
    expect(within(pending).getByRole('status').textContent).toContain('Storing reranker/bge in workspace.toml…');
    expect(pending.textContent).not.toContain('Not tested');
    expect(pending.getAttribute('aria-busy')).toBe('true');
    for (const name of ['Test reranker/bge', 'Remove reranker/bge']) expect(within(pending).getByRole('button', { name }).getAttribute('aria-disabled')).toBe('true');
    await act(async () => release({ body: { services: [QWEN, service('reranker', 'bge', 'http://127.0.0.1:9090')] } }));
    await waitFor(() => expect(within(services.getByRole('listitem', { name: 'reranker/bge' })).getByRole('status').textContent).toContain('Connected · bge@1'));
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

  it('removes a binding from the list at once, and Undo puts it back where it was, having written nothing', async () => {
    const BGE = service('reranker', 'bge', 'http://127.0.0.1:9090');
    const api = mockApi(routes({ 'GET /services': { body: { services: [QWEN, BGE] } } }));
    render(<Harness />);
    const services = within(await screen.findByRole('region', { name: 'Services' }));
    const qwen = await services.findByRole('listitem', { name: 'generator/qwen' });
    fireEvent.click(within(qwen).getByRole('button', { name: 'Remove generator/qwen' }));
    // The message that offers Undo takes the row's place in the list, so nothing below it moves.
    const slot = services.getByRole('listitem', { name: 'generator/qwen, removed' });
    expect(services.getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual(['generator/qwen, removed', 'reranker/bge']);
    const undo = within(slot).getByRole('button', { name: 'Undo' });
    await waitFor(() => expect(document.activeElement).toBe(undo));
    const description = document.getElementById(undo.getAttribute('aria-describedby') ?? '')?.textContent ?? '';
    expect(description).toContain('Removed generator/qwen.');
    expect(description).not.toContain('Undo');
    fireEvent.click(undo);
    expect(services.getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual(['generator/qwen', 'reranker/bge']);
    expect(api.requests.filter((r) => !r.startsWith('GET'))).toEqual([]);
  });

  describe('the undo window', () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('writes the removal once the window closes, and keeps the row’s place saying so', async () => {
      const api = mockApi(routes({ 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      expect(api.requests).not.toContain('DELETE /api/v1/services/generator/qwen');
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/generator/qwen'));
      const slot = services.getByRole('listitem', { name: 'generator/qwen, removed' });
      await waitFor(() => expect(within(slot).queryByRole('button', { name: 'Undo' })).toBeNull());
      expect(slot.textContent).toContain('Removed generator/qwen.');
    });

    it('moves focus from Undo to the removed binding’s place when the window closes', async () => {
      mockApi(routes({ 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      const slot = services.getByRole('listitem', { name: 'generator/qwen, removed' });
      await waitFor(() => expect(document.activeElement).toBe(within(slot).getByRole('button', { name: 'Undo' })));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      await waitFor(() => expect(within(slot).queryByRole('button', { name: 'Undo' })).toBeNull());
      expect(document.activeElement).toBe(slot);
    });

    it('does not delete a binding whose address changed meanwhile, and says why', async () => {
      const moved = service('generator', 'qwen', 'http://127.0.0.1:8181');
      const api = mockApi(routes({ 'GET /services': [{ body: { services: [QWEN] } }, { body: { services: [moved] } }], 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      const slot = await services.findByRole('listitem', { name: 'generator/qwen, not removed' });
      expect(slot.textContent).toContain('http://127.0.0.1:8181');
      expect(api.requests.filter((r) => r.startsWith('DELETE'))).toEqual([]);
      expect(within(services.getByRole('listitem', { name: 'generator/qwen' })).getByText('http://127.0.0.1:8181', { selector: '.rg-setup__service-head code' })).toBeTruthy();
    });

    it('keeps the removal pending when binding the same name again is refused', async () => {
      const api = mockApi(routes({ 'PUT /services/{family}/{name}': problem('binding_refused', 422, 'the uri is not http'), 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      const form = within(services.getByRole('form', { name: 'Connect a service' }));
      fireEvent.change(form.getByLabelText('Family'), { target: { value: 'generator' } });
      fireEvent.change(form.getByLabelText('Name'), { target: { value: 'qwen' } });
      fireEvent.change(form.getByLabelText('Address'), { target: { value: 'ftp://h' } });
      fireEvent.click(form.getByRole('button', { name: 'Connect' }));
      await waitFor(() => expect(form.getByLabelText('Address').getAttribute('aria-invalid')).toBe('true'));
      // The window is open again: its Undo still brings the row back.
      fireEvent.click(within(services.getByRole('listitem', { name: 'generator/qwen, removed' })).getByRole('button', { name: 'Undo' }));
      expect(services.getByRole('listitem', { name: 'generator/qwen' })).toBeTruthy();
      expect(api.requests.filter((r) => r.startsWith('DELETE'))).toEqual([]);
    });

    it('writes the removal when the window closes after a refused re-bind', async () => {
      const api = mockApi(routes({ 'PUT /services/{family}/{name}': problem('binding_refused', 422, 'the uri is not http'), 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      const form = within(services.getByRole('form', { name: 'Connect a service' }));
      fireEvent.change(form.getByLabelText('Family'), { target: { value: 'generator' } });
      fireEvent.change(form.getByLabelText('Name'), { target: { value: 'qwen' } });
      fireEvent.change(form.getByLabelText('Address'), { target: { value: 'ftp://h' } });
      fireEvent.click(form.getByRole('button', { name: 'Connect' }));
      await waitFor(() => expect(form.getByLabelText('Address').getAttribute('aria-invalid')).toBe('true'));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/generator/qwen'));
    });

    const rebind = async (services: ReturnType<typeof within>, uri: string) => {
      const form = within(services.getByRole('form', { name: 'Connect a service' }));
      fireEvent.change(form.getByLabelText('Family'), { target: { value: 'reranker' } });
      fireEvent.change(form.getByLabelText('Name'), { target: { value: 'bge' } });
      fireEvent.change(form.getByLabelText('Address'), { target: { value: uri } });
      fireEvent.click(form.getByRole('button', { name: 'Connect' }));
      return form;
    };
    const BGE = service('reranker', 'bge', 'http://127.0.0.1:9090');
    const E5 = service('embedder', 'e5', 'http://127.0.0.1:9191');
    const refused = problem('binding_refused', 422, 'the uri is not http');

    it('writes a paused removal through its check when another binding is removed during the re-bind', async () => {
      let refuse: (reply: typeof refused) => void = () => {};
      const api = mockApi(
        routes({
          'GET /services': { body: { services: [BGE, E5] } },
          'PUT /services/{family}/{name}': () => new Promise((resolve) => (refuse = resolve)),
          'DELETE /services/{family}/{name}': { body: { services: [E5] } },
        }),
      );
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'reranker/bge' })).getByRole('button', { name: 'Remove reranker/bge' }));
      const form = await rebind(services, 'ftp://h');
      await waitFor(() => expect(api.requests).toContain('PUT /api/v1/services/reranker/bge'));
      fireEvent.click(within(services.getByRole('listitem', { name: 'embedder/e5' })).getByRole('button', { name: 'Remove embedder/e5' }));
      // The paused removal is written now, through the check: still bound where it was removed, so it is deleted.
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/reranker/bge'));
      await act(async () => refuse(refused));
      await waitFor(() => expect(form.getByLabelText('Address').getAttribute('aria-invalid')).toBe('true'));
      expect(api.requests.filter((r) => r.startsWith('DELETE'))).toEqual(['DELETE /api/v1/services/reranker/bge']);
      const bge = services.getByRole('listitem', { name: 'reranker/bge, removed' });
      expect(within(bge).queryByRole('button', { name: 'Undo' })).toBeNull();
      expect(within(services.getByRole('listitem', { name: 'embedder/e5, removed' })).getByRole('button', { name: 'Undo' })).toBeTruthy();
      expect(services.queryByRole('listitem', { name: 'reranker/bge' })).toBeNull();
      // Nothing is left to fire for bge.
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS * 2);
      });
      expect(api.requests.filter((r) => r === 'DELETE /api/v1/services/reranker/bge')).toHaveLength(1);
    });

    it('writes a paused removal through its check when the page is left during the re-bind, and leaves no timer behind', async () => {
      let refuse: (reply: typeof refused) => void = () => {};
      const api = mockApi(
        routes({
          'GET /services': { body: { services: [BGE, E5] } },
          'PUT /services/{family}/{name}': () => new Promise((resolve) => (refuse = resolve)),
          'DELETE /services/{family}/{name}': { body: { services: [E5] } },
        }),
      );
      const view = render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'reranker/bge' })).getByRole('button', { name: 'Remove reranker/bge' }));
      await rebind(services, 'ftp://h');
      await waitFor(() => expect(api.requests).toContain('PUT /api/v1/services/reranker/bge'));
      view.unmount();
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/reranker/bge'));
      await act(async () => refuse(refused));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS * 2);
      });
      expect(api.requests.filter((r) => r.startsWith('DELETE'))).toEqual(['DELETE /api/v1/services/reranker/bge']);
      expect(vi.getTimerCount()).toBe(0);
    });

    it('writes nothing when the listing read before the delete fails, brings the row back and says why', async () => {
      const api = mockApi(
        routes({
          'GET /services': [{ body: { services: [QWEN] } }, problem('backend_failed', 500, 'workspace.toml could not be read: permission denied.', 'Check the file’s permissions, then retry.')],
          'DELETE /services/{family}/{name}': { body: { services: [] } },
        }),
      );
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      const alert = await services.findByRole('alert');
      expect(alert.textContent).toContain('workspace.toml could not be read: permission denied.');
      expect(alert.textContent).toContain('Check the file’s permissions, then retry.');
      expect(services.getByRole('listitem', { name: 'generator/qwen' })).toBeTruthy();
      expect(api.requests.filter((r) => r.startsWith('DELETE'))).toEqual([]);
    });

    it('writes a pending removal at once when another binding is removed, each keeping its own place', async () => {
      const BGE = service('reranker', 'bge', 'http://127.0.0.1:9090');
      const api = mockApi(routes({ 'GET /services': { body: { services: [QWEN, BGE] } }, 'DELETE /services/{family}/{name}': { body: { services: [BGE] } } }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      fireEvent.click(within(services.getByRole('listitem', { name: 'reranker/bge' })).getByRole('button', { name: 'Remove reranker/bge' }));
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/generator/qwen'));
      expect(api.requests).not.toContain('DELETE /api/v1/services/reranker/bge');
      await waitFor(() => expect(services.getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual(['generator/qwen, removed', 'reranker/bge, removed']));
      expect(within(services.getByRole('listitem', { name: 'reranker/bge, removed' })).getByRole('button', { name: 'Undo' })).toBeTruthy();
    });

    it('writes a pending removal when the page is left', async () => {
      const api = mockApi(routes({ 'DELETE /services/{family}/{name}': { body: { services: [] } } }));
      const view = render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      view.unmount();
      await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/services/generator/qwen'));
    });

    it('brings the row back and says why when the removal is refused', async () => {
      mockApi(routes({ 'DELETE /services/{family}/{name}': problem('service_not_found', 404, 'No service generator/qwen is bound.', 'Reload the list of services.') }));
      render(<Harness />);
      const services = within(await screen.findByRole('region', { name: 'Services' }));
      fireEvent.click(within(await services.findByRole('listitem', { name: 'generator/qwen' })).getByRole('button', { name: 'Remove generator/qwen' }));
      await act(async () => {
        vi.advanceTimersByTime(UNDO_WINDOW_MS);
      });
      const alert = await services.findByRole('alert');
      expect(alert.textContent).toContain('No service generator/qwen is bound.');
      expect(alert.textContent).toContain('Reload the list of services.');
      const back = services.getByRole('listitem', { name: 'generator/qwen' });
      // Focus was on Undo, inside the slot: it follows the binding back to its row.
      await waitFor(() => expect(document.activeElement).toBe(back));
    });
  });

  it('marks a probe refused for another reason as refused, in the API’s words, and announces a success in the row’s status line', async () => {
    mockApi(
      routes({
        'POST /services/{family}/{name}/probe': [problem('request_invalid', 400, 'generator/qwen reports an identity only for a served model; none was given', 'Give the served model the node uses.'), { body: { identity: 'qwen2.5-7b-instruct' } }],
      }),
    );
    render(<Harness />);
    const qwen = await within(await screen.findByRole('region', { name: 'Services' })).findByRole('listitem', { name: 'generator/qwen' });
    fireEvent.click(within(qwen).getByRole('button', { name: 'Test generator/qwen' }));
    const alert = await within(qwen).findByRole('alert');
    expect(alert.textContent).toContain('none was given');
    expect(alert.textContent).toContain('Give the served model the node uses.');
    expect(within(qwen).getByText('refused').closest('.rg-status')?.getAttribute('data-state')).toBe('warning');

    fireEvent.change(within(qwen).getByRole('textbox', { name: 'Served model for generator/qwen' }), { target: { value: 'qwen2.5-7b-instruct' } });
    fireEvent.click(within(qwen).getByRole('button', { name: 'Test generator/qwen' }));
    await waitFor(() => expect(within(qwen).getByRole('status').textContent).toContain('Connected · qwen2.5-7b-instruct'));
  });

  it('says, beside the Connect form, that a build without remote stores a binding and refuses to test it', async () => {
    mockApi(routes());
    render(<Harness workspace={{ status: 'loaded', value: { ...WORKSPACE, capabilities: { ...WORKSPACE.capabilities, remote: false } } }} />);
    const services = await screen.findByRole('region', { name: 'Services' });
    expect(services.textContent).toContain('This build has no remote feature');
  });

  it('shows the server’s impl_not_in_build on the row when a build without remote is asked to test', async () => {
    mockApi(routes({ 'POST /services/{family}/{name}/probe': problem('impl_not_in_build', 422, 'generator/qwen is a Remote component, and this build lacks the remote feature', 'Rebuild with the remote feature.') }));
    render(<Harness workspace={{ status: 'loaded', value: { ...WORKSPACE, capabilities: { ...WORKSPACE.capabilities, remote: false } } }} />);
    const qwen = await within(await screen.findByRole('region', { name: 'Services' })).findByRole('listitem', { name: 'generator/qwen' });
    fireEvent.click(within(qwen).getByRole('button', { name: 'Test generator/qwen' }));
    const alert = await within(qwen).findByRole('alert');
    expect(alert.textContent).toContain('this build lacks the remote feature');
    expect(alert.textContent).toContain('impl_not_in_build');
    expect(within(qwen).getByText('refused')).toBeTruthy();
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

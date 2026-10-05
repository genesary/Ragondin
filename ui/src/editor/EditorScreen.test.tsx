/** @vitest-environment happy-dom */
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { PipelineDetail, Workspace } from '../api/types.ts';
import { useRoute } from '../routes.ts';
import type { RequestState } from '../shell/states.tsx';
import { EditorScreen } from './EditorScreen.tsx';
import { exampleDocument } from './example.ts';
import { HYBRID, SERVICES, WORKSPACE } from './fixtures.ts';
import { rememberFork } from './session.ts';
import { recentPipelines, rememberPipeline } from './recent.ts';

const HASH = 'c'.repeat(64);
/** The recorded workspace before its first pipeline: the first launch. */
const EMPTY_WORKSPACE: Workspace = { ...WORKSPACE, counts: { ...WORKSPACE.counts, pipelines: 0 } };
// The editor's chunk loads lazily, then validation waits out its debounce: under a loaded test run that can pass a second.
const SLOW = 5000;

/**
 * A node's card on `canvas`, once the library has drawn it. The canvas holds
 * its own `ReactFlowProvider`, so the library takes the nodes into its store
 * in an effect: the canvas's region is in the document a commit before its
 * cards are, and a query made the moment the region appears can find none.
 */
function findNode(canvas: HTMLElement, id: string): Promise<HTMLElement> {
  return waitFor(
    () => {
      const node = canvas.querySelector<HTMLElement>(`.react-flow__node[data-id="${id}"]`);
      if (node === null) throw new Error(`no card for \`${id}\` on the canvas yet`);
      return node;
    },
    { timeout: SLOW },
  );
}

function Harness({ name, node, workspace = { status: 'loaded', value: WORKSPACE } }: { name?: string; node?: string; workspace?: RequestState<Workspace> }) {
  const [client] = useState(() => createApiClient());
  return <EditorScreen client={client} name={name} node={node} workspace={workspace} />;
}

/** The screen as the shell mounts it: following the address. */
function Routed() {
  const [client] = useState(() => createApiClient());
  const route = useRoute();
  if (route?.screen !== 'editor') return null;
  return <EditorScreen client={client} name={route.name} node={route.node} workspace={{ status: 'loaded', value: EMPTY_WORKSPACE }} />;
}

const ROUTES: MockRoutes = {
  'GET /services': { body: SERVICES },
  'POST /pipelines/validate': { body: { hash: HASH, rendering: null } },
};

/** `HYBRID` as `GET /pipelines/{name}` serves it: its text, and its typed document. */
const STORED_DETAIL: PipelineDetail = { name: 'hybrid', document: 'pipeline: …\n', etag: 'e'.repeat(64), hash: HASH, error: null, typed: HYBRID, canonical: true, ends_in_answer_up_to: null };
const STORED: MockRoutes = { ...ROUTES, 'GET /pipelines/{name}': { body: STORED_DETAIL } };

afterEach(async () => {
  // The editor flushes what it holds when it closes: let that land on the mocks before `fetch` is restored.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 50));
  vi.unstubAllGlobals();
  window.sessionStorage.clear();
  window.localStorage.clear();
});

describe('the Editor screen', () => {
  it('before a pipeline is opened, offers a new one, and opens the canvas on an empty document with the palette this build allows', async () => {
    const api = mockApi(ROUTES);
    render(<Harness />);
    expect(await screen.findByRole('heading', { name: 'No pipeline open' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Start a new pipeline' }));
    expect(await findNode(await screen.findByRole('application', { name: 'Pipeline New pipeline' }), 'query')).toBeTruthy();
    const palette = await screen.findByRole('region', { name: 'Palette' });
    expect(within(palette).getByRole('button', { name: /^qwen/ })).toBeTruthy();
    expect(api.requests).toContain('GET /api/v1/services');
    expect(await screen.findByText(HASH.slice(0, 12))).toBeTruthy();
  });

  it('judges kinds during the drag with the ports the capabilities serve', async () => {
    mockApi(ROUTES);
    render(<Harness />);
    fireEvent.click(await screen.findByRole('button', { name: 'Start a new pipeline' }));
    const palette = await screen.findByRole('region', { name: 'Palette' });
    fireEvent.click(within(palette).getByRole('button', { name: /^bm25/ }));
    fireEvent.click(within(palette).getByRole('button', { name: /^concat/ }));
    const canvas = screen.getByRole('application', { name: 'Pipeline New pipeline' });
    const port = (id: string, side: string) => canvas.querySelectorAll<HTMLElement>(`.react-flow__node[data-id="${id}"] .rg-port[data-side="${side}"]`);
    expect([...port('concat', 'in')].map((p) => p.getAttribute('data-kind'))).toEqual(['query', 'chunks']);
    fireEvent.pointerDown(port('bm25', 'out')[0]!, { button: 0 });
    expect(port('concat', 'in')[0]!.getAttribute('title')).toBe('`bm25` feeds `concat` at port 0: expected query, found chunks.');
    expect(port('concat', 'in')[1]!.getAttribute('data-drop')).toBe('refused');
  });

  it('opens a stored pipeline on the canvas from its typed document, and sends that document to validation', async () => {
    const api = mockApi(STORED);
    render(<Harness name="hybrid" />);
    const canvas = await screen.findByRole('application', { name: 'Pipeline hybrid' }, { timeout: SLOW });
    expect(await findNode(canvas, 'fused')).toBeTruthy();
    expect(api.requests).toContain('GET /api/v1/pipelines/hybrid');
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/pipelines/validate'), { timeout: SLOW });
    expect(api.bodies[api.requests.indexOf('POST /api/v1/pipelines/validate')]).toEqual({ typed: HYBRID });
    expect(await screen.findByText(HASH.slice(0, 12))).toBeTruthy();
  });

  it('opens a stored pipeline that does not validate, with the server’s words on it', async () => {
    mockApi({
      ...STORED,
      'GET /pipelines/{name}': { body: { ...STORED_DETAIL, hash: null, error: { detail: 'the pipeline does not validate', location: { node: 'fused', edge: null } } } },
      'POST /pipelines/validate': { problem: { type: 'urn:ragondin:problem:pipeline_invalid', title: 'The pipeline is invalid', status: 422, code: 'pipeline_invalid', detail: 'the pipeline does not validate: dangling', hint: 'Correct it.', location: { node: 'fused', edge: null } } },
    });
    render(<Harness name="hybrid" />);
    const canvas = await screen.findByRole('application', { name: 'Pipeline hybrid' }, { timeout: SLOW });
    await waitFor(() => expect(canvas.querySelector('.react-flow__node[data-id="fused"] .rg-node')?.getAttribute('data-status')).toBe('invalid'), { timeout: SLOW });
  });

  it('restores the node the address selects, and writes the node selected into the address', async () => {
    mockApi(STORED);
    window.location.hash = '#editor/hybrid/node/lexical';
    render(<Harness name="hybrid" node="lexical" />);
    expect(await screen.findByRole('complementary', { name: 'lexical' }, { timeout: SLOW })).toBeTruthy();
    const canvas = screen.getByRole('application', { name: 'Pipeline hybrid' });
    fireEvent.click(await findNode(canvas, 'vectors'));
    await waitFor(() => expect(window.location.hash).toBe('#editor/hybrid/node/vectors'), { timeout: SLOW });
  });

  it('says a stored pipeline whose text does not read opens as text only, in the server’s words, and offers a new one', async () => {
    mockApi({
      ...STORED,
      'GET /pipelines/{name}': { body: { ...STORED_DETAIL, document: 'pipeline: [', hash: null, typed: null, error: { detail: 'could not parse configuration: line 1', location: { node: null, edge: null } } } },
    });
    render(<Harness name="hybrid" />);
    expect(await screen.findByRole('heading', { name: 'hybrid cannot be opened on the canvas' })).toBeTruthy();
    expect(screen.getByText(/could not parse configuration: line 1/)).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Start a new pipeline' }).getAttribute('href')).toBe('#editor');
  });

  it('opens a stored pipeline at its stored positions, and writes its changes over the etag it read', async () => {
    const api = mockApi({
      ...STORED,
      'GET /pipelines/{name}/layout': { body: { layout: { version: 1, nodes: { lexical: { x: 1600, y: 800 } } } } },
      'PUT /pipelines/{name}': { body: { name: 'hybrid', etag: 'f'.repeat(64), hash: HASH } },
    });
    render(<Harness name="hybrid" />);
    const canvas = await screen.findByRole('application', { name: 'Pipeline hybrid' }, { timeout: SLOW });
    expect(api.requests).toContain('GET /api/v1/pipelines/hybrid/layout');
    expect((await findNode(canvas, 'lexical')).style.transform).toContain('1600px');
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid'), { timeout: SLOW });
    expect(api.headers[api.requests.indexOf('PUT /api/v1/pipelines/hybrid')]!['If-Match']).toBe(`"${'e'.repeat(64)}"`);
  });

  it('opens a hand-written pipeline, and warns before its first save', async () => {
    mockApi({ ...STORED, 'GET /pipelines/{name}': { body: { ...STORED_DETAIL, canonical: false } }, 'GET /pipelines/{name}/layout': { body: { layout: null } } });
    render(<Harness name="hybrid" />);
    await screen.findByRole('application', { name: 'Pipeline hybrid' }, { timeout: SLOW });
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    expect(await screen.findByRole('region', { name: 'This file was written by hand' }, { timeout: SLOW })).toBeTruthy();
  });

  it('names the run a pipeline was forked from', async () => {
    mockApi({ ...STORED, 'GET /pipelines/{name}/layout': { body: { layout: null } } });
    rememberFork('hybrid', 'a'.repeat(64));
    render(<Harness name="hybrid" />);
    expect(await screen.findByText(/Forked from run/, undefined, { timeout: SLOW })).toBeTruthy();
  });

  it('on a workspace with no pipeline, opens on the example, writes no file until the first edit, and keeps editing once it is written', { timeout: 3 * SLOW }, async () => {
    const api = mockApi({
      ...ROUTES,
      'GET /pipelines': { body: { pipelines: [] } },
      'PUT /pipelines/{name}': { body: { name: 'example', etag: 'f'.repeat(64), hash: HASH } },
      'PUT /pipelines/{name}/layout': { body: { layout: null } },
    });
    window.location.hash = '#editor';
    render(<Routed />);
    const canvas = await screen.findByRole('application', { name: 'Pipeline Example pipeline' }, { timeout: SLOW });
    expect(await findNode(canvas, 'lexical')).toBeTruthy();
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/pipelines/validate'), { timeout: SLOW });
    expect((api.bodies[api.requests.indexOf('POST /api/v1/pipelines/validate')] as { typed: unknown }).typed).toEqual(exampleDocument(WORKSPACE.capabilities));
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    // The first write asks the name, offering the one proposed.
    const prompt = await screen.findByRole('region', { name: 'Name this pipeline' }, { timeout: SLOW });
    expect((within(prompt).getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value).toBe('example');
    await waitFor(() => expect(within(prompt).getByRole('button', { name: 'Save' }).getAttribute('aria-disabled')).toBeNull(), { timeout: SLOW });
    fireEvent.click(within(prompt).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(window.location.hash).toBe('#editor/example'), { timeout: SLOW });
    expect(api.headers[api.requests.indexOf('PUT /api/v1/pipelines/example')]!['If-None-Match']).toBe('*');
    // The same editor, its history kept: the address naming the file it wrote does not reopen it.
    expect(await screen.findByRole('heading', { name: 'example' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Undo' }).getAttribute('aria-disabled')).toBeNull();
    expect(api.requests).not.toContain('GET /api/v1/pipelines/example');
  });

  it('opens no example when the listing finds a pipeline the shell’s count is too old to know', async () => {
    mockApi({ ...ROUTES, 'GET /pipelines': { body: { pipelines: [{ name: 'hybrid', etag: 'e'.repeat(64), modified_ms: null, hash: HASH, ends_in_answer: false, error: null }] } } });
    render(<Harness workspace={{ status: 'loaded', value: EMPTY_WORKSPACE }} />);
    expect(await screen.findByRole('heading', { name: 'No pipeline open' })).toBeTruthy();
  });

  it('on a workspace holding pipelines, offers a new one or an import', async () => {
    mockApi({ ...ROUTES, 'GET /pipelines': { body: { pipelines: [{ name: 'hybrid', etag: 'e'.repeat(64), modified_ms: null, hash: HASH, ends_in_answer: false, error: null }] } } });
    render(<Harness />);
    expect(await screen.findByRole('heading', { name: 'No pipeline open' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Import a pipeline' }));
    expect(screen.getByRole('textbox', { name: 'Pipeline document (YAML)' })).toBeTruthy();
  });

  it('shows a failed read of the services with Retry, in place of the editor', async () => {
    const api = mockApi({ ...ROUTES, 'GET /services': [{ network: 'down' }, { body: SERVICES }] });
    render(<Harness />);
    fireEvent.click(await screen.findByRole('button', { name: 'Start a new pipeline' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(screen.getByRole('region', { name: 'Palette' })).toBeTruthy());
    expect(api.requests.filter((r) => r === 'GET /api/v1/services')).toHaveLength(2);
  });
});

describe('opening a pipeline from the editor', () => {
  const LISTED: MockRoutes = { ...ROUTES, 'GET /pipelines': { body: { pipelines: ['hybrid', 'lexical', 'rag'].map((name) => ({ name, etag: 'e'.repeat(64), modified_ms: null, hash: HASH, error: null, ends_in_answer: true })) } } };

  it('offers the recent pipelines first, then every pipeline, from the empty state', async () => {
    rememberPipeline('rag');
    rememberPipeline('lexical');
    mockApi(LISTED);
    render(<Harness />);
    const recent = await screen.findByRole('list', { name: 'Recent pipelines' });
    expect(within(recent).getAllByRole('link').map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
      ['lexical', '#editor/lexical'],
      ['rag', '#editor/rag'],
    ]);
    const picker = screen.getByRole('combobox', { name: 'Open a pipeline' }) as HTMLSelectElement;
    expect([...picker.options].filter((o) => !o.disabled).map((o) => o.value)).toEqual(['lexical', 'rag', 'hybrid']);
    fireEvent.change(picker, { target: { value: 'hybrid' } });
    expect(window.location.hash).toBe('#editor/hybrid');
  });

  it('remembers a pipeline opened, and offers the others from the editor’s header', async () => {
    mockApi({ ...LISTED, 'GET /pipelines/{name}': { body: STORED_DETAIL } });
    render(<Harness name="hybrid" />);
    const picker = (await screen.findByRole('combobox', { name: 'Open a pipeline' }, { timeout: SLOW })) as HTMLSelectElement;
    expect(picker.value).toBe('hybrid');
    expect(recentPipelines()).toEqual(['hybrid']);
    fireEvent.change(picker, { target: { value: 'rag' } });
    expect(window.location.hash).toBe('#editor/rag');
  });
});


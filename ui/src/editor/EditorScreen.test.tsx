/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { PipelineDetail, Workspace } from '../api/types.ts';
import type { RequestState } from '../shell/states.tsx';
import { EditorScreen } from './EditorScreen.tsx';
import { HYBRID, SERVICES, WORKSPACE } from './fixtures.ts';

const HASH = 'c'.repeat(64);
// The editor's chunk loads lazily, then validation waits out its debounce: under a loaded test run that can pass a second.
const SLOW = 5000;

function Harness({ name, node, workspace = { status: 'loaded', value: WORKSPACE } }: { name?: string; node?: string; workspace?: RequestState<Workspace> }) {
  const [client] = useState(() => createApiClient());
  return <EditorScreen client={client} name={name} node={node} workspace={workspace} />;
}

const ROUTES: MockRoutes = {
  'GET /services': { body: SERVICES },
  'POST /pipelines/validate': { body: { hash: HASH } },
};

/** `HYBRID` as `GET /pipelines/{name}` serves it: its text, and its typed document. */
const STORED_DETAIL: PipelineDetail = { name: 'hybrid', document: 'pipeline: …\n', etag: 'e'.repeat(64), hash: HASH, error: null, typed: HYBRID };
const STORED: MockRoutes = { ...ROUTES, 'GET /pipelines/{name}': { body: STORED_DETAIL } };

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('the Editor screen', () => {
  it('before a pipeline is opened, offers a new one, and opens the canvas on an empty document with the palette this build allows', async () => {
    const api = mockApi(ROUTES);
    render(<Harness />);
    expect(screen.getByRole('heading', { name: 'No pipeline open' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Start a new pipeline' }));
    expect(await screen.findByRole('application', { name: 'Pipeline New pipeline' })).toBeTruthy();
    expect(screen.getByRole('application', { name: 'Pipeline New pipeline' }).querySelector('.react-flow__node[data-id="query"]')).toBeTruthy();
    const palette = await screen.findByRole('region', { name: 'Palette' });
    expect(within(palette).getByRole('button', { name: /^qwen/ })).toBeTruthy();
    expect(api.requests).toContain('GET /api/v1/services');
    expect(await screen.findByText(HASH)).toBeTruthy();
  });

  it('judges kinds during the drag with the ports the capabilities serve', async () => {
    mockApi(ROUTES);
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Start a new pipeline' }));
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
    expect(canvas.querySelector('.react-flow__node[data-id="fused"]')).toBeTruthy();
    expect(api.requests).toContain('GET /api/v1/pipelines/hybrid');
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/pipelines/validate'), { timeout: SLOW });
    expect(api.bodies[api.requests.indexOf('POST /api/v1/pipelines/validate')]).toEqual({ typed: HYBRID });
    expect(await screen.findByText(HASH)).toBeTruthy();
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
    fireEvent.click(canvas.querySelector('.react-flow__node[data-id="vectors"]')!);
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

  it('shows a failed read of the services with Retry, in place of the editor', async () => {
    const api = mockApi({ ...ROUTES, 'GET /services': [{ network: 'down' }, { body: SERVICES }] });
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Start a new pipeline' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(screen.getByRole('region', { name: 'Palette' })).toBeTruthy());
    expect(api.requests.filter((r) => r === 'GET /api/v1/services')).toHaveLength(2);
  });
});

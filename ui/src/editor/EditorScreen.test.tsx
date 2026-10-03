/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Workspace } from '../api/types.ts';
import type { RequestState } from '../shell/states.tsx';
import { EditorScreen } from './EditorScreen.tsx';
import { SERVICES, WORKSPACE } from './fixtures.ts';

const HASH = 'c'.repeat(64);

function Harness({ name, workspace = { status: 'loaded', value: WORKSPACE } }: { name?: string; workspace?: RequestState<Workspace> }) {
  const [client] = useState(() => createApiClient());
  return <EditorScreen client={client} name={name} workspace={workspace} />;
}

const ROUTES: MockRoutes = {
  'GET /services': { body: SERVICES },
  'POST /pipelines/validate': { body: { hash: HASH } },
  'GET /pipelines/{name}': { body: { name: 'hybrid', document: 'pipeline:\n  inputs: [q]\n  nodes: []\n', etag: 'e'.repeat(64), hash: HASH, error: null } },
};

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

  it('says why a stored pipeline cannot be opened on the canvas yet, and offers a new one instead', async () => {
    const api = mockApi(ROUTES);
    render(<Harness name="hybrid" />);
    expect(await screen.findByRole('heading', { name: 'hybrid cannot be opened on the canvas yet' })).toBeTruthy();
    expect(screen.getByText(/serves a pipeline as its text/)).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Start a new pipeline' }).getAttribute('href')).toBe('#editor');
    expect(api.requests).toContain('GET /api/v1/pipelines/hybrid');
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

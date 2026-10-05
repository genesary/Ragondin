/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Layout, RunDetail } from '../api/types.ts';
import { HYBRID_DETAIL } from '../replay/fixtures.ts';
import { ForkButton } from './Fork.tsx';
import { forkedFrom } from './session.ts';

// A run's configuration as `bench` kept it: comments and all, byte for byte.
const CONFIGURATION = '# tuned on 2026-10-01\npipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }\n';
const RUN: RunDetail = { ...HYBRID_DETAIL, configuration: CONFIGURATION, launched_as: { name: 'hybrid', held: null, prefix_of: null } };
const LAYOUT: Layout = { version: 1, nodes: { lexical: { x: 16, y: 32 } } };

function Harness() {
  const [client] = useState(() => createApiClient());
  return <ForkButton client={client} run={RUN.id} />;
}

function setup(routes: MockRoutes = {}) {
  const api = mockApi({
    'GET /runs/{id}': { body: RUN },
    'GET /pipelines': { body: { pipelines: [{ name: 'hybrid', etag: 'e'.repeat(64), modified_ms: null, hash: null, error: null }] } },
    'PUT /pipelines/{name}': { body: { name: 'hybrid-fork', etag: 'f'.repeat(64), hash: RUN.inputs.pipeline } },
    'GET /runs/{id}/layout': { body: { layout: null } },
    'PUT /pipelines/{name}/layout': { body: { layout: LAYOUT } },
    ...routes,
  });
  render(<Harness />);
  return api;
}

afterEach(() => {
  vi.unstubAllGlobals();
  window.sessionStorage.clear();
  window.location.hash = '';
});

describe('forking a run', () => {
  it('writes the run’s configuration byte for byte to a new pipeline file, and opens the editor on it naming the run', async () => {
    const api = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    await waitFor(() => expect(window.location.hash).toBe('#editor/hybrid-fork'));
    const at = api.requests.indexOf('PUT /api/v1/pipelines/hybrid-fork');
    expect(api.bodies[at]).toEqual({ document: CONFIGURATION });
    expect(api.headers[at]!['If-None-Match']).toBe('*');
    expect(api.requests).toContain(`GET /api/v1/runs/${RUN.id}`);
    expect(forkedFrom('hybrid-fork')).toBe(RUN.id);
    // No layout was copied at launch: the editor lays the fork out itself.
    expect(api.requests).not.toContain('PUT /api/v1/pipelines/hybrid-fork/layout');
  });

  it('copies the layout copied at launch when there is one', async () => {
    const api = setup({ 'GET /runs/{id}/layout': { body: { layout: LAYOUT } } });
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    await waitFor(() => expect(window.location.hash).toBe('#editor/hybrid-fork'));
    expect(api.bodies[api.requests.indexOf('PUT /api/v1/pipelines/hybrid-fork/layout')]).toEqual(LAYOUT);
  });

  it('takes the next free name when the one proposed is taken between the listing and the write', async () => {
    const taken = { problem: { type: 'urn:ragondin:problem:precondition_failed', title: 'Precondition failed', status: 412, code: 'precondition_failed' as const, detail: 'a document is stored', hint: 'Read it again.', location: null } };
    const api = setup({ 'PUT /pipelines/{name}': [taken, { body: { name: 'hybrid-fork-2', etag: 'f'.repeat(64), hash: RUN.inputs.pipeline } }] });
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    await waitFor(() => expect(window.location.hash).toBe('#editor/hybrid-fork-2'));
    expect(api.requests.filter((r) => r.startsWith('PUT /api/v1/pipelines/hybrid-fork'))).toEqual(['PUT /api/v1/pipelines/hybrid-fork', 'PUT /api/v1/pipelines/hybrid-fork-2']);
    expect(api.bodies[api.requests.indexOf('PUT /api/v1/pipelines/hybrid-fork-2')]).toEqual({ document: CONFIGURATION });
  });

  it('says in words when the layout could not be copied, and offers the fork', async () => {
    setup({
      'GET /runs/{id}/layout': { body: { layout: LAYOUT } },
      'PUT /pipelines/{name}/layout': { problem: { type: 'urn:ragondin:problem:backend_failed', title: 'Backend failed', status: 500, code: 'backend_failed', detail: 'disk full', hint: 'Free space.', location: null } },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    const said = await screen.findByRole('status');
    expect(said.textContent).toContain('Forked as hybrid-fork, but its layout could not be copied: disk full');
    expect(window.location.hash).toBe('');
    expect(screen.getByRole('link', { name: 'Open hybrid-fork in the editor' }).getAttribute('href')).toBe('#editor/hybrid-fork');
  });

  it('says why when the write is refused, and opens nothing', async () => {
    setup({ 'PUT /pipelines/{name}': { problem: { type: 'urn:ragondin:problem:pipeline_invalid', title: 'Invalid', status: 422, code: 'pipeline_invalid', detail: 'a key no component reads', hint: 'Fix it.', location: null } } });
    fireEvent.click(screen.getByRole('button', { name: 'Fork this run' }));
    expect((await screen.findByRole('alert')).textContent).toContain('a key no component reads');
    expect(window.location.hash).toBe('');
  });
});

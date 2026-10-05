/** @vitest-environment happy-dom */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Problem } from '../api/types.ts';
import { ImportPanel } from './Import.tsx';

const HAND = '# my pipeline\npipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n';
const MIS_KINDED: { problem: Problem } = {
  problem: {
    type: 'urn:ragondin:problem:pipeline_invalid',
    title: 'The pipeline is invalid',
    status: 422,
    code: 'pipeline_invalid',
    detail: 'the pipeline does not validate: the configuration wires two nodes incompatibly',
    hint: 'Correct the node or edge named in `location`, then validate again.',
    location: { node: 'ranked', edge: { from: 'legs', to: 'ranked', port: 0 } },
  },
};

function Harness({ onImported }: { onImported: (name: string) => void }) {
  const [client] = useState(() => createApiClient());
  return <ImportPanel client={client} onImported={onImported} onCancel={() => {}} />;
}

function setup(routes: MockRoutes) {
  const onImported = vi.fn();
  const api = mockApi(routes);
  render(<Harness onImported={onImported} />);
  return { api, onImported };
}

const fill = (text: string, name: string) => {
  fireEvent.change(screen.getByRole('textbox', { name: 'Pipeline document (YAML)' }), { target: { value: text } });
  fireEvent.change(screen.getByRole('textbox', { name: 'Pipeline name' }), { target: { value: name } });
};

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('importing a pipeline', () => {
  it('shows a located error and writes nothing when the document does not validate', async () => {
    const { api, onImported } = setup({ 'POST /pipelines/validate': MIS_KINDED });
    fill(HAND, 'mine');
    fireEvent.click(screen.getByRole('button', { name: 'Validate and import' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('wires two nodes incompatibly');
    expect(alert.textContent).toContain('At node `ranked`, on the edge `legs` → `ranked`, port 0.');
    expect(api.bodies[api.requests.indexOf('POST /api/v1/pipelines/validate')]).toEqual({ document: HAND });
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    expect(onImported).not.toHaveBeenCalled();
  });

  it('writes the text as it was given, under the name given, creating it', async () => {
    const { api, onImported } = setup({
      'POST /pipelines/validate': { body: { hash: 'c'.repeat(64), rendering: 'version: 3\n' } },
      'PUT /pipelines/{name}': { body: { name: 'mine', etag: 'e'.repeat(64), hash: 'c'.repeat(64) } },
    });
    fill(HAND, 'mine');
    fireEvent.click(screen.getByRole('button', { name: 'Validate and import' }));
    await waitFor(() => expect(onImported).toHaveBeenCalledWith('mine'));
    const at = api.requests.indexOf('PUT /api/v1/pipelines/mine');
    expect(api.bodies[at]).toEqual({ document: HAND });
    expect(api.headers[at]!['If-None-Match']).toBe('*');
  });

  it('says a name already taken, and writes nothing over it', async () => {
    const { onImported } = setup({
      'POST /pipelines/validate': { body: { hash: 'c'.repeat(64), rendering: null } },
      'PUT /pipelines/{name}': { problem: { type: 'urn:ragondin:problem:precondition_failed', title: 'Precondition failed', status: 412, code: 'precondition_failed', detail: 'a document is stored', hint: 'Read it again.', location: null } },
    });
    fill(HAND, 'taken');
    fireEvent.click(screen.getByRole('button', { name: 'Validate and import' }));
    expect(within(await screen.findByRole('alert')).getByText(/A pipeline named `taken` already exists/)).toBeTruthy();
    expect(onImported).not.toHaveBeenCalled();
  });

  it('reads a file chosen from disk into the document, and proposes its name', async () => {
    setup({});
    const file = new File([HAND], 'hybrid.yaml', { type: 'application/yaml' });
    fireEvent.change(screen.getByLabelText('Choose a YAML file'), { target: { files: [file] } });
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Pipeline document (YAML)' }) as HTMLTextAreaElement).value).toBe(HAND));
    expect((screen.getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value).toBe('hybrid');
  });
});

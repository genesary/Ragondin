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
    expect(alert.textContent).toContain('At node ranked, on the edge legs → ranked, port 0.');
    // Said once: the title says it does not validate, so the server's prefix saying it again is dropped.
    expect(alert.textContent).not.toContain('does not validate');
    expect(api.bodies[api.requests.indexOf('POST /api/v1/pipelines/validate')]).toEqual({ document: HAND });
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    expect(onImported).not.toHaveBeenCalled();
  });

  it('writes the text as it was given, under the name given, creating it', async () => {
    const { api, onImported } = setup({
      'POST /pipelines/validate': { body: { hash: 'c'.repeat(64), rendering: 'version: 1\n' } },
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
    const alert = await screen.findByRole('alert');
    expect(alert.querySelector('b code')?.textContent).toBe('taken');
    expect(alert.textContent).toContain('A pipeline named taken already exists');
    expect(alert.textContent).not.toContain('`');
    expect(onImported).not.toHaveBeenCalled();
  });

  it('reads a file chosen from disk into the document, and proposes its name', async () => {
    setup({});
    const file = new File([HAND], 'hybrid.yaml', { type: 'application/yaml' });
    fireEvent.change(screen.getByLabelText(/Choose a YAML file/), { target: { files: [file] } });
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Pipeline document (YAML)' }) as HTMLTextAreaElement).value).toBe(HAND));
    expect((screen.getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value).toBe('hybrid');
  });

  it('draws the file chooser as a button, and names the file chosen', async () => {
    const { container } = render(<Harness onImported={() => {}} />);
    const input = within(container).getByLabelText(/Choose a YAML file/) as HTMLInputElement;
    expect(input.className).toBe('rg-visually-hidden');
    expect(container.querySelector('label.rg-btn[for="' + input.id + '"]')).toBeTruthy();
    expect(within(container).getByText('No file chosen')).toBeTruthy();
    fireEvent.change(input, { target: { files: [new File([HAND], 'hybrid.yaml')] } });
    expect(await within(container).findByText('hybrid.yaml')).toBeTruthy();
  });

  it('keeps the name in step with the file chosen until the name is typed', async () => {
    setup({});
    const chooser = screen.getByLabelText(/Choose a YAML file/);
    const name = () => (screen.getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value;
    fireEvent.change(chooser, { target: { files: [new File([HAND], 'first.yaml')] } });
    await waitFor(() => expect(name()).toBe('first'));
    fireEvent.change(chooser, { target: { files: [new File([HAND], 'second.yml')] } });
    await waitFor(() => expect(name()).toBe('second'));
    fireEvent.change(screen.getByRole('textbox', { name: 'Pipeline name' }), { target: { value: 'mine' } });
    fireEvent.change(chooser, { target: { files: [new File([HAND], 'third.yaml')] } });
    await waitFor(() => expect((screen.getByRole('textbox', { name: 'Pipeline document (YAML)' }) as HTMLTextAreaElement).value).toBe(HAND));
    expect(name()).toBe('mine');
  });

  it('says beside the button why it cannot import yet', () => {
    setup({});
    expect(screen.getByText('Paste a pipeline document or choose a file.', { selector: '.rg-editor__import-why' })).toBeTruthy();
    fireEvent.change(screen.getByRole('textbox', { name: 'Pipeline document (YAML)' }), { target: { value: HAND } });
    expect(screen.getByText('Give the pipeline a name.', { selector: '.rg-editor__import-why' })).toBeTruthy();
    fireEvent.change(screen.getByRole('textbox', { name: 'Pipeline name' }), { target: { value: 'mine' } });
    expect(document.querySelector('.rg-editor__import-why')).toBeNull();
  });
});

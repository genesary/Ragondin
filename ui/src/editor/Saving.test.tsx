/** @vitest-environment happy-dom */
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { PipelineValidated, PipelineWritten, Problem } from '../api/types.ts';
import type { WireDocument } from './document.ts';
import { Editor } from './Editor.tsx';
import { GRAMMAR, HYBRID, SERVICES, WORKSPACE } from './fixtures.ts';
import type { FileInit } from './saving.ts';
import type { EditorLayout } from './store.ts';

const HASH = 'b'.repeat(64);
const ETAG = 'e'.repeat(64);
const NEW_ETAG = 'f'.repeat(64);
const RENDERING = 'version: 1\npipeline:\n  inputs: [question]\n';
const VALID: { body: PipelineValidated } = { body: { hash: HASH, rendering: RENDERING } };
// Every node of the hybrid placed, so the canvas lays nothing out itself.
const LAYOUT: EditorLayout = { question: { x: 0, y: 0 }, lexical: { x: 288, y: 0 }, vectors: { x: 288, y: 160 }, fused: { x: 576, y: 0 }, reranked: { x: 864, y: 0 } };
const STORED: FileInit = { name: 'hybrid', etag: ETAG, canonical: true, proposed: 'hybrid' };

const problem = (code: Problem['code'], status: number, detail: string, location: Problem['location'] = null): { problem: Problem } => ({
  problem: { type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint: 'Do something.', location },
});
const DANGLING = problem('pipeline_invalid', 422, 'the pipeline does not validate: `fused` names `nowhere`', { node: 'fused', edge: null });
const STALE = problem('precondition_failed', 412, 'the stored document is at another etag');

type Props = { file?: FileInit; initial?: WireDocument; forkedFrom?: string | null; onNamed?: (name: string) => void; onReload?: () => void };

function Harness({ file = STORED, initial = HYBRID, forkedFrom = null, onNamed = () => {}, onReload = () => {} }: Props) {
  const [client] = useState(() => createApiClient());
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <div style={{ width: 1400, height: 800 }}>
      <Editor
        client={client}
        title="New pipeline"
        initial={initial}
        layout={LAYOUT}
        capabilities={WORKSPACE.capabilities}
        services={SERVICES.services}
        grammar={GRAMMAR}
        selected={selected}
        onSelect={setSelected}
        file={file}
        forkedFrom={forkedFrom}
        onNamed={onNamed}
        onReload={onReload}
      />
    </div>
  );
}

// Typed as the reply itself, never as the route's optional slot, which may be `undefined` under `exactOptionalPropertyTypes`.
const WRITTEN: { body: PipelineWritten } = { body: { name: 'hybrid', etag: NEW_ETAG, hash: HASH } };

function setup(routes: MockRoutes, props: Props = {}) {
  const api = mockApi({ 'POST /pipelines/validate': VALID, 'PUT /pipelines/{name}': WRITTEN, 'PUT /pipelines/{name}/layout': { body: { layout: null } }, ...routes });
  const view = render(<Harness {...props} />);
  return { ...view, api };
}

const writes = (api: ReturnType<typeof mockApi>, name = 'hybrid') => api.requests.flatMap((r, i) => (r === `PUT /api/v1/pipelines/${name}` ? [{ body: api.bodies[i], headers: api.headers[i]!, at: i }] : []));
const validations = (api: ReturnType<typeof mockApi>) => api.requests.flatMap((r, i) => (r === 'POST /api/v1/pipelines/validate' ? [i] : []));
const place = (impl: RegExp) => fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: impl }));
const saveLine = () => screen.getByTestId('save-state');
/** Answers the question a first write asks: the name offered, or `name` typed over it. */
const nameIt = async (name?: string) => {
  const prompt = await screen.findByRole('region', { name: 'Name this pipeline' });
  if (name !== undefined) fireEvent.change(within(prompt).getByRole('textbox', { name: 'Pipeline name' }), { target: { value: name } });
  await waitFor(() => expect(within(prompt).getByRole('button', { name: 'Save' }).getAttribute('aria-disabled')).toBeNull());
  fireEvent.click(within(prompt).getByRole('button', { name: 'Save' }));
};
// Past the validation's debounce and its answer, then a little more: what a write would have needed to be sent.
const settle = () => new Promise((resolve) => setTimeout(resolve, 600));

afterEach(async () => {
  // The editor flushes what it holds when it closes: let that land on the mocks before `fetch` is restored.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 50));
  vi.unstubAllGlobals();
  window.sessionStorage.clear();
});

describe('continuous saving', () => {
  it('marks a required key the node lacks once the server refuses the save, in the inspector and on the card', async () => {
    const MISSING = problem('pipeline_invalid', 422, 'the pipeline is refused: node `dense`: `embedder` is required', { node: 'dense', edge: null });
    const { api, container } = setup({ 'PUT /pipelines/{name}': MISSING });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^dense/);
    const field = await screen.findByLabelText('embedder');
    expect(field.getAttribute('aria-invalid')).toBeNull();
    expect(container.querySelector('.react-flow__node[data-id="dense"] .rg-node__todo')?.textContent).toBe('1 parameter to set');
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    await waitFor(() => expect(screen.getByLabelText('embedder').getAttribute('aria-invalid')).toBe('true'));
    expect(screen.getByText('Required: the pipeline cannot be saved or run without it.')).toBeTruthy();
    const card = container.querySelector('.react-flow__node[data-id="dense"] .rg-node');
    expect(card?.getAttribute('data-status')).toBe('invalid');
    expect(container.querySelector('.react-flow__node[data-id="dense"] .rg-node__todo')).toBeNull();
  });

  it('writes nothing for the document it opened', async () => {
    const { api } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    await settle();
    expect(writes(api)).toEqual([]);
    // Nothing was saved: the file is as it was opened.
    expect(saveLine().textContent).toBe('No changes since it was opened');
  });

  it('writes a valid change after the debounce, validated first, over the etag it read, as the typed document', async () => {
    const { api } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    const [write] = writes(api);
    expect(write!.headers['If-Match']).toBe(`"${ETAG}"`);
    expect(write!.headers['If-None-Match']).toBeUndefined();
    const sent = write!.body as { typed: WireDocument };
    expect(Object.keys(sent)).toEqual(['typed']);
    expect(sent.typed.pipeline.nodes.at(-1)?.id).toBe('rrf');
    // The validation of that very document came first.
    expect(validations(api).at(-1)!).toBeLessThan(write!.at);
    await waitFor(() => expect(saveLine().textContent).toBe('Saved'));
    expect(screen.getByText(HASH.slice(0, 12))).toBeTruthy();
    // The next write names the etag the last one answered.
    place(/^concat/);
    await waitFor(() => expect(writes(api)).toHaveLength(2));
    expect(writes(api)[1]!.headers['If-Match']).toBe(`"${NEW_ETAG}"`);
  });

  it('never writes a document the server calls invalid, and says it is unsaved with its error count', async () => {
    const { api } = setup({ 'POST /pipelines/validate': [VALID, DANGLING] });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await waitFor(() => expect(saveLine().textContent).toBe('Unsaved — 1 error'));
    await settle();
    expect(writes(api)).toEqual([]);
  });

  it('writes the layout when a node moves, and leaves the document alone', async () => {
    const { api, container } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const node = container.querySelector<HTMLElement>('.react-flow__node[data-id="lexical"]')!;
    node.focus();
    fireEvent.keyDown(node, { key: 'ArrowRight' });
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid/layout'));
    const body = api.bodies[api.requests.indexOf('PUT /api/v1/pipelines/hybrid/layout')] as { version: number; nodes: Record<string, { x: number; y: number }> };
    expect(body.version).toBe(1);
    expect(body.nodes.lexical).toEqual({ x: 304, y: 0 });
    await settle();
    expect(writes(api)).toEqual([]);
  });

  it('can export the server’s rendering of the document, and nothing else', async () => {
    const { api } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Export' }).getAttribute('aria-disabled')).not.toBe('true'));
    fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    const exported = screen.getByRole('textbox', { name: 'The document as the server renders it' }) as HTMLTextAreaElement;
    expect(exported.value).toBe(RENDERING);
    expect(screen.getByRole('button', { name: 'Download hybrid.yaml' })).toBeTruthy();
    // Read-only, said so, and selected whole when focused, ready to copy.
    expect(exported.readOnly).toBe(true);
    expect(exported.getAttribute('aria-readonly')).toBe('true');
    const select = vi.spyOn(exported, 'select');
    fireEvent.focus(exported);
    expect(select).toHaveBeenCalled();
  });
});

describe('a file changed on disk', () => {
  it('stops saving on a stale etag, writes nothing until a choice is made, and offers reload or a new file', async () => {
    const onReload = vi.fn();
    const { api } = setup({ 'PUT /pipelines/{name}': (body) => ('typed' in body ? STALE : WRITTEN) }, { onReload });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    expect(within(message).getByText(/unsaved changes on the canvas are lost/)).toBeTruthy();
    expect(saveLine().textContent).toBe('Not saved: the file changed on disk');
    place(/^concat/);
    await settle();
    expect(writes(api)).toHaveLength(1);
    fireEvent.click(within(message).getByRole('button', { name: 'Discard my changes and reload' }));
    expect(onReload).toHaveBeenCalledOnce();
  });

  it('keeps the canvas as a new file under the name given, created, and goes on writing it', async () => {
    const onNamed = vi.fn();
    const { api } = setup(
      {
        'PUT /pipelines/{name}': [STALE, { body: { name: 'hybrid-mine', etag: NEW_ETAG, hash: HASH } }, { body: { name: 'hybrid-mine', etag: 'a'.repeat(64), hash: HASH } }],
      },
      { onNamed },
    );
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    fireEvent.change(within(message).getByRole('textbox', { name: 'New file name' }), { target: { value: 'hybrid-mine' } });
    fireEvent.click(within(message).getByRole('button', { name: 'Save as a new file' }));
    await waitFor(() => expect(writes(api, 'hybrid-mine')).toHaveLength(1));
    expect(writes(api, 'hybrid-mine')[0]!.headers['If-None-Match']).toBe('*');
    await waitFor(() => expect(onNamed).toHaveBeenCalledWith('hybrid-mine'));
    expect(screen.getByRole('heading', { name: 'hybrid-mine' })).toBeTruthy();
    place(/^concat/);
    await waitFor(() => expect(writes(api, 'hybrid-mine')).toHaveLength(2));
    expect(writes(api, 'hybrid-mine')[1]!.headers['If-Match']).toBe(`"${NEW_ETAG}"`);
  });

  it('never saves an invalid document as a new file, and says why', async () => {
    const { api } = setup({ 'POST /pipelines/validate': [VALID, VALID, DANGLING], 'PUT /pipelines/{name}': STALE });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    place(/^concat/);
    await waitFor(() => expect(saveLine().textContent).toBe('Not saved: the file changed on disk'));
    await waitFor(() => expect(within(message).getByRole('button', { name: 'Save as a new file' }).getAttribute('aria-disabled')).toBe('true'));
    expect(message.textContent).toContain('Only a document the server calls valid is written');
    fireEvent.change(within(message).getByRole('textbox', { name: 'New file name' }), { target: { value: 'hybrid-mine' } });
    fireEvent.click(within(message).getByRole('button', { name: 'Save as a new file' }));
    await settle();
    expect(writes(api, 'hybrid-mine')).toEqual([]);
  });

  it('says that changes made while it asks are kept on the canvas and not written', async () => {
    const { api } = setup({ 'PUT /pipelines/{name}': STALE });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    expect(message.textContent).toContain('Changes made while this question is open stay on the canvas and are not written until you choose.');
  });

  it('pauses the layout while it asks, and writes it beside the file chosen', async () => {
    const { api, container } = setup({ 'PUT /pipelines/{name}': [STALE, { body: { name: 'hybrid-mine', etag: NEW_ETAG, hash: HASH } }] });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    await settle();
    const layouts = () => api.requests.filter((r) => r.endsWith('/layout'));
    const before = layouts().length;
    const node = container.querySelector<HTMLElement>('.react-flow__node[data-id="lexical"]')!;
    node.focus();
    fireEvent.keyDown(node, { key: 'ArrowRight' });
    await settle();
    expect(layouts()).toHaveLength(before);
    fireEvent.change(within(message).getByRole('textbox', { name: 'New file name' }), { target: { value: 'hybrid-mine' } });
    fireEvent.click(within(message).getByRole('button', { name: 'Save as a new file' }));
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid-mine/layout'));
    const moved = api.bodies[api.requests.lastIndexOf('PUT /api/v1/pipelines/hybrid-mine/layout')] as { nodes: Record<string, { x: number }> };
    expect(moved.nodes.lexical!.x).toBe(304);
    expect(layouts().filter((r) => r === 'PUT /api/v1/pipelines/hybrid/layout')).toHaveLength(before);
  });
});

describe('the layout and the file it is written beside', () => {
  it('writes positions already written beside one file again beside a new file saved from it', async () => {
    const { api, container } = setup({ 'PUT /pipelines/{name}': [STALE, { body: { name: 'hybrid-mine', etag: NEW_ETAG, hash: HASH } }] });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const node = container.querySelector<HTMLElement>('.react-flow__node[data-id="lexical"]')!;
    node.focus();
    fireEvent.keyDown(node, { key: 'ArrowRight' });
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid/layout'));
    const beside = api.bodies[api.requests.lastIndexOf('PUT /api/v1/pipelines/hybrid/layout')];
    // A change the file refuses: the positions stay as they were written.
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^bm25/ }));
    const message = await screen.findByRole('region', { name: 'The file changed on disk' });
    await settle();
    const written = api.requests.filter((r) => r === 'PUT /api/v1/pipelines/hybrid/layout').length;
    fireEvent.change(within(message).getByRole('textbox', { name: 'New file name' }), { target: { value: 'hybrid-mine' } });
    fireEvent.click(within(message).getByRole('button', { name: 'Save as a new file' }));
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid-mine/layout'));
    const copied = api.bodies[api.requests.lastIndexOf('PUT /api/v1/pipelines/hybrid-mine/layout')] as { nodes: Record<string, unknown> };
    expect(copied.nodes.lexical).toEqual((beside as { nodes: Record<string, unknown> }).nodes.lexical);
    expect(api.requests.filter((r) => r === 'PUT /api/v1/pipelines/hybrid/layout')).toHaveLength(written);
  });

  it('writes no positions for a document not yet on disk, and writes them beside it once it is', async () => {
    const { api, container } = setup({ 'PUT /pipelines/{name}': { body: { name: 'example', etag: NEW_ETAG, hash: HASH } } }, { file: { name: null, etag: null, canonical: true, proposed: 'example' } });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const node = container.querySelector<HTMLElement>('.react-flow__node[data-id="lexical"]')!;
    node.focus();
    fireEvent.keyDown(node, { key: 'ArrowRight' });
    await settle();
    expect(api.requests.filter((r) => r.endsWith('/layout'))).toEqual([]);
    fireEvent.click(screen.getByRole('button', { name: 'Keep this pipeline' }));
    await nameIt();
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/example/layout'));
    expect(api.requests.indexOf('PUT /api/v1/pipelines/example')).toBeLessThan(api.requests.indexOf('PUT /api/v1/pipelines/example/layout'));
  });
});

describe('leaving the editor', () => {
  it('writes a change still inside the debounce, validated first, when the editor closes', async () => {
    const { api, unmount } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    unmount();
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    expect(validations(api)).toHaveLength(2);
    expect((writes(api)[0]!.body as { typed: WireDocument }).typed.pipeline.nodes.at(-1)?.id).toBe('rrf');
    expect(writes(api)[0]!.headers['If-Match']).toBe(`"${ETAG}"`);
  });

  it('writes nothing invalid when the editor closes', async () => {
    const { api, unmount } = setup({ 'POST /pipelines/validate': [VALID, DANGLING] });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    unmount();
    await waitFor(() => expect(validations(api)).toHaveLength(2));
    await settle();
    expect(writes(api)).toEqual([]);
  });

  it('sends the newer document queued behind a write in flight, over the etag that write answers', async () => {
    let answer: (reply: { body: { name: string; etag: string; hash: string } }) => void = () => {};
    let calls = 0;
    const { api, unmount } = setup({
      'PUT /pipelines/{name}': () => {
        calls += 1;
        return calls === 1 ? new Promise((resolve) => (answer = resolve)) : { body: { name: 'hybrid', etag: 'a'.repeat(64), hash: HASH } };
      },
    });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    place(/^concat/);
    unmount();
    answer({ body: { name: 'hybrid', etag: NEW_ETAG, hash: HASH } });
    await waitFor(() => expect(writes(api)).toHaveLength(2));
    expect(writes(api)[1]!.headers['If-Match']).toBe(`"${NEW_ETAG}"`);
    expect((writes(api)[1]!.body as { typed: WireDocument }).typed.pipeline.nodes.at(-1)?.id).toBe('concat');
  });

  it('stops the flush when the write in flight answers a stale etag', async () => {
    let answer: (reply: typeof STALE) => void = () => {};
    let calls = 0;
    const { api, unmount } = setup({
      'PUT /pipelines/{name}': () => {
        calls += 1;
        return calls === 1 ? new Promise((resolve) => (answer = resolve)) : WRITTEN;
      },
    });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    place(/^concat/);
    unmount();
    answer(STALE);
    await settle();
    expect(writes(api)).toHaveLength(1);
  });

  it('writes nothing held behind a prompt when the editor closes', async () => {
    const { api, unmount } = setup({}, { file: { ...STORED, canonical: false } });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await screen.findByRole('region', { name: 'This file was written by hand' });
    unmount();
    await settle();
    expect(writes(api)).toEqual([]);
  });

  it('writes a position still inside its debounce when the editor closes', async () => {
    const { api, container, unmount } = setup({});
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const node = container.querySelector<HTMLElement>('.react-flow__node[data-id="lexical"]')!;
    node.focus();
    fireEvent.keyDown(node, { key: 'ArrowRight' });
    unmount();
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/hybrid/layout'));
  });

  it('asks the browser to confirm leaving the page while something is not saved', async () => {
    const { api } = setup({ 'POST /pipelines/validate': [VALID, DANGLING] });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const clean = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(clean);
    expect(clean.defaultPrevented).toBe(false);
    place(/^rrf/);
    await waitFor(() => expect(saveLine().textContent).toBe('Unsaved — 1 error'));
    const dirty = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(dirty);
    expect(dirty.defaultPrevented).toBe(true);
  });

  it('says the save state politely to assistive technology', () => {
    setup({});
    expect(saveLine().getAttribute('role')).toBe('status');
  });
});

describe('a file written by hand', () => {
  const HAND: FileInit = { ...STORED, canonical: false };

  it('asks once before its first write, naming everything the rendering drops, and writes nothing meanwhile', async () => {
    const { api } = setup({}, { file: HAND });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'This file was written by hand' });
    for (const dropped of ['comments', 'formatting', 'key order', 'expanded references', 'keys the pipeline schema does not read']) expect(message.textContent).toContain(dropped);
    await settle();
    expect(writes(api)).toEqual([]);
    fireEvent.click(within(message).getByRole('button', { name: 'Rewrite this file' }));
    await waitFor(() => expect(writes(api)).toHaveLength(1));
    expect(writes(api)[0]!.headers['If-Match']).toBe(`"${ETAG}"`);
    // Asked once: the next change is written without a word.
    place(/^concat/);
    await waitFor(() => expect(writes(api)).toHaveLength(2));
    expect(screen.queryByRole('region', { name: 'This file was written by hand' })).toBeNull();
  });

  it('says no change before any, never “Saved” for a file it has not written', async () => {
    const { api } = setup({}, { file: HAND });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    await settle();
    expect(saveLine().textContent).toBe('No changes since it was opened');
  });

  it('asks in a short message, its two choices side by side, what the rewrite drops one click away', async () => {
    const { api } = setup({}, { file: HAND });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'This file was written by hand' });
    const rewrite = within(message).getByRole('button', { name: 'Rewrite this file' });
    const saveAs = within(message).getByRole('button', { name: 'Save as a new file' });
    expect(rewrite.closest('.rg-editor__choices')).not.toBeNull();
    expect(rewrite.closest('.rg-editor__choices')).toBe(saveAs.closest('.rg-editor__choices'));
    const details = message.querySelector('details');
    expect(details?.querySelector('summary')?.textContent).toBe('What a rewrite drops');
    expect(message.querySelector('.rg-inline p')?.textContent).not.toContain('key order');
  });

  it('“Save as a new file” leaves the original untouched', async () => {
    const onNamed = vi.fn();
    const { api } = setup({ 'PUT /pipelines/{name}': { body: { name: 'mine', etag: NEW_ETAG, hash: HASH } } }, { file: HAND, onNamed });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    const message = await screen.findByRole('region', { name: 'This file was written by hand' });
    fireEvent.change(within(message).getByRole('textbox', { name: 'New file name' }), { target: { value: 'mine' } });
    fireEvent.click(within(message).getByRole('button', { name: 'Save as a new file' }));
    await waitFor(() => expect(onNamed).toHaveBeenCalledWith('mine'));
    expect(writes(api, 'mine')[0]!.headers['If-None-Match']).toBe('*');
    expect(writes(api)).toEqual([]);
  });

  it('remembers “Rewrite this file” for the file for the session', async () => {
    const first = setup({}, { file: HAND });
    await waitFor(() => expect(validations(first.api)).toHaveLength(1));
    place(/^rrf/);
    fireEvent.click(within(await screen.findByRole('region', { name: 'This file was written by hand' })).getByRole('button', { name: 'Rewrite this file' }));
    await waitFor(() => expect(writes(first.api)).toHaveLength(1));
    first.unmount();
    vi.unstubAllGlobals();

    const again = setup({}, { file: HAND });
    await waitFor(() => expect(validations(again.api)).toHaveLength(1));
    place(/^rrf/);
    await waitFor(() => expect(writes(again.api)).toHaveLength(1));
    expect(screen.queryByRole('region', { name: 'This file was written by hand' })).toBeNull();
  });
});

describe('a document never written', () => {
  const UNNAMED: FileInit = { name: null, etag: null, canonical: true, proposed: 'example' };

  it('writes no file until “Keep this pipeline”, then asks its name and creates it under the one offered', async () => {
    const onNamed = vi.fn();
    const { api } = setup({ 'PUT /pipelines/{name}': { body: { name: 'example', etag: NEW_ETAG, hash: HASH } } }, { file: UNNAMED, onNamed });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    await settle();
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    expect(saveLine().textContent).toBe('Not written yet: the first change, or Keep this pipeline, asks for its name');
    fireEvent.click(screen.getByRole('button', { name: 'Keep this pipeline' }));
    const prompt = await screen.findByRole('region', { name: 'Name this pipeline' });
    expect((within(prompt).getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value).toBe('example');
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    await nameIt();
    await waitFor(() => expect(writes(api, 'example')).toHaveLength(1));
    expect(writes(api, 'example')[0]!.headers['If-None-Match']).toBe('*');
    await waitFor(() => expect(onNamed).toHaveBeenCalledWith('example'));
  });

  it('creates itself under the name offered when the editor closes while its name is asked, never over another file', async () => {
    const { api, unmount } = setup({ 'PUT /pipelines/{name}': { body: { name: 'example', etag: NEW_ETAG, hash: HASH } } }, { file: UNNAMED });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    fireEvent.click(screen.getByRole('button', { name: 'Keep this pipeline' }));
    await screen.findByRole('region', { name: 'Name this pipeline' });
    unmount();
    await waitFor(() => expect(writes(api, 'example')).toHaveLength(1));
    expect(writes(api, 'example')[0]!.headers['If-None-Match']).toBe('*');
  });

  it('offers “Run up to this node” once it is on disk, launching the file it wrote', async () => {
    const { api, container } = setup({ 'PUT /pipelines/{name}': { body: { name: 'example', etag: NEW_ETAG, hash: HASH } } }, { file: UNNAMED });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const runEntry = () => {
      fireEvent.keyDown(container.querySelector('.react-flow__node[data-id="fused"]')!, { key: 'F10', shiftKey: true });
      return within(screen.getByRole('menu', { name: 'Node fused' })).getByRole('menuitem', { name: /Run up to this node/ });
    };
    expect(runEntry().getAttribute('aria-disabled')).toBe('true');
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: 'Keep this pipeline' }));
    await nameIt();
    await waitFor(() => expect(saveLine().textContent).toBe('Saved'));
    const run = runEntry();
    expect(run.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(run);
    expect(window.location.hash).toBe('#runs?launch=example&up_to=fused');
  });

  it('asks its name at its first change, and is created under the name given', async () => {
    const onNamed = vi.fn();
    const { api } = setup({ 'PUT /pipelines/{name}': { body: { name: 'lexical-only', etag: NEW_ETAG, hash: HASH } } }, { file: UNNAMED, onNamed });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    place(/^rrf/);
    await screen.findByRole('region', { name: 'Name this pipeline' });
    expect(saveLine().textContent).toBe('Not saved yet: name it to save it');
    await settle();
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    await nameIt('lexical-only');
    await waitFor(() => expect(writes(api, 'lexical-only')).toHaveLength(1));
    expect(writes(api, 'lexical-only')[0]!.headers['If-None-Match']).toBe('*');
    await waitFor(() => expect(onNamed).toHaveBeenCalledWith('lexical-only'));
  });

  it('takes a name typed in its title as the one its first write offers', async () => {
    const { api } = setup({}, { file: UNNAMED });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const field = screen.getByRole('textbox', { name: 'Pipeline name' });
    fireEvent.change(field, { target: { value: 'mine' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(await screen.findByRole('heading', { name: 'mine' })).toBeTruthy();
    expect(api.requests.filter((r) => r.includes('/rename'))).toEqual([]);
    place(/^rrf/);
    const prompt = await screen.findByRole('region', { name: 'Name this pipeline' });
    expect((within(prompt).getByRole('textbox', { name: 'Pipeline name' }) as HTMLInputElement).value).toBe('mine');
  });
});

describe('a fork', () => {
  it('names the run it was forked from in the header', () => {
    setup({}, { forkedFrom: 'a'.repeat(64) });
    expect(screen.getByText(/Forked from run/).textContent).toContain('aaaaaaaa');
  });
});

describe('renaming a file from its title', () => {
  const RENAMED = { body: { name: 'lexical-only', etag: ETAG, modified_ms: null, hash: HASH, error: null, ends_in_answer: true, fault: null as string | null } };
  const rename = (to: string) => {
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    const field = screen.getByRole('textbox', { name: 'Pipeline name' });
    fireEvent.change(field, { target: { value: to } });
    fireEvent.keyDown(field, { key: 'Enter' });
  };

  it('moves the file on disk over the etag held, then writes on under the new name', async () => {
    const onNamed = vi.fn();
    const { api } = setup({ 'POST /pipelines/{name}/rename': RENAMED, 'PUT /pipelines/{name}': { body: { name: 'lexical-only', etag: NEW_ETAG, hash: HASH } } }, { onNamed });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    rename('lexical-only');
    expect(await screen.findByRole('heading', { name: 'lexical-only' })).toBeTruthy();
    const at = api.requests.indexOf('POST /api/v1/pipelines/hybrid/rename');
    expect(api.bodies[at]).toEqual({ to: 'lexical-only' });
    expect(api.headers[at]!['If-Match']).toBe(`"${ETAG}"`);
    await waitFor(() => expect(onNamed).toHaveBeenCalledWith('lexical-only'));
    place(/^rrf/);
    await waitFor(() => expect(writes(api, 'lexical-only')).toHaveLength(1));
    expect(writes(api, 'lexical-only')[0]!.headers['If-Match']).toBe(`"${ETAG}"`);
    expect(writes(api)).toEqual([]);
  });

  it('holds the document and its positions while the rename is out, then writes them under the new name', async () => {
    let answer: (reply: typeof RENAMED) => void = () => {};
    const out = new Promise<typeof RENAMED>((resolve) => (answer = resolve));
    const { api } = setup({ 'POST /pipelines/{name}/rename': () => out, 'PUT /pipelines/{name}': { body: { name: 'lexical-only', etag: NEW_ETAG, hash: HASH } } });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    rename('lexical-only');
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/pipelines/hybrid/rename'));
    place(/^rrf/);
    await settle();
    expect(api.requests.filter((r) => r.startsWith('PUT'))).toEqual([]);
    answer(RENAMED);
    await waitFor(() => expect(writes(api, 'lexical-only')).toHaveLength(1));
    await waitFor(() => expect(api.requests).toContain('PUT /api/v1/pipelines/lexical-only/layout'));
    expect(api.requests.filter((r) => r.startsWith('PUT /api/v1/pipelines/hybrid'))).toEqual([]);
  });

  it('takes a rename the server did but could not finish as done, and says what did not follow', async () => {
    const fault = 'pipeline hybrid is renamed lexical-only, but not all of it followed: its layout hybrid.layout.json stays where it was';
    const { api } = setup({ 'POST /pipelines/{name}/rename': { body: { ...RENAMED.body, fault } }, 'PUT /pipelines/{name}': { body: { name: 'lexical-only', etag: NEW_ETAG, hash: HASH } } });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    rename('lexical-only');
    expect(await screen.findByRole('heading', { name: 'lexical-only' })).toBeTruthy();
    expect(screen.getByText(fault, { exact: false })).toBeTruthy();
  });

  it('says why a rename was refused, and keeps the name', async () => {
    const { api } = setup({ 'POST /pipelines/{name}/rename': problem('pipeline_exists', 409, 'a pipeline named lexical already exists') });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    rename('lexical');
    const field = await screen.findByRole('textbox', { name: 'Pipeline name' });
    await waitFor(() => expect(field.getAttribute('aria-invalid')).toBe('true'));
    const help = document.getElementById(`${field.id}-help`)!;
    expect(help.querySelector('code')?.textContent).toBe('lexical');
    expect(help.textContent).toBe('A pipeline named lexical already exists: choose another name.');
    fireEvent.keyDown(field, { key: 'Escape' });
    expect(screen.getByRole('heading', { name: 'hybrid' })).toBeTruthy();
  });
});


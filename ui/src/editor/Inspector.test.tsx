/** @vitest-environment happy-dom */
// The inspector's words and controls: code spans for what the server and the
// editor name, ports counted from one, a small mark when nothing is wrong, a
// served parameter kept in its served kind, and a value committed while it
// is typed.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Problem } from '../api/types.ts';
import { float, int, list, str } from '../parameters.ts';
import type { WireDocument } from './document.ts';
import { Editor } from './Editor.tsx';
import { GRAMMAR, HYBRID, SERVICES, WORKSPACE } from './fixtures.ts';
import { PARAM_DEBOUNCE_MS } from './EditorInspector.tsx';

const HASH = 'c'.repeat(64);
const DRAFT: WireDocument = {
  pipeline: { inputs: HYBRID.pipeline.inputs, nodes: [...HYBRID.pipeline.nodes, { id: 'second', component: 'reranker', impl: 'cross_encoder', inputs: [], params: {} }] },
};

function Harness({ initial = DRAFT, start = null }: { initial?: WireDocument; start?: string | null }) {
  const [client] = useState(() => createApiClient());
  const [selected, setSelected] = useState<string | null>(start);
  return (
    <div style={{ width: 1400, height: 800 }}>
      <Editor client={client} title="Draft" initial={initial} capabilities={WORKSPACE.capabilities} services={SERVICES.services} grammar={GRAMMAR} selected={selected} onSelect={setSelected} />
    </div>
  );
}

function setup(routes: MockRoutes = { 'POST /pipelines/validate': { body: { hash: HASH, rendering: 'pipeline: {}\n' } } }, props: Parameters<typeof Harness>[0] = {}) {
  const api = mockApi(routes);
  const view = render(<Harness {...props} />);
  return { ...view, api };
}

const invalid = (detail: string, location: NonNullable<Problem['location']>): { problem: Problem } => ({
  problem: { type: 'urn:ragondin:problem:pipeline_invalid', title: 'Pipeline invalid', status: 422, detail, code: 'pipeline_invalid', hint: 'Correct it.', location },
});
const validations = (api: ReturnType<typeof mockApi>) => api.requests.flatMap((r, i) => (r === 'POST /api/v1/pipelines/validate' ? [api.bodies[i] as { typed: WireDocument }] : []));
const inspector = (id: string) => screen.getByRole('complementary', { name: id });

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('code spans, never literal backticks', () => {
  it('draws a taken id’s refusal with the id as code', async () => {
    setup(undefined, { start: 'lexical' });
    const field = within(inspector('lexical')).getByLabelText('Node id');
    fireEvent.change(field, { target: { value: 'vectors' } });
    fireEvent.blur(field);
    const help = await waitFor(() => {
      const line = document.getElementById(`${field.id}-help`);
      expect(line?.querySelector('code')?.textContent).toBe('vectors');
      return line!;
    });
    expect(help.textContent).not.toContain('`');
  });

  it('draws the server’s words in the header and on the node with their code spans', async () => {
    const { container } = setup({ 'POST /pipelines/validate': invalid('`a` feeding `b` would close a cycle', { node: 'fused', edge: null }) }, { start: 'fused' });
    await waitFor(() => expect(container.querySelector('.rg-editor__verdict code')?.textContent).toBe('a'));
    expect(container.querySelector('.rg-editor__verdict')!.textContent).not.toContain('`');
    const slot = inspector('fused').querySelector('.rg-editor-inspector__verdict')!;
    expect(slot.querySelector('code')?.textContent).toBe('a');
    expect(slot.textContent).not.toContain('`');
  });
});

describe('the input ports, counted from one', () => {
  it('names each row Input N with its kind and what feeds it, or that it is not connected', async () => {
    setup(undefined, { start: 'reranked' });
    const rows = within(inspector('reranked')).getAllByRole('listitem');
    expect(rows.map((r) => r.getAttribute('aria-label'))).toEqual(['Input 1 (query), from question', 'Input 2 (candidates), from fused']);
    expect(rows[0]!.textContent).toContain('Input 1 (query): question');
    expect(within(inspector('reranked')).getByRole('button', { name: 'Remove the edge into input 2' })).toBeTruthy();
  });

  it('says an empty port is not connected', () => {
    setup(undefined, { start: 'second' });
    const row = within(inspector('second')).getAllByRole('listitem')[0]!;
    expect(row.getAttribute('aria-label')).toBe('Input 1 (query), not connected');
    expect(row.textContent).toContain('Input 1 (query): not connected');
  });
});

describe('the verdict slot when nothing is wrong', () => {
  it('shows a small valid mark rather than a sentence', async () => {
    setup(undefined, { start: 'lexical' });
    const slot = () => inspector('lexical').querySelector('.rg-editor-inspector__verdict')!;
    await waitFor(() => expect(slot().getAttribute('data-valid')).toBe('true'));
    expect(slot().textContent).toBe('Valid');
  });
});

describe('a served parameter keeps its served kind', () => {
  it('offers no other kind for a served key whose value is in its kind', () => {
    setup(undefined, { start: 'lexical' });
    const panel = inspector('lexical');
    expect(within(panel).queryByRole('combobox', { name: 'Kind of top_k' })).toBeNull();
    expect(within(panel).getByText('Integer', { selector: '.rg-editor-inspector__kind' })).toBeTruthy();
  });

  it('offers its served kind back for a served key written in another kind, and nothing else', () => {
    const doc: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['question'], params: { top_k: str('10') } }] } };
    setup(undefined, { initial: doc, start: 'lexical' });
    const kind = within(inspector('lexical')).getByRole('combobox', { name: 'Kind of top_k' }) as HTMLSelectElement;
    expect([...kind.options].map((o) => o.textContent)).toEqual(['Text', 'Integer']);
  });

  it('still offers every kind for a key the implementation does not serve', () => {
    const doc: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['question'], params: { top_k: int('10'), extra: int('1') } }] } };
    setup(undefined, { initial: doc, start: 'lexical' });
    const kind = within(inspector('lexical')).getByRole('combobox', { name: 'Kind of extra' }) as HTMLSelectElement;
    expect(kind.options).toHaveLength(5);
  });
});

describe('a refusal under a field', () => {
  it('scrolls the field and its error into view, so the footer never hides it', async () => {
    const scrolled = vi.fn();
    Element.prototype.scrollIntoView = scrolled;
    setup(undefined, { start: 'lexical' });
    const field = within(inspector('lexical')).getByLabelText('top_k');
    fireEvent.change(field, { target: { value: 'many' } });
    fireEvent.blur(field);
    await waitFor(() => expect(field.getAttribute('aria-invalid')).toBe('true'));
    expect(scrolled).toHaveBeenCalledWith({ block: 'nearest' });
  });
});

describe('a value typed is committed after a short rest', () => {
  it('sends the new value without waiting for the field to lose focus', async () => {
    const { api } = setup(undefined, { start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('top_k');
    fireEvent.change(field, { target: { value: '25' } });
    await waitFor(() => expect(validations(api).at(-1)?.typed.pipeline.nodes[0]?.params.top_k).toEqual(int('25')), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    expect(document.activeElement === field || document.activeElement === document.body).toBe(true);
  });

  it('does not commit a value its kind cannot carry while it is typed, nor say so before the field is left', async () => {
    const { api } = setup(undefined, { start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('top_k');
    fireEvent.change(field, { target: { value: '2x' } });
    await new Promise((r) => setTimeout(r, PARAM_DEBOUNCE_MS + 400));
    expect(validations(api)).toHaveLength(1);
    expect(field.getAttribute('aria-invalid')).toBeNull();
  });
});

describe('a value committed after a rest is left as typed until the field is left', () => {
  // A float and a list on the first node: the two kinds whose field shows a value otherwise than it may be typed.
  const TYPED: WireDocument = {
    pipeline: { inputs: DRAFT.pipeline.inputs, nodes: DRAFT.pipeline.nodes.map((n, i) => (i === 0 ? { ...n, params: { ...n.params, weight: float(2.5), names: list(str('a')) } } : n)) },
  };
  const rest = () => new Promise((r) => setTimeout(r, PARAM_DEBOUNCE_MS + 300));
  const lastParams = (api: ReturnType<typeof mockApi>) => validations(api).at(-1)?.typed.pipeline.nodes[0]?.params;

  it('keeps "1." as typed once 1.0 is committed, so the next digit makes 1.5, and shows 1.5 when left', async () => {
    const { api } = setup(undefined, { initial: TYPED, start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('weight') as HTMLInputElement;
    fireEvent.change(field, { target: { value: '1.' } });
    await waitFor(() => expect(lastParams(api)?.weight).toEqual(float(1)), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    await rest();
    expect(field.value).toBe('1.');
    fireEvent.change(field, { target: { value: '1.5' } });
    await waitFor(() => expect(lastParams(api)?.weight).toEqual(float(1.5)), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    expect(field.value).toBe('1.5');
  });

  it('shows a committed float in its own form once the field is left', async () => {
    const { api } = setup(undefined, { initial: TYPED, start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('weight') as HTMLInputElement;
    fireEvent.change(field, { target: { value: '3.' } });
    await waitFor(() => expect(lastParams(api)?.weight).toEqual(float(3)), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    expect(field.value).toBe('3.');
    fireEvent.blur(field);
    await waitFor(() => expect(field.value).toBe('3.0'));
  });

  it('keeps a list ending in ", " as typed once it is committed, so the next item follows the comma', async () => {
    const { api } = setup(undefined, { initial: TYPED, start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('names') as HTMLInputElement;
    fireEvent.change(field, { target: { value: 'a, b, ' } });
    await waitFor(() => expect(lastParams(api)?.names).toEqual(list(str('a'), str('b'))), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    await rest();
    expect(field.value).toBe('a, b, ');
    fireEvent.change(field, { target: { value: 'a, b, c' } });
    await waitFor(() => expect(lastParams(api)?.names).toEqual(list(str('a'), str('b'), str('c'))), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    fireEvent.blur(field);
    expect(field.value).toBe('a, b, c');
  });

  it('shows a value changed from outside, an undo, in place of what was typed', async () => {
    const { api } = setup(undefined, { initial: TYPED, start: 'lexical' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = within(inspector('lexical')).getByLabelText('weight') as HTMLInputElement;
    fireEvent.change(field, { target: { value: '1.' } });
    await waitFor(() => expect(lastParams(api)?.weight).toEqual(float(1)), { timeout: PARAM_DEBOUNCE_MS + 1500 });
    fireEvent.keyDown(inspector('lexical'), { key: 'z', ctrlKey: true });
    await waitFor(() => expect(field.value).toBe('2.5'));
  });
});

describe('the header hash', () => {
  it('shows twelve characters, the whole hash in its title, and copies the whole hash on click', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    const { container } = setup();
    const button = await waitFor(() => within(container.querySelector('.rg-editor__verdict') as HTMLElement).getByRole('button', { name: /Copy the canonical hash/ }));
    expect(button.textContent).toContain(HASH.slice(0, 12));
    expect(button.textContent).not.toContain(HASH.slice(0, 13));
    expect(button.getAttribute('title')).toBe(HASH);
    fireEvent.click(button);
    expect(writeText).toHaveBeenCalledWith(HASH);
    expect(await within(container).findByText('Copied.')).toBeTruthy();
  });
});

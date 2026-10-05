/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { mockApi, type MockRoutes } from '../api/testing.ts';
import type { Problem } from '../api/types.ts';
import type { WireDocument } from './document.ts';
import { parseRules } from '../../design/testing/css.ts';
import editorCss from './Editor.css?raw';
import { Editor } from './Editor.tsx';
import { GRAMMAR, HYBRID, HYBRID_RAG, SERVICES, WORKSPACE } from './fixtures.ts';
import { bool, float, int, list, str } from '../parameters.ts';
import type { PortGrammar } from './ports.ts';
import { byWords } from '../words.testing.ts';

const HASH = 'b'.repeat(64);
// The hybrid, and a reranker with nothing wired yet.
const DRAFT: WireDocument = {
  pipeline: { inputs: HYBRID.pipeline.inputs, nodes: [...HYBRID.pipeline.nodes, { id: 'second', component: 'reranker', impl: 'cross_encoder', inputs: [], params: {} }] },
};

const invalid = (detail: string, location: NonNullable<Problem['location']>): { problem: Problem } => ({
  problem: { type: 'urn:ragondin:problem:pipeline_invalid', title: 'Pipeline invalid', status: 422, detail, code: 'pipeline_invalid', hint: 'Correct the pipeline, then validate it again.', location },
});
const KIND_MISMATCH =
  'the configuration wires two nodes incompatibly\n  edge: `lexical` feeds `reranked` at port 1\n  expected: chunks\n  found: query';

function Harness({ initial = DRAFT, grammar = GRAMMAR, start = null, stored = null }: { initial?: WireDocument; grammar?: PortGrammar | null; start?: string | null; stored?: string | null }) {
  const [client] = useState(() => createApiClient());
  const [selected, setSelected] = useState<string | null>(start);
  return (
    <div style={{ width: 1400, height: 800 }}>
      <Editor client={client} title="Draft" stored={stored} initial={initial} capabilities={WORKSPACE.capabilities} services={SERVICES.services} grammar={grammar} selected={selected} onSelect={setSelected} />
      <output data-testid="selected">{selected ?? ''}</output>
    </div>
  );
}

function setup(routes: MockRoutes = { 'POST /pipelines/validate': { body: { hash: HASH, rendering: null } } }, props: Parameters<typeof Harness>[0] = {}) {
  const api = mockApi(routes);
  const view = render(<Harness {...props} />);
  return { ...view, api };
}

const nodeEl = (root: HTMLElement, id: string) => root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement | null;
const inPort = (root: HTMLElement, id: string, port: number) => nodeEl(root, id)!.querySelectorAll<HTMLElement>('.rg-port[data-side="in"]')[port]!;
const outPort = (root: HTMLElement, id: string) => nodeEl(root, id)!.querySelector<HTMLElement>('.rg-port[data-side="out"]')!;
const validations = (api: ReturnType<typeof mockApi>) => api.requests.flatMap((r, i) => (r === 'POST /api/v1/pipelines/validate' ? [api.bodies[i] as { typed: WireDocument }] : []));
const nodesOf = (body: { typed: WireDocument }) => body.typed.pipeline.nodes;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('live validation', () => {
  it('sends the current wire-schema document after a mutation, and nothing else, and shows the server’s hash', async () => {
    const { api, container } = setup();
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    await waitFor(() => expect(validations(api)).toHaveLength(2));
    const sent = validations(api)[1]!;
    expect(Object.keys(sent)).toEqual(['typed']);
    const doc = sent.typed;
    expect(Object.keys(doc)).toEqual(['pipeline']);
    expect(nodesOf(sent).at(-1)).toEqual({ id: 'rrf', component: 'fusion', impl: 'rrf', inputs: [], params: {} });
    expect(JSON.stringify(sent)).not.toMatch(/"x"|"y"|position/);
    expect(await within(container).findByText(HASH.slice(0, 12))).toBeTruthy();
  });

  it('places a located error on the node and the edge it names, and in the inspector, in the server’s words, and announces it', async () => {
    const { container } = setup({ 'POST /pipelines/validate': invalid(KIND_MISMATCH, { node: 'reranked', edge: { from: 'fused', to: 'reranked', port: 1 } }) }, { start: 'reranked' });
    await waitFor(() => expect(nodeEl(container, 'reranked')!.querySelector('.rg-node')?.getAttribute('data-status')).toBe('invalid'));
    expect(within(nodeEl(container, 'reranked')!).getByText(/wires two nodes incompatibly/)).toBeTruthy();
    expect(container.querySelector('path.rg-edge[data-from="fused"][data-to="reranked"]')?.getAttribute('data-invalid')).toBe('true');
    const inspector = screen.getByRole('complementary', { name: 'reranked' });
    expect(within(inspector).getAllByText(/wires two nodes incompatibly/).length).toBeGreaterThan(0);
    const row = within(inspector).getByRole('listitem', { name: /Input 2/ });
    expect(row.getAttribute('data-invalid')).toBe('true');
    expect(row.querySelector('small')?.textContent).toBe('The server names this edge.');
    expect(screen.getAllByRole('status').map((s) => s.textContent).find((t) => t?.startsWith('Not valid:'))).toContain('wires two nodes incompatibly');
    expect(within(container).queryByText(HASH.slice(0, 12))).toBeNull();
  });

  it('never shows a hash of its own: the header waits for the server', async () => {
    let answer: (v: { body: { hash: string; rendering: string | null } }) => void = () => {};
    const { container, api } = setup({ 'POST /pipelines/validate': () => new Promise((resolve) => (answer = resolve)) });
    expect(await within(container).findByText('Checking with the server…')).toBeTruthy();
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    expect(within(container).getByText('Checking with the server…')).toBeTruthy();
    await act(async () => answer({ body: { hash: HASH, rendering: null } }));
    expect(await within(container).findByText(HASH.slice(0, 12))).toBeTruthy();
  });
});

describe('the inspector\'s verdict slot when the server names no problem here', () => {
  const slot = () => screen.getByRole('complementary', { name: 'lexical' }).querySelector('.rg-editor-inspector__verdict')!.textContent;

  it('says it is checking while a request is out, and that the node is clear once the document validates', async () => {
    setup(undefined, { start: 'lexical' });
    expect(slot()).toBe('Checking with the server…');
    await waitFor(() => expect(slot()).toBe('Valid'));
  });

  it('says validation stopped at another problem, since the server names only the first', async () => {
    setup({ 'POST /pipelines/validate': invalid(KIND_MISMATCH, { node: 'reranked', edge: null }) }, { start: 'lexical' });
    await waitFor(() => expect(slot()).toBe('The server stopped at a problem elsewhere; it has not judged this node past it.'));
  });

  it('says there is no verdict when the request failed', async () => {
    setup({ 'POST /pipelines/validate': { network: 'down' } }, { start: 'lexical' });
    await waitFor(() => expect(slot()).toBe('No verdict: the request to the server failed.'));
  });
});

describe('edges drawn by drag, judged by the grammar', () => {
  it('refuses a chunks-producing port onto a port expecting a query, naming both kinds, and creates no edge', async () => {
    const { api, container } = setup();
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    const query = inPort(container, 'second', 0);
    expect(query.getAttribute('data-drop')).toBe('refused');
    expect(query.getAttribute('title')).toBe('`lexical` feeds `second` at port 0: expected query, found chunks.');
    fireEvent.pointerUp(query);
    await new Promise((r) => setTimeout(r, 450));
    expect(validations(api)).toHaveLength(1);
  });

  it('creates the edge on a compatible port, as one undoable step', async () => {
    const { api, container } = setup();
    fireEvent.pointerDown(outPort(container, 'question'), { button: 0 });
    fireEvent.pointerUp(inPort(container, 'second', 0));
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'second')?.inputs).toEqual(['question']));
    expect(container.querySelector('path.rg-edge[data-from="question"][data-to="second"]')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(container.querySelector('path.rg-edge[data-from="question"][data-to="second"]')).toBeNull();
    expect(nodeEl(container, 'second')).toBeTruthy();
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'second')?.inputs).toEqual([]));
  });

  it('refuses an edge that would close a cycle, as such, during the drag', () => {
    const { container } = setup();
    fireEvent.pointerDown(outPort(container, 'reranked'), { button: 0 });
    expect(inPort(container, 'fused', 2).getAttribute('title')).toBe('`reranked` feeding `fused` would close a cycle: `fused` already feeds `reranked`.');
  });

  it('refuses an occupied port with what it holds', () => {
    const { container } = setup();
    fireEvent.pointerDown(outPort(container, 'vectors'), { button: 0 });
    expect(inPort(container, 'reranked', 1).getAttribute('title')).toBe('Port 1 of `reranked` already holds `fused`.');
  });
});

describe('the node menu', () => {
  const openMenu = (container: HTMLElement, id: string) => {
    fireEvent.keyDown(nodeEl(container, id)!, { key: 'F10', shiftKey: true });
    return screen.getByRole('menu', { name: `Node ${id}` });
  };

  it('opens on Shift+F10 with its entries, focus on the first', () => {
    const { container } = setup();
    const menu = openMenu(container, 'fused');
    expect(within(menu).getAllByRole('menuitem').map((m) => m.querySelector('b')?.textContent)).toEqual(['Open parameters', 'Duplicate', 'Connect output to…', 'Run up to this node', 'Delete node']);
    expect(document.activeElement).toBe(within(menu).getAllByRole('menuitem')[0]);
  });

  describe('"Run up to this node"', () => {
    const OPENED = { initial: HYBRID_RAG, stored: 'hybrid-rag' };
    const runEntry = (container: HTMLElement, id: string) => within(openMenu(container, id)).getByRole('menuitem', { name: /Run up to this node/ });

    it('is enabled on a ranking node, its second line naming what is kept and skipped and the generation saved, and opens the launch panel cut there', () => {
      window.history.replaceState(null, '', '/#editor/hybrid-rag');
      const { container } = setup(undefined, OPENED);
      const run = runEntry(container, 'reranked');
      expect(run.getAttribute('aria-disabled')).toBeNull();
      expect(within(run).getByText('Keeps lexical, vectors, fused and reranked. Skips context and answer, so no generation cost.')).toBeTruthy();
      fireEvent.click(run);
      expect(window.location.hash).toBe('#runs?launch=hybrid-rag&up_to=reranked');
    });

    it('is refused, with its reason, on the pipeline’s output and on a context builder', () => {
      const { container } = setup(undefined, OPENED);
      const output = runEntry(container, 'answer');
      expect(output.getAttribute('aria-disabled')).toBe('true');
      expect(within(output).getByText('This node is the pipeline’s output: the prefix would be the whole pipeline.')).toBeTruthy();
      fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
      const context = runEntry(container, 'context');
      expect(context.getAttribute('aria-disabled')).toBe('true');
      expect(within(context).getByText('A context is scored by nothing: run up to the node that feeds it chunks.')).toBeTruthy();
    });

    it('is refused on a pipeline that is not stored, or whose canvas changed since it was opened', () => {
      const { container, unmount } = setup(undefined, { initial: HYBRID_RAG });
      expect(within(runEntry(container, 'reranked')).getByText('Not a workspace pipeline yet: a run takes a stored document.')).toBeTruthy();
      unmount();
      const opened = setup(undefined, OPENED);
      fireEvent.click(within(openMenu(opened.container, 'vectors')).getByRole('menuitem', { name: /Duplicate/ }));
      expect(within(runEntry(opened.container, 'reranked')).getByText('The canvas differs from the stored document, and a run takes the stored one.')).toBeTruthy();
    });
  });

  it('deletes a node, leaving its consumers marked invalid and no edge to the missing node', async () => {
    const { api, container } = setup();
    fireEvent.click(within(openMenu(container, 'fused')).getByRole('menuitem', { name: /Delete node/ }));
    expect(nodeEl(container, 'fused')).toBeNull();
    expect(container.querySelector('path.rg-edge[data-from="fused"]')).toBeNull();
    expect(nodeEl(container, 'reranked')!.querySelector('.rg-node')?.getAttribute('data-status')).toBe('invalid');
    expect(within(nodeEl(container, 'reranked')!).getByText(byWords('Port 1 names `fused`, which is not a node or an input.'))).toBeTruthy();
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'reranked')?.inputs).toEqual(['question', 'fused']));
  });

  it('duplicates a node and selects the copy', () => {
    const { container } = setup();
    fireEvent.click(within(openMenu(container, 'lexical')).getByRole('menuitem', { name: /Duplicate/ }));
    expect(nodeEl(container, 'bm25')).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'bm25' })).toBeTruthy();
  });

  it('connects by keyboard: "Connect output to…" lists every port, each open or refused with its reason', () => {
    const { container } = setup();
    fireEvent.click(within(openMenu(container, 'lexical')).getByRole('menuitem', { name: /Connect output to/ }));
    const menu = screen.getByRole('menu', { name: 'Node lexical' });
    const refused = within(menu).getByRole('menuitem', { name: /^second, port 0/ });
    expect(refused.getAttribute('aria-disabled')).toBe('true');
    expect(within(refused).getByText(byWords('`lexical` feeds `second` at port 0: expected query, found chunks.'))).toBeTruthy();
    expect(document.activeElement).toBe(within(menu).getAllByRole('menuitem')[0]);
    fireEvent.click(within(menu).getByRole('menuitem', { name: /^fused, port 2/ }));
    expect(container.querySelector('path.rg-edge[data-from="lexical"][data-to="fused"][data-port="2"]')).toBeTruthy();
  });

  it('opens a declared input\'s menu straight on the ports it can feed, with no one-entry menu before them', () => {
    const { container } = setup();
    const menu = openMenu(container, 'question');
    const titles = within(menu).getAllByRole('menuitem').map((m) => m.querySelector('b')?.textContent);
    expect(titles).not.toContain('Connect output to…');
    expect(titles).not.toContain('Back to the node menu');
    expect(titles[0]).toMatch(/^lexical, port 0/);
    expect(document.activeElement).toBe(within(menu).getAllByRole('menuitem')[0]);
    fireEvent.click(within(menu).getByRole('menuitem', { name: /^second, port 0/ }));
    expect(container.querySelector('path.rg-edge[data-from="question"][data-to="second"][data-port="0"]')).toBeTruthy();
  });

  it('says so when a declared input has no port to feed yet', () => {
    const { container } = setup(undefined, { initial: { pipeline: { inputs: ['question'], nodes: [] } } });
    const menu = openMenu(container, 'question');
    const only = within(menu).getByRole('menuitem');
    expect(only.getAttribute('aria-disabled')).toBe('true');
    expect(only.textContent).toBe('No input port to connect toPlace a node from the palette first.');
  });

  it('after a delete, gives focus to the node that fed it, so the next undo is heard and brings the node back', () => {
    const { container } = setup();
    fireEvent.click(within(openMenu(container, 'fused')).getByRole('menuitem', { name: /Delete node/ }));
    expect(document.activeElement).toBe(nodeEl(container, 'lexical'));
    fireEvent.keyDown(document.activeElement!, { key: 'z', metaKey: true });
    expect(nodeEl(container, 'fused')).toBeTruthy();
  });

  it('hears undo with focus on the page itself', () => {
    const { container } = setup();
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    act(() => (document.activeElement as HTMLElement | null)?.blur());
    expect(document.activeElement).toBe(document.body);
    fireEvent.keyDown(document.body, { key: 'z', ctrlKey: true });
    expect(nodeEl(container, 'rrf')).toBeNull();
  });

  it('only offers to remove the last edge into a node, so no input slides into another port', () => {
    setup(undefined, { start: 'reranked' });
    const inspector = screen.getByRole('complementary', { name: 'reranked' });
    expect(within(inspector).queryByRole('button', { name: 'Remove the edge into input 1' })).toBeNull();
    expect(within(inspector).getByRole('button', { name: 'Remove the edge into input 2' })).toBeTruthy();
  });

  it('opens the parameters: selects the node and moves focus into the inspector', () => {
    const { container } = setup();
    fireEvent.click(within(openMenu(container, 'lexical')).getByRole('menuitem', { name: /Open parameters/ }));
    const inspector = screen.getByRole('complementary', { name: 'lexical' });
    expect(inspector.contains(document.activeElement)).toBe(true);
  });
});

describe('a selection undo or redo takes away', () => {
  const palette = () => within(screen.getByRole('region', { name: 'Palette' }));
  const noInspector = (id: string) => {
    expect(screen.getByTestId('selected').textContent).toBe('');
    expect(screen.queryByRole('complementary', { name: id })).toBeNull();
    expect(screen.queryByText('pipeline input', { selector: '.rg-canvas__inspector *' })).toBeNull();
  };

  it('is cleared when undo takes away a node just placed, never shown as a declared input', () => {
    const { container } = setup();
    fireEvent.click(palette().getByRole('button', { name: /^rrf/ }));
    expect(screen.getByRole('complementary', { name: 'rrf' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(nodeEl(container, 'rrf')).toBeNull();
    noInspector('rrf');
  });

  it('is cleared when undo takes back a rename', () => {
    const { container } = setup(undefined, { start: 'fused' });
    const id = screen.getByLabelText('Node id');
    fireEvent.change(id, { target: { value: 'rrf' } });
    fireEvent.keyDown(id, { key: 'Enter' });
    expect(screen.getByRole('complementary', { name: 'rrf' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(nodeEl(container, 'fused')).toBeTruthy();
    noInspector('rrf');
  });

  it('is cleared when undo takes away a duplicate', () => {
    const { container } = setup();
    fireEvent.keyDown(nodeEl(container, 'lexical')!, { key: 'F10', shiftKey: true });
    fireEvent.click(within(screen.getByRole('menu', { name: 'Node lexical' })).getByRole('menuitem', { name: /Duplicate/ }));
    expect(screen.getByRole('complementary', { name: 'bm25' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(nodeEl(container, 'bm25')).toBeNull();
    noInspector('bm25');
  });

  it('still shows a declared input as one', () => {
    setup(undefined, { start: 'question' });
    expect(within(screen.getByRole('complementary', { name: 'question' })).getByText('pipeline input')).toBeTruthy();
  });

  it('lets a request to enter the inspector expire once another node is selected, so focus is not taken into it', () => {
    const { container } = setup();
    fireEvent.keyDown(nodeEl(container, 'lexical')!, { key: 'F10', shiftKey: true });
    const open = within(screen.getByRole('menu', { name: 'Node lexical' })).getByRole('menuitem', { name: /Open parameters/ });
    act(() => {
      open.click();
      nodeEl(container, 'vectors')!.click();
    });
    expect(screen.getByTestId('selected').textContent).toBe('vectors');
    expect(screen.getByRole('complementary', { name: 'vectors' }).contains(document.activeElement)).toBe(false);
  });

  it('lets a focus request for a node that went away expire, so a redo later does not take focus', () => {
    const { container } = setup();
    act(() => {
      palette().getByRole('button', { name: /^rrf/ }).click();
      container.querySelector<HTMLElement>('.rg-editor')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true }));
    });
    expect(nodeEl(container, 'rrf')).toBeNull();
    const concat = palette().getByRole('button', { name: /^concat/ });
    act(() => concat.focus());
    fireEvent.keyDown(concat, { key: 'z', ctrlKey: true, shiftKey: true });
    expect(nodeEl(container, 'rrf')).toBeTruthy();
    expect(document.activeElement).toBe(concat);
  });
});

describe('the keyboard', () => {
  it('opens the insert list on `/` and places the chosen entry, then focuses it', async () => {
    const { container } = setup();
    fireEvent.keyDown(container.querySelector('.react-flow')!, { key: '/' });
    const list = screen.getByRole('menu', { name: 'Insert a node' });
    expect(document.activeElement).toBe(within(list).getAllByRole('menuitem')[0]);
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toMatch(/^dense/);
    fireEvent.keyDown(document.activeElement!, { key: 'Enter' });
    expect(screen.queryByRole('menu', { name: 'Insert a node' })).toBeNull();
    expect(nodeEl(container, 'dense')).toBeTruthy();
    // The library draws a node a frame after the render that adds it, and focus follows it there.
    await waitFor(() => expect(document.activeElement).toBe(nodeEl(container, 'dense')));
  });

  it('places a node with the starting value of each required key, and nothing for an optional one', async () => {
    const { api, container } = setup();
    fireEvent.keyDown(container.querySelector('.react-flow')!, { key: '/' });
    fireEvent.keyDown(document.activeElement!, { key: 'Enter' });
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'bm25')?.params).toEqual({ top_k: int('10') }));
  });

  it('closes the insert list on Escape, placing nothing', () => {
    const { container } = setup();
    fireEvent.keyDown(container.querySelector('.react-flow')!, { key: '/' });
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(screen.queryByRole('menu', { name: 'Insert a node' })).toBeNull();
    expect(container.querySelectorAll('.react-flow__node')).toHaveLength(6);
  });

  it('undoes and redoes from the keyboard, and from the two buttons', () => {
    const { container } = setup();
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    expect(nodeEl(container, 'rrf')).toBeTruthy();
    fireEvent.keyDown(container.querySelector('.rg-editor')!, { key: 'z', ctrlKey: true });
    expect(nodeEl(container, 'rrf')).toBeNull();
    fireEvent.keyDown(container.querySelector('.rg-editor')!, { key: 'z', metaKey: true, shiftKey: true });
    expect(nodeEl(container, 'rrf')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(nodeEl(container, 'rrf')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Redo' }));
    expect(nodeEl(container, 'rrf')).toBeTruthy();
  });

  it('undoes with focus on a checkbox, which has no undo of its own', async () => {
    const { api } = setup(undefined, { start: 'fused' });
    fireEvent.change(screen.getByLabelText('New parameter'), { target: { value: 'normalize' } });
    fireEvent.change(screen.getByLabelText('Kind'), { target: { value: 'bool' } });
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'true' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
    const box = screen.getByRole('checkbox', { name: 'normalize' });
    fireEvent.keyDown(box, { key: 'z', ctrlKey: true });
    expect(screen.queryByRole('checkbox', { name: 'normalize' })).toBeNull();
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params).toEqual({ k: int('60') }));
  });

  it('leaves Ctrl+Z inside a text field to the field', () => {
    const { container } = setup(undefined, { start: 'lexical' });
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    fireEvent.keyDown(screen.getByLabelText('New parameter'), { key: 'z', ctrlKey: true });
    expect(nodeEl(container, 'rrf')).toBeTruthy();
  });
});

describe('the inspector’s "Run up to here"', () => {
  it('opens the launch panel cut at the node, and is refused with the menu’s reason where the menu refuses', () => {
    window.history.replaceState(null, '', '/#editor/hybrid-rag');
    const { unmount } = setup(undefined, { initial: HYBRID_RAG, stored: 'hybrid-rag', start: 'fused' });
    const run = screen.getByRole('button', { name: 'Run up to here' });
    expect(run.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(run);
    expect(window.location.hash).toBe('#runs?launch=hybrid-rag&up_to=fused');
    unmount();

    setup(undefined, { initial: HYBRID_RAG, stored: 'hybrid-rag', start: 'context' });
    const refused = screen.getByRole('button', { name: 'Run up to here' });
    expect(refused.getAttribute('aria-disabled')).toBe('true');
    expect(screen.getByText('A context is scored by nothing: run up to the node that feeds it chunks.')).toBeTruthy();
  });
});

describe('the inspector in write mode', () => {
  it('shows the family, the implementation and the id, and every parameter as a field in key order', () => {
    setup(undefined, { start: 'vectors' });
    const inspector = screen.getByRole('complementary', { name: 'vectors' });
    expect(within(inspector).getByText('retriever/dense')).toBeTruthy();
    expect((within(inspector).getByLabelText('Node id') as HTMLInputElement).value).toBe('vectors');
    expect(within(inspector).getAllByRole('textbox').map((f) => f.getAttribute('id')?.split('-param-').at(-1)?.split('-').at(-1))).toEqual([
      'id',
      'embedder',
      'passage_prefix',
      'query_prefix',
      'served_model',
      'top_k',
      'key',
      'value',
    ]);
  });

  it('lists every parameter the implementation takes, the ones not set included, each with its kind and what it is for', () => {
    setup(undefined, { start: 'vectors' });
    const inspector = screen.getByRole('complementary', { name: 'vectors' });
    const prefix = within(inspector).getByLabelText('query_prefix') as HTMLInputElement;
    expect(prefix.value).toBe('');
    expect(within(inspector).getByText('Text prepended to a query before it is embedded; absent, none. Never empty.')).toBeTruthy();
    expect(within(inspector).getAllByText(/^Not set · text/)).toHaveLength(2);
    // The set ones say what they are for too.
    expect(within(inspector).getByText('How many chunks the node returns, best first.')).toBeTruthy();
  });

  it('says a required key the node does not set is required, as information, before any save is attempted', () => {
    const { container } = setup(undefined, { initial: HYBRID_RAG, start: 'context' });
    const inspector = screen.getByRole('complementary', { name: 'context' });
    expect(within(inspector).getAllByText('Required.')).toHaveLength(2);
    expect(within(inspector).queryByText('Required: the pipeline cannot be saved or run without it.')).toBeNull();
    expect(within(inspector).getByLabelText('budget').getAttribute('aria-invalid')).toBeNull();
    expect(nodeEl(container, 'context')!.querySelector('.rg-node')?.getAttribute('data-status')).not.toBe('invalid');
  });

  it('cues a node with required keys to set on its card from the moment it is there, as information, not as an error', () => {
    const { container } = setup(undefined, { initial: HYBRID_RAG });
    const card = nodeEl(container, 'context')!;
    const cue = card.querySelector('.rg-node__todo');
    expect(cue?.textContent).toBe('2 parameters to set');
    expect(card.querySelector('.rg-node')?.getAttribute('data-status')).toBeNull();
    expect(card.querySelector('.rg-node__msg')).toBeNull();
    expect(card.getAttribute('aria-label')).toContain('2 parameters to set');
    // A complete node has none.
    expect(nodeEl(container, 'lexical')!.querySelector('.rg-node__todo')).toBeNull();
  });

  it('links a parameter not set to the line saying so and to what it is for', () => {
    setup(undefined, { initial: HYBRID_RAG, start: 'context' });
    const field = screen.getByLabelText('budget');
    const described = (field.getAttribute('aria-describedby') ?? '').split(' ').map((id) => document.getElementById(id)?.textContent);
    expect(described).toContain('Not set · integer, zero or more');
    expect(described).toContain('The cap on the size of the context, in the unit the implementation counts.');
    expect(described).toContain('Required.');
  });

  it('links a set parameter to what it is for', () => {
    setup(undefined, { start: 'lexical' });
    const field = screen.getByLabelText('top_k');
    const described = (field.getAttribute('aria-describedby') ?? '').split(' ').map((id) => document.getElementById(id)?.textContent);
    expect(described).toContain('How many chunks the node returns, best first.');
  });

  it('sets a text parameter to the empty text on Enter in its empty field, and never on leaving it', async () => {
    const { api } = setup(undefined, { initial: HYBRID_RAG, start: 'context' });
    const separator = screen.getByLabelText('separator');
    fireEvent.blur(separator);
    fireEvent.keyDown(separator, { key: 'Enter' });
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'context')?.params).toEqual({ separator: str('') }));
  });

  it('sets a parameter not yet set, in the kind it is served with', async () => {
    const { api } = setup(undefined, { initial: HYBRID_RAG, start: 'context' });
    const field = screen.getByLabelText('budget');
    fireEvent.change(field, { target: { value: '500' } });
    fireEvent.blur(field);
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'context')?.params).toEqual({ budget: int('500') }));
    const separator = screen.getByLabelText('separator');
    fireEvent.change(separator, { target: { value: '12' } });
    fireEvent.blur(separator);
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'context')?.params).toEqual({ budget: int('500'), separator: str('12') }));
  });

  it('refuses a value the served kind cannot carry, in words, and sets nothing', () => {
    setup(undefined, { initial: HYBRID_RAG, start: 'context' });
    const field = screen.getByLabelText('budget');
    fireEvent.change(field, { target: { value: 'lots' } });
    fireEvent.blur(field);
    expect(screen.getByText('An integer is a whole number, such as 60: "lots" is not one.')).toBeTruthy();
  });

  it('sets a parameter, keeping its type', async () => {
    const { api } = setup(undefined, { start: 'lexical' });
    const field = screen.getByLabelText('top_k');
    fireEvent.change(field, { target: { value: '50' } });
    fireEvent.blur(field);
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'lexical')?.params).toEqual({ top_k: int('50') }));
  });

  it('refuses an id another node has, before the server does, and renames on a free one', () => {
    const { container } = setup(undefined, { start: 'fused' });
    const id = screen.getByLabelText('Node id');
    fireEvent.change(id, { target: { value: 'lexical' } });
    fireEvent.keyDown(id, { key: 'Enter' });
    expect(screen.getByText(byWords('`lexical` is taken: a node, an input or an edge already names it.'))).toBeTruthy();
    expect(nodeEl(container, 'fused')).toBeTruthy();
    fireEvent.change(id, { target: { value: 'rrf' } });
    fireEvent.keyDown(id, { key: 'Enter' });
    expect(nodeEl(container, 'rrf')).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'rrf' })).toBeTruthy();
  });

  it.each([
    ['10', int('10')],
    ['-3', int('-3')],
    ['.5', float(0.5)],
    ['1.', float(1)],
    ['1.0', float(1)],
    ['1e3', float(1000)],
    ['-2.5e-1', float(-0.25)],
    ['a, b', str('a, b')],
    ['true', str('true')],
    ['bge', str('bge')],
  ])('reads a new value %s, of no kind picked, as %j', async (text, value) => {
    const { api } = setup(undefined, { start: 'fused' });
    fireEvent.change(screen.getByLabelText('New parameter'), { target: { value: 'w' } });
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: text } });
    fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params['w']).toEqual(value));
  });

  it.each([
    ['bool', 'true', bool(true)],
    ['list', 'a, b', list(str('a'), str('b'))],
    ['list', '1, 2.5', list(int('1'), float(2.5))],
    ['float', '60', float(60)],
    ['string', '60', str('60')],
  ])('reads a new value of the kind picked, %s, from %s', async (kind, text, value) => {
    const { api } = setup(undefined, { start: 'fused' });
    fireEvent.change(screen.getByLabelText('New parameter'), { target: { value: 'w' } });
    fireEvent.change(screen.getByLabelText('Kind'), { target: { value: kind } });
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: text } });
    fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params['w']).toEqual(value));
  });

  it('refuses a new value its kind cannot carry, in words, and adds nothing', () => {
    setup(undefined, { start: 'fused' });
    const inspector = within(screen.getByRole('complementary', { name: 'fused' }));
    fireEvent.change(screen.getByLabelText('New parameter'), { target: { value: 'w' } });
    fireEvent.change(screen.getByLabelText('Kind'), { target: { value: 'int' } });
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: '1.5' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
    expect(inspector.getByText('An integer is a whole number, such as 60: "1.5" is not one.')).toBeTruthy();
    expect(screen.queryByLabelText('w')).toBeNull();
  });

  // A key the implementation does not serve, so every kind is offered: a served one keeps its served kind.
  const UNSERVED: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: [], params: { k: int('60'), weight: int('60'), tag: str('bge') } }] } };

  it('shows each value’s kind beside it, and changes the kind alone when another is picked', async () => {
    const { api } = setup(undefined, { initial: UNSERVED, start: 'fused' });
    const kind = screen.getByLabelText('Kind of weight') as HTMLSelectElement;
    expect(kind.value).toBe('int');
    fireEvent.change(kind, { target: { value: 'float' } });
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params['weight']).toEqual(float(60)));
    expect((screen.getByLabelText('weight') as HTMLInputElement).value).toBe('60.0');
    fireEvent.change(screen.getByLabelText('Kind of weight'), { target: { value: 'string' } });
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params['weight']).toEqual(str('60.0')));
  });

  it('refuses a change of kind the value cannot take, in words, and keeps the value', () => {
    setup({ 'POST /pipelines/validate': { body: { hash: HASH, rendering: null } } }, { initial: UNSERVED, start: 'fused' });
    fireEvent.change(screen.getByLabelText('Kind of tag'), { target: { value: 'int' } });
    expect(screen.getByText('An integer is a whole number, such as 60: "bge" is not one.')).toBeTruthy();
    expect((screen.getByLabelText('Kind of tag') as HTMLSelectElement).value).toBe('string');
  });

  it('draws a float with its fractional part, so 60.0 never reads as the integer 60', () => {
    const initial: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: [], params: { k: float(60) } }] } };
    setup(undefined, { initial, start: 'fused' });
    expect((screen.getByLabelText('k') as HTMLInputElement).value).toBe('60.0');
    expect((screen.getByLabelText('Kind of k') as HTMLSelectElement).value).toBe('float');
  });

  it.each([
    ['1e999', 'A float is a finite number: "1e999" is not one.'],
    ['Infinity', 'A float is a finite number: "Infinity" is not one.'],
    ['NaN', 'A float is a finite number: "NaN" is not one.'],
  ])('refuses a non-finite float, %s, in words, before it is sent', async (text, words) => {
    const initial: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: [], params: { k: float(60) } }] } };
    const { api } = setup(undefined, { initial, start: 'fused' });
    await waitFor(() => expect(validations(api)).toHaveLength(1));
    const field = screen.getByLabelText('k');
    fireEvent.change(field, { target: { value: text } });
    fireEvent.blur(field);
    expect(screen.getByText(words)).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(validations(api)).toHaveLength(1);
  });

  it('keeps an integer whole, however wide, and refuses one wider than 64 bits', async () => {
    const { api } = setup(undefined, { start: 'lexical' });
    const field = screen.getByLabelText('top_k');
    fireEvent.change(field, { target: { value: '-9223372036854775808' } });
    fireEvent.blur(field);
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'lexical')?.params['top_k']).toEqual(int('-9223372036854775808')));
    fireEvent.change(field, { target: { value: '9223372036854775808' } });
    fireEvent.blur(field);
    expect(screen.getByText('An integer has at most 64 bits: "9223372036854775808" is wider.')).toBeTruthy();
  });

  it('refuses a rename to an id a dangling input still names, which would silently take its edge back', () => {
    const { container } = setup();
    fireEvent.keyDown(nodeEl(container, 'fused')!, { key: 'F10', shiftKey: true });
    fireEvent.click(within(screen.getByRole('menu', { name: 'Node fused' })).getByRole('menuitem', { name: /Delete node/ }));
    fireEvent.click(nodeEl(container, 'vectors')!);
    const id = screen.getByLabelText('Node id');
    fireEvent.change(id, { target: { value: 'fused' } });
    fireEvent.keyDown(id, { key: 'Enter' });
    expect(screen.getByText(byWords('`fused` is taken: a node, an input or an edge already names it.'))).toBeTruthy();
    expect(nodeEl(container, 'vectors')).toBeTruthy();
    expect(nodeEl(container, 'fused')).toBeNull();
  });

  describe('a value nobody changed is left as it is', () => {
    const LISTS: WireDocument = {
      pipeline: {
        inputs: ['question'],
        nodes: [
          {
            id: 'fused',
            component: 'fusion',
            impl: 'rrf',
            inputs: [],
            params: {
              k: float(60),
              label: str(' 60'),
              mixed: list(int('1'), float(0.5)),
              nested: list(list(str('a'), str('b')), str('c')),
              texts: list(str('10'), str('true')),
            },
          },
        ],
      },
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 300));

    it('sends nothing when a field is tabbed through with its text untouched', async () => {
      const { api } = setup(undefined, { initial: LISTS, start: 'fused' });
      await waitFor(() => expect(validations(api)).toHaveLength(1));
      for (const name of ['k', 'label', 'mixed', 'nested', 'texts']) {
        const field = screen.getByLabelText(name);
        fireEvent.focus(field);
        fireEvent.blur(field);
        fireEvent.keyDown(field, { key: 'Enter' });
      }
      await settle();
      expect(validations(api)).toHaveLength(1);
      expect(screen.queryByText(/is not one|is neither|is wider/)).toBeNull();
    });

    it('gives each item of a list its own kind, so a mixed list stays editable', async () => {
      const { api } = setup(undefined, { initial: LISTS, start: 'fused' });
      const field = screen.getByLabelText('mixed') as HTMLInputElement;
      expect(field.value).toBe('1, 0.5');
      fireEvent.change(field, { target: { value: '1, 0.75' } });
      fireEvent.blur(field);
      await waitFor(() => expect(nodesOf(validations(api).at(-1)!)[0]!.params['mixed']).toEqual(list(int('1'), float(0.75))));
      fireEvent.change(field, { target: { value: '2, 3, 0.25' } });
      fireEvent.blur(field);
      await waitFor(() => expect(nodesOf(validations(api).at(-1)!)[0]!.params['mixed']).toEqual(list(int('2'), int('3'), float(0.25))));
    });

    it('matches the items by their text, not their place, when the list grows or shrinks', async () => {
      const initial: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: [], params: { w: list(int('1'), float(0.5), int('2')), t: list(str('10'), str('x')) } }] } };
      const { api } = setup(undefined, { initial, start: 'fused' });
      const w = screen.getByLabelText('w');
      fireEvent.change(w, { target: { value: '1, 2' } });
      fireEvent.blur(w);
      await waitFor(() => expect(nodesOf(validations(api).at(-1)!)[0]!.params['w']).toEqual(list(int('1'), int('2'))));
      const t = screen.getByLabelText('t');
      fireEvent.change(t, { target: { value: 'y, x, 10' } });
      fireEvent.blur(t);
      await waitFor(() => expect(nodesOf(validations(api).at(-1)!)[0]!.params['t']).toEqual(list(str('y'), str('x'), str('10'))));
    });

    it('keeps the items a person did not touch as they were, a text item reading as a number included', async () => {
      const { api } = setup(undefined, { initial: LISTS, start: 'fused' });
      const field = screen.getByLabelText('texts');
      fireEvent.change(field, { target: { value: '10, true, 7' } });
      fireEvent.blur(field);
      await waitFor(() => expect(nodesOf(validations(api).at(-1)!)[0]!.params['texts']).toEqual(list(str('10'), str('true'), int('7'))));
    });

    it('shows a list holding a list, but does not edit it, and says why', async () => {
      const { api } = setup(undefined, { initial: LISTS, start: 'fused' });
      await waitFor(() => expect(validations(api)).toHaveLength(1));
      const field = screen.getByLabelText('nested') as HTMLInputElement;
      expect(field.readOnly).toBe(true);
      expect(screen.getByText('A list holding a list is shown, not edited, here: its field would flatten it. Edit it in the file.')).toBeTruthy();
      expect((screen.getByLabelText('Kind of nested') as HTMLSelectElement).disabled).toBe(true);
      fireEvent.change(field, { target: { value: 'a, b, c' } });
      fireEvent.blur(field);
      await settle();
      expect(validations(api)).toHaveLength(1);
    });

    it('shows a list holding text with a comma, but does not edit it, and says why', () => {
      const initial: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: [], params: { names: list(str('a, b'), str('c')) } }] } };
      setup(undefined, { initial, start: 'fused' });
      expect((screen.getByLabelText('names') as HTMLInputElement).readOnly).toBe(true);
      expect(screen.getByText('A list holding text with a comma, or with spaces at its ends, is shown, not edited, here: its field would split or trim it. Edit it in the file.')).toBeTruthy();
    });
  });

  it('adds and removes a parameter', async () => {
    const { api } = setup(undefined, { start: 'fused' });
    fireEvent.change(screen.getByLabelText('New parameter'), { target: { value: 'weights' } });
    fireEvent.change(screen.getByLabelText('Kind'), { target: { value: 'list' } });
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: '0.7, 0.3' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add parameter' }));
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params).toEqual({ k: int('60'), weights: list(float(0.7), float(0.3)) }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove k' }));
    await waitFor(() => expect(nodesOf(validations(api).at(-1)!).find((n) => n.id === 'fused')?.params).toEqual({ weights: list(float(0.7), float(0.3)) }));
  });
});

describe('a parameter row', () => {
  const rule = (selector: string) => parseRules(editorCss).find((r) => r.selector === selector);
  it('keeps the value field a width of its own, whatever the kind’s label or error says', () => {
    const row = rule('.rg-editor-inspector__param');
    expect(row?.declarations.get('grid-template-columns')).toBe('minmax(6rem, 1fr) 8rem auto');
    // The tops line up, so the select's box sits level with the value's whatever lines sit under either.
    expect(row?.declarations.get('align-items')).toBe('start');
  });

  it('cuts a long kind label to its column, and drops the remove button level with the boxes', () => {
    const label = rule('.rg-editor-inspector__param .rg-field__label');
    expect(label?.declarations.get('white-space')).toBe('nowrap');
    expect(label?.declarations.get('text-overflow')).toBe('ellipsis');
    expect(rule('.rg-editor-inspector__param > .rg-btn')?.declarations.get('margin-top')).toBe('calc(20px + (var(--size-control) - var(--size-control-s)) / 2)');
  });
});

describe('nothing moves under the pointer when a verdict lands', () => {
  const rule = (selector: string) => parseRules(editorCss).find((r) => r.selector === selector);
  it('gives the inspector\'s verdict slot a fixed height, its overflow scrolling inside', () => {
    const slot = rule('.rg-editor-inspector__verdict');
    expect(slot?.declarations.get('height')).toBe('96px');
    expect(slot?.declarations.has('min-height')).toBe(false);
    expect(slot?.declarations.get('overflow-y')).toBe('auto');
  });

  it('gives the header\'s verdict a fixed height that scrolls, so a long report is never cut off and moves nothing', () => {
    const line = rule('.rg-editor__verdict');
    expect(line?.declarations.get('height')).toBe('40px');
    expect(line?.declarations.get('overflow-y')).toBe('auto');
    expect(line?.declarations.has('-webkit-line-clamp')).toBe(false);
  });

  it('gives the verdict a row of its own on a narrow screen', () => {
    const narrow = parseRules(editorCss).find((r) => r.selector === '.rg-editor__verdict' && r.atRule?.includes('max-width: 900px'));
    expect(narrow?.declarations.get('grid-column')).toBe('1 / -1');
  });

  it('stacks the inspector under the canvas on a narrow screen, so the canvas keeps a width', () => {
    const narrow = parseRules(editorCss).find((r) => r.selector === '.rg-editor__stage .rg-canvas-frame' && r.atRule?.includes('max-width: 900px'));
    expect(narrow?.declarations.get('flex-direction')).toBe('column');
  });

  it('keeps a port row\'s marker to one line of fixed height, present or not', () => {
    const marker = rule('.rg-editor-inspector__inputs li small');
    expect(marker?.declarations.get('white-space')).toBe('nowrap');
    expect(marker?.declarations.get('height')).toBe('16px');
  });
});

describe('with no grammar from the API', () => {
  it('refuses no kind during the drag, and leaves the verdict to the server', () => {
    const { container } = setup(undefined, { grammar: null });
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    expect(inPort(container, 'second', 0).getAttribute('data-drop')).toBe('open');
  });
});

describe('deleting with the keyboard, and an edge by its own Remove', () => {
  it('removes the focused node on Delete, and an undo brings it back', () => {
    const { container } = setup();
    fireEvent.keyDown(nodeEl(container, 'fused')!, { key: 'Delete' });
    expect(nodeEl(container, 'fused')).toBeNull();
    expect(document.activeElement).toBe(nodeEl(container, 'lexical'));
    fireEvent.keyDown(document.activeElement!, { key: 'z', ctrlKey: true });
    expect(nodeEl(container, 'fused')).toBeTruthy();
  });

  it('never deletes the declared input by key', () => {
    const { container } = setup();
    fireEvent.keyDown(nodeEl(container, 'question')!, { key: 'Backspace' });
    expect(nodeEl(container, 'question')).toBeTruthy();
  });

  it('removes the last edge into a node from the edge itself, undoably, and refuses an earlier one, saying why', () => {
    const { container } = setup();
    fireEvent.click(container.querySelector('path.rg-edge[data-from="question"][data-to="reranked"]')!);
    const refused = screen.getByRole('button', { name: 'Remove the edge question → reranked' });
    expect(refused.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(container.querySelector('path.rg-edge[data-from="fused"][data-to="reranked"]')!);
    fireEvent.click(screen.getByRole('button', { name: 'Remove the edge fused → reranked' }));
    expect(container.querySelector('path.rg-edge[data-from="fused"][data-to="reranked"]')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    expect(container.querySelector('path.rg-edge[data-from="fused"][data-to="reranked"]')).toBeTruthy();
  });
});

describe('the Launch action', () => {
  it('opens the Runs launch panel on the stored pipeline', async () => {
    setup(undefined, { initial: HYBRID, stored: 'hybrid' });
    const launch = screen.getByRole('button', { name: 'Launch…' });
    await waitFor(() => expect(launch.getAttribute('aria-disabled')).toBeNull());
    fireEvent.click(launch);
    expect(window.location.hash).toBe('#runs?launch=hybrid');
  });

  it('is refused, saying why, while the canvas differs from the stored pipeline', async () => {
    window.location.hash = '';
    setup(undefined, { initial: HYBRID, stored: 'hybrid' });
    const launch = screen.getByRole('button', { name: /^Launch…/ });
    await waitFor(() => expect(launch.getAttribute('aria-disabled')).toBeNull());
    fireEvent.click(within(screen.getByRole('region', { name: 'Palette' })).getByRole('button', { name: /^rrf/ }));
    await waitFor(() => expect(launch.getAttribute('aria-disabled')).toBe('true'));
    expect(document.getElementById(launch.getAttribute('aria-describedby')!)?.textContent).toBe('The canvas differs from the stored document, and a run takes the stored one: wait for it to be saved.');
    fireEvent.click(launch);
    expect(window.location.hash).not.toBe('#runs?launch=hybrid');
  });

  it('is refused, saying why, for a document not yet in the workspace or changed since', () => {
    setup(undefined, { initial: HYBRID });
    const launch = screen.getByRole('button', { name: /^Launch…/ });
    expect(launch.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(launch.getAttribute('aria-describedby')!)?.textContent).toBe('Not a workspace pipeline yet: a run takes a stored document.');
  });
});


describe('the editor from the keyboard and on a narrow screen', () => {
  const at = (selector: string, media: string | null) => parseRules(editorCss).find((r) => r.selector === selector && (media === null ? r.atRule === null : r.atRule?.includes(media) === true));

  it('opens with a link past the bar and the palette, which gives focus to the canvas\'s first node', () => {
    const { container } = setup();
    const editor = container.querySelector('.rg-editor') as HTMLElement;
    const skip = within(editor).getByRole('button', { name: 'Skip to the canvas' });
    const first = editor.querySelector('a[href], button, input, select, textarea, [tabindex="0"]');
    expect(first).toBe(skip);
    fireEvent.click(skip);
    expect(document.activeElement).toBe(nodeEl(container, 'question'));
  });

  it('hides that link until it has focus, then shows it over the bar', () => {
    expect(at('.rg-skip', null)?.declarations.get('position')).toBe('absolute');
    expect(at('.rg-skip:not(:focus)', null)?.declarations.get('clip-path')).toBe('inset(50%)');
  });

  it('says, on a phone, that the editor is made for a wide screen, and nothing above that width', () => {
    setup();
    const notice = screen.getByText(/The editor is made for a wide screen/);
    expect(notice.closest('.rg-editor__narrow')).toBeTruthy();
    expect(at('.rg-editor__narrow', null)?.declarations.get('display')).toBe('none');
    expect(at('.rg-editor__narrow', 'max-width: 640px')?.declarations.get('display')).toBe('block');
  });

  it('keeps the canvas near the top on a narrow screen: the stacked palette scrolls inside a bounded height', () => {
    const palette = at('.rg-palette', 'max-width: 900px');
    expect(palette?.declarations.get('max-height')).toBe('12rem');
  });
});

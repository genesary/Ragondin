/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { StrictMode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { Graph } from '../api/types.ts';
import { int } from '../parameters.ts';
import { parseRules } from '../../design/testing/css.ts';
import css from './Canvas.css?raw';
import { Canvas, type CanvasProps } from './Canvas.tsx';

// A query, two retrievers and a reranker whose second port is still empty.
const GRAPH: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [
    { id: 'lexical', family: 'retriever', implementation: 'bm25', parameters: { top_k: int('100') } },
    { id: 'reranked', family: 'reranker', implementation: 'cross_encoder', parameters: {} },
    { id: 'vectors', family: 'retriever', implementation: 'dense', parameters: {} },
  ],
  edges: [
    { from: 'question', to: 'lexical', port: 0, kind: 'query' },
    { from: 'question', to: 'reranked', port: 0, kind: 'query' },
  ],
};
const PORTS: CanvasProps['ports'] = {
  question: { inputs: [], output: 'query' },
  lexical: { inputs: ['query'], output: 'chunks' },
  vectors: { inputs: ['query'], output: 'chunks' },
  reranked: { inputs: ['query', 'chunks'], output: 'chunks' },
};
// What the editor would refuse: a query into a chunks port, and any filled port.
const refuse = (from: string, to: string, port: number) => {
  if (to === 'reranked' && port === 0) return 'Port 0 of `reranked` already holds `question`.';
  if (to === 'lexical' && port === 0) return 'Port 0 of `lexical` already holds `question`.';
  if (from === 'question' && to === 'reranked' && port === 1) return '`question` feeds `reranked` at port 1: expected chunks, found query.';
  if (from === to) return 'a node cannot feed itself';
  return null;
};

const nodeEl = (root: HTMLElement, id: string) => root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement;
const inPort = (root: HTMLElement, id: string, port: number) => nodeEl(root, id).querySelectorAll<HTMLElement>('.rg-port[data-side="in"]')[port]!;
const outPort = (root: HTMLElement, id: string) => nodeEl(root, id).querySelector<HTMLElement>('.rg-port[data-side="out"]')!;

function renderWrite(props: Partial<CanvasProps> = {}) {
  const onConnect = vi.fn();
  const onMove = vi.fn();
  const view = render(
    <div style={{ width: 1200, height: 600 }}>
      <Canvas graph={GRAPH} label="Pipeline draft" mode="write" ports={PORTS} refuse={refuse} onConnect={onConnect} onMove={onMove} {...props} />
    </div>,
  );
  return { ...view, onConnect, onMove };
}

describe('Canvas in write mode, drawing', () => {
  it('draws each node the ports it is given, filled only where an edge meets them', () => {
    const { container } = renderWrite();
    const ports = [...nodeEl(container, 'reranked').querySelectorAll('.rg-port[data-side="in"]')];
    expect(ports.map((p) => p.getAttribute('data-kind'))).toEqual(['query', 'chunks']);
    expect(ports.map((p) => p.hasAttribute('data-connected'))).toEqual([true, false]);
  });

  it('lets a node be dragged, and leaves read mode as it was', () => {
    const { container } = renderWrite();
    expect(nodeEl(container, 'lexical').classList.contains('draggable')).toBe(true);
  });

  it('draws a node the validation named as invalid, with the server’s words inside the card and in its name', () => {
    const { container } = renderWrite({ issues: { reranked: 'configuration is not a valid pipeline: …' } });
    const card = nodeEl(container, 'reranked');
    expect(card.querySelector('.rg-node')?.getAttribute('data-status')).toBe('invalid');
    expect(within(card).getByText('configuration is not a valid pipeline: …')).toBeTruthy();
    expect(card.getAttribute('aria-label')).toContain('invalid');
  });

  it('marks an edge the validation named', () => {
    const { container } = renderWrite({ invalidEdges: ['question->reranked:0'] });
    expect(container.querySelector('path.rg-edge[data-to="reranked"]')?.getAttribute('data-invalid')).toBe('true');
    expect(container.querySelector('path.rg-edge[data-to="lexical"]')?.hasAttribute('data-invalid')).toBe(false);
  });

  it('describes each node by the keys write mode adds', () => {
    const { container } = renderWrite();
    const described = nodeEl(container, 'lexical').getAttribute('aria-describedby')!;
    expect(document.getElementById(described)?.textContent).toBe('Enter selects, Shift+F10 opens the menu, the arrow keys move it, Escape clears.');
  });
});

describe('Canvas in write mode, an edge drawn by drag', () => {
  it('shows every port that cannot take the edge as refused, with the reason, while it is drawn', () => {
    const { container } = renderWrite();
    fireEvent.pointerDown(outPort(container, 'question'), { button: 0 });
    const refused = inPort(container, 'reranked', 1);
    expect(refused.getAttribute('data-drop')).toBe('refused');
    expect(refused.getAttribute('title')).toBe('`question` feeds `reranked` at port 1: expected chunks, found query.');
    expect(inPort(container, 'vectors', 0).getAttribute('data-drop')).toBe('open');
  });

  it('says the reason of the port under the pointer, in a status line that is always there', () => {
    const { container } = renderWrite();
    const status = within(container).getByRole('status');
    expect(status.textContent).toBe('');
    fireEvent.pointerDown(outPort(container, 'question'), { button: 0 });
    expect(status.textContent).toBe('Connecting from question. Drop on an open port; Escape cancels.');
    fireEvent.pointerEnter(inPort(container, 'reranked', 1));
    expect(status.textContent).toBe('question feeds reranked at port 1: expected chunks, found query.');
    expect(status.querySelector('code')?.textContent).toBe('question');
  });

  it('creates nothing when dropped on a refused port, and the edge when dropped on an open one', () => {
    const { container, onConnect } = renderWrite();
    fireEvent.pointerDown(outPort(container, 'question'), { button: 0 });
    fireEvent.pointerUp(inPort(container, 'reranked', 1));
    expect(onConnect).not.toHaveBeenCalled();
    expect(inPort(container, 'vectors', 0).hasAttribute('data-drop')).toBe(false);
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    fireEvent.pointerUp(inPort(container, 'reranked', 1));
    expect(onConnect).toHaveBeenCalledWith('lexical', 'reranked', 1);
  });

  it('makes the edge once under StrictMode, which runs a state update twice', () => {
    const onConnect = vi.fn();
    const { container } = render(
      <StrictMode>
        <div style={{ width: 1200, height: 600 }}>
          <Canvas graph={GRAPH} label="Pipeline draft" mode="write" ports={PORTS} refuse={refuse} onConnect={onConnect} />
        </div>
      </StrictMode>,
    );
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    fireEvent.pointerUp(inPort(container, 'reranked', 1));
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it('cancels on Escape, and on a release anywhere but a port', () => {
    const { container, onConnect } = renderWrite();
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    fireEvent.keyDown(nodeEl(container, 'lexical'), { key: 'Escape' });
    expect(inPort(container, 'reranked', 1).hasAttribute('data-drop')).toBe(false);
    fireEvent.pointerDown(outPort(container, 'lexical'), { button: 0 });
    act(() => {
      window.dispatchEvent(new Event('pointerup'));
    });
    expect(inPort(container, 'reranked', 1).hasAttribute('data-drop')).toBe(false);
    expect(onConnect).not.toHaveBeenCalled();
  });

  it('lets the pointer reach a port in write mode, which the library’s stylesheet takes from a handle it cannot connect', () => {
    const rule = parseRules(css).find((r) => r.selector.includes('[data-mode="write"]') && r.selector.includes('.rg-port'));
    expect(rule?.declarations.get('pointer-events')).toBe('all');
  });

  it('lets the status line wrap below the toolbar on a narrow screen rather than cut its reason off', () => {
    const narrow = parseRules(css).filter((r) => r.selector === '.rg-canvas__status' && r.atRule?.includes('max-width'));
    expect(narrow.at(-1)?.declarations.get('white-space')).toBe('normal');
  });

  it('starts no edge in read mode', () => {
    const { container } = renderWrite({ mode: 'read' });
    fireEvent.pointerDown(outPort(container, 'question'), { button: 0 });
    expect(inPort(container, 'reranked', 0).hasAttribute('data-drop')).toBe(false);
  });
});

describe('Canvas in write mode, the keyboard', () => {
  it('moves the focused node a grid step with an arrow key, four with Shift', () => {
    const { container, onMove } = renderWrite({ layout: { lexical: { x: 320, y: 64 } } });
    fireEvent.keyDown(nodeEl(container, 'lexical'), { key: 'ArrowRight' });
    expect(onMove).toHaveBeenLastCalledWith('lexical', { x: 336, y: 64 });
    fireEvent.keyDown(nodeEl(container, 'lexical'), { key: 'ArrowUp', shiftKey: true });
    expect(onMove).toHaveBeenLastCalledWith('lexical', { x: 320, y: 0 });
  });

  it('asks for the insert list on `/`', () => {
    const onInsert = vi.fn();
    const { container } = renderWrite({ onInsert });
    fireEvent.keyDown(container.querySelector('.react-flow') as HTMLElement, { key: '/' });
    expect(onInsert).toHaveBeenCalledTimes(1);
  });

  it('moves nothing with the arrow keys in read mode', () => {
    const { container, onMove } = renderWrite({ mode: 'read' });
    fireEvent.keyDown(nodeEl(container, 'lexical'), { key: 'ArrowRight' });
    expect(onMove).not.toHaveBeenCalled();
  });
});

describe('Canvas in write mode, the node menu', () => {
  it('opens from the node’s more control, and hands the entries a way to close it', () => {
    let close: (() => void) | undefined;
    const { container } = renderWrite({
      menu: (id, done) => {
        close = done;
        return <button role="menuitem">Delete {id}</button>;
      },
    });
    fireEvent.click(within(nodeEl(container, 'vectors')).getByRole('button', { name: 'More actions for vectors' }));
    expect(screen.getByRole('menuitem', { name: 'Delete vectors' })).toBeTruthy();
    act(() => close?.());
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('Canvas, a click inside the node menu', () => {
  it('neither selects the node nor closes the menu: the entry decides', () => {
    const onSelect = vi.fn();
    const { container } = renderWrite({ onSelect, menu: () => <button role="menuitem">Stay</button> });
    fireEvent.keyDown(nodeEl(container, 'vectors'), { key: 'F10', shiftKey: true });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Stay' }));
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole('menu')).toBeTruthy();
  });
});

describe('Canvas in write mode, a node dropped from the palette', () => {
  it('hands the dropped item and where it landed to the caller', () => {
    const onDropItem = vi.fn();
    const { container } = renderWrite({ onDropItem });
    const data = new Map([['application/x-ragondin-node', '{"component":"fusion","impl":"rrf"}']]);
    const dataTransfer = { types: [...data.keys()], getData: (type: string) => data.get(type) ?? '', dropEffect: 'none' };
    const pane = container.querySelector('.rg-canvas') as HTMLElement;
    fireEvent.dragOver(pane, { dataTransfer });
    fireEvent.drop(pane, { dataTransfer, clientX: 10, clientY: 10 });
    expect(onDropItem).toHaveBeenCalledWith('{"component":"fusion","impl":"rrf"}', expect.objectContaining({ x: expect.any(Number), y: expect.any(Number) }));
  });
});

/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { parseRules } from '../../design/testing/css.ts';
import css from './Canvas.css?raw';
import { Canvas } from './Canvas.tsx';
import { HYBRID_RERANK_GEN } from './fixtures.ts';
import nodeCss from './NodeCard.css?raw';
import { NODE_WIDTH, resolveLayout } from './layout.ts';
import { toModel } from './model.ts';
import { portTop } from './Port.tsx';

const TOPOLOGICAL = ['question', 'lexical', 'vectors', 'fused', 'reranked', 'prompt', 'answer'];

const nodeEl = (root: HTMLElement, id: string) => root.querySelector(`.react-flow__node[data-id="${id}"]`) as HTMLElement;
const card = (root: HTMLElement, id: string) => nodeEl(root, id).querySelector('.rg-node') as HTMLElement;
const translate = (el: HTMLElement) => {
  const m = /translate\((-?[\d.]+)px,\s*(-?[\d.]+)px\)/.exec(el.style.transform);
  if (m === null) throw new Error(`no translate in ${el.style.transform}`);
  return { x: Number(m[1]), y: Number(m[2]) };
};

function renderCanvas(props: Partial<Parameters<typeof Canvas>[0]> = {}) {
  return render(
    <div style={{ width: 1200, height: 600 }}>
      <Canvas graph={HYBRID_RERANK_GEN} label="Pipeline hybrid-rerank-gen" {...props} />
    </div>,
  );
}

describe('Canvas, drawing the lowered graph', () => {
  it('renders every node with its family tile, glyph, name and implementation', () => {
    const { container } = renderCanvas();
    for (const node of toModel(HYBRID_RERANK_GEN).nodes) {
      const c = card(container, node.id);
      expect(c, node.id).toBeTruthy();
      expect(c.querySelector(`.rg-tile[data-family="${node.family}"] svg`)).toBeTruthy();
      expect(within(c).getByText(node.id)).toBeTruthy();
      expect(within(c).getByText(node.impl)).toBeTruthy();
    }
  });

  it('names each node for assistive technology by family and name', () => {
    const { container } = renderCanvas();
    expect(nodeEl(container, 'reranked').getAttribute('aria-label')).toBe('reranker reranked, reranker/cross_encoder');
  });

  it('draws every edge between the right ports, the output on the right to the numbered input on the left', () => {
    const { container } = renderCanvas();
    const { positions } = resolveLayout(toModel(HYBRID_RERANK_GEN));
    const edges = [...container.querySelectorAll('path.rg-edge')];
    expect(edges).toHaveLength(HYBRID_RERANK_GEN.edges.length);
    for (const e of HYBRID_RERANK_GEN.edges) {
      const path = container.querySelector(`path.rg-edge[data-from="${e.from}"][data-to="${e.to}"][data-port="${e.port}"]`);
      expect(path, `${e.from} -> ${e.to}:${e.port}`).toBeTruthy();
      expect(path?.getAttribute('data-kind')).toBe(e.kind);
      const nums = (path?.getAttribute('d') ?? '').match(/-?[\d.]+/g)!.map(Number);
      const [x1, y1] = nums;
      const [x2, y2] = nums.slice(-2);
      const from = positions[e.from]!;
      const to = positions[e.to]!;
      expect(x1).toBeCloseTo(from.x + NODE_WIDTH + 6);
      expect(y1).toBeCloseTo(from.y + portTop(0) + 5);
      expect(x2).toBeCloseTo(to.x - 6);
      expect(y2).toBeCloseTo(to.y + portTop(e.port) + 5);
    }
  });

  it('types every port by its shape, through its kind', () => {
    const { container } = renderCanvas();
    const ins = [...nodeEl(container, 'answer').querySelectorAll('.rg-port[data-side="in"]')].map((p) => p.getAttribute('data-kind'));
    expect(ins).toEqual(['query', 'context']);
    expect(nodeEl(container, 'answer').querySelector('.rg-port[data-side="out"]')?.getAttribute('data-kind')).toBe('answer');
    expect(nodeEl(container, 'question').querySelector('.rg-port[data-side="out"]')?.getAttribute('data-kind')).toBe('query');
    expect(nodeEl(container, 'reranked').querySelector('.rg-port[data-side="out"]')?.getAttribute('data-kind')).toBe('chunks');
  });

  it('draws the grid and the legend: each family on screen with its tile and name, and each port shape with its word', () => {
    const { container } = renderCanvas();
    expect(container.querySelector('.rg-canvas__grid')).toBeTruthy();
    const legend = container.querySelector('.rg-canvas__legend') as HTMLElement;
    for (const name of ['query', 'retriever', 'fusion', 'reranker', 'context builder', 'generator']) expect(within(legend).getByText(name, { selector: '[data-legend="family"]' })).toBeTruthy();
    expect(legend.querySelectorAll('.rg-tile')).toHaveLength(6);
    for (const [kind, word] of [['query', 'query'], ['chunks', 'candidates'], ['context', 'context'], ['answer', 'answer']] as const)
      expect(within(legend).getByText(word, { selector: '[data-legend="port"]' }).querySelector(`.rg-port[data-kind="${kind}"]`)).toBeTruthy();
  });
});

describe('Canvas layout', () => {
  it('with no stored layout, places every node automatically, and hands the positions back to persist', () => {
    const onAutoPlaced = vi.fn();
    const { container } = renderCanvas({ onAutoPlaced });
    const { positions } = resolveLayout(toModel(HYBRID_RERANK_GEN));
    for (const id of TOPOLOGICAL) expect(translate(nodeEl(container, id))).toEqual(positions[id]);
    expect(onAutoPlaced).toHaveBeenCalledWith(positions);
  });

  it('with a stored layout, renders at the stored positions and reports nothing', () => {
    const onAutoPlaced = vi.fn();
    const layout = Object.fromEntries(TOPOLOGICAL.map((id, i) => [id, { x: i * 320, y: 32 }]));
    const { container } = renderCanvas({ layout, onAutoPlaced });
    for (const id of TOPOLOGICAL) expect(translate(nodeEl(container, id))).toEqual(layout[id]);
    expect(onAutoPlaced).not.toHaveBeenCalled();
  });

  it('says when it laid the graph out itself, and how many nodes it placed when the stored layout missed some', () => {
    const all = renderCanvas();
    expect(within(all.container).getByText('Laid out automatically')).toBeTruthy();
    all.unmount();
    const layout = Object.fromEntries(TOPOLOGICAL.filter((id) => id !== 'prompt').map((id, i) => [id, { x: i * 320, y: 32 }]));
    const some = renderCanvas({ layout });
    expect(within(some.container).getByText('1 node placed automatically')).toBeTruthy();
    some.unmount();
    const stored = renderCanvas({ layout: { ...layout, prompt: { x: 2000, y: 32 } } });
    expect(within(stored.container).queryByText(/automatically/)).toBeNull();
  });

  it('places a node missing from the stored layout automatically, and reports that node alone', () => {
    const onAutoPlaced = vi.fn();
    const layout = Object.fromEntries(TOPOLOGICAL.filter((id) => id !== 'prompt').map((id, i) => [id, { x: i * 320, y: 32 }]));
    renderCanvas({ layout, onAutoPlaced });
    expect(onAutoPlaced).toHaveBeenCalledTimes(1);
    expect(Object.keys(onAutoPlaced.mock.calls[0]![0])).toEqual(['prompt']);
  });
});

describe('Canvas keyboard and selection', () => {
  it('lets Tab walk the nodes in topological order, and nothing else inside the graph', () => {
    const { container } = renderCanvas();
    const pane = container.querySelector('.react-flow') as HTMLElement;
    const stops = [...pane.querySelectorAll('[tabindex]')].filter((el) => el.getAttribute('tabindex') !== '-1');
    expect(stops.map((el) => el.getAttribute('data-id'))).toEqual(TOPOLOGICAL);
  });

  it('selects the focused node on Enter, and clears the selection on Escape', () => {
    const onSelect = vi.fn();
    const { container } = renderCanvas({ onSelect });
    const fused = nodeEl(container, 'fused');
    act(() => fused.focus());
    fireEvent.keyDown(fused, { key: 'Enter' });
    expect(card(container, 'fused').getAttribute('data-selected')).toBe('true');
    expect(fused.getAttribute('aria-label')).toContain(', selected');
    expect(onSelect).toHaveBeenLastCalledWith('fused');
    fireEvent.keyDown(fused, { key: 'Escape' });
    expect(card(container, 'fused').getAttribute('data-selected')).toBeNull();
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });

  it('selects one node at a time, on a click too', () => {
    const { container } = renderCanvas();
    fireEvent.click(nodeEl(container, 'lexical'));
    fireEvent.click(nodeEl(container, 'vectors'));
    expect(container.querySelectorAll('.rg-node[data-selected="true"]')).toHaveLength(1);
    expect(card(container, 'vectors').getAttribute('data-selected')).toBe('true');
  });

  it('shows the inspector slot beside the canvas for the selected node only', () => {
    const { container } = renderCanvas({ inspector: (id) => <p>inspecting {id}</p> });
    expect(screen.queryByText(/inspecting/)).toBeNull();
    fireEvent.click(nodeEl(container, 'reranked'));
    expect(screen.getByText('inspecting reranked')).toBeTruthy();
  });

  it('opens the node menu shell on Shift+F10 on the focused node, and Escape closes it and returns focus', () => {
    const { container } = renderCanvas();
    const prompt = nodeEl(container, 'prompt');
    act(() => prompt.focus());
    fireEvent.keyDown(prompt, { key: 'F10', shiftKey: true });
    const menu = screen.getByRole('menu', { name: 'Node prompt' });
    expect(menu).toBeTruthy();
    expect(document.activeElement && menu.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(prompt);
  });

  it('opens the menu with the context-menu key and on right-click too, with the entries the caller gives', () => {
    const { container } = renderCanvas({ menu: (id) => <button role="menuitem">Open {id}</button> });
    const lexical = nodeEl(container, 'lexical');
    fireEvent.keyDown(lexical, { key: 'ContextMenu' });
    expect(screen.getByRole('menuitem', { name: 'Open lexical' })).toBeTruthy();
    fireEvent.keyDown(screen.getByRole('menuitem'), { key: 'Escape' });
    fireEvent.contextMenu(nodeEl(container, 'vectors'));
    expect(screen.getByRole('menuitem', { name: 'Open vectors' })).toBeTruthy();
  });

  it('closes an open menu when another node is selected', () => {
    const { container } = renderCanvas();
    fireEvent.keyDown(nodeEl(container, 'lexical'), { key: 'ContextMenu' });
    expect(screen.getByRole('menu')).toBeTruthy();
    fireEvent.keyDown(nodeEl(container, 'vectors'), { key: 'Enter' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(card(container, 'vectors').getAttribute('data-selected')).toBe('true');
  });

  it('draws the focus ring on the focused node', () => {
    const rule = parseRules(nodeCss).find((r) => r.selector.includes('.react-flow__node:focus-visible > .rg-node'));
    expect(rule?.declarations.get('outline')).toBe('2px solid var(--focus-ring)');
  });
});

describe('Canvas overlay', () => {
  it('passes each node its overlay, and leaves the others plain', () => {
    const { container } = renderCanvas({ overlay: { reranked: { ranks: [1, 3], durationMs: 349, share: 0.85 } } });
    expect(card(container, 'reranked').getAttribute('data-replay')).toBe('true');
    expect(card(container, 'reranked').querySelectorAll('.rg-rankstrip > i[data-cell="hit"]')).toHaveLength(2);
    expect(card(container, 'fused').getAttribute('data-replay')).toBeNull();
  });

  it('marks a failed node in its accessible name', () => {
    const { container } = renderCanvas({ overlay: { answer: { error: 'the service did not answer' } } });
    expect(nodeEl(container, 'answer').getAttribute('aria-label')).toContain(', failed');
  });
});

describe('Two canvases on one page', () => {
  it('share neither selection nor layout', () => {
    const layoutA = Object.fromEntries(TOPOLOGICAL.map((id, i) => [id, { x: i * 320, y: 0 }]));
    const { container } = render(
      <>
        <div data-side="a">
          <Canvas graph={HYBRID_RERANK_GEN} label="A" layout={layoutA} />
        </div>
        <div data-side="b">
          <Canvas graph={HYBRID_RERANK_GEN} label="B" />
        </div>
      </>,
    );
    const a = container.querySelector('[data-side="a"]') as HTMLElement;
    const b = container.querySelector('[data-side="b"]') as HTMLElement;
    fireEvent.click(nodeEl(a, 'fused'));
    expect(card(a, 'fused').getAttribute('data-selected')).toBe('true');
    expect(card(b, 'fused').getAttribute('data-selected')).toBeNull();
    expect(translate(nodeEl(a, 'fused'))).toEqual(layoutA['fused']);
    expect(translate(nodeEl(b, 'fused'))).toEqual(resolveLayout(toModel(HYBRID_RERANK_GEN)).positions['fused']);
  });
});

describe('Canvas motion', () => {
  it('animates nothing but selection and focus, and only through the duration tokens that reduced motion collapses', () => {
    const transitions = [...parseRules(css), ...parseRules(nodeCss)].flatMap((r) => (r.declarations.has('transition') ? [r.declarations.get('transition')!] : []));
    expect(transitions).toEqual(['box-shadow var(--duration-micro) var(--ease-standard), outline-offset var(--duration-micro) var(--ease-standard)']);
    expect(css).not.toMatch(/animation/);
  });

  it('switches off the canvas library’s own edge animation and viewport transitions', () => {
    const { container } = renderCanvas();
    expect(container.querySelector('.react-flow__edge.animated')).toBeNull();
  });
});

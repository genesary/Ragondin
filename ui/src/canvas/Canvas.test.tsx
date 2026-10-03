/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { parseRules } from '../../design/testing/css.ts';
import css from './Canvas.css?raw';
import { Canvas } from './Canvas.tsx';
import { GATED_GEN, HYBRID_RERANK_GEN } from './fixtures.ts';
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

  it('marks a node that was not run in its accessible name', () => {
    const { container } = renderCanvas({ overlay: { answer: { notRun: true } } });
    expect(nodeEl(container, 'answer').getAttribute('aria-label')).toBe('generator answer, generator/answerer, not run');
  });

  it('describes a replayed node by what its card shows, then by the keys, since inside the application a screen reader hears no card text', () => {
    const { container } = renderCanvas({ overlay: { reranked: { metric: { name: 'ndcg@10', value: '0.8610' }, ranks: [1, 3], durationMs: 349, share: 0.85 } } });
    const ids = nodeEl(container, 'reranked').getAttribute('aria-describedby')!.split(' ');
    expect(ids.map((id) => document.getElementById(id)?.textContent)).toEqual([
      "ndcg@10 0.8610. 2 gold passages in the top 10, at rank 1, 3. 349 ms, 85% of this query's time.",
      'Enter selects, Shift+F10 opens the menu, Escape clears.',
    ]);
    const plain = nodeEl(container, 'fused').getAttribute('aria-describedby')!.split(' ');
    expect(plain.map((id) => document.getElementById(id)?.textContent)).toEqual(['Enter selects, Shift+F10 opens the menu, Escape clears.']);
  });

  it('keeps the descriptions of two canvases apart', () => {
    const { container } = render(
      <>
        <div data-side="a">
          <Canvas graph={HYBRID_RERANK_GEN} label="A" overlay={{ fused: { durationMs: 1 } }} />
        </div>
        <div data-side="b">
          <Canvas graph={HYBRID_RERANK_GEN} label="B" overlay={{ fused: { durationMs: 2 } }} />
        </div>
      </>,
    );
    const text = (side: string) => {
      const id = nodeEl(container.querySelector(`[data-side="${side}"]`) as HTMLElement, 'fused').getAttribute('aria-describedby')!.split(' ')[0]!;
      return document.getElementById(id)?.textContent;
    };
    expect([text('a'), text('b')]).toEqual(['1 ms.', '2 ms.']);
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

describe('Canvas, round 1 of review', () => {
  it('fills a port only where an edge meets it', () => {
    const { container } = renderCanvas();
    expect(nodeEl(container, 'fused').querySelector('.rg-port[data-side="out"]')?.getAttribute('data-connected')).toBe('true');
    expect(nodeEl(container, 'fused').querySelector('.rg-port[data-side="in"]')?.getAttribute('data-connected')).toBe('true');
    expect(nodeEl(container, 'answer').querySelector('.rg-port[data-side="out"]')?.getAttribute('data-connected')).toBeNull();
  });

  it('draws an opaque edge into a ring-and-dot port, and names it in the legend', () => {
    const { container } = renderCanvas({ graph: GATED_GEN });
    expect(container.querySelector('path.rg-edge[data-from="gate"][data-to="answer"]')?.getAttribute('data-kind')).toBe('opaque');
    const port = nodeEl(container, 'answer').querySelector('.rg-port[data-side="in"][data-kind="opaque"]');
    expect(port?.querySelector('.rg-port__dot')).toBeTruthy();
    expect(nodeEl(container, 'gate').querySelector('.rg-tile[data-family="control"]')).toBeTruthy();
    const legend = container.querySelector('.rg-canvas__legend') as HTMLElement;
    expect(within(legend).getByText('other', { selector: '[data-legend="port"]' }).querySelector('.rg-port__dot')).toBeTruthy();
  });

  it('describes each node by the keys that work in read mode', () => {
    const { container } = renderCanvas();
    const described = nodeEl(container, 'fused').getAttribute('aria-describedby')!;
    expect(document.getElementById(described)?.textContent).toBe('Enter selects, Shift+F10 opens the menu, Escape clears.');
  });

  it('names the graph region it hands its keys to', () => {
    renderCanvas();
    expect(screen.getByRole('application', { name: 'Pipeline hybrid-rerank-gen' })).toBeTruthy();
  });

  it('lays out from the graph alone: an overlay moves no node and changes nothing it reports', () => {
    const plain = vi.fn();
    const replay = vi.fn();
    const a = renderCanvas({ onAutoPlaced: plain });
    const before = Object.fromEntries(TOPOLOGICAL.map((id) => [id, translate(nodeEl(a.container, id))]));
    a.unmount();
    const b = renderCanvas({ onAutoPlaced: replay, overlay: { reranked: { metric: { name: 'nDCG@10', value: '0.861' }, ranks: [1], discarded: 90, durationMs: 349, share: 0.85, error: 'x' } } });
    for (const id of TOPOLOGICAL) expect(translate(nodeEl(b.container, id))).toEqual(before[id]);
    expect(replay.mock.calls).toEqual(plain.mock.calls);
  });

  it('keeps the canvas library’s attribution visible', () => {
    const { container } = renderCanvas();
    const link = container.querySelector('.react-flow__attribution a');
    expect(link?.textContent).toBe('React Flow');
    // Restyled with tokens, never hidden.
    const rules = parseRules(css).filter((r) => r.selector.includes('.react-flow__attribution'));
    expect(rules.length).toBeGreaterThan(0);
    for (const rule of rules) {
      expect(rule.declarations.get('display')).not.toBe('none');
      expect(rule.declarations.get('visibility')).toBeUndefined();
    }
    expect(rules.some((r) => r.declarations.get('color') === 'var(--ink-3)')).toBe(true);
  });

  it('announces nothing on its own as the zoom changes', () => {
    const { container } = renderCanvas();
    expect(container.querySelector('.rg-canvas__zoom')?.hasAttribute('aria-live')).toBe(false);
  });

  it('puts the toolbar first in the tab order, before the nodes', () => {
    const { container } = renderCanvas();
    const stops = [...container.querySelectorAll<HTMLElement>('button, a[href], [tabindex]')].filter((el) => el.tabIndex >= 0);
    const names = stops.map((el) => el.getAttribute('data-id') ?? el.textContent);
    expect(names.slice(0, 3)).toEqual(['Zoom out', 'Zoom in', 'Fit graph']);
    expect(names.slice(3, 3 + TOPOLOGICAL.length)).toEqual(TOPOLOGICAL);
  });

  it('draws nothing that can be dragged or connected in read mode', () => {
    const { container } = renderCanvas();
    expect(container.querySelector('.react-flow__node.draggable')).toBeNull();
    expect(container.querySelector('.react-flow__handle.connectable')).toBeNull();
    expect(container.querySelector('.react-flow__handle.connectableend')).toBeNull();
  });

  it('renders the same graph in both themes, with no colour of its own', () => {
    const { container } = render(
      <>
        {(['light', 'dark'] as const).map((theme) => (
          <div key={theme} data-theme={theme}>
            <Canvas graph={HYBRID_RERANK_GEN} label={theme} overlay={{ reranked: { ranks: [1] } }} />
          </div>
        ))}
      </>,
    );
    for (const theme of ['light', 'dark']) {
      const root = container.querySelector(`[data-theme="${theme}"]`) as HTMLElement;
      expect(root.querySelectorAll('.rg-node')).toHaveLength(TOPOLOGICAL.length);
      expect(root.querySelectorAll('path.rg-edge')).toHaveLength(HYBRID_RERANK_GEN.edges.length);
      expect(root.querySelector('.rg-canvas__legend')).toBeTruthy();
    }
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\b(rgb|hsl|oklch)a?\(/);
  });
});

describe('the node menu', () => {
  function openOn(id: string, entries = 3) {
    const view = renderCanvas({
      menu: (node) => (
        <>
          {Array.from({ length: entries }, (_, i) => (
            <button key={i} role="menuitem">
              item {i} of {node}
            </button>
          ))}
        </>
      ),
    });
    const el = nodeEl(view.container, id);
    act(() => el.focus());
    fireEvent.keyDown(el, { key: 'F10', shiftKey: true });
    return { ...view, el };
  }

  it('moves between entries with the arrow keys, wrapping, and Home and End', () => {
    openOn('fused');
    const items = screen.getAllByRole('menuitem');
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(items[0]!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1]!, { key: 'ArrowUp' });
    fireEvent.keyDown(items[0]!, { key: 'ArrowUp' });
    expect(document.activeElement).toBe(items[2]);
    fireEvent.keyDown(items[2]!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(items[0]);
    fireEvent.keyDown(items[0]!, { key: 'End' });
    expect(document.activeElement).toBe(items[2]);
    fireEvent.keyDown(items[2]!, { key: 'Home' });
    expect(document.activeElement).toBe(items[0]);
  });

  it('leaves the selection alone when Escape closes the menu', () => {
    const { container, el } = openOn('fused');
    fireEvent.keyDown(el, { key: 'Enter' });
    fireEvent.keyDown(el, { key: 'F10', shiftKey: true });
    fireEvent.keyDown(screen.getAllByRole('menuitem')[0]!, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(card(container, 'fused').getAttribute('data-selected')).toBe('true');
    expect(document.activeElement).toBe(el);
  });

  it('closes on Tab and gives focus back to the node', () => {
    const { el } = openOn('fused');
    fireEvent.keyDown(screen.getAllByRole('menuitem')[0]!, { key: 'Tab' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(el);
  });

  it('closes when focus leaves it', () => {
    openOn('fused');
    const zoom = screen.getByRole('button', { name: 'Zoom in' });
    act(() => zoom.focus());
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(zoom);
  });
});

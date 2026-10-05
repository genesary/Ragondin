import dagre from '@dagrejs/dagre';
import { describe, expect, it, vi } from 'vitest';
import tokensCss from '../../design/tokens.css?raw';
import { HYBRID_RERANK_GEN } from './fixtures.ts';
import { GRID, NODE_HEIGHT, NODE_WIDTH, RANK_GAP, boundsOf, extentOf, fitArea, nodeSize, resolveLayout } from './layout.ts';
import { toModel } from './model.ts';

const model = toModel(HYBRID_RERANK_GEN);

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

function boxes(positions: Record<string, { x: number; y: number }>): Map<string, Box> {
  return new Map(model.nodes.map((n) => [n.id, { ...positions[n.id]!, ...nodeSize() }]));
}

const tokenValue = (name: string): number => {
  const found = new RegExp(`--${name}:\\s*([0-9.]+)px;`).exec(tokensCss);
  if (found === null) throw new Error(`no token ${name}`);
  return Number.parseFloat(found[1]!);
};

describe('the layout constants', () => {
  it('are the design system tokens they stand for', () => {
    expect(NODE_WIDTH).toBe(tokenValue('size-node'));
    expect(GRID).toBe(tokenValue('size-grid'));
    expect(RANK_GAP).toBe(tokenValue('space-16'));
  });
});

describe('resolveLayout', () => {
  it('lays a graph without a stored layout out left to right, on the grid, with no overlap', () => {
    const { positions, autoPlaced } = resolveLayout(model);
    expect([...autoPlaced].sort()).toEqual(model.nodes.map((n) => n.id).sort());
    for (const edge of model.edges) expect(positions[edge.from]!.x).toBeLessThan(positions[edge.to]!.x);
    const all = [...boxes(positions).entries()];
    for (const [i, [a, boxA]] of all.entries())
      for (const [b, boxB] of all.slice(i + 1)) expect(overlaps(boxA, boxB), `${a} overlaps ${b}`).toBe(false);
    for (const p of Object.values(positions)) {
      expect(p.x % GRID).toBe(0);
      expect(p.y % GRID).toBe(0);
    }
  });

  it('puts every node at its stored position when the layout names them all, and flags none', () => {
    const stored = Object.fromEntries(model.nodes.map((n, i) => [n.id, { x: i * 300, y: 48 }]));
    const { positions, autoPlaced } = resolveLayout(model, stored);
    expect(positions).toEqual(stored);
    expect(autoPlaced).toEqual([]);
  });

  it('places a node the stored layout misses automatically, clear of the stored ones, and flags it', () => {
    const { positions: auto } = resolveLayout(model);
    const stored = Object.fromEntries(model.nodes.filter((n) => n.id !== 'reranked').map((n) => [n.id, auto[n.id]!]));
    // Move one stored node onto the place the automatic layout gives the missing one.
    stored['fused'] = auto['reranked']!;
    const { positions, autoPlaced } = resolveLayout(model, stored);
    expect(autoPlaced).toEqual(['reranked']);
    for (const id of Object.keys(stored)) expect(positions[id]).toEqual(stored[id]);
    const placed = boxes(positions);
    for (const id of Object.keys(stored)) expect(overlaps(placed.get('reranked')!, placed.get(id)!), `reranked overlaps ${id}`).toBe(false);
  });

  it('gives every card one envelope, whatever it shows, so replay data never moves a node', () => {
    expect(nodeSize()).toEqual({ width: NODE_WIDTH, height: NODE_HEIGHT });
    // The layout takes the graph and the stored positions, and nothing else.
    expect(resolveLayout.length).toBe(2);
  });

  it('runs no automatic layout when the stored layout names every node', () => {
    const spy = vi.spyOn(dagre, 'layout');
    const stored = Object.fromEntries(model.nodes.map((n, i) => [n.id, { x: i * 300, y: 48 }]));
    resolveLayout(model, stored);
    expect(spy).not.toHaveBeenCalled();
    resolveLayout(model, {});
    expect(spy).toHaveBeenCalledTimes(1);
    spy.mockRestore();
  });

  it('ignores a stored position for a node the graph no longer has', () => {
    const { positions } = resolveLayout(model, { gone: { x: 0, y: 0 } });
    expect(positions).not.toHaveProperty('gone');
  });

  it('is never fed anything but the graph and positions: the same input gives the same layout', () => {
    expect(resolveLayout(model)).toEqual(resolveLayout(model));
  });
});

describe('the area a view is fitted to', () => {
  const placed = { a: { x: 32, y: 16 }, b: { x: 320, y: 160 } };

  it('is the box around every card, from the top-left card corner', () => {
    expect(boundsOf(placed)).toEqual({ x: 32, y: 16, width: 320 - 32 + NODE_WIDTH, height: 160 - 16 + NODE_HEIGHT });
  });

  it('grows to a shared frame from the same corner, so two canvases given one frame share one zoom and one origin', () => {
    const bounds = boundsOf(placed);
    expect(fitArea(bounds, { width: 2000, height: 100 })).toEqual({ x: 32, y: 16, width: 2000, height: bounds.height });
    expect(fitArea(bounds)).toEqual(bounds);
  });

  it('is measured on the automatic layout alone, for a graph, so a screen can frame two graphs alike', () => {
    expect(extentOf(HYBRID_RERANK_GEN)).toEqual({ width: boundsOf(resolveLayout(model).positions).width, height: boundsOf(resolveLayout(model).positions).height });
  });
});

import dagre from '@dagrejs/dagre';
import type { Layout, Position } from '../api/types.ts';
import type { CanvasModel, CanvasNode } from './model.ts';

export type { Position };

/**
 * A stored layout: the `nodes` of the API's `Layout`, node id to a card's
 * top-left corner. Presentation only — nothing here hashes it, and the canvas
 * is whole without one.
 */
export type StoredLayout = Readonly<Layout['nodes']>;

export type ResolvedLayout = {
  positions: Record<string, Position>;
  /** The nodes the automatic layout placed because the stored layout did not name them, in canvas order. */
  autoPlaced: string[];
};

// Geometry the design system's tokens fix (layout.test.ts holds them equal):
// --size-node, --size-grid, and --space-16 between node columns.
export const NODE_WIDTH = 224;
export const GRID = 16;
export const RANK_GAP = 64;
// Between two cards of one column; two grid steps, so snapping to the grid
// still leaves a step between them.
const NODE_GAP = 2 * GRID;

// One envelope for every card, whatever it shows: the tallest a card gets
// (NodeCard.css) — the head, the replay body's four rows, and the failure
// message. The layout reads the graph alone, so the replay data of the query
// on screen never moves a node, and what `onAutoPlaced` reports does not
// depend on it. The canvas measures the real card once it is on screen.
const HEAD = 56;
const ROW = 32;
const ERROR = 56;
export const NODE_HEIGHT = HEAD + 4 * ROW + GRID + ERROR;

export function nodeSize(): { width: number; height: number } {
  return { width: NODE_WIDTH, height: NODE_HEIGHT };
}

const snap = (v: number) => Math.round(v / GRID) * GRID;

/** dagre, left to right, every card snapped to the grid. */
function automatic(model: CanvasModel): Record<string, Position> {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: 'LR', ranksep: RANK_GAP, nodesep: NODE_GAP, marginx: 0, marginy: 0 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const node of model.nodes) g.setNode(node.id, nodeSize());
  for (const edge of model.edges) if (g.hasNode(edge.from) && g.hasNode(edge.to)) g.setEdge(edge.from, edge.to);
  dagre.layout(g);
  const out: Record<string, Position> = {};
  for (const node of model.nodes) {
    const { x, y, width, height } = g.node(node.id);
    out[node.id] = { x: snap(x - width / 2), y: snap(y - height / 2) };
  }
  return out;
}

/**
 * Where every card goes: at its stored position when the layout names it,
 * else where the automatic layout puts it, moved down the grid until it is
 * clear of every card already placed. With no stored layout every node is
 * placed automatically, and every one is flagged so the caller can persist it.
 */
export function resolveLayout(model: CanvasModel, stored?: StoredLayout): ResolvedLayout {
  const positions: Record<string, Position> = {};
  const placed: { x: number; y: number; width: number; height: number }[] = [];
  const missing: CanvasNode[] = [];
  for (const node of model.nodes) {
    const at = stored?.[node.id];
    if (at === undefined) {
      missing.push(node);
      continue;
    }
    positions[node.id] = { x: at.x, y: at.y };
    placed.push({ ...at, ...nodeSize() });
  }
  const clashes = (box: (typeof placed)[number]) =>
    placed.some((p) => box.x < p.x + p.width + GRID && p.x < box.x + box.width + GRID && box.y < p.y + p.height + GRID && p.y < box.y + box.height + GRID);
  // dagre runs only when a node needs it.
  const auto = missing.length === 0 ? {} : automatic(model);
  for (const node of missing) {
    const box = { ...auto[node.id]!, ...nodeSize() };
    if (stored !== undefined) while (clashes(box)) box.y += GRID;
    positions[node.id] = { x: box.x, y: box.y };
    placed.push(box);
  }
  return { positions, autoPlaced: missing.map((n) => n.id) };
}

import dagre from '@dagrejs/dagre';
import type { Graph, Layout, Position } from '../api/types.ts';
import { toModel, type CanvasModel, type CanvasNode } from './model.ts';

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
  // dagre runs only when a node needs it.
  const auto = missing.length === 0 ? {} : automatic(model);
  for (const node of missing) {
    const box = { ...auto[node.id]!, ...nodeSize() };
    if (stored !== undefined) while (clashes(placed, box)) box.y += GRID;
    positions[node.id] = { x: box.x, y: box.y };
    placed.push(box);
  }
  return { positions, autoPlaced: missing.map((n) => n.id) };
}

type Placed = { x: number; y: number; width: number; height: number };

/** Whether `box` comes within a grid step of a card in `placed`. */
const clashes = (placed: readonly Placed[], box: Placed) =>
  placed.some((p) => box.x < p.x + p.width + GRID && p.x < box.x + box.width + GRID && box.y < p.y + p.height + GRID && p.y < box.y + box.height + GRID);

/**
 * Where a card asked for at `at` goes among the cards at `positions`: `at`,
 * snapped to the grid, moved down the grid until it is clear of every one —
 * the rule the stored layout's missing nodes follow, for a node placed.
 */
export function clearSpot(positions: Readonly<Record<string, Position>>, at: Position): Position {
  const placed = Object.values(positions).map((p) => ({ ...p, ...nodeSize() }));
  const box = { x: snap(at.x), y: snap(at.y), ...nodeSize() };
  while (clashes(placed, box)) box.y += GRID;
  return { x: box.x, y: box.y };
}

/** A box in the graph's coordinates. */
export type Box = { x: number; y: number; width: number; height: number };

/** The box around every card at `positions`, from the top-left card corner. */
export function boundsOf(positions: Readonly<Record<string, Position>>): Box {
  const { width, height } = nodeSize();
  const at = Object.values(positions);
  if (at.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
  const x = Math.min(...at.map((p) => p.x));
  const y = Math.min(...at.map((p) => p.y));
  return { x, y, width: Math.max(...at.map((p) => p.x)) + width - x, height: Math.max(...at.map((p) => p.y)) + height - y };
}

/**
 * The area a view is fitted to: the cards' box, grown to `frame` from the
 * same top-left corner. Two canvases of one size fitted to one frame share
 * a zoom and an origin, so their graphs line up rank by rank.
 */
export function fitArea(bounds: Box, frame?: { width: number; height: number }): Box {
  if (frame === undefined) return bounds;
  return { ...bounds, width: Math.max(bounds.width, frame.width), height: Math.max(bounds.height, frame.height) };
}

/** The size of a graph's automatic layout: what a screen frames two graphs alike by. */
export function extentOf(graph: Graph): { width: number; height: number } {
  const { width, height } = boundsOf(resolveLayout(toModel(graph)).positions);
  return { width, height };
}

import dagre from '@dagrejs/dagre';
import type { CanvasModel, CanvasNode, NodeOverlay } from './model.ts';

/** A card's top-left corner on the canvas, in canvas pixels at zoom 1. */
export type Position = { x: number; y: number };

/**
 * A stored layout: node id to position, as the caller read it. Presentation
 * only — nothing here hashes it, and the canvas is whole without one.
 */
export type StoredLayout = Readonly<Record<string, Position>>;

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

// A card's height, from the rows it draws (NodeCard.css): the head, then the
// parameter row in edit-and-read, or the replay body's rows. An estimate on
// the high side, so the automatic layout never lets two cards touch; the
// canvas measures the real card once it is on screen.
const HEAD = 56;
const ROW = 32;
const ERROR = 56;

export function nodeSize(node: CanvasNode, overlay?: NodeOverlay): { width: number; height: number } {
  let height = HEAD;
  if (overlay === undefined) {
    if (node.param !== undefined) height += ROW;
  } else {
    const rows = [overlay.metric, overlay.ranks, overlay.discarded, overlay.durationMs ?? overlay.share].filter((r) => r !== undefined).length;
    if (rows > 0) height += ROW * rows + GRID;
  }
  if (overlay?.error !== undefined) height += ERROR;
  return { width: NODE_WIDTH, height };
}

const snap = (v: number) => Math.round(v / GRID) * GRID;

/** dagre, left to right, every card snapped to the grid. */
function automatic(model: CanvasModel, overlays: Readonly<Record<string, NodeOverlay>>): Record<string, Position> {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: 'LR', ranksep: RANK_GAP, nodesep: NODE_GAP, marginx: 0, marginy: 0 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const node of model.nodes) g.setNode(node.id, nodeSize(node, overlays[node.id]));
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
export function resolveLayout(model: CanvasModel, stored?: StoredLayout, overlays: Readonly<Record<string, NodeOverlay>> = {}): ResolvedLayout {
  const auto = automatic(model, overlays);
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
    placed.push({ ...at, ...nodeSize(node, overlays[node.id]) });
  }
  const clashes = (box: (typeof placed)[number]) =>
    placed.some((p) => box.x < p.x + p.width + GRID && p.x < box.x + box.width + GRID && box.y < p.y + p.height + GRID && p.y < box.y + box.height + GRID);
  for (const node of missing) {
    const box = { ...auto[node.id]!, ...nodeSize(node, overlays[node.id]) };
    if (stored !== undefined) while (clashes(box)) box.y += GRID;
    positions[node.id] = { x: box.x, y: box.y };
    placed.push(box);
  }
  return { positions, autoPlaced: missing.map((n) => n.id) };
}

// The editor store: the wire-schema document and the layout beside it, with
// undo and redo. Every mutation is one step; a change that changes nothing is
// none. Selection is not here: it is the address's (src/routes.ts).
// ARCHITECTURE.md § The editor.
import type { ParameterValue } from '../api/types.ts';
import type { Position } from '../canvas/index.ts';
import { freshId, type WireDocument, type WireNode } from './document.ts';

/** Positions by node id: presentation, never sent to validation (ADR-016 § 4). */
export type EditorLayout = Readonly<Record<string, Position>>;

type Snapshot = { doc: WireDocument; layout: EditorLayout };

export type EditorState = Snapshot & { past: readonly Snapshot[]; future: readonly Snapshot[] };

export type EditorAction =
  /** A node placed from the palette, with the parameters it is placed with: its required keys' starting values. */
  | { type: 'add'; component: string; impl: string; position?: Position; params?: Readonly<Record<string, ParameterValue>> }
  | { type: 'connect'; from: string; to: string; port: number }
  | { type: 'disconnect'; node: string; port: number }
  | { type: 'setParam'; node: string; key: string; value: ParameterValue }
  | { type: 'removeParam'; node: string; key: string }
  | { type: 'rename'; node: string; to: string }
  | { type: 'remove'; node: string }
  | { type: 'duplicate'; node: string }
  | { type: 'move'; node: string; position: Position }
  /** Many nodes moved at once — "Tidy layout" — as one step. */
  | { type: 'arrange'; positions: Readonly<Record<string, Position>> }
  /** Where the canvas's automatic layout put nodes no position named: kept, never a step. */
  | { type: 'placed'; positions: Readonly<Record<string, Position>> }
  | { type: 'undo' }
  | { type: 'redo' };

/** Where a duplicate lands: two grid steps right and down of its source. */
const DUPLICATE_OFFSET = 32;

export const initialEditor = (doc: WireDocument, layout: EditorLayout = {}): EditorState => ({ doc, layout, past: [], future: [] });
export const canUndo = (state: EditorState) => state.past.length > 0;
export const canRedo = (state: EditorState) => state.future.length > 0;

const withNodes = (doc: WireDocument, nodes: WireNode[]): WireDocument => ({ ...doc, pipeline: { ...doc.pipeline, nodes } });
const mapNode = (doc: WireDocument, id: string, change: (node: WireNode) => WireNode): WireDocument => withNodes(doc, doc.pipeline.nodes.map((n) => (n.id === id ? change(n) : n)));
const without = <T,>(record: Readonly<Record<string, T>>, key: string): Record<string, T> => Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));

/** The document and layout one mutation leaves, or null when it does not apply. */
function apply(state: Snapshot, action: Exclude<EditorAction, { type: 'undo' | 'redo' | 'placed' }>): Snapshot | null {
  const { doc, layout } = state;
  const find = (id: string) => doc.pipeline.nodes.find((n) => n.id === id);
  switch (action.type) {
    case 'add': {
      const id = freshId(doc, action.impl);
      const node: WireNode = { id, component: action.component, impl: action.impl, inputs: [], params: { ...action.params } };
      return { doc: withNodes(doc, [...doc.pipeline.nodes, node]), layout: action.position === undefined ? layout : { ...layout, [id]: action.position } };
    }
    case 'connect': {
      const to = find(action.to);
      if (to === undefined || action.port !== to.inputs.length) return null;
      return { doc: mapNode(doc, action.to, (n) => ({ ...n, inputs: [...n.inputs, action.from] })), layout };
    }
    case 'disconnect': {
      // Inputs are a list in port order: removing any but the last would
      // slide the ones after it into another port, a silent rewiring.
      const node = find(action.node);
      if (node === undefined || action.port !== node.inputs.length - 1) return null;
      return { doc: mapNode(doc, action.node, (n) => ({ ...n, inputs: n.inputs.slice(0, -1) })), layout };
    }
    case 'setParam':
      return { doc: mapNode(doc, action.node, (n) => ({ ...n, params: { ...n.params, [action.key]: action.value } })), layout };
    case 'removeParam':
      return { doc: mapNode(doc, action.node, (n) => ({ ...n, params: without(n.params, action.key) })), layout };
    case 'rename': {
      const { node: from, to } = action;
      if (to === '' || to === from || freshId(doc, to) !== to) return null;
      const nodes = doc.pipeline.nodes.map((n) => ({ ...n, id: n.id === from ? to : n.id, inputs: n.inputs.map((i) => (i === from ? to : i)) }));
      const moved = layout[from] === undefined ? layout : { ...without(layout, from), [to]: layout[from] };
      return { doc: withNodes(doc, nodes), layout: moved };
    }
    case 'remove':
      return { doc: withNodes(doc, doc.pipeline.nodes.filter((n) => n.id !== action.node)), layout: without(layout, action.node) };
    case 'duplicate': {
      const source = find(action.node);
      if (source === undefined) return null;
      const id = freshId(doc, source.impl);
      const at = layout[action.node];
      return {
        doc: withNodes(doc, [...doc.pipeline.nodes, { ...source, id, inputs: [...source.inputs], params: { ...source.params } }]),
        layout: at === undefined ? layout : { ...layout, [id]: { x: at.x + DUPLICATE_OFFSET, y: at.y + DUPLICATE_OFFSET } },
      };
    }
    case 'move':
      return { doc, layout: { ...layout, [action.node]: action.position } };
    case 'arrange':
      return { doc, layout: { ...layout, ...action.positions } };
  }
}

const same = (a: Snapshot, b: Snapshot) => JSON.stringify(a) === JSON.stringify(b);

export function editorReducer(state: EditorState, action: EditorAction): EditorState {
  const current: Snapshot = { doc: state.doc, layout: state.layout };
  if (action.type === 'placed') {
    // Only a node nothing has placed yet takes the position, in the present
    // and in every state undo and redo reach, so neither moves it again.
    const fill = (s: Snapshot): Snapshot => ({ ...s, layout: { ...action.positions, ...s.layout } });
    return { ...fill(current), past: state.past.map(fill), future: state.future.map(fill) };
  }
  if (action.type === 'undo') {
    const previous = state.past.at(-1);
    return previous === undefined ? state : { ...previous, past: state.past.slice(0, -1), future: [current, ...state.future] };
  }
  if (action.type === 'redo') {
    const next = state.future[0];
    return next === undefined ? state : { ...next, past: [...state.past, current], future: state.future.slice(1) };
  }
  const changed = apply(current, action);
  if (changed === null || same(changed, current)) return state;
  return { ...changed, past: [...state.past, current], future: [] };
}

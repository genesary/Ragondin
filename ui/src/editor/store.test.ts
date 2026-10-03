import { describe, expect, it } from 'vitest';
import { emptyDocument, validationRequest } from './document.ts';
import { HYBRID } from './fixtures.ts';
import { canRedo, canUndo, editorReducer, initialEditor, type EditorAction, type EditorState } from './store.ts';

const run = (state: EditorState, ...actions: EditorAction[]) => actions.reduce(editorReducer, state);
const bytes = (state: EditorState) => validationRequest(state.doc).document;
const node = (state: EditorState, id: string) => state.doc.pipeline.nodes.find((n) => n.id === id);

describe('the editor store', () => {
  it('opens on a document and a layout, with nothing to undo or redo', () => {
    const state = initialEditor(HYBRID, { lexical: { x: 0, y: 0 } });
    expect(state.doc).toEqual(HYBRID);
    expect(state.layout).toEqual({ lexical: { x: 0, y: 0 } });
    expect(canUndo(state)).toBe(false);
    expect(canRedo(state)).toBe(false);
  });

  it('adds a node with a fresh id derived from its implementation, at a position', () => {
    const state = run(initialEditor(HYBRID), { type: 'add', component: 'retriever', impl: 'bm25', position: { x: 32, y: 48 } });
    expect(node(state, 'bm25')).toEqual({ id: 'bm25', component: 'retriever', impl: 'bm25', inputs: [], params: {} });
    expect(state.layout['bm25']).toEqual({ x: 32, y: 48 });
  });

  it('connects a producer to the next port of a consumer', () => {
    const state = run(initialEditor(emptyDocument()), { type: 'add', component: 'retriever', impl: 'bm25', position: { x: 0, y: 0 } }, { type: 'connect', from: 'query', to: 'bm25', port: 0 });
    expect(node(state, 'bm25')?.inputs).toEqual(['query']);
  });

  it('makes every mutation one undo step: add, connect, set a parameter, then undo three times, gives back the same bytes', () => {
    const start = initialEditor(HYBRID);
    const edited = run(
      start,
      { type: 'add', component: 'reranker', impl: 'cross_encoder', position: { x: 0, y: 0 } },
      { type: 'connect', from: 'question', to: 'cross_encoder', port: 0 },
      { type: 'setParam', node: 'cross_encoder', key: 'top_k', value: 5 },
    );
    expect(node(edited, 'cross_encoder')).toEqual({ id: 'cross_encoder', component: 'reranker', impl: 'cross_encoder', inputs: ['question'], params: { top_k: 5 } });
    const once = run(edited, { type: 'undo' });
    expect(node(once, 'cross_encoder')).toEqual({ id: 'cross_encoder', component: 'reranker', impl: 'cross_encoder', inputs: ['question'], params: {} });
    const twice = run(once, { type: 'undo' });
    expect(node(twice, 'cross_encoder')).toEqual({ id: 'cross_encoder', component: 'reranker', impl: 'cross_encoder', inputs: [], params: {} });
    const undone = run(twice, { type: 'undo' });
    expect(node(undone, 'cross_encoder')).toBeUndefined();
    expect(bytes(undone)).toBe(bytes(start));
    expect(canUndo(undone)).toBe(false);
  });

  it('redoes what was undone, and forgets the redo once something else changes', () => {
    const edited = run(initialEditor(HYBRID), { type: 'setParam', node: 'lexical', key: 'top_k', value: 50 });
    const undone = run(edited, { type: 'undo' });
    expect(node(undone, 'lexical')?.params['top_k']).toBe(100);
    expect(canRedo(undone)).toBe(true);
    expect(node(run(undone, { type: 'redo' }), 'lexical')?.params['top_k']).toBe(50);
    const branched = run(undone, { type: 'setParam', node: 'lexical', key: 'top_k', value: 7 });
    expect(canRedo(branched)).toBe(false);
  });

  it('records no step for a change that changes nothing', () => {
    const same = run(initialEditor(HYBRID), { type: 'setParam', node: 'lexical', key: 'top_k', value: 100 });
    expect(canUndo(same)).toBe(false);
  });

  it('removes a parameter', () => {
    const state = run(initialEditor(HYBRID), { type: 'removeParam', node: 'vectors', key: 'embedder' });
    expect(node(state, 'vectors')?.params).toEqual({ top_k: 100 });
  });

  it('renames a node, and every input naming it and its position follow', () => {
    const state = run(initialEditor(HYBRID, { fused: { x: 1, y: 2 } }), { type: 'rename', node: 'fused', to: 'rrf' });
    expect(node(state, 'rrf')).toBeDefined();
    expect(node(state, 'reranked')?.inputs).toEqual(['question', 'rrf']);
    expect(state.layout).toEqual({ rrf: { x: 1, y: 2 } });
  });

  it('refuses a rename to an id a dangling input still names, recording nothing', () => {
    const deleted = run(initialEditor(HYBRID), { type: 'remove', node: 'fused' });
    const state = run(deleted, { type: 'rename', node: 'vectors', to: 'fused' });
    expect(node(state, 'vectors')).toBeDefined();
    expect(state.past).toHaveLength(1);
  });

  it('refuses a rename to an id already taken, recording nothing', () => {
    const state = run(initialEditor(HYBRID), { type: 'rename', node: 'fused', to: 'lexical' });
    expect(state.doc).toEqual(HYBRID);
    expect(canUndo(state)).toBe(false);
  });

  it('deletes a node and its position, and leaves its consumers naming it rather than rewiring them', () => {
    const state = run(initialEditor(HYBRID, { fused: { x: 1, y: 2 } }), { type: 'remove', node: 'fused' });
    expect(node(state, 'fused')).toBeUndefined();
    expect(node(state, 'reranked')?.inputs).toEqual(['question', 'fused']);
    expect(state.layout).toEqual({});
  });

  it('places a node of a deleted one\'s implementation under a new id, so the deleted node\'s consumers stay unwired', () => {
    const state = run(initialEditor(HYBRID), { type: 'remove', node: 'fused' }, { type: 'add', component: 'fusion', impl: 'fused' });
    expect(node(state, 'fused-2')).toBeDefined();
    expect(node(state, 'fused')).toBeUndefined();
    expect(node(state, 'reranked')?.inputs).toEqual(['question', 'fused']);
  });

  it('duplicates a node under a fresh id, with its parameters and inputs, beside it', () => {
    const state = run(initialEditor(HYBRID, { lexical: { x: 0, y: 0 } }), { type: 'duplicate', node: 'lexical' });
    expect(node(state, 'bm25')).toEqual({ id: 'bm25', component: 'retriever', impl: 'bm25', inputs: ['question'], params: { top_k: 100 } });
    expect(state.layout['bm25']).toEqual({ x: 32, y: 32 });
  });

  it('removes the last edge into a node, and no other, so no input slides into another port', () => {
    const last = run(initialEditor(HYBRID), { type: 'disconnect', node: 'reranked', port: 1 });
    expect(node(last, 'reranked')?.inputs).toEqual(['question']);
    const inner = run(initialEditor(HYBRID), { type: 'disconnect', node: 'reranked', port: 0 });
    expect(node(inner, 'reranked')?.inputs).toEqual(['question', 'fused']);
    expect(canUndo(inner)).toBe(false);
  });

  it('keeps where the automatic layout put a node without making it a step, in every state undo can reach', () => {
    const moved = run(initialEditor(HYBRID), { type: 'move', node: 'lexical', position: { x: 0, y: 0 } });
    const placed = run(moved, { type: 'placed', positions: { fused: { x: 320, y: 16 } } });
    expect(placed.layout).toEqual({ lexical: { x: 0, y: 0 }, fused: { x: 320, y: 16 } });
    expect(run(placed, { type: 'placed', positions: { lexical: { x: 9, y: 9 } } }).layout['lexical']).toEqual({ x: 0, y: 0 });
    expect(placed.past).toHaveLength(1);
    expect(run(placed, { type: 'undo' }).layout).toEqual({ fused: { x: 320, y: 16 } });
  });

  it('moves a node, as one step', () => {
    const state = run(initialEditor(HYBRID), { type: 'move', node: 'lexical', position: { x: 64, y: 16 } });
    expect(state.layout['lexical']).toEqual({ x: 64, y: 16 });
    expect(run(state, { type: 'undo' }).layout['lexical']).toBeUndefined();
  });
});

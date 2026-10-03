import { describe, expect, it } from 'vitest';
import { danglingInputs, emptyDocument, freshId, toGraph, validationRequest, type WireDocument } from './document.ts';
import { GRAMMAR, HYBRID } from './fixtures.ts';

describe('the wire-schema document', () => {
  it('starts empty: one declared input, no node, and no version line, which reads as the version the server writes', () => {
    expect(emptyDocument()).toEqual({ pipeline: { inputs: ['query'], nodes: [] } });
  });

  it('gives a placed node an id derived from its implementation, the first free one', () => {
    expect(freshId(emptyDocument(), 'bm25')).toBe('bm25');
    const doc: WireDocument = { pipeline: { inputs: ['query'], nodes: [{ id: 'bm25', component: 'retriever', impl: 'bm25', inputs: [], params: {} }] } };
    expect(freshId(doc, 'bm25')).toBe('bm25-2');
    doc.pipeline.nodes.push({ id: 'bm25-2', component: 'retriever', impl: 'bm25', inputs: [], params: {} });
    expect(freshId(doc, 'bm25')).toBe('bm25-3');
  });

  it('never gives a node an id some input still names, so a deleted node placed again does not take back its edges', () => {
    const doc: WireDocument = { pipeline: { inputs: ['query'], nodes: [{ id: 'concat', component: 'context_builder', impl: 'concat', inputs: ['query', 'bm25'], params: {} }] } };
    expect(freshId(doc, 'bm25')).toBe('bm25-2');
  });

  it('never gives a node the id of a declared input', () => {
    expect(freshId(emptyDocument(), 'query')).toBe('query-2');
  });

  it('is sent to validation as the wire schema and nothing else: no position, no presentation', () => {
    const body = validationRequest(HYBRID);
    expect(Object.keys(body)).toEqual(['document']);
    expect(JSON.parse(body.document)).toEqual(HYBRID);
  });
});

describe('the graph the canvas draws from the document', () => {
  it('lists the declared inputs, the nodes sorted by id, and one edge per input in port order', () => {
    const graph = toGraph(HYBRID, GRAMMAR);
    expect(graph.inputs).toEqual([{ id: 'question', kind: 'query' }]);
    expect(graph.nodes.map((n) => n.id)).toEqual(['fused', 'lexical', 'reranked', 'vectors']);
    expect(graph.nodes.find((n) => n.id === 'lexical')).toEqual({ id: 'lexical', family: 'retriever', implementation: 'bm25', parameters: { top_k: 100 } });
    expect(graph.edges.filter((e) => e.to === 'reranked')).toEqual([
      { from: 'question', to: 'reranked', port: 0, kind: 'query' },
      { from: 'fused', to: 'reranked', port: 1, kind: 'chunks' },
    ]);
  });

  it('types an edge by what its producer puts out, as the grammar says, and as opaque when nothing says', () => {
    expect(toGraph(HYBRID, GRAMMAR).edges.find((e) => e.from === 'lexical')?.kind).toBe('chunks');
    expect(toGraph(HYBRID, null).edges.find((e) => e.from === 'lexical')?.kind).toBe('opaque');
  });

  it('draws no edge from a node that does not exist', () => {
    const doc: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: ['gone'], params: {} }] } };
    expect(toGraph(doc, GRAMMAR).edges).toEqual([]);
  });

  it('marks a consumer whose input names no node as invalid, in words', () => {
    const doc: WireDocument = {
      pipeline: { inputs: ['question'], nodes: [{ id: 'fused', component: 'fusion', impl: 'rrf', inputs: ['question', 'gone'], params: {} }] },
    };
    expect(danglingInputs(doc)).toEqual({ fused: 'Port 1 names `gone`, which is not a node or an input.' });
    expect(danglingInputs(HYBRID)).toEqual({});
  });
});

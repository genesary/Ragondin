import { describe, expect, it } from 'vitest';
import type { Graph } from '../api/types.ts';
import { HYBRID_RERANK_GEN } from './fixtures.ts';
import { toModel } from './model.ts';

const byId = (graph: Graph, id: string) => {
  const node = toModel(graph).nodes.find((n) => n.id === id);
  if (node === undefined) throw new Error(`no node ${id}`);
  return node;
};

describe('toModel', () => {
  it('draws the declared inputs as query nodes, beside every node of the graph', () => {
    const ids = toModel(HYBRID_RERANK_GEN).nodes.map((n) => n.id);
    expect([...ids].sort()).toEqual(['answer', 'fused', 'lexical', 'prompt', 'question', 'reranked', 'vectors']);
    expect(byId(HYBRID_RERANK_GEN, 'question')).toMatchObject({ family: 'query', inputs: [], output: 'query' });
  });

  it('maps each API family onto the design system family, and an unknown one onto the neutral control tile', () => {
    const families = Object.fromEntries(toModel(HYBRID_RERANK_GEN).nodes.map((n) => [n.id, n.family]));
    expect(families).toEqual({
      question: 'query',
      lexical: 'retriever',
      vectors: 'retriever',
      fused: 'fusion',
      reranked: 'reranker',
      prompt: 'context',
      answer: 'generator',
    });
    const extension: Graph = {
      inputs: [],
      nodes: [{ id: 'x', family: 'extension', implementation: 'branch', parameters: {} }],
      edges: [],
    };
    expect(byId(extension, 'x').family).toBe('control');
  });

  it('names the implementation as the configuration spells it', () => {
    expect(byId(HYBRID_RERANK_GEN, 'lexical').impl).toBe('retriever/bm25');
    expect(byId(HYBRID_RERANK_GEN, 'prompt').impl).toBe('context_builder/concat');
    expect(byId(HYBRID_RERANK_GEN, 'question').impl).toBe('pipeline input');
  });

  it('shows the one key parameter of each family, and nothing when it is absent', () => {
    expect(byId(HYBRID_RERANK_GEN, 'lexical').param).toEqual({ name: 'top_k', value: '3' });
    expect(byId(HYBRID_RERANK_GEN, 'reranked').param).toEqual({ name: 'top_k', value: '10' });
    expect(byId(HYBRID_RERANK_GEN, 'fused').param).toEqual({ name: 'k', value: '60' });
    expect(byId(HYBRID_RERANK_GEN, 'prompt').param).toEqual({ name: 'max_chunks', value: '5' });
    expect(byId(HYBRID_RERANK_GEN, 'answer').param).toEqual({ name: 'temperature', value: '0.2' });
    const bare: Graph = {
      inputs: [],
      nodes: [{ id: 'r', family: 'retriever', implementation: 'bm25', parameters: { label: 'x' } }],
      edges: [],
    };
    expect(byId(bare, 'r').param).toBeUndefined();
  });

  it('types every input port by the kind of its edge, in port order, and the output by what the node puts out', () => {
    expect(byId(HYBRID_RERANK_GEN, 'answer')).toMatchObject({ inputs: ['query', 'context'], output: 'answer' });
    expect(byId(HYBRID_RERANK_GEN, 'reranked')).toMatchObject({ inputs: ['query', 'chunks'], output: 'chunks' });
    expect(byId(HYBRID_RERANK_GEN, 'fused')).toMatchObject({ inputs: ['chunks', 'chunks'], output: 'chunks' });
    expect(byId(HYBRID_RERANK_GEN, 'prompt')).toMatchObject({ inputs: ['query', 'chunks'], output: 'context' });
  });

  it('keeps one edge per entry of the graph, from output port to the numbered input port', () => {
    const { edges } = toModel(HYBRID_RERANK_GEN);
    expect(edges).toHaveLength(HYBRID_RERANK_GEN.edges.length);
    expect(edges).toContainEqual({ id: 'prompt->answer:1', from: 'prompt', to: 'answer', port: 1, kind: 'context' });
  });

  it('orders the nodes topologically, inputs first and ties broken by the canonical order', () => {
    expect(toModel(HYBRID_RERANK_GEN).nodes.map((n) => n.id)).toEqual([
      'question',
      'lexical',
      'vectors',
      'fused',
      'reranked',
      'prompt',
      'answer',
    ]);
  });
});

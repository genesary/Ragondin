import { describe, expect, it } from 'vitest';
import type { Graph } from '../api/types.ts';
import { float, str } from '../parameters.ts';
import { HYBRID_RERANK_GEN } from './fixtures.ts';
import { describeOverlay, toModel } from './model.ts';

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
    // A family named like an inherited object property is still unknown.
    for (const family of ['constructor', 'toString', '__proto__']) {
      const odd: Graph = { inputs: [], nodes: [{ id: 'x', family, implementation: 'y', parameters: {} }], edges: [] };
      expect(byId(odd, 'x').family, family).toBe('control');
    }
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
    expect(byId(HYBRID_RERANK_GEN, 'prompt').param).toEqual({ name: 'budget', value: '2000' });
    expect(byId(HYBRID_RERANK_GEN, 'answer').param).toEqual({ name: 'temperature', value: '0.2' });
    const bare: Graph = {
      inputs: [],
      nodes: [{ id: 'r', family: 'retriever', implementation: 'bm25', parameters: { label: str('x') } }],
      edges: [],
    };
    expect(byId(bare, 'r').param).toBeUndefined();
  });

  it('draws a float with its fractional part, so it never reads as the integer of the same value', () => {
    const graph: Graph = {
      inputs: [],
      nodes: [{ id: 'f', family: 'fusion', implementation: 'rrf', parameters: { k: float(60) } }],
      edges: [],
    };
    expect(byId(graph, 'f').param).toEqual({ name: 'k', value: '60.0' });
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

describe('describeOverlay', () => {
  it('says in words what the replay card draws: the metric, the rank strip, the discarded count and the time', () => {
    expect(describeOverlay({ metric: { name: 'ndcg@10', value: '0.8610' }, ranks: [3, 1], discarded: 2, durationMs: 349, share: 0.85 })).toBe(
      "ndcg@10 0.8610. 2 gold passages in the top 10, at rank 1, 3. 2 discarded. 349 ms, 85% of this query's time.",
    );
  });

  it('says each field on its own, and nothing for an empty overlay', () => {
    expect(describeOverlay({ durationMs: 4 })).toBe('4 ms.');
    expect(describeOverlay({ share: 0.5 })).toBe("50% of this query's time.");
    expect(describeOverlay({ share: 0.004 })).toBe("under 1% of this query's time.");
    expect(describeOverlay({ ranks: [] })).toBe('0 gold passages in the top 10.');
    expect(describeOverlay({})).toBe('');
  });

  it('says why a node shows no result: it failed, with the message, or it was not run', () => {
    expect(describeOverlay({ error: 'the service did not answer', durationMs: 3 })).toBe('Failed: the service did not answer. 3 ms.');
    expect(describeOverlay({ notRun: true })).toBe('Not run.');
  });
});

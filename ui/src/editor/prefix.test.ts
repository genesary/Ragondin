import { describe, expect, it } from 'vitest';
import { HYBRID, HYBRID_RAG } from './fixtures.ts';
import { runUpTo } from './prefix.ts';

const STORED = { name: 'hybrid-rag', unchanged: true };

describe('runUpTo', () => {
  it('opens on a ranking node, naming what is kept, what is skipped, and the generation it saves', () => {
    expect(runUpTo(HYBRID_RAG, 'reranked', STORED)).toEqual({
      kind: 'open',
      line: 'Keeps lexical, vectors, fused and reranked. Skips context and answer, so no generation cost.',
    });
  });

  it('names no generation cost when no generator is skipped', () => {
    expect(runUpTo(HYBRID, 'fused', { name: 'hybrid', unchanged: true })).toEqual({ kind: 'open', line: 'Keeps lexical, vectors and fused. Skips reranked.' });
  });

  it('keeps only what the node reads: the sibling leg is skipped with everything after', () => {
    expect(runUpTo(HYBRID_RAG, 'lexical', STORED)).toEqual({ kind: 'open', line: 'Keeps lexical. Skips vectors, fused, reranked, context and answer, so no generation cost.' });
  });

  it('refuses the pipeline’s output, a context builder, and a node that is not there, each with its reason', () => {
    expect(runUpTo(HYBRID_RAG, 'answer', STORED)).toEqual({ kind: 'refused', reason: 'This node is the pipeline’s output: the prefix would be the whole pipeline.' });
    expect(runUpTo(HYBRID_RAG, 'context', STORED)).toEqual({ kind: 'refused', reason: 'A context is scored by nothing: run up to the node that feeds it chunks.' });
    expect(runUpTo(HYBRID_RAG, 'question', STORED).kind).toBe('refused');
  });

  it('refuses while the canvas is not the stored document, since a run takes the stored one', () => {
    expect(runUpTo(HYBRID_RAG, 'reranked', { name: null, unchanged: true })).toEqual({ kind: 'refused', reason: 'Not a workspace pipeline yet: a run takes a stored document.' });
    expect(runUpTo(HYBRID_RAG, 'reranked', { name: 'hybrid-rag', unchanged: false })).toEqual({
      kind: 'refused',
      reason: 'The canvas differs from the stored document, and a run takes the stored one.',
    });
  });
});

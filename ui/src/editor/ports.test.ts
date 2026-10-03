import { describe, expect, it } from 'vitest';
import type { WireDocument } from './document.ts';
import { GRAMMAR, HYBRID, WORKSPACE } from './fixtures.ts';
import { grammarOf, portsOf, refusal } from './ports.ts';

const node = (doc: WireDocument, id: string) => doc.pipeline.nodes.find((n) => n.id === id)!;

// A reranker with nothing wired yet, beside the hybrid.
const WITH_FRESH: WireDocument = {
  pipeline: {
    inputs: HYBRID.pipeline.inputs,
    nodes: [...HYBRID.pipeline.nodes, { id: 'second', component: 'reranker', impl: 'cross_encoder', inputs: [], params: {} }],
  },
};

describe('the grammar, from this build\'s capabilities', () => {
  it('reads each node family\'s ports from `GET /workspace`, and none for `embedder`, which no node is', () => {
    expect(grammarOf(WORKSPACE.capabilities)).toEqual(GRAMMAR);
  });
});

describe('the ports a node draws in write mode', () => {
  it('draws a fixed family its declared ports, typed, filled or not', () => {
    expect(portsOf(node(WITH_FRESH, 'second'), GRAMMAR)).toEqual({ inputs: ['query', 'chunks'], output: 'chunks' });
  });

  it('draws a variadic family one port per input and one open port after them', () => {
    expect(portsOf(node(HYBRID, 'fused'), GRAMMAR)).toEqual({ inputs: ['chunks', 'chunks', 'chunks'], output: 'chunks' });
  });

  it('with no grammar, draws one port per input and one open after, of no known kind', () => {
    expect(portsOf(node(HYBRID, 'reranked'), null)).toEqual({ inputs: ['opaque', 'opaque', 'opaque'], output: 'opaque' });
  });
});

describe('an edge refused during the drag', () => {
  it('accepts a producer of the kind the port expects, at the next free port', () => {
    expect(refusal(WITH_FRESH, GRAMMAR, 'question', 'second', 0)).toBeNull();
  });

  it('refuses a mis-kinded edge naming both kinds, in the order the validation names them', () => {
    expect(refusal(WITH_FRESH, GRAMMAR, 'lexical', 'second', 0)).toBe('`lexical` feeds `second` at port 0: expected query, found chunks.');
  });

  it('refuses a port that already holds an edge, naming what it holds', () => {
    expect(refusal(HYBRID, GRAMMAR, 'vectors', 'reranked', 1)).toBe('Port 1 of `reranked` already holds `fused`.');
  });

  it('refuses a port past the next free one, since inputs are a list in port order', () => {
    expect(refusal(WITH_FRESH, GRAMMAR, 'fused', 'second', 1)).toBe('Fill port 0 of `second` first: its inputs are a list, in port order.');
  });

  it('refuses a port the family does not declare', () => {
    const full: WireDocument = { pipeline: { inputs: ['question'], nodes: [{ id: 'r', component: 'retriever', impl: 'bm25', inputs: ['question'], params: {} }] } };
    expect(refusal(full, GRAMMAR, 'question', 'r', 1)).toBe('`r` declares no port at position 1.');
  });

  it('refuses an edge that would close a cycle, as such', () => {
    const doc: WireDocument = {
      pipeline: {
        inputs: ['question'],
        nodes: [
          { id: 'a', component: 'fusion', impl: 'rrf', inputs: [], params: {} },
          { id: 'b', component: 'fusion', impl: 'rrf', inputs: ['a'], params: {} },
        ],
      },
    };
    expect(refusal(doc, GRAMMAR, 'b', 'a', 0)).toBe('`b` feeding `a` would close a cycle: `a` already feeds `b`.');
    expect(refusal(doc, GRAMMAR, 'a', 'a', 1)).toBe('`a` feeding `a` would close a cycle: a node cannot feed itself.');
  });

  it('refuses no kind without a grammar, and still refuses an occupied port and a cycle', () => {
    expect(refusal(WITH_FRESH, null, 'lexical', 'second', 0)).toBeNull();
    expect(refusal(HYBRID, null, 'vectors', 'reranked', 1)).toBe('Port 1 of `reranked` already holds `fused`.');
    expect(refusal(HYBRID, null, 'reranked', 'lexical', 1)).toBe('`reranked` feeding `lexical` would close a cycle: `lexical` already feeds `reranked`.');
  });

  it('refuses an edge into a declared input, which consumes nothing', () => {
    expect(refusal(HYBRID, GRAMMAR, 'lexical', 'question', 0)).toBe('`question` is a declared input: it consumes nothing.');
  });
});

import { describe, expect, it } from 'vitest';
import type { Capabilities } from '../api/types.ts';
import { int, str } from '../parameters.ts';
import { exampleDocument, freshName } from './example.ts';
import { WORKSPACE } from './fixtures.ts';

/** The recorded capabilities, with the ONNX embedder carried: a build with `bm25` and `onnx`. */
const WITH_ONNX: Capabilities = {
  ...WORKSPACE.capabilities,
  families: WORKSPACE.capabilities.families.map((f) => (f.family === 'embedder' ? { ...f, local: ['onnx'], not_carried: [] } : f)),
};
const without = (caps: Capabilities, family: string, impl: string): Capabilities => ({
  ...caps,
  families: caps.families.map((f) => (f.family === family ? { ...f, local: f.local.filter((l) => l !== impl) } : f)),
});
const carried = (caps: Capabilities, family: string, impl: string) => caps.families.find((f) => f.family === family)?.local.includes(impl) === true;

describe('the first-launch example', () => {
  it('is the lexical leg alone on a build whose dense retriever needs a service', () => {
    // The recorded build carries `dense` but no local embedder: a dense leg would need a bound service.
    expect(exampleDocument(WORKSPACE.capabilities)).toEqual({
      pipeline: { inputs: ['query'], nodes: [{ id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['query'], params: { top_k: int('10') } }] },
    });
  });

  it('is the two legs and a fusion on a build that embeds locally', () => {
    const doc = exampleDocument(WITH_ONNX)!;
    expect(doc.pipeline.nodes.map((n) => [n.id, n.component, n.impl, n.inputs])).toEqual([
      ['lexical', 'retriever', 'bm25', ['query']],
      ['vectors', 'retriever', 'dense', ['query']],
      ['fused', 'fusion', 'rrf', ['lexical', 'vectors']],
    ]);
    expect(doc.pipeline.nodes[1]!.params.embedder).toEqual(str('onnx'));
    // Every node is one this build carries, so it runs without any service.
    for (const node of doc.pipeline.nodes) expect(carried(WITH_ONNX, node.component, node.impl), node.id).toBe(true);
  });

  it('is the dense leg alone when the build carries no lexical retriever, and nothing when it carries neither', () => {
    expect(exampleDocument(without(WITH_ONNX, 'retriever', 'bm25'))!.pipeline.nodes.map((n) => n.id)).toEqual(['vectors']);
    expect(exampleDocument(without(WORKSPACE.capabilities, 'retriever', 'bm25'))).toBeNull();
  });
});

describe('a fresh name', () => {
  it('is the base, or the first numbered one free', () => {
    expect(freshName('example', [])).toBe('example');
    expect(freshName('example', ['example', 'example-2'])).toBe('example-3');
  });

  it('is never taken in another case, since a filesystem that ignores case holds one file', () => {
    expect(freshName('hybrid', ['Hybrid'])).toBe('hybrid-2');
  });
});

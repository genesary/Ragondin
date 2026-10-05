import { describe, expect, it } from 'vitest';
import type { Capabilities } from '../api/types.ts';
import { int } from '../parameters.ts';
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

const LEXICAL = {
  pipeline: { inputs: ['query'], nodes: [{ id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['query'], params: { top_k: int('10') } }] },
};

describe('the first-launch example', () => {
  it('is the lexical leg alone, which runs without any service', () => {
    expect(exampleDocument(WORKSPACE.capabilities)).toEqual(LEXICAL);
  });

  it('is the lexical leg alone on a build that embeds locally too: a dense leg needs a model file the workspace does not hold', () => {
    expect(exampleDocument(WITH_ONNX)).toEqual(LEXICAL);
  });

  it('is nothing on a build without a lexical retriever', () => {
    expect(exampleDocument(without(WITH_ONNX, 'retriever', 'bm25'))).toBeNull();
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

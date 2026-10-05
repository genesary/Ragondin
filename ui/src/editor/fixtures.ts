import type { ServiceListing, Workspace } from '../api/types.ts';
import type { WireDocument } from './document.ts';
import { int, str } from '../parameters.ts';
import type { PortGrammar } from './ports.ts';

/**
 * Each node family's ports, as `ragondin-pipeline` declares them
 * (`consumed_kinds`, `produced_kind`; ADR-C16): what `grammarOf` must read
 * from the recorded capabilities below. The bundle never imports this file.
 */
export const GRAMMAR: PortGrammar = {
  retriever: { produces: 'chunks', consumes: { fixed: ['query'] } },
  fusion: { produces: 'chunks', consumes: { variadic: 'chunks' } },
  reranker: { produces: 'chunks', consumes: { fixed: ['query', 'chunks'] } },
  context_builder: { produces: 'context', consumes: { fixed: ['query', 'chunks'] } },
  generator: { produces: 'answer', consumes: { fixed: ['query', 'context'] } },
};

/** A hybrid retrieval pipeline with a reranker, in the wire schema. */
export const HYBRID: WireDocument = {
  pipeline: {
    inputs: ['question'],
    nodes: [
      { id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['question'], params: { top_k: int('100') } },
      { id: 'vectors', component: 'retriever', impl: 'dense', inputs: ['question'], params: { top_k: int('100'), embedder: str('bge') } },
      { id: 'fused', component: 'fusion', impl: 'rrf', inputs: ['lexical', 'vectors'], params: { k: int('60') } },
      { id: 'reranked', component: 'reranker', impl: 'cross_encoder', inputs: ['question', 'fused'], params: { top_k: int('10') } },
    ],
  },
};

/** {@link HYBRID} with a context builder and a generator after the reranker. */
export const HYBRID_RAG: WireDocument = {
  pipeline: {
    inputs: ['question'],
    nodes: [
      ...HYBRID.pipeline.nodes,
      { id: 'context', component: 'context_builder', impl: 'concat', inputs: ['question', 'reranked'], params: {} },
      { id: 'answer', component: 'generator', impl: 'qwen', inputs: ['question', 'context'], params: {} },
    ],
  },
};

/**
 * A true diamond: one retrieval leg read by two rerankers, whose rankings a
 * fusion joins, then a context and an answer.
 */
export const DIAMOND: WireDocument = {
  pipeline: {
    inputs: ['question'],
    nodes: [
      { id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['question'], params: {} },
      { id: 'first', component: 'reranker', impl: 'cross_encoder', inputs: ['question', 'lexical'], params: {} },
      { id: 'second', component: 'reranker', impl: 'cross_encoder', inputs: ['question', 'lexical'], params: {} },
      { id: 'fused', component: 'fusion', impl: 'rrf', inputs: ['first', 'second'], params: {} },
      { id: 'context', component: 'context_builder', impl: 'concat', inputs: ['question', 'fused'], params: {} },
      { id: 'answer', component: 'generator', impl: 'qwen', inputs: ['question', 'context'], params: {} },
    ],
  },
};

/**
 * Two declared inputs, which the server refuses — a pipeline declares exactly
 * one — and a document being edited can hold anyway.
 */
export const TWO_INPUTS: WireDocument = {
  pipeline: {
    inputs: ['question', 'filter'],
    nodes: [
      { id: 'lexical', component: 'retriever', impl: 'bm25', inputs: ['question'], params: {} },
      { id: 'filtered', component: 'retriever', impl: 'bm25', inputs: ['filter'], params: {} },
      { id: 'fused', component: 'fusion', impl: 'rrf', inputs: ['lexical', 'filtered'], params: {} },
      { id: 'reranked', component: 'reranker', impl: 'cross_encoder', inputs: ['question', 'fused'], params: {} },
    ],
  },
};

/**
 * A recorded `GET /workspace` answer: the capabilities a build with `ui`,
 * `bm25` and `remote`, and without `onnx` or `stub`, served on 2026-10-03
 * (`cargo build -p ragondin --features ui,bm25,remote`), copied verbatim. A
 * `remote` build carries `dense`; `cross_encoder`, the ONNX embedder and
 * `stub_generator` are what it does not carry, each with the binary's reason.
 */
export const WORKSPACE: Workspace = {
  path: '/home/ada/ragondin-ws',
  build: '0.1.0+aaaaaaaaaaaa',
  settings: { datasets: '/home/ada/ragondin-ws/datasets', services: [] },
  capabilities: {
    families: [
      { family: 'retriever', local: ['bm25', 'dense'], ports: { produces: 'chunks', consumes: { shape: 'fixed', kinds: ['query'] } }, not_carried: [] },
      { family: 'fusion', local: ['rrf'], ports: { produces: 'chunks', consumes: { shape: 'variadic', kind: 'chunks' } }, not_carried: [] },
      { family: 'reranker', local: [], ports: { produces: 'chunks', consumes: { shape: 'fixed', kinds: ['query', 'chunks'] } }, not_carried: [{ name: 'cross_encoder', reason: 'needs the `onnx` feature' }] },
      { family: 'context_builder', local: ['concat'], ports: { produces: 'context', consumes: { shape: 'fixed', kinds: ['query', 'chunks'] } }, not_carried: [] },
      { family: 'generator', local: [], ports: { produces: 'answer', consumes: { shape: 'fixed', kinds: ['query', 'context'] } }, not_carried: [{ name: 'stub_generator', reason: 'needs the `stub` feature' }] },
      { family: 'embedder', local: [], ports: null, not_carried: [{ name: 'onnx', reason: 'needs the `onnx` feature' }] },
    ],
    remote: true,
  },
  counts: { pipelines: 1, runs: 0, benchmarks_ready: 1, services_connected: 1 },
};

/** A recorded `GET /services` answer: one generator and one embedder bound. */
export const SERVICES: ServiceListing = {
  services: [
    { family: 'generator', name: 'qwen', uri: '127.0.0.1:8080', connected: true, identity: 'qwen2.5-7b-instruct' },
    { family: 'embedder', name: 'bge', uri: '127.0.0.1:8081', connected: false, identity: null },
  ],
};

import { float, int, str } from '../parameters.ts';
import type { Graph } from '../api/types.ts';

/**
 * `hybrid-rerank-gen`: the graph `GET /runs/{id}` serves for the M3 exit
 * criterion's generation pipeline (bm25 and dense legs, rrf, a cross-encoder,
 * a context builder, a generator), lowered as the API lowers it — nodes sorted
 * by id, one edge per entry of a node's inputs in port order. The parameters
 * are trimmed, and `max_chunks` and `temperature` set, so that every family
 * shows its key parameter. The tests and the design-system preview draw it;
 * nothing in the bundle imports it.
 */
export const HYBRID_RERANK_GEN: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [
    {
      id: 'answer',
      family: 'generator',
      implementation: 'answerer',
      parameters: { served_model: str('qwen2.5-7b-instruct'), temperature: float(0.2) },
    },
    { id: 'fused', family: 'fusion', implementation: 'rrf', parameters: { k: int('60') } },
    { id: 'lexical', family: 'retriever', implementation: 'bm25', parameters: { top_k: int('3') } },
    { id: 'prompt', family: 'context_builder', implementation: 'concat', parameters: { max_chunks: int('5'), separator: str('\n') } },
    { id: 'reranked', family: 'reranker', implementation: 'cross_encoder', parameters: { top_k: int('10') } },
    { id: 'vectors', family: 'retriever', implementation: 'dense', parameters: { top_k: int('3'), embedder: str('onnx') } },
  ],
  edges: [
    { from: 'question', to: 'answer', port: 0, kind: 'query' },
    { from: 'prompt', to: 'answer', port: 1, kind: 'context' },
    { from: 'lexical', to: 'fused', port: 0, kind: 'chunks' },
    { from: 'vectors', to: 'fused', port: 1, kind: 'chunks' },
    { from: 'question', to: 'lexical', port: 0, kind: 'query' },
    { from: 'question', to: 'prompt', port: 0, kind: 'query' },
    { from: 'reranked', to: 'prompt', port: 1, kind: 'chunks' },
    { from: 'question', to: 'reranked', port: 0, kind: 'query' },
    { from: 'fused', to: 'reranked', port: 1, kind: 'chunks' },
    { from: 'question', to: 'vectors', port: 0, kind: 'query' },
  ],
};

/**
 * A graph with an extension node, whose output is `opaque`: a lexical leg
 * passed through a `gate` before the generator. The neutral diamond tile and
 * the `opaque` port are drawn from it.
 */
export const GATED_GEN: Graph = {
  inputs: [{ id: 'question', kind: 'query' }],
  nodes: [
    { id: 'answer', family: 'generator', implementation: 'answerer', parameters: {} },
    { id: 'gate', family: 'extension', implementation: 'threshold', parameters: { min_score: float(0.4) } },
    { id: 'lexical', family: 'retriever', implementation: 'bm25', parameters: { top_k: int('10') } },
  ],
  edges: [
    { from: 'question', to: 'answer', port: 0, kind: 'query' },
    { from: 'gate', to: 'answer', port: 1, kind: 'opaque' },
    { from: 'lexical', to: 'gate', port: 0, kind: 'chunks' },
    { from: 'question', to: 'lexical', port: 0, kind: 'query' },
  ],
};

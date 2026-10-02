//! Runs built in the test, over a benchmark built or loaded in the test: the
//! record a harness would have stored, assembled from the pieces the store
//! holds — a configuration document, one typed trace per query, the metrics —
//! with the identity tuple's two dataset digests computed by
//! `ragondin-benchmarks`' own definitions.
//!
//! Nothing here scores anything: the metrics a run is given are the ones a
//! test states, from a reference implementation or by hand, and the API is
//! what is checked against them.

use std::collections::BTreeMap;

use ragondin_benchmarks::identity::{dataset_version, CorpusIndex};
use ragondin_benchmarks::Benchmark;
use ragondin_experiments::{
    lower_configuration, ConfigDocument, Metrics, Run, RunId, RunInputs, Trace, TraceChunk,
    TraceDocument, TraceNode, TraceSummary,
};
use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, QueryId};

/// A retriever, then a reranker: the ranking the metrics read is the
/// reranker's, and the retriever's is a second one to score per node.
pub const RERANKED: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: leg
      component: retriever
      impl: dense
      inputs: [question]
    - id: reranked
      component: reranker
      impl: cross_encoder
      inputs: [question, leg]
";

/// [`RERANKED`], then a context builder and a generator: the ranking the
/// metrics read is still the reranker's (the builder's chunks port names it),
/// and the answer is the generator's.
pub const GENERATION: &str = "\
pipeline:
  inputs: [question]
  nodes:
    - id: leg
      component: retriever
      impl: dense
      inputs: [question]
    - id: reranked
      component: reranker
      impl: cross_encoder
      inputs: [question, leg]
    - id: prompt
      component: context_builder
      impl: concat
      inputs: [question, reranked]
    - id: answer
      component: generator
      impl: answerer
      inputs: [question, prompt]
";

/// The run id `[byte; 32]` renders to.
pub fn run_id(byte: u8) -> RunId {
    RunId::from_digest([byte; 32])
}

/// A run of `config` over `benchmark`, filed under `run_id(id)`, with the
/// dataset digests the harness would record for it: `dataset_version` over
/// the benchmark, `index_version` over the chunk set derived from its corpus.
pub fn run_over(
    id: u8,
    config: &str,
    benchmark: &Benchmark,
    traces: Vec<(&str, Trace)>,
    metrics: &[(&str, f64)],
) -> Run {
    let config = ConfigDocument::new(config);
    let pipeline = lower_configuration(&config).expect("a test configuration lowers");
    let mut recorded = Metrics::default();
    for (name, value) in metrics {
        recorded.insert(*name, *value);
    }
    Run {
        id: run_id(id),
        inputs: RunInputs {
            pipeline: pipeline.content_hash(),
            dataset_version: dataset_version(benchmark),
            index_version: CorpusIndex::build(benchmark.corpus()).version().to_owned(),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: recorded,
        config,
        traces: traces
            .into_iter()
            .map(|(query, trace)| (QueryId::new(query), TraceDocument::from(trace)))
            .collect(),
        bindings: Vec::new(),
        times: None,
    }
}

/// One chunk of a produced ranking or context.
pub fn chunk(chunk: &str, document: &str, score: f64) -> TraceChunk {
    TraceChunk {
        chunk: ChunkId::new(chunk),
        document: DocId::new(document),
        score,
    }
}

/// A ranking whose chunks are the documents themselves, as the one-chunk-per-
/// document derivation names them, scored in descending order.
pub fn documents(ids: &[&str]) -> Vec<TraceChunk> {
    ids.iter()
        .enumerate()
        .map(|(rank, id)| chunk(id, id, 1.0 / (rank as f64 + 1.0)))
        .collect()
}

/// A node that produced `output` after `nanos`, fed by `inputs`.
pub fn node(id: &str, inputs: Vec<TraceSummary>, output: TraceSummary, nanos: u64) -> TraceNode {
    TraceNode {
        node: NodeId::new(id),
        inputs,
        output: Some(output),
        duration_nanos: nanos,
        error: None,
    }
}

/// A node that failed.
pub fn failed(id: &str, inputs: Vec<TraceSummary>, error: &str, nanos: u64) -> TraceNode {
    TraceNode {
        node: NodeId::new(id),
        inputs,
        output: None,
        duration_nanos: nanos,
        error: Some(error.to_owned()),
    }
}

pub fn query(id: &str) -> TraceSummary {
    TraceSummary::Query {
        id: QueryId::new(id),
    }
}

pub fn ranked(chunks: Vec<TraceChunk>) -> TraceSummary {
    TraceSummary::RankedChunks { chunks }
}

pub fn counted(count: u64) -> TraceSummary {
    TraceSummary::Chunks { count }
}

/// The trace of one query through [`RERANKED`]: `leg` produced `retrieved`,
/// `reranked` produced `reranked`.
pub fn reranked_trace(
    query_id: &str,
    retrieved: Vec<TraceChunk>,
    reranked: Vec<TraceChunk>,
) -> Trace {
    let count = retrieved.len() as u64;
    Trace {
        nodes: vec![
            node("leg", vec![query(query_id)], ranked(retrieved), 1_000),
            node(
                "reranked",
                vec![query(query_id), counted(count)],
                ranked(reranked),
                2_000,
            ),
        ],
    }
}

/// The trace of one query through [`GENERATION`]: [`reranked_trace`]'s two
/// rankings, a context holding the first `kept` chunks of the reranked one,
/// and `answer`.
pub fn generation_trace(
    query_id: &str,
    retrieved: Vec<TraceChunk>,
    reranked: Vec<TraceChunk>,
    kept: usize,
    answer: &str,
) -> Trace {
    let mut trace = reranked_trace(query_id, retrieved, reranked.clone());
    let context: Vec<TraceChunk> = reranked.iter().take(kept).cloned().collect();
    let text = context
        .iter()
        .map(|chunk| chunk.chunk.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let text_bytes = text.len() as u64;
    trace.nodes.push(node(
        "prompt",
        vec![query(query_id), counted(reranked.len() as u64)],
        TraceSummary::Context {
            chunks: context.clone(),
            text,
        },
        3_000,
    ));
    trace.nodes.push(node(
        "answer",
        vec![
            query(query_id),
            TraceSummary::ContextSize {
                count: context.len() as u64,
                text_bytes,
            },
        ],
        TraceSummary::Answer {
            text: answer.to_owned(),
        },
        4_000,
    ));
    trace
}

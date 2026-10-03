//! What the cache costs and saves, measured — not a test of behaviour, and
//! ignored by default. Run it by hand, in release:
//!
//! ```text
//! cargo test --release -p ragondin-api --lib cache::cost -- --ignored --nocapture
//! ```
//!
//! It files runs of a retriever and a reranker in a `FileSystemRunStore` —
//! the store `GET /runs` loads from — over a benchmark that judges one
//! document per query, and times, per run, each step `GET /runs` and
//! `GET /runs/{id}/queries` take: the load, the key ([`Key::of`]), a hit,
//! and the computation a hit saves. The figures it prints are what
//! `ARCHITECTURE.md` § *The cache: a choice made here* records.

use std::time::{Duration, Instant};

use ragondin_benchmarks::{Benchmark, Qrels};
use ragondin_experiments::{
    lower_configuration, ConfigDocument, FileSystemRunStore, Metrics, Run, RunId, RunInputs,
    RunStore, Trace, TraceChunk, TraceDocument, TraceNode, TraceSummary,
};
use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, Document, Query, QueryId};

use super::*;
use crate::derived::{self, median_query_latency, Outputs};
use crate::handlers::{lower, read_traces};

const CONFIG: &str = "\
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

/// The metrics the runs record, each read per query by `/queries`.
const METRICS: [&str; 3] = ["mrr", "ndcg@10", "recall@100"];

fn document(query: usize, rank: usize) -> String {
    format!("doc-{query}-{rank}")
}

fn ranking(query: usize, k: usize) -> Vec<TraceChunk> {
    (0..k)
        .map(|rank| TraceChunk {
            chunk: ChunkId::new(format!("{}#0", document(query, rank))),
            document: DocId::new(document(query, rank)),
            score: 1.0 / (rank as f64 + 1.0),
        })
        .collect()
}

/// `queries` queries, each judging the third document of its own ranking.
fn benchmark(queries: usize) -> Benchmark {
    let mut qrels = Qrels::new();
    for q in 0..queries {
        qrels.insert(QueryId::new(format!("q{q}")), DocId::new(document(q, 2)), 1);
    }
    Benchmark::new(
        vec![Document {
            id: DocId::new(document(0, 2)),
            text: "a document".to_owned(),
            metadata: Default::default(),
        }],
        (0..queries)
            .map(|q| Query {
                id: QueryId::new(format!("q{q}")),
                text: format!("question {q}"),
            })
            .collect(),
        qrels,
    )
}

/// A run whose retriever ranked `k` chunks per query and whose reranker kept
/// a tenth of them.
pub(super) fn run(byte: u8, queries: usize, k: usize) -> Run {
    let config = ConfigDocument::new(CONFIG);
    let pipeline = lower_configuration(&config).unwrap();
    let traces = (0..queries)
        .map(|q| {
            let id = QueryId::new(format!("q{q}"));
            let trace = Trace {
                nodes: vec![
                    TraceNode {
                        node: NodeId::new("leg"),
                        inputs: vec![TraceSummary::Query { id: id.clone() }],
                        output: Some(TraceSummary::RankedChunks {
                            chunks: ranking(q, k),
                        }),
                        duration_nanos: 1_000 + q as u64,
                        error: None,
                    },
                    TraceNode {
                        node: NodeId::new("reranked"),
                        inputs: vec![
                            TraceSummary::Query { id: id.clone() },
                            TraceSummary::Chunks { count: k as u64 },
                        ],
                        output: Some(TraceSummary::RankedChunks {
                            chunks: ranking(q, k / 10),
                        }),
                        duration_nanos: 2_000,
                        error: None,
                    },
                ],
            };
            (id, TraceDocument::from(trace))
        })
        .collect();
    Run {
        id: RunId::from_digest([byte; 32]),
        inputs: RunInputs {
            pipeline: pipeline.content_hash(),
            dataset_version: "0".repeat(64),
            index_version: "1".repeat(64),
            model_hashes: BTreeMap::new(),
            engine_version: "0.0.0".to_owned(),
        },
        metrics: METRICS
            .into_iter()
            .map(|name| (name, 0.5))
            .collect::<Metrics>(),
        config,
        traces,
        bindings: Vec::new(),
        times: None,
        provenance: None,
    }
}

/// `total`, over `runs` runs, as milliseconds per run.
fn per_run(total: Duration, runs: usize) -> String {
    format!("{:>8.3} ms", total.as_secs_f64() * 1e3 / runs as f64)
}

/// How many times each step is timed: the fastest is kept, so a machine
/// busy with something else for one of them does not inflate the figure.
const REPEATS: usize = 7;

/// What `step` returns, and the fastest of [`REPEATS`] timings of it.
fn timed<T>(mut step: impl FnMut() -> T) -> (T, Duration) {
    let mut fastest = Duration::MAX;
    let mut value = None;
    for _ in 0..REPEATS {
        let started = Instant::now();
        let got = step();
        fastest = fastest.min(started.elapsed());
        value = Some(got);
    }
    (value.expect("REPEATS is not zero"), fastest)
}

#[test]
#[ignore = "a measurement, run by hand in release"]
fn what_the_cache_costs_and_saves_per_run() {
    const RUNS: usize = 20;
    for (queries, k) in [(300, 100), (1000, 100)] {
        let workspace = std::env::temp_dir()
            .join(format!("ragondin-api-cache-cost-{}", std::process::id()))
            .join(format!("{queries}-{k}"));
        let _ = std::fs::remove_dir_all(&workspace);
        let store = FileSystemRunStore::new(workspace.join("runs"));
        for byte in 0..RUNS {
            store.save(&run(byte as u8, queries, k)).unwrap();
        }
        let benchmark = benchmark(queries);

        // Both endpoints: the store's load of every run, traces included.
        let (loaded, load) = timed(|| {
            let ids = store.ids().unwrap();
            ids.iter()
                .map(|id| store.load(id).unwrap())
                .collect::<Vec<Run>>()
        });
        let (keys, key) = timed(|| {
            loaded
                .iter()
                .map(|run| Key::of("build", run))
                .collect::<Vec<Key>>()
        });

        // `GET /runs`: the median latency, computed, then read back.
        let (latencies, latency_compute) = timed(|| {
            loaded
                .iter()
                .map(median_query_latency)
                .collect::<Vec<Option<u64>>>()
        });
        for (key, latency) in keys.iter().zip(&latencies) {
            write_latency(&workspace, key, *latency).unwrap();
        }
        let ((), latency_hit) = timed(|| {
            for (key, latency) in keys.iter().zip(&latencies) {
                assert_eq!(read_latency(&workspace, key).unwrap(), Some(*latency));
            }
        });

        // `GET /runs/{id}/queries`: the run's traces typed (done hit or
        // miss), then its per-query and per-node figures, computed, then read
        // back.
        let (typed, typing) = timed(|| {
            loaded
                .iter()
                .map(|run| (lower(run).unwrap(), read_traces(run).unwrap()))
                .collect::<Vec<_>>()
        });
        let metrics = derived::Metrics::of(METRICS);
        let (figures, figures_compute) = timed(|| {
            typed
                .iter()
                .map(|(pipeline, traces)| {
                    let outputs = Outputs::of(pipeline);
                    Figures {
                        queries: traces
                            .iter()
                            .map(|(query, trace)| {
                                let scores = derived::query_scores(
                                    &metrics, &outputs, &benchmark, query, trace,
                                );
                                (query.as_str().to_owned(), scores)
                            })
                            .collect(),
                        nodes: derived::node_figures(&metrics, pipeline, traces, Some(&benchmark)),
                    }
                })
                .collect::<Vec<Figures>>()
        });
        for (key, figures) in keys.iter().zip(&figures) {
            write(&workspace, key, figures).unwrap();
        }
        let ((), figures_hit) = timed(|| {
            for (key, figures) in keys.iter().zip(&figures) {
                assert_eq!(read(&workspace, key).unwrap().as_ref(), Some(figures));
            }
        });

        let bytes: u64 = loaded
            .iter()
            .map(|run| {
                let path = workspace
                    .join("runs")
                    .join(run.id.to_string())
                    .join("traces.json");
                std::fs::metadata(path).unwrap().len()
            })
            .sum();
        println!(
            "{RUNS} runs of {queries} queries, k = {k}: {:.1} MB of traces.json per run; per run:",
            bytes as f64 / RUNS as f64 / 1e6
        );
        println!("  load                       {}", per_run(load, RUNS));
        println!("  key (Key::of)              {}", per_run(key, RUNS));
        println!("  GET /runs, latency");
        println!(
            "    hit                      {}",
            per_run(latency_hit, RUNS)
        );
        println!(
            "    computed                 {}",
            per_run(latency_compute, RUNS)
        );
        println!(
            "    with the cache (load + key + hit)  {}",
            per_run(load + key + latency_hit, RUNS)
        );
        println!(
            "    without it (load + computed)       {}",
            per_run(load + latency_compute, RUNS)
        );
        println!("  GET /runs/{{id}}/queries, figures");
        println!("    traces typed             {}", per_run(typing, RUNS));
        println!(
            "    hit                      {}",
            per_run(figures_hit, RUNS)
        );
        println!(
            "    computed                 {}",
            per_run(figures_compute, RUNS)
        );
        println!(
            "    with the cache (load + typed + key + hit)  {}",
            per_run(load + typing + key + figures_hit, RUNS)
        );
        println!(
            "    without it (load + typed + computed)       {}",
            per_run(load + typing + figures_compute, RUNS)
        );
        let _ = std::fs::remove_dir_all(&workspace);
    }
}

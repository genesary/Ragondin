//! What the cache costs and saves, measured — not a test of behaviour, and
//! ignored by default. Run it by hand, in release:
//!
//! ```text
//! cargo test --release -p ragondin-api --lib cache::cost -- --ignored --nocapture
//! ```
//!
//! It files runs in a `FileSystemRunStore` — the store `GET /runs` loads
//! from — and times, per run, each step `GET /runs` and
//! `GET /runs/{id}/queries` take: the load, the key ([`Key::of`]), a hit,
//! and the computation a hit saves. It does so for each [`Shape`]: a
//! retrieval-only run (a retriever and a reranker, three ranking metrics)
//! and a generation run (the same, then a context builder and a generator;
//! the ranking metrics and both answer metrics) whose context holds 3 kB of
//! passage text or 10 kB, each over a benchmark that
//! judges one document per query and over one that grades a hundred. The
//! figures it prints are what `ARCHITECTURE.md` § *The cache: a choice made
//! here* records.

use std::time::{Duration, Instant};

use ragondin_benchmarks::{Benchmark, Qrels, ReferenceAnswers};
use ragondin_experiments::{
    lower_configuration, ConfigDocument, FileSystemRunStore, Metrics, Run, RunId, RunInputs,
    RunStore, Trace, TraceChunk, TraceDocument, TraceNode, TraceSummary,
};
use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, Document, Query, QueryId};

use super::*;
use crate::derived::{self, median_query_latency, Outputs};
use crate::handlers::{lower, read_traces};

const RETRIEVAL: &str = "\
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

const GENERATION: &str = "\
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

/// The ranking metrics every run records, each read per query by `/queries`.
const RANKING: [&str; 3] = ["mrr", "ndcg@10", "recall@100"];

/// What a generation run records besides: both answer metrics.
const ANSWERS: [&str; 2] = ["exact_match", "token_f1"];

/// How many chunks of the reranked ranking a generation run's context keeps.
const KEPT: usize = 5;

/// One kind of run the measurement files, and the benchmark it is scored
/// against.
#[derive(Clone, Copy, Debug)]
struct Shape {
    name: &'static str,
    /// Whether the run ends in a context builder and a generator.
    generation: bool,
    /// The bytes of passage text each of the [`KEPT`] chunks puts in a
    /// generation run's context. The context's text is in the trace, so the
    /// key reads it; no figure does. The context builder's budget sets it,
    /// so it is measured at two sizes: about a hundred words — a BEIR
    /// passage — and about three hundred.
    passage_bytes: usize,
    /// How many documents the qrels grade per query.
    judged: usize,
}

const SHAPES: [Shape; 6] = [
    Shape {
        name: "retrieval-only, 1 judged per query",
        generation: false,
        passage_bytes: 0,
        judged: 1,
    },
    Shape {
        name: "retrieval-only, 100 graded per query",
        generation: false,
        passage_bytes: 0,
        judged: 100,
    },
    Shape {
        name: "generation, 3 kB context, 1 judged per query",
        generation: true,
        passage_bytes: 600,
        judged: 1,
    },
    Shape {
        name: "generation, 3 kB context, 100 graded per query",
        generation: true,
        passage_bytes: 600,
        judged: 100,
    },
    Shape {
        name: "generation, 10 kB context, 1 judged per query",
        generation: true,
        passage_bytes: 2000,
        judged: 1,
    },
    Shape {
        name: "generation, 10 kB context, 100 graded per query",
        generation: true,
        passage_bytes: 2000,
        judged: 100,
    },
];

impl Shape {
    fn metrics(self) -> Vec<&'static str> {
        let mut names = RANKING.to_vec();
        if self.generation {
            names.extend(ANSWERS);
        }
        names
    }
}

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

/// `query`'s answer, or its reference `r`: thirty words, a third of a
/// reference's shared with no answer, so that neither answer metric is
/// trivially one.
fn answer(query: usize, reference: Option<usize>) -> String {
    (0..30)
        .map(|word| match reference {
            Some(r) if word % 3 == 0 => format!("ref{r}word{word}"),
            _ => format!("q{query}word{word}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `queries` queries, judged as `shape` says: the third document of the
/// query's own ranking at the top grade, and with more judgments, others at
/// lower grades: every other one `document(q, 2 + j)`, an index from 4 to
/// 100 of the query's own ranking — the run ranks indices `0..k`, so with
/// `k = 100` the last, index 100, is not ranked — and the rest documents no
/// run ranks. A generation shape gives each query three
/// reference answers.
fn benchmark(shape: Shape, queries: usize) -> Benchmark {
    let mut qrels = Qrels::new();
    let mut references = ReferenceAnswers::new();
    for q in 0..queries {
        let id = QueryId::new(format!("q{q}"));
        qrels.insert(id.clone(), DocId::new(document(q, 2)), 3);
        for j in 1..shape.judged {
            let doc = if j % 2 == 0 {
                document(q, 2 + j)
            } else {
                format!("unranked-{q}-{j}")
            };
            qrels.insert(id.clone(), DocId::new(doc), 1 + (j % 2) as u8);
        }
        if shape.generation {
            references.insert(id, (0..3).map(|r| answer(q, Some(r))).collect());
        }
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
    .with_reference_answers(references)
}

/// One query's trace through `shape`'s pipeline: the retriever ranked `k`
/// chunks, the reranker kept a tenth of them, and a generation run's context
/// holds the first [`KEPT`] of those with their passage text.
fn trace(shape: Shape, q: usize, k: usize) -> Trace {
    let id = QueryId::new(format!("q{q}"));
    let mut nodes = vec![
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
    ];
    if shape.generation {
        let context: Vec<TraceChunk> = ranking(q, k / 10).into_iter().take(KEPT).collect();
        let text = context
            .iter()
            .map(|chunk| {
                let mut passage = format!("{}: ", chunk.chunk.as_str());
                while passage.len() < shape.passage_bytes {
                    passage.push_str("a passage word ");
                }
                passage
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text_bytes = text.len() as u64;
        nodes.push(TraceNode {
            node: NodeId::new("prompt"),
            inputs: vec![
                TraceSummary::Query { id: id.clone() },
                TraceSummary::Chunks {
                    count: (k / 10) as u64,
                },
            ],
            output: Some(TraceSummary::Context {
                chunks: context.clone(),
                text,
            }),
            duration_nanos: 3_000,
            error: None,
        });
        nodes.push(TraceNode {
            node: NodeId::new("answer"),
            inputs: vec![
                TraceSummary::Query { id },
                TraceSummary::ContextSize {
                    count: context.len() as u64,
                    text_bytes,
                },
            ],
            output: Some(TraceSummary::Answer {
                text: answer(q, None),
            }),
            duration_nanos: 400_000,
            error: None,
        });
    }
    Trace { nodes }
}

/// A retrieval-only run of `queries` queries whose retriever ranked `k`
/// chunks per query and whose reranker kept a tenth of them.
pub(super) fn run(byte: u8, queries: usize, k: usize) -> Run {
    run_of(SHAPES[0], byte, queries, k)
}

/// A run of `shape`'s pipeline over `queries` queries, each traced by
/// [`trace`].
fn run_of(shape: Shape, byte: u8, queries: usize, k: usize) -> Run {
    let config = ConfigDocument::new(if shape.generation {
        GENERATION
    } else {
        RETRIEVAL
    });
    let pipeline = lower_configuration(&config).unwrap();
    let traces = (0..queries)
        .map(|q| {
            (
                QueryId::new(format!("q{q}")),
                TraceDocument::from(trace(shape, q, k)),
            )
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
        metrics: shape
            .metrics()
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
    for shape in SHAPES {
        for (queries, k) in [(300, 100), (1000, 100)] {
            measure(shape, RUNS, queries, k);
        }
    }
    let _ = std::fs::remove_dir_all(scratch());
}

/// The directory every measurement of this process files its runs under.
fn scratch() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("ragondin-api-cache-cost-{}", std::process::id()))
}

/// Files `runs` runs of `shape` and prints what each step costs per run.
fn measure(shape: Shape, runs: usize, queries: usize, k: usize) {
    let workspace = scratch().join(format!(
        "{}-{}-{}-{queries}-{k}",
        shape.generation, shape.passage_bytes, shape.judged
    ));
    let _ = std::fs::remove_dir_all(&workspace);
    let store = FileSystemRunStore::new(workspace.join("runs"));
    for byte in 0..runs {
        store.save(&run_of(shape, byte as u8, queries, k)).unwrap();
    }
    let benchmark = benchmark(shape, queries);

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

    // `GET /runs/{id}/queries`: the run's traces typed (done hit or miss),
    // then its per-query and per-node figures, computed, then read back.
    let (typed, typing) = timed(|| {
        loaded
            .iter()
            .map(|run| (lower(run).unwrap(), read_traces(run).unwrap()))
            .collect::<Vec<_>>()
    });
    let metrics = derived::Metrics::of(shape.metrics());
    let (figures, figures_compute) = timed(|| {
        typed
            .iter()
            .map(|(pipeline, traces)| {
                let outputs = Outputs::of(pipeline);
                Figures {
                    queries: traces
                        .iter()
                        .map(|(query, trace)| {
                            let scores =
                                derived::query_scores(&metrics, &outputs, &benchmark, query, trace);
                            (query.as_str().to_owned(), scores)
                        })
                        .collect(),
                    nodes: derived::node_figures(&metrics, pipeline, traces, Some(&benchmark)),
                }
            })
            .collect::<Vec<Figures>>()
    });
    // The figures read what the shape says they read: every metric of every
    // query, the answer metrics included for a generation run.
    for figures in &figures {
        for scores in figures.queries.values() {
            assert_eq!(scores.len(), shape.metrics().len(), "{}", shape.name);
        }
    }
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
        "{}: {runs} runs of {queries} queries, k = {k}: {:.1} MB of traces.json per run; per run:",
        shape.name,
        bytes as f64 / runs as f64 / 1e6
    );
    println!("  load                       {}", per_run(load, runs));
    println!("  key (Key::of)              {}", per_run(key, runs));
    println!("  GET /runs, latency");
    println!(
        "    hit                      {}",
        per_run(latency_hit, runs)
    );
    println!(
        "    computed                 {}",
        per_run(latency_compute, runs)
    );
    println!(
        "    with the cache (load + key + hit)  {}",
        per_run(load + key + latency_hit, runs)
    );
    println!(
        "    without it (load + computed)       {}",
        per_run(load + latency_compute, runs)
    );
    println!("  GET /runs/{{id}}/queries, figures");
    println!("    traces typed             {}", per_run(typing, runs));
    println!(
        "    hit                      {}",
        per_run(figures_hit, runs)
    );
    println!(
        "    computed                 {}",
        per_run(figures_compute, runs)
    );
    println!(
        "    with the cache (load + typed + key + hit)  {}",
        per_run(load + typing + key + figures_hit, runs)
    );
    println!(
        "    without it (load + typed + computed)       {}",
        per_run(load + typing + figures_compute, runs)
    );
    let _ = std::fs::remove_dir_all(&workspace);
}

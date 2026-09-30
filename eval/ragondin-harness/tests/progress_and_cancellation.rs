//! Progress per query and cancellation between queries, over the miniature
//! BEIR fixture.
//!
//! `evaluate_observed` is the entry point a long-running caller drives: it
//! hands each query's rendered trace to an observer the moment the query
//! finishes, and reads a cancellation signal at the boundary between two
//! queries — never inside one. `evaluate` is the same loop with an observer
//! that does nothing and a signal nobody sets.
//!
//! The engine's own call count is read through a retriever that wraps
//! `ragondin-stub`'s and counts its calls, registered exactly as a composition
//! root would register any retriever (INV-7). The fixture pipeline has two
//! retrieval legs, so one executed query is two calls.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use ragondin_benchmarks::{BeirAdapter, Benchmark, BenchmarkAdapter};
use ragondin_config::{ConfigSource, LocalFile};
use ragondin_contracts::{ComponentError, RetrieveParams, Retriever};
use ragondin_engine::EngineContext;
use ragondin_experiments::{ConfigDocument, Run, TraceDocument};
use ragondin_harness::{
    evaluate, evaluate_observed, CorpusIndex, Evaluation, HarnessError, QueryProgress,
};
use ragondin_pipeline::{LogicalPipeline, ParamValue};
use ragondin_stub::{StubFusion, StubRetriever};
use ragondin_types::{Query, QueryId, ScoredChunk};

/// Retrieval legs per query in `stub-over-beir-mini.yaml`.
const LEGS: usize = 2;

fn beir_mini() -> Benchmark {
    BeirAdapter::new(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../ragondin-benchmarks/tests/fixtures/beir-mini"),
    )
    .load()
    .expect("the checked-in BEIR fixture loads")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

async fn pipeline(path: &Path) -> (LogicalPipeline, ConfigDocument) {
    let pipeline = LocalFile::new(path)
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration");
    let text = std::fs::read_to_string(path).expect("the fixture is readable");
    (pipeline, ConfigDocument::new(text))
}

/// A stub retriever that counts its calls and, optionally, sets a signal when
/// a given call begins — which is a signal set while a query is in flight.
struct Counting {
    inner: StubRetriever,
    calls: Arc<AtomicUsize>,
    set_on_call: Option<(usize, Arc<AtomicBool>)>,
}

#[async_trait]
impl Retriever for Counting {
    async fn retrieve(
        &self,
        query: &Query,
        params: &RetrieveParams,
    ) -> Result<Vec<ScoredChunk>, ComponentError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some((at, signal)) = &self.set_on_call {
            if call == *at {
                signal.store(true, Ordering::SeqCst);
            }
        }
        self.inner.retrieve(query, params).await
    }
}

/// The composition root of these tests: the counting retriever under the name
/// the fixtures ask for, sharing one counter across every leg it constructs.
fn counting_context(
    calls: &Arc<AtomicUsize>,
    set_on_call: Option<(usize, Arc<AtomicBool>)>,
) -> EngineContext {
    let calls = Arc::clone(calls);
    let mut ctx = EngineContext::new();
    ctx.register_retriever(
        "stub_retriever",
        Box::new(move |config| {
            let Some(ParamValue::String(label)) = config.get("label") else {
                return Err("`label` must be a string".into());
            };
            Ok(Box::new(Counting {
                inner: StubRetriever::new(label.clone()),
                calls: Arc::clone(&calls),
                set_on_call: set_on_call.clone(),
            }))
        }),
    );
    ctx.register_fusion("stub_interleave", Box::new(|_| Ok(Box::new(StubFusion))));
    ctx
}

/// One observer call, owned.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    query: QueryId,
    position: usize,
    total: usize,
    trace: TraceDocument,
}

impl Seen {
    fn from(progress: &QueryProgress<'_>) -> Self {
        Self {
            query: progress.query.clone(),
            position: progress.position,
            total: progress.total,
            trace: progress.trace.clone(),
        }
    }
}

fn evaluation<'a>(
    pipeline: &'a LogicalPipeline,
    config: &'a ConfigDocument,
    benchmark: &'a Benchmark,
    index: &'a CorpusIndex,
) -> Evaluation<'a> {
    Evaluation {
        pipeline,
        config,
        benchmark,
        index,
        cutoff: 10,
        model_hashes: Default::default(),
    }
}

#[tokio::test]
async fn observer_sees_every_query_in_order_with_the_trace_the_run_files() {
    let (pipeline, config) = pipeline(&fixture("stub-over-beir-mini.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut seen = Vec::new();

    let run = evaluate_observed(
        &evaluation(&pipeline, &config, &benchmark, &index),
        &counting_context(&calls, None),
        |progress: QueryProgress<'_>| seen.push(Seen::from(&progress)),
        &AtomicBool::new(false),
    )
    .await
    .expect("the stub pipeline cannot fail on this benchmark");

    assert_eq!(
        seen.iter().map(|s| s.query.clone()).collect::<Vec<_>>(),
        [
            QueryId::new("q-1"),
            QueryId::new("q-2"),
            QueryId::new("0042")
        ],
        "benchmark order — the order the queries were executed in, not the \
         sorted order the run files traces in"
    );
    assert_eq!(
        seen.iter()
            .map(|s| (s.position, s.total))
            .collect::<Vec<_>>(),
        [(1, 3), (2, 3), (3, 3)]
    );
    for s in &seen {
        assert_eq!(
            s.trace, run.traces[&s.query],
            "the observer receives the very document the run files"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3 * LEGS);
}

#[tokio::test]
async fn a_signal_set_between_queries_stops_the_run_before_the_next_one() {
    let (pipeline, config) = pipeline(&fixture("stub-over-beir-mini.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();

    let stopped = evaluate_observed(
        &evaluation(&pipeline, &config, &benchmark, &index),
        &counting_context(&calls, None),
        |progress: QueryProgress<'_>| {
            seen.push(Seen::from(&progress));
            // Set once the second query's observer call has happened: the
            // next thing the loop does is read the signal.
            if progress.position == 2 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
        &cancel,
    )
    .await
    .expect_err("the signal stops the run");

    assert!(
        matches!(stopped, HarnessError::Cancelled { completed: 2 }),
        "{stopped}"
    );
    assert_eq!(
        seen.len(),
        2,
        "exactly the two queries that ran were observed"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2 * LEGS,
        "the engine executed no third query"
    );
}

#[tokio::test]
async fn a_signal_set_before_the_first_query_runs_nothing() {
    let (pipeline, config) = pipeline(&fixture("stub-over-beir-mini.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut observed = 0;

    let stopped = evaluate_observed(
        &evaluation(&pipeline, &config, &benchmark, &index),
        &counting_context(&calls, None),
        |_: QueryProgress<'_>| observed += 1,
        &AtomicBool::new(true),
    )
    .await
    .expect_err("the signal is read before the first query");

    assert!(
        matches!(stopped, HarnessError::Cancelled { completed: 0 }),
        "{stopped}"
    );
    assert_eq!(observed, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_signal_set_during_a_query_is_honoured_only_at_the_boundary() {
    let (pipeline, config) = pipeline(&fixture("stub-over-beir-mini.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut seen = Vec::new();
    let mut set_when_observed = Vec::new();

    // The signal is set by the second query's *first* retrieval leg, while
    // that query is in flight and its second leg has yet to run.
    let stopped = evaluate_observed(
        &evaluation(&pipeline, &config, &benchmark, &index),
        &counting_context(&calls, Some((LEGS + 1, Arc::clone(&cancel)))),
        |progress: QueryProgress<'_>| {
            seen.push(Seen::from(&progress));
            set_when_observed.push(cancel.load(Ordering::SeqCst));
        },
        &cancel,
    )
    .await
    .expect_err("the signal stops the run at the next boundary");

    assert!(
        matches!(stopped, HarnessError::Cancelled { completed: 2 }),
        "the query in flight counts as run: {stopped}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2 * LEGS,
        "the query in flight ran to its end — its second leg included — and \
         no third query began"
    );
    assert_eq!(
        seen.iter().map(|s| s.position).collect::<Vec<_>>(),
        [1, 2],
        "the query in flight was observed"
    );
    assert_eq!(
        set_when_observed,
        [false, true],
        "its observer ran with the signal already set, so the signal was not \
         read inside the query"
    );
}

#[tokio::test]
async fn a_failing_query_reaches_the_observer_before_the_error_returns() {
    let (pipeline, config) = pipeline(&fixture("refusing-pipeline.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut seen = Vec::new();

    let failed = evaluate_observed(
        &evaluation(&pipeline, &config, &benchmark, &index),
        &counting_context(&calls, None),
        |progress: QueryProgress<'_>| seen.push(Seen::from(&progress)),
        &AtomicBool::new(false),
    )
    .await
    .expect_err("`top_k: 0` asks the stub for a result that cannot exist");

    let HarnessError::Execute { query, trace, .. } = failed else {
        panic!("the failure is the query's: {failed}")
    };
    assert_eq!(seen.len(), 1, "the failing query is observed once");
    assert_eq!(seen[0].query, query);
    assert_eq!((seen[0].position, seen[0].total), (1, 3));
    assert_eq!(
        seen[0].trace, trace,
        "the observer and the error carry the same trace"
    );
}

#[tokio::test]
async fn evaluate_without_an_observer_behaves_as_before() {
    let (pipeline, config) = pipeline(&fixture("stub-over-beir-mini.yaml")).await;
    let benchmark = beir_mini();
    let index = CorpusIndex::build(benchmark.corpus());
    let calls = Arc::new(AtomicUsize::new(0));
    let evaluation = evaluation(&pipeline, &config, &benchmark, &index);

    let before: Run = evaluate(&evaluation, &counting_context(&calls, None))
        .await
        .expect("the stub pipeline cannot fail on this benchmark");
    let through_the_new_entry_point = evaluate_observed(
        &evaluation,
        &counting_context(&calls, None),
        |_: QueryProgress<'_>| {},
        &AtomicBool::new(false),
    )
    .await
    .expect("the stub pipeline cannot fail on this benchmark");

    // Every field of the record agrees; the traces agree once each node's
    // wall-clock duration is set aside, since two executions never take the
    // same nanoseconds.
    assert_eq!(before.id, through_the_new_entry_point.id);
    assert_eq!(before.inputs, through_the_new_entry_point.inputs);
    assert_eq!(before.config, through_the_new_entry_point.config);
    assert_eq!(before.bindings, through_the_new_entry_point.bindings);
    assert_eq!(
        before.metrics.iter().collect::<Vec<_>>(),
        through_the_new_entry_point
            .metrics
            .iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        without_durations(&before),
        without_durations(&through_the_new_entry_point)
    );
}

/// A run's traces by query, each node's `duration_nanos` removed.
fn without_durations(run: &Run) -> Vec<(QueryId, serde_json::Value)> {
    run.traces
        .iter()
        .map(|(query, trace)| {
            let mut value = trace.as_value().clone();
            for node in value["nodes"]
                .as_array_mut()
                .expect("a trace lists its nodes")
            {
                node.as_object_mut()
                    .expect("a node is an object")
                    .remove("duration_nanos")
                    .expect("every node carries its duration");
            }
            (query.clone(), value)
        })
        .collect()
}

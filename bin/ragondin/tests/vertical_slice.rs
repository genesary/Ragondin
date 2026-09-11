//! The first end-to-end vertical slice, run from the composition root.
//!
//! `ragondin-engine`'s own tests already execute a pipeline, but they hand-build
//! a `PhysicalPipeline` from components declared inline, because INV-5 forbids
//! the engine a dependency on any crate under `components/`. Three things only
//! this crate can exercise, and this file is where they first run together:
//!
//! - **the front half** — a YAML file *on disk*, through `ragondin-config`, into
//!   a `LogicalPipeline`;
//! - **the registration path** — real component crates registered on an
//!   `EngineContext` through the ordinary `register_*` API. Every call below is
//!   one a crate outside this workspace could write verbatim, which is INV-7
//!   exercised rather than asserted;
//! - **the composition root** — `docs/code-architecture.md` §4.3 says only the
//!   binary knows both the engine and the concrete components, and this test
//!   lives here because of that rule.
//!
//! The trace is read from the executor's **return value** (INV-10); nothing here
//! consults a log.

use std::path::PathBuf;

use ragondin_config::{ConfigSource, LocalFile};
use ragondin_engine::{plan_physical, Engine, EngineContext, ExecutionTrace, Output, ValueSummary};
use ragondin_pipeline::{LogicalPipeline, ParamValue};
use ragondin_stub::{StubFusion, StubRetriever};
use ragondin_types::{Query, QueryId};

/// The checked-in fixture, read from disk rather than built in code: a
/// hand-built pipeline would skip the half of the path this test exists for.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stub-pipeline.yaml")
}

/// The composition root's whole job: construct an empty context and register
/// what this binary was built with.
///
/// Both registrations go through the public `register_*` API, with the
/// constructor reading the node's configuration out of `Params` — the bridge
/// from the untyped configuration map to a typed component
/// (`docs/code-architecture.md` §6.3). `ragondin-stub` is a leaf that knows
/// nothing of `ragondin-pipeline`, so that bridge is written here, exactly as
/// it would be for a third-party crate.
fn register_stubs(ctx: &mut EngineContext) {
    ctx.register_retriever(
        "stub_retriever",
        Box::new(|config| {
            let label = match config.get("label") {
                Some(ParamValue::String(label)) => label.clone(),
                Some(other) => {
                    return Err(format!("`label` must be a string, found {other:?}").into())
                }
                None => return Err("`label` is required".into()),
            };
            Ok(Box::new(StubRetriever::new(label)))
        }),
    );
    ctx.register_fusion("stub_interleave", Box::new(|_| Ok(Box::new(StubFusion))));
}

/// Loads the fixture, plans it and runs it, returning everything the
/// assertions below read.
async fn run_the_slice() -> (LogicalPipeline, Output, ExecutionTrace) {
    let logical = LocalFile::new(fixture())
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration");

    let mut ctx = EngineContext::new();
    register_stubs(&mut ctx);

    let plan = plan_physical(&logical, &ctx).expect("every `impl:` name is registered above");

    let query = Query {
        id: QueryId::new("q-1"),
        text: "what does the vertical slice do".to_string(),
    };
    let (output, trace) = Engine::new().execute(&plan, query).await;

    (
        logical,
        output.expect("the stubs cannot fail on this pipeline"),
        trace,
    )
}

fn ids(hits: &Output) -> Vec<&str> {
    hits.iter().map(|hit| hit.chunk.id.as_str()).collect()
}

#[tokio::test]
async fn the_fixture_on_disk_becomes_a_logical_pipeline() {
    let logical = LocalFile::new(fixture())
        .load()
        .await
        .expect("the checked-in fixture is a valid configuration");

    assert_eq!(
        logical
            .inputs()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["question"],
        "the graph's signature (ADR-C18) survives the load"
    );
    assert_eq!(
        logical
            .nodes()
            .iter()
            .map(|node| node.id().as_str())
            .collect::<Vec<_>>(),
        ["combined", "left", "right"],
        "validation sorts the node list by id, whatever order the file listed them in"
    );
}

#[tokio::test]
async fn the_slice_executes_and_returns_the_interleaved_output() {
    let (_, output, _) = run_the_slice().await;

    // `left` is configured with `top_k: 3` and `right` with `top_k: 2`, and the
    // fusion interleaves them rank by rank in the order the node wires them.
    assert_eq!(
        ids(&output),
        ["left-0", "right-0", "left-1", "right-1", "left-2"]
    );
    assert!(
        output.windows(2).all(|pair| pair[0].score > pair[1].score),
        "the fused list honours the ranking contract: {:?}",
        output.iter().map(|hit| hit.score).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn the_returned_trace_holds_one_entry_per_node_in_execution_order() {
    let (logical, _, trace) = run_the_slice().await;

    assert_eq!(
        trace.nodes.len(),
        logical.nodes().len(),
        "one NodeTrace per node of the plan"
    );
    assert_eq!(
        trace
            .nodes
            .iter()
            .map(|node| node.node.as_str())
            .collect::<Vec<_>>(),
        ["left", "right", "combined"],
        "the trace is in execution order, not the canonical order the plan stores"
    );

    let [left, right, combined] = &trace.nodes[..] else {
        panic!("the plan has exactly three nodes")
    };
    assert_eq!(
        left.inputs,
        [ValueSummary::Query {
            id: QueryId::new("q-1")
        }],
        "a retrieval leg consumes the declared pipeline input"
    );
    assert_eq!(left.output, Some(ValueSummary::Chunks { count: 3 }));
    assert_eq!(right.output, Some(ValueSummary::Chunks { count: 2 }));
    assert_eq!(
        combined.inputs,
        [
            ValueSummary::Chunks { count: 3 },
            ValueSummary::Chunks { count: 2 }
        ],
        "the fusion's inputs arrive in port order, never reordered (ADR-C16)"
    );
    assert_eq!(combined.output, Some(ValueSummary::Chunks { count: 5 }));
    assert!(
        trace.nodes.iter().all(|node| node.error.is_none()),
        "nothing failed"
    );
}

#[tokio::test]
async fn two_runs_agree_on_the_output_and_on_the_logical_hash() {
    // The stubs carry no randomness, and the hash is over the canonical logical
    // form (INV-8) — so a second run of the same file is the same run.
    let (first_logical, first_output, _) = run_the_slice().await;
    let (second_logical, second_output, _) = run_the_slice().await;

    assert_eq!(ids(&first_output), ids(&second_output));
    assert_eq!(
        first_logical.content_hash(),
        second_logical.content_hash(),
        "the same file on disk content-addresses to the same pipeline"
    );
}

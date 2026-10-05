//! The pipeline endpoints over the file backend: `GET /pipelines`,
//! `GET`/`PUT /pipelines/{name}`, `GET`/`PUT /pipelines/{name}/layout` and
//! `POST /pipelines/validate`, against a real workspace on disk.

mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use ragondin_api::fs::{FsPipelines, Workspace};
use serde_json::{json, Value};
use support::{
    app_with_backends, fakes, get, json as body_json, send, write_request, FakeRunStore, UNREAD_KEY,
};

/// The pipeline the tests write: valid, and formatted the way a person
/// formats one — comments, a flow map — so that a re-serialization would show.
const HYBRID: &str = "# a hand-written pipeline\npipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params: { top_k: 10 }   # keep it small\n";

/// The same pipeline, formatted otherwise.
const HYBRID_REFORMATTED: &str = "pipeline:\n  inputs:\n    - question\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs:\n        - question\n      params:\n        top_k: 10\n";

/// A configuration whose reranker is fed (chunks, query): the kind check
/// refuses it.
const MIS_KINDED: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: legs\n      component: retriever\n      impl: stub_retriever\n      inputs: [question]\n      params: { top_k: 3 }\n    - id: ranked\n      component: reranker\n      impl: some_reranker\n      inputs: [legs, question]\n";

fn scratch(test_name: &str) -> Workspace {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("pipelines")
        .join(test_name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("the scratch directory is creatable");
    Workspace::open(&path).expect("a workspace opens")
}

fn server(workspace: &Workspace) -> ragondin_api::Server {
    let mut backends = fakes(FakeRunStore::default());
    backends.pipelines = Arc::new(FsPipelines::new(workspace));
    app_with_backends(backends)
}

fn document(text: &str) -> Value {
    json!({ "document": text })
}

/// Writes `text` as a new pipeline, through the API, and returns its etag.
async fn create(workspace: &Workspace, name: &str, text: &str) -> String {
    let response = send(
        server(workspace),
        write_request(
            "PUT",
            &format!("/api/v1/pipelines/{name}"),
            &document(text),
            &[("if-none-match", "*")],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response
        .headers()
        .get("etag")
        .expect("a write answers its etag")
        .to_str()
        .unwrap()
        .to_owned();
    etag
}

fn on_disk(workspace: &Workspace, name: &str) -> Vec<u8> {
    fs::read(workspace.pipelines().join(format!("{name}.yaml"))).expect("the file is there")
}

#[tokio::test]
async fn a_pipeline_read_carries_the_etag_of_its_bytes() {
    let workspace = scratch("etag");
    fs::write(workspace.pipelines().join("hybrid.yaml"), HYBRID).unwrap();

    let response = send(server(&workspace), get("/api/v1/pipelines/hybrid")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let header = response
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = body_json(response).await;
    assert_eq!(body["document"], HYBRID);
    assert_eq!(body["name"], "hybrid");
    // The etag is a digest of the bytes: quoted in the header, bare in the body.
    assert_eq!(header, format!("\"{}\"", body["etag"].as_str().unwrap()));
    assert_eq!(body["etag"].as_str().unwrap().len(), 64);
    assert!(body["hash"].is_string(), "{body}");
    assert_eq!(body["error"], Value::Null);

    // Other bytes, another etag, though the pipeline is the same.
    fs::write(
        workspace.pipelines().join("hybrid.yaml"),
        HYBRID_REFORMATTED,
    )
    .unwrap();
    let again = body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid")).await).await;
    assert_ne!(again["etag"], body["etag"]);
    assert_eq!(again["hash"], body["hash"]);
}

#[tokio::test]
async fn an_absent_pipeline_is_pipeline_not_found() {
    let workspace = scratch("absent");

    for name in ["absent", "..%2Fescape"] {
        let response = send(
            server(&workspace),
            get(&format!("/api/v1/pipelines/{name}")),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name}");
        assert_eq!(body_json(response).await["code"], "pipeline_not_found");
    }
}

#[tokio::test]
async fn the_listing_names_each_pipeline_with_its_hash_or_its_error() {
    let workspace = scratch("listing");
    fs::write(workspace.pipelines().join("hybrid.yaml"), HYBRID).unwrap();
    fs::write(workspace.pipelines().join("broken.yaml"), MIS_KINDED).unwrap();
    fs::write(workspace.pipelines().join("hybrid.layout.json"), "{}").unwrap();
    fs::write(workspace.pipelines().join(".hybrid.write-1-1.yaml"), HYBRID).unwrap();

    let body = body_json(send(server(&workspace), get("/api/v1/pipelines")).await).await;

    let pipelines = body["pipelines"].as_array().expect("a list");
    let names: Vec<&str> = pipelines
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["broken", "hybrid"]);
    assert_eq!(pipelines[0]["hash"], Value::Null);
    assert_eq!(pipelines[0]["error"]["location"]["node"], "ranked");
    assert!(pipelines[1]["hash"].is_string());
    assert_eq!(pipelines[1]["error"], Value::Null);
    assert!(pipelines[1]["modified_ms"].as_u64().unwrap() > 0);
    assert_eq!(pipelines[1]["etag"].as_str().unwrap().len(), 64);
}

/// A retriever, a context builder and a generator: its output is an answer.
const ANSWERING: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n    - id: concat\n      component: context_builder\n      impl: concat\n      inputs: [question, lexical]\n    - id: generate\n      component: generator\n      impl: answerer\n      inputs: [question, concat]\n";

#[tokio::test]
async fn the_listing_says_whether_each_pipeline_ends_in_an_answer() {
    let workspace = scratch("listing_output");
    fs::write(workspace.pipelines().join("answering.yaml"), ANSWERING).unwrap();
    fs::write(workspace.pipelines().join("broken.yaml"), MIS_KINDED).unwrap();
    fs::write(workspace.pipelines().join("hybrid.yaml"), HYBRID).unwrap();

    let body = body_json(send(server(&workspace), get("/api/v1/pipelines")).await).await;

    let ends: Vec<&Value> = body["pipelines"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|p| &p["ends_in_answer"])
        .collect();
    // A document that does not validate has no output to speak of.
    assert_eq!(ends, [&json!(true), &Value::Null, &json!(false)]);
}

#[tokio::test]
async fn a_pipeline_says_which_of_its_nodes_a_cut_ending_in_an_answer_stops_at() {
    let workspace = scratch("detail_output");
    fs::write(workspace.pipelines().join("answering.yaml"), ANSWERING).unwrap();
    fs::write(workspace.pipelines().join("broken.yaml"), MIS_KINDED).unwrap();

    let answering =
        body_json(send(server(&workspace), get("/api/v1/pipelines/answering")).await).await;
    let broken = body_json(send(server(&workspace), get("/api/v1/pipelines/broken")).await).await;

    // Cut at a node, the pipeline ends in what that node produces: only the
    // generator's cut — the whole pipeline here — ends in an answer.
    assert_eq!(
        answering["ends_in_answer_up_to"],
        json!({ "lexical": false, "concat": false, "generate": true })
    );
    // A document that does not validate has no cut to speak of.
    assert_eq!(broken["ends_in_answer_up_to"], Value::Null);
}

#[tokio::test]
async fn a_write_with_a_stale_etag_is_refused_with_412_and_the_current_etag() {
    let workspace = scratch("stale");
    let first = create(&workspace, "hybrid", HYBRID).await;
    let current = {
        let response = send(
            server(&workspace),
            write_request(
                "PUT",
                "/api/v1/pipelines/hybrid",
                &document(HYBRID_REFORMATTED),
                &[("if-match", &first)],
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    };
    let before = on_disk(&workspace, "hybrid");

    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(HYBRID),
            &[("if-match", &first)],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        response.headers().get("etag").unwrap().to_str().unwrap(),
        current
    );
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "precondition_failed");
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains(current.trim_matches('"')),
        "{problem}"
    );
    // And as a member of its own, for a client that reads the body alone.
    assert_eq!(problem["etag"], current.trim_matches('"'));
    assert_eq!(on_disk(&workspace, "hybrid"), before);
}

#[tokio::test]
async fn a_write_without_a_precondition_or_creating_over_a_file_is_refused() {
    let workspace = scratch("preconditions");
    create(&workspace, "hybrid", HYBRID).await;

    for headers in [vec![], vec![("if-none-match", "*")]] {
        let response = send(
            server(&workspace),
            write_request(
                "PUT",
                "/api/v1/pipelines/hybrid",
                &document(HYBRID_REFORMATTED),
                &headers,
            ),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{headers:?}"
        );
        assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
    }
    // `If-Match` on a file that is not there: nothing to match.
    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/other",
            &document(HYBRID),
            &[("if-match", "\"0000\"")],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    assert!(!workspace.pipelines().join("other.yaml").exists());
}

#[tokio::test]
async fn a_write_with_the_current_etag_stores_the_exact_bytes_sent() {
    let workspace = scratch("current");
    let etag = create(&workspace, "hybrid", HYBRID).await;
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());

    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(HYBRID_REFORMATTED),
            &[("if-match", &etag)],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let header = response
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let written = body_json(response).await;
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID_REFORMATTED.as_bytes());
    assert_eq!(header, format!("\"{}\"", written["etag"].as_str().unwrap()));
    assert!(written["hash"].is_string());

    let read = body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid")).await).await;
    assert_eq!(read["document"], HYBRID_REFORMATTED);
    assert_eq!(read["etag"], written["etag"]);
}

#[tokio::test]
async fn a_write_that_does_not_validate_is_refused_and_changes_nothing() {
    let workspace = scratch("invalid");
    let etag = create(&workspace, "hybrid", HYBRID).await;

    for text in [
        MIS_KINDED,
        "pipeline: [not, a, graph",
        "pipeline:\n  nodes: []\n",
    ] {
        let response = send(
            server(&workspace),
            write_request(
                "PUT",
                "/api/v1/pipelines/hybrid",
                &document(text),
                &[("if-match", &etag)],
            ),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{text}"
        );
        assert_eq!(body_json(response).await["code"], "pipeline_invalid");
        assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
    }
    // Nor is a new file created by an invalid write.
    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/fresh",
            &document(MIS_KINDED),
            &[("if-none-match", "*")],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let names: Vec<String> = fs::read_dir(workspace.pipelines())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["hybrid.yaml"]);
}

#[tokio::test]
async fn a_document_written_back_unchanged_keeps_its_bytes_etag_and_hash() {
    let workspace = scratch("unchanged");
    fs::write(workspace.pipelines().join("hybrid.yaml"), HYBRID).unwrap();
    let read = body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid")).await).await;
    let etag = format!("\"{}\"", read["etag"].as_str().unwrap());

    let written = body_json(
        send(
            server(&workspace),
            write_request(
                "PUT",
                "/api/v1/pipelines/hybrid",
                &document(read["document"].as_str().unwrap()),
                &[("if-match", &etag)],
            ),
        )
        .await,
    )
    .await;

    assert_eq!(written["etag"], read["etag"]);
    assert_eq!(written["hash"], read["hash"]);
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
}

/// A key the composition root says no component reads: the fake launcher
/// refuses it as the binary refuses an unread key, naming the node.
#[tokio::test]
async fn a_write_the_composition_root_refuses_is_refused_in_its_words() {
    let workspace = scratch("unread_key");
    let text = HYBRID.replace(
        "params: { top_k: 10 }",
        &format!("params: {{ top_k: 10, {UNREAD_KEY}: 3 }}"),
    );

    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(&text),
            &[("if-none-match", "*")],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "pipeline_invalid");
    assert_eq!(problem["location"]["node"], "lexical");
    let detail = problem["detail"].as_str().unwrap();
    assert!(
        detail.contains(&format!("node `lexical`: `{UNREAD_KEY}` is not a key")),
        "{detail}"
    );
    assert!(!workspace.pipelines().join("hybrid.yaml").exists());

    // `validate` applies none of the composition root's checks (ADR-C32 § 2),
    // as `ragondin validate` applies none: the document hashes.
    let validated = send(
        server(&workspace),
        write_request("POST", "/api/v1/pipelines/validate", &document(&text), &[]),
    )
    .await;
    assert_eq!(validated.status(), StatusCode::OK);
}

/// A parameter whose value is a URL is a parameter like any other: whether
/// it is read is the composition root's to say, and nothing here looks at
/// the value.
#[tokio::test]
async fn a_url_valued_parameter_is_accepted() {
    let workspace = scratch("url_valued");
    let text = HYBRID.replace(
        "params: { top_k: 10 }",
        "params: { top_k: 10, query_prefix: \"http://example.org/q \" }",
    );

    for request in [
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(&text),
            &[("if-none-match", "*")],
        ),
        write_request("POST", "/api/v1/pipelines/validate", &document(&text), &[]),
    ] {
        let response = send(server(&workspace), request).await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(on_disk(&workspace, "hybrid"), text.as_bytes());
}

#[tokio::test]
async fn if_match_star_matches_any_stored_document_and_nothing_absent() {
    let workspace = scratch("if_match_star");
    create(&workspace, "hybrid", HYBRID).await;

    let replaced = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(HYBRID_REFORMATTED),
            &[("if-match", "*")],
        ),
    )
    .await;
    assert_eq!(replaced.status(), StatusCode::OK);
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID_REFORMATTED.as_bytes());

    let absent = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/other",
            &document(HYBRID),
            &[("if-match", "*")],
        ),
    )
    .await;
    assert_eq!(absent.status(), StatusCode::PRECONDITION_FAILED);
    assert!(!workspace.pipelines().join("other.yaml").exists());
}

/// A header the write reads, sent twice, is refused naming it, as a
/// repeated query parameter is: which of the two would hold is a guess.
#[tokio::test]
async fn if_match_sent_twice_is_parameter_invalid_naming_it() {
    let workspace = scratch("if_match_twice");
    let etag = create(&workspace, "hybrid", HYBRID).await;

    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(HYBRID_REFORMATTED),
            &[("if-match", &etag), ("if-match", "*")],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "parameter_invalid");
    assert_eq!(problem["name"], "If-Match");
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
}

/// A precondition header whose value is not text is `request_invalid`, as a
/// problem body, as it was before the headers were declared.
#[tokio::test]
async fn a_precondition_header_that_is_not_text_is_request_invalid() {
    let workspace = scratch("if_match_not_text");
    create(&workspace, "hybrid", HYBRID).await;

    let mut request = write_request(
        "PUT",
        "/api/v1/pipelines/hybrid",
        &document(HYBRID_REFORMATTED),
        &[],
    );
    request.headers_mut().insert(
        "if-match",
        axum::http::HeaderValue::from_bytes(b"\"\xff\"").unwrap(),
    );
    let response = send(server(&workspace), request).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers()["content-type"],
        "application/problem+json"
    );
    assert_eq!(body_json(response).await["code"], "request_invalid");
}

/// A header the write does not read is ignored: a request carries many.
#[tokio::test]
async fn a_header_the_write_does_not_read_is_ignored() {
    let workspace = scratch("unrelated_header");
    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid",
            &document(HYBRID),
            &[
                ("if-none-match", "*"),
                ("x-unrelated", "1"),
                ("x-unrelated", "2"),
            ],
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
}

/// Eight writes naming the same etag, at once: one stores its bytes, the
/// seven others are refused with the etag the winner left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_with_the_same_etag_have_exactly_one_winner() {
    let workspace = scratch("concurrent");
    let etag = create(&workspace, "hybrid", HYBRID).await;
    let app = server(&workspace);

    let writes = (0..8).map(|n| {
        let (app, etag) = (app.clone(), etag.clone());
        let text = format!("# writer {n}\n{HYBRID}");
        tokio::spawn(async move {
            let response = send(
                app,
                write_request(
                    "PUT",
                    "/api/v1/pipelines/hybrid",
                    &document(&text),
                    &[("if-match", &etag)],
                ),
            )
            .await;
            (response.status(), text)
        })
    });
    let mut outcomes = Vec::new();
    for write in writes.collect::<Vec<_>>() {
        outcomes.push(write.await.unwrap());
    }

    let winners: Vec<&String> = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .map(|(_, text)| text)
        .collect();
    assert_eq!(winners.len(), 1, "{outcomes:?}");
    assert!(
        outcomes
            .iter()
            .all(|(status, _)| *status == StatusCode::OK
                || *status == StatusCode::PRECONDITION_FAILED)
    );
    assert_eq!(on_disk(&workspace, "hybrid"), winners[0].as_bytes());
}

/// With only `Fresh.yaml` stored, `fresh` names it on a filesystem that
/// ignores case and nothing on one that does not: a read or a layout under
/// `fresh` is refused as a write is, naming `Fresh`, and no
/// `fresh.layout.json` appears beside `Fresh.yaml`.
#[tokio::test]
async fn reads_and_layouts_under_a_case_alias_are_refused_naming_the_stored_name() {
    let workspace = scratch("case_alias_reads");
    fs::write(workspace.pipelines().join("Fresh.yaml"), HYBRID).unwrap();
    let layout = json!({ "version": 1, "nodes": { "lexical": { "x": 1.0, "y": 2.0 } } });

    for request in [
        get("/api/v1/pipelines/fresh"),
        get("/api/v1/pipelines/fresh/layout"),
        write_request("PUT", "/api/v1/pipelines/fresh/layout", &layout, &[]),
    ] {
        let path = request.uri().to_string();
        let response = send(server(&workspace), request).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let problem = body_json(response).await;
        assert_eq!(problem["code"], "request_invalid", "{path}");
        assert!(
            problem["detail"].as_str().unwrap().contains("`Fresh`"),
            "{path}: {problem}"
        );
    }
    let names: Vec<String> = fs::read_dir(workspace.pipelines())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["Fresh.yaml"]);
    // The stored name itself reads.
    let read = send(server(&workspace), get("/api/v1/pipelines/Fresh")).await;
    assert_eq!(read.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_name_differing_from_a_stored_one_only_in_case_is_refused() {
    let workspace = scratch("case_alias");
    create(&workspace, "hybrid", HYBRID).await;

    let response = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/Hybrid",
            &document(HYBRID_REFORMATTED),
            &[("if-none-match", "*")],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "request_invalid");
    assert!(
        problem["detail"].as_str().unwrap().contains("`hybrid`"),
        "{problem}"
    );
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
    let names: Vec<_> = fs::read_dir(workspace.pipelines()).unwrap().collect();
    assert_eq!(names.len(), 1);
}

#[tokio::test]
async fn a_name_that_is_not_one_file_name_is_refused() {
    let workspace = scratch("names");

    for name in [
        ".hidden",
        "a.",
        "validate",
        &"x".repeat(65),
        "CON",
        "nul",
        "Aux.v1",
        "com1",
        "LPT9",
    ] {
        let response = send(
            server(&workspace),
            write_request(
                "PUT",
                &format!("/api/v1/pipelines/{name}"),
                &document(HYBRID),
                &[("if-none-match", "*")],
            ),
        )
        .await;

        assert!(
            response.status().is_client_error(),
            "{name}: {}",
            response.status()
        );
    }
    let names: Vec<_> = fs::read_dir(workspace.pipelines()).unwrap().collect();
    assert!(names.is_empty());
}

#[tokio::test]
async fn validate_returns_the_hash_of_the_canonical_form() {
    let workspace = scratch("validate");

    let first = body_json(
        send(
            server(&workspace),
            write_request("POST", "/api/v1/pipelines/validate", &document(HYBRID), &[]),
        )
        .await,
    )
    .await;
    let second = body_json(
        send(
            server(&workspace),
            write_request(
                "POST",
                "/api/v1/pipelines/validate",
                &document(HYBRID_REFORMATTED),
                &[],
            ),
        )
        .await,
    )
    .await;

    assert_eq!(first["hash"].as_str().unwrap().len(), 64);
    assert_eq!(first["hash"], second["hash"]);
}

#[tokio::test]
async fn validate_locates_a_mis_kinded_edge_in_the_cli_words() {
    let workspace = scratch("mis_kinded");

    let response = send(
        server(&workspace),
        write_request(
            "POST",
            "/api/v1/pipelines/validate",
            &document(MIS_KINDED),
            &[],
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "pipeline_invalid");
    assert_eq!(
        problem["location"],
        json!({ "node": "ranked", "edge": { "from": "legs", "to": "ranked", "port": 0 } })
    );
    let detail = problem["detail"].as_str().unwrap();
    for line in [
        "wires two nodes incompatibly",
        "edge: `legs` feeds `ranked` at port 0",
        "expected: query",
        "found: chunks",
    ] {
        assert!(detail.contains(line), "{line:?} in {detail}");
    }
}

#[tokio::test]
async fn a_body_that_is_not_the_request_s_json_is_request_invalid() {
    let workspace = scratch("bad_body");

    for body in [
        json!("just a string"),
        json!({ "doc": HYBRID }),
        json!({ "document": HYBRID, "documnet": HYBRID }),
    ] {
        let response = send(
            server(&workspace),
            write_request("POST", "/api/v1/pipelines/validate", &body, &[]),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body_json(response).await["code"], "request_invalid");
    }
}

#[tokio::test]
async fn the_layout_round_trips_beside_the_document_and_never_changes_the_hash() {
    let workspace = scratch("layout");
    create(&workspace, "hybrid", HYBRID).await;
    let before = body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid")).await).await;

    let absent =
        body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid/layout")).await).await;
    assert_eq!(absent, json!({ "layout": null }));

    let layout = json!({ "version": 1, "nodes": { "lexical": { "x": 120.5, "y": -40.0 } } });
    let response = send(
        server(&workspace),
        write_request("PUT", "/api/v1/pipelines/hybrid/layout", &layout, &[]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    assert!(workspace.pipelines().join("hybrid.layout.json").is_file());
    let read =
        body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid/layout")).await).await;
    assert_eq!(read, json!({ "layout": layout }));
    let after = body_json(send(server(&workspace), get("/api/v1/pipelines/hybrid")).await).await;
    assert_eq!(after["hash"], before["hash"]);
    assert_eq!(after["etag"], before["etag"]);
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());

    // A layout of another version, or of a pipeline that is not there, is refused.
    let refused = send(
        server(&workspace),
        write_request(
            "PUT",
            "/api/v1/pipelines/hybrid/layout",
            &json!({ "version": 2, "nodes": {} }),
            &[],
        ),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    let absent = send(
        server(&workspace),
        write_request("PUT", "/api/v1/pipelines/absent/layout", &layout, &[]),
    )
    .await;
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);
}

/// Every refusal `POST /pipelines/validate` makes, its whole problem body
/// pinned byte for byte: the load and its words are `ragondin-config`'s, and
/// this is what keeps a change there from reaching a client unannounced.
#[tokio::test]
async fn validate_refuses_every_branch_with_the_problem_body_it_always_has() {
    let workspace = scratch("refusal_golden");
    let node = "    - id: legs\n      component: retriever\n      impl: bm25\n";
    let unlocated = json!({ "node": null, "edge": null });
    let at_legs = json!({ "node": "legs", "edge": null });
    let cases = [
        (
            "version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n".to_owned(),
            "the configuration is written in a schema version this build cannot read: unsupported pipeline schema version 99: this build reads version 1",
            unlocated.clone(),
        ),
        (
            "pipeline:\n  inputs: [q\n  nodes: []\n".to_owned(),
            "could not parse configuration: did not find expected ',' or ']' at line 3 column 8, while parsing a flow sequence at line 2 column 11",
            unlocated.clone(),
        ),
        (
            "pipeline: [\n".to_owned(),
            "could not parse configuration: pipeline: invalid type: sequence, expected struct RawGraph at line 1 column 11",
            unlocated.clone(),
        ),
        (
            "pipeline:\n  inputs: [q]\n".to_owned(),
            "could not parse configuration: pipeline: missing field `nodes` at line 2 column 3",
            unlocated.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [a, b]\n  nodes:\n{node}      inputs: [a]\n"),
            "configuration is not a valid pipeline: a pipeline must declare exactly one input, found 2",
            unlocated.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [q]\n{node}      inputs: [q]\n"),
            "configuration is not a valid pipeline: duplicate node id `legs`",
            at_legs.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [nowhere]\n"),
            "configuration is not a valid pipeline: node `legs`: input `nowhere` names neither a node nor a declared input",
            at_legs.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [legs]\n  nodes:\n{node}      inputs: [legs]\n"),
            "configuration is not a valid pipeline: declared input `legs` is also a node id",
            at_legs.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [legs]\n"),
            "configuration is not a valid pipeline: cycle in the pipeline's data edges: legs",
            at_legs.clone(),
        ),
        (
            format!("pipeline:\n  inputs: [q]\n  nodes:\n{node}      inputs: [q]\n      params: {{ top_k: .inf }}\n"),
            "configuration is not a valid pipeline: node `legs`: parameter `top_k` is not a finite number",
            at_legs.clone(),
        ),
        (
            "pipeline:\n  inputs: [q]\n  nodes:\n    - { id: legs, component: summarizer, impl: x, inputs: [q] }\n".to_owned(),
            "configuration is not a valid pipeline: node `legs`: unknown component `summarizer`",
            at_legs.clone(),
        ),
        (
            MIS_KINDED.to_owned(),
            "the configuration wires two nodes incompatibly\n  edge: `legs` feeds `ranked` at port 0\n  expected: query\n  found: chunks",
            json!({ "node": "ranked", "edge": { "from": "legs", "to": "ranked", "port": 0 } }),
        ),
        (
            MIS_KINDED.replace("inputs: [legs, question]", "inputs: [question, legs, legs]"),
            "the configuration wires two nodes incompatibly\n  edge: `legs` feeds `ranked` at port 2\n  expected: nothing — `ranked` declares no port at position 2\n  found: chunks",
            json!({ "node": "ranked", "edge": { "from": "legs", "to": "ranked", "port": 2 } }),
        ),
    ];

    for (text, detail, location) in cases {
        let response = send(
            server(&workspace),
            write_request("POST", "/api/v1/pipelines/validate", &document(&text), &[]),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{text}"
        );
        assert_eq!(
            body_json(response).await,
            json!({
                "type": "urn:ragondin:problem:pipeline_invalid",
                "title": "The pipeline is invalid",
                "status": 422,
                "code": "pipeline_invalid",
                "detail": format!("the pipeline does not validate: {detail}"),
                "hint": "Correct the node or edge named in `location`, then validate again.",
                "location": location,
            }),
            "{text}"
        );
    }
}

// The typed document (ADR-C40): what `GET /pipelines/{name}` adds beside the
// text, and what `POST /pipelines/validate` reads from the editor.

/// A pipeline holding a float written with a fractional part, an integer, a
/// flag, text that reads as another kind, and a list.
const KINDS: &str = "pipeline:\n  inputs: [question]\n  nodes:\n    - id: lexical\n      component: retriever\n      impl: bm25\n      inputs: [question]\n      params:\n        top_k: 10\n        k: 60.0\n        exact: true\n        label: 'yes'\n        weights: [1, 0.5]\n";

/// `KINDS` as the editor holds it.
fn kinds_typed() -> Value {
    json!({
        "version": 1,
        "pipeline": {
            "inputs": ["question"],
            "nodes": [{
                "id": "lexical",
                "component": "retriever",
                "impl": "bm25",
                "inputs": ["question"],
                "params": {
                    "exact": { "kind": "bool", "value": true },
                    "k": { "kind": "float", "value": 60.0 },
                    "label": { "kind": "string", "value": "yes" },
                    "top_k": { "kind": "int", "value": "10" },
                    "weights": { "kind": "list", "value": [
                        { "kind": "int", "value": "1" },
                        { "kind": "float", "value": 0.5 },
                    ] },
                },
            }],
        },
    })
}

async fn read_detail(workspace: &Workspace, name: &str, text: &str) -> Value {
    fs::write(workspace.pipelines().join(format!("{name}.yaml")), text).unwrap();
    body_json(send(server(workspace), get(&format!("/api/v1/pipelines/{name}"))).await).await
}

async fn validate(workspace: &Workspace, body: &Value) -> (StatusCode, Value) {
    let response = send(
        server(workspace),
        write_request("POST", "/api/v1/pipelines/validate", body, &[]),
    )
    .await;
    (response.status(), body_json(response).await)
}

#[tokio::test]
async fn a_read_carries_the_typed_document_every_value_with_its_kind() {
    let workspace = scratch("typed_read");

    let body = read_detail(&workspace, "kinds", KINDS).await;

    assert_eq!(body["document"], KINDS);
    assert_eq!(body["typed"], kinds_typed());
}

#[tokio::test]
async fn a_document_that_reads_but_does_not_validate_still_carries_its_typed_document() {
    let workspace = scratch("typed_invalid");

    let body = read_detail(&workspace, "broken", MIS_KINDED).await;

    assert_eq!(body["hash"], Value::Null);
    assert!(body["error"].is_object(), "{body}");
    let nodes = body["typed"]["pipeline"]["nodes"]
        .as_array()
        .expect("typed");
    assert_eq!(nodes[1]["id"], "ranked");
    assert_eq!(nodes[1]["inputs"], json!(["legs", "question"]));
}

#[tokio::test]
async fn a_document_that_does_not_read_or_holds_a_value_the_type_cannot_carry_is_text_only() {
    let workspace = scratch("typed_text_only");
    let nan = KINDS.replace("k: 60.0", "k: .nan");
    let infinite = KINDS.replace("k: 60.0", "k: -.inf");

    for (name, text) in [
        ("syntax", "pipeline: ["),
        (
            "version",
            "version: 99\npipeline:\n  inputs: [q]\n  nodes: []\n",
        ),
        ("nan", nan.as_str()),
        ("infinite", infinite.as_str()),
    ] {
        let body = read_detail(&workspace, name, text).await;

        assert_eq!(body["typed"], Value::Null, "{name}: {body}");
        assert_eq!(body["document"], text, "{name}");
    }
}

#[tokio::test]
async fn validating_the_typed_document_hashes_what_validating_its_text_hashes() {
    let workspace = scratch("typed_validate");

    let (status, typed) = validate(&workspace, &json!({ "typed": kinds_typed() })).await;
    let (_, text) = validate(&workspace, &document(KINDS)).await;

    assert_eq!(status, StatusCode::OK, "{typed}");
    assert_eq!(typed["hash"], text["hash"]);
}

#[tokio::test]
async fn a_float_and_an_integer_of_one_value_validate_to_two_hashes() {
    let workspace = scratch("typed_kinds_hash");
    let with_k = |value: Value| {
        let mut typed = kinds_typed();
        typed["pipeline"]["nodes"][0]["params"]["k"] = value;
        json!({ "typed": typed })
    };

    // The browser's `JSON.stringify(60.0)` is `60`: the tag keeps it a float.
    let (_, float) = validate(&workspace, &with_k(json!({ "kind": "float", "value": 60 }))).await;
    let (_, integer) = validate(&workspace, &with_k(json!({ "kind": "int", "value": "60" }))).await;
    let (_, text) = validate(&workspace, &document(KINDS)).await;
    let (_, integer_text) = validate(&workspace, &document(&KINDS.replace("60.0", "60"))).await;

    assert_eq!(float["hash"], text["hash"]);
    assert_ne!(integer["hash"], float["hash"]);
    assert_eq!(integer["hash"], integer_text["hash"]);
}

#[tokio::test]
async fn a_typed_document_the_pass_refuses_is_pipeline_invalid_located() {
    let workspace = scratch("typed_dangling");
    let mut typed = kinds_typed();
    typed["pipeline"]["nodes"][0]["inputs"] = json!(["nowhere"]);

    let (status, problem) = validate(&workspace, &json!({ "typed": typed })).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "pipeline_invalid");
    assert_eq!(problem["location"]["node"], "lexical");
}

/// A typed document the renderer cannot write so that it reads back — here a
/// parameter name too long, once rendered, for YAML to read as a key — is refused as
/// `pipeline_invalid` in the renderer's words, never rendered otherwise.
#[tokio::test]
async fn a_typed_document_the_renderer_cannot_write_is_pipeline_invalid_in_its_words() {
    let workspace = scratch("typed_unrenderable");
    let mut typed = kinds_typed();
    typed["pipeline"]["nodes"][0]["params"]["k".repeat(1100)] =
        json!({ "kind": "bool", "value": true });

    let (status, problem) = validate(&workspace, &json!({ "typed": typed })).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert_eq!(problem["code"], "pipeline_invalid");
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("a parameter name too long once rendered"),
        "{problem}"
    );
}

#[tokio::test]
async fn a_typed_document_in_a_version_this_build_cannot_read_is_pipeline_invalid() {
    let workspace = scratch("typed_version");
    let mut typed = kinds_typed();
    typed["version"] = json!(2);

    let (status, problem) = validate(&workspace, &json!({ "typed": typed })).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{problem}");
    assert!(
        problem["detail"]
            .as_str()
            .unwrap()
            .contains("schema version this build cannot read"),
        "{problem}"
    );
}

#[tokio::test]
async fn a_typed_document_without_a_version_is_read_in_the_version_this_build_reads() {
    let workspace = scratch("typed_no_version");
    let mut typed = kinds_typed();
    typed.as_object_mut().unwrap().remove("version");

    let (status, body) = validate(&workspace, &json!({ "typed": typed })).await;
    let (_, text) = validate(&workspace, &document(KINDS)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hash"], text["hash"]);
}

#[tokio::test]
async fn a_value_its_kind_does_not_carry_is_request_invalid() {
    let workspace = scratch("typed_bad_values");

    for value in [
        json!({ "kind": "int", "value": 10 }),
        json!({ "kind": "int", "value": "1.0" }),
        json!({ "kind": "int", "value": "1e3" }),
        json!({ "kind": "int", "value": " 10" }),
        json!({ "kind": "int", "value": "+10" }),
        json!({ "kind": "int", "value": "010" }),
        json!({ "kind": "int", "value": "-0" }),
        json!({ "kind": "int", "value": "" }),
        json!({ "kind": "int", "value": "9223372036854775808" }),
        json!({ "kind": "float", "value": "60.0" }),
        json!({ "kind": "bool", "value": "true" }),
        json!({ "kind": "string", "value": 1 }),
        json!({ "kind": "map", "value": {} }),
        json!(10),
        json!({ "kind": "int" }),
        json!({ "kind": "int", "value": "1", "extra": 1 }),
    ] {
        let mut typed = kinds_typed();
        typed["pipeline"]["nodes"][0]["params"]["k"] = value.clone();

        let (status, problem) = validate(&workspace, &json!({ "typed": typed })).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{value}: {problem}");
        assert_eq!(problem["code"], "request_invalid", "{value}");
    }
}

#[tokio::test]
async fn the_widest_integers_travel_whole() {
    let workspace = scratch("typed_wide_integers");
    let text = KINDS.replace("top_k: 10", "top_k: -9223372036854775808");

    let body = read_detail(&workspace, "wide", &text).await;
    let top_k = &body["typed"]["pipeline"]["nodes"][0]["params"]["top_k"];
    let (_, typed) = validate(&workspace, &json!({ "typed": body["typed"] })).await;
    let (_, from_text) = validate(&workspace, &document(&text)).await;

    assert_eq!(
        top_k,
        &json!({ "kind": "int", "value": "-9223372036854775808" })
    );
    assert_eq!(typed["hash"], from_text["hash"]);
}

#[tokio::test]
async fn a_body_that_is_neither_form_is_request_invalid() {
    let workspace = scratch("typed_bad_body");

    for body in [
        json!({ "document": KINDS, "typed": kinds_typed() }),
        json!({ "typed": KINDS }),
        json!({ "typed": { "pipeline": { "inputs": [], "nodes": [] }, "extra": 1 } }),
        json!({ "typed": { "version": 1 } }),
    ] {
        let (status, problem) = validate(&workspace, &body).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {problem}");
        assert_eq!(problem["code"], "request_invalid", "{body}");
    }
}

#[tokio::test]
async fn a_typed_document_read_from_a_file_validates_to_the_hash_of_its_text() {
    // What the editor does on opening a stored pipeline: it sends back the
    // typed document it read, and the hash is the one its text has.
    let workspace = scratch("typed_read_validate");
    let detail = read_detail(&workspace, "kinds", KINDS).await;

    let (status, body) = validate(&workspace, &json!({ "typed": detail["typed"] })).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["hash"], detail["hash"]);
}

// The editor's writes: a typed document stored as the server renders it, the
// read saying whether a file's text is that rendering, the rendering a
// validation answers for an export, and a run's layout read for a fork.

/// Writes `body` to `name` with `headers`, answering the status and body.
async fn put(
    workspace: &Workspace,
    name: &str,
    body: &Value,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let response = send(
        server(workspace),
        write_request("PUT", &format!("/api/v1/pipelines/{name}"), body, headers),
    )
    .await;
    (response.status(), body_json(response).await)
}

#[tokio::test]
async fn a_typed_write_stores_the_server_s_rendering_and_hashes_as_its_validation() {
    let workspace = scratch("typed_write");

    let (_, validated) = validate(&workspace, &json!({ "typed": kinds_typed() })).await;
    let (status, written) = put(
        &workspace,
        "kinds",
        &json!({ "typed": kinds_typed() }),
        &[("if-none-match", "*")],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{written}");
    let stored = String::from_utf8(on_disk(&workspace, "kinds")).unwrap();
    // The rendering, never the JSON the browser sent: block YAML that states
    // its version (ADR-C41).
    assert!(stored.starts_with("version: "), "{stored}");
    assert_eq!(validated["rendering"], stored.as_str());
    assert_eq!(written["hash"], validated["hash"]);
    let (_, text) = validate(&workspace, &document(&stored)).await;
    assert_eq!(text["hash"], written["hash"]);
}

#[tokio::test]
async fn a_typed_write_honours_the_precondition_and_writes_nothing_invalid() {
    let workspace = scratch("typed_write_guarded");
    let etag = create(&workspace, "kinds", KINDS).await;
    let mut dangling = kinds_typed();
    dangling["pipeline"]["nodes"][0]["inputs"] = json!(["nowhere"]);
    let typed = json!({ "typed": kinds_typed() });

    let (stale, problem) = put(&workspace, "kinds", &typed, &[("if-match", "\"0000\"")]).await;
    let (missing, _) = put(&workspace, "kinds", &typed, &[]).await;
    let (invalid, refused) = put(
        &workspace,
        "kinds",
        &json!({ "typed": dangling }),
        &[("if-match", &etag)],
    )
    .await;

    assert_eq!(stale, StatusCode::PRECONDITION_FAILED, "{problem}");
    assert_eq!(problem["code"], "precondition_failed");
    assert_eq!(missing, StatusCode::PRECONDITION_FAILED);
    assert_eq!(invalid, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["location"]["node"], "lexical");
    assert_eq!(on_disk(&workspace, "kinds"), KINDS.as_bytes());
}

#[tokio::test]
async fn the_read_says_whether_the_text_is_the_server_s_own_rendering() {
    let workspace = scratch("canonical_flag");
    let (status, written) = put(
        &workspace,
        "rendered",
        &json!({ "typed": kinds_typed() }),
        &[("if-none-match", "*")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{written}");
    let rendered =
        body_json(send(server(&workspace), get("/api/v1/pipelines/rendered")).await).await;
    let commented = format!("# a comment\n{}", rendered["document"].as_str().unwrap());

    assert_eq!(rendered["canonical"], true, "{rendered}");
    assert_eq!(
        read_detail(&workspace, "commented", &commented).await["canonical"],
        false
    );
    assert_eq!(
        read_detail(&workspace, "hand", HYBRID).await["canonical"],
        false
    );
    assert_eq!(
        read_detail(&workspace, "syntax", "pipeline: [").await["canonical"],
        false
    );
}

#[tokio::test]
async fn validate_answers_the_rendering_a_write_would_store() {
    let workspace = scratch("validate_rendering");

    let (_, typed) = validate(&workspace, &json!({ "typed": kinds_typed() })).await;
    let (_, text) = validate(&workspace, &document(KINDS)).await;

    // A text is answered with the rendering of the document it reads to, so
    // an import exports as the editor would write it.
    assert_eq!(text["rendering"], typed["rendering"]);
    let rendering = typed["rendering"].as_str().expect("a rendering");
    assert!(rendering.starts_with("version: "), "{rendering}");
    assert!(rendering.contains("k: 60.0"), "{rendering}");
}

#[tokio::test]
async fn a_run_s_layout_is_read_from_the_layouts_copied_at_launch() {
    let workspace = scratch("run_layout");
    let run = support::fixture_run();
    let id = run.id.to_string();
    let hash = run.inputs.pipeline.to_string();
    let serve = || {
        let mut backends = fakes(FakeRunStore::holding([run.clone()]));
        backends.pipelines = Arc::new(FsPipelines::new(&workspace));
        app_with_backends(backends)
    };

    let absent = body_json(send(serve(), get(&format!("/api/v1/runs/{id}/layout"))).await).await;
    fs::write(
        workspace.layouts().join(format!("{hash}.json")),
        r#"{"version":1,"nodes":{"lexical":{"x":16.0,"y":32.0}}}"#,
    )
    .unwrap();
    let present = body_json(send(serve(), get(&format!("/api/v1/runs/{id}/layout"))).await).await;
    let unknown = send(
        serve(),
        get(&format!("/api/v1/runs/{}/layout", "0".repeat(64))),
    )
    .await;

    assert_eq!(absent, json!({ "layout": null }));
    assert_eq!(
        present,
        json!({ "layout": { "version": 1, "nodes": { "lexical": { "x": 16.0, "y": 32.0 } } } })
    );
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

/// `POST /pipelines/{from}/rename` with `to`, stating `if_match`.
async fn rename(
    workspace: &Workspace,
    from: &str,
    to: &str,
    if_match: Option<&str>,
) -> axum::http::Response<axum::body::Body> {
    let headers: Vec<(&str, &str)> = if_match
        .map(|etag| ("if-match", etag))
        .into_iter()
        .collect();
    send(
        server(workspace),
        write_request(
            "POST",
            &format!("/api/v1/pipelines/{from}/rename"),
            &json!({ "to": to }),
            &headers,
        ),
    )
    .await
}

fn pairing_file(pipeline: &str, other: &str) -> String {
    format!(
        "{{\"version\":1,\"pipeline\":\"{pipeline}\",\"other\":\"{other}\",\"pairs\":[{{\"node\":\"lexical\",\"other\":\"lexical\"}}]}}"
    )
}

#[tokio::test]
async fn a_rename_moves_the_document_its_layout_and_its_pairings() {
    let workspace = scratch("rename");
    let etag = create(&workspace, "hybrid", HYBRID).await;
    create(&workspace, "baseline", HYBRID_REFORMATTED).await;
    create(&workspace, "third", HYBRID_REFORMATTED).await;
    let dir = workspace.pipelines();
    fs::write(
        dir.join("hybrid.layout.json"),
        r#"{"version":1,"nodes":{"lexical":{"x":16.0,"y":32.0}}}"#,
    )
    .unwrap();
    // A pairing kept from the renamed pipeline, and one kept towards it.
    fs::create_dir_all(dir.join("hybrid.pairing")).unwrap();
    fs::write(
        dir.join("hybrid.pairing/baseline.json"),
        pairing_file("hybrid", "baseline"),
    )
    .unwrap();
    fs::create_dir_all(dir.join("third.pairing")).unwrap();
    fs::write(
        dir.join("third.pairing/hybrid.json"),
        pairing_file("third", "hybrid"),
    )
    .unwrap();

    let response = rename(&workspace, "hybrid", "lexical-only", Some(&etag)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let header = response
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = body_json(response).await;
    assert_eq!(body["name"], "lexical-only");
    // The bytes move as they are, so the etag is the one the rename named.
    assert_eq!(header, etag);
    assert_eq!(format!("\"{}\"", body["etag"].as_str().unwrap()), etag);
    assert!(body["hash"].is_string(), "{body}");
    assert_eq!(on_disk(&workspace, "lexical-only"), HYBRID.as_bytes());
    assert!(!dir.join("hybrid.yaml").exists());
    assert!(!dir.join("hybrid.layout.json").exists());
    assert_eq!(
        body_json(
            send(
                server(&workspace),
                get("/api/v1/pipelines/lexical-only/layout")
            )
            .await
        )
        .await,
        json!({ "layout": { "version": 1, "nodes": { "lexical": { "x": 16.0, "y": 32.0 } } } })
    );
    assert!(!dir.join("hybrid.pairing").exists());
    let moved: Value =
        serde_json::from_slice(&fs::read(dir.join("lexical-only.pairing/baseline.json")).unwrap())
            .unwrap();
    assert_eq!(
        (moved["pipeline"].as_str(), moved["other"].as_str()),
        (Some("lexical-only"), Some("baseline"))
    );
    assert!(!dir.join("third.pairing/hybrid.json").exists());
    let towards: Value =
        serde_json::from_slice(&fs::read(dir.join("third.pairing/lexical-only.json")).unwrap())
            .unwrap();
    assert_eq!(
        (towards["pipeline"].as_str(), towards["other"].as_str()),
        (Some("third"), Some("lexical-only"))
    );
    let old = send(server(&workspace), get("/api/v1/pipelines/hybrid")).await;
    assert_eq!(old.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_rename_with_a_stale_etag_or_none_moves_nothing() {
    let workspace = scratch("rename_stale");
    create(&workspace, "hybrid", HYBRID).await;
    let stale = format!("\"{}\"", "0".repeat(64));

    for if_match in [Some(stale.as_str()), None] {
        let response = rename(&workspace, "hybrid", "other", if_match).await;

        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{if_match:?}"
        );
        assert_eq!(body_json(response).await["code"], "precondition_failed");
        assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
        assert!(!workspace.pipelines().join("other.yaml").exists());
    }
}

#[tokio::test]
async fn a_rename_onto_a_taken_name_is_pipeline_exists_and_moves_nothing() {
    let workspace = scratch("rename_taken");
    let etag = create(&workspace, "hybrid", HYBRID).await;
    create(&workspace, "baseline", HYBRID_REFORMATTED).await;

    let response = rename(&workspace, "hybrid", "baseline", Some(&etag)).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_json(response).await;
    assert_eq!(body["code"], "pipeline_exists");
    assert!(
        body["detail"].as_str().unwrap().contains("baseline"),
        "{body}"
    );
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
    assert_eq!(
        on_disk(&workspace, "baseline"),
        HYBRID_REFORMATTED.as_bytes()
    );
}

#[tokio::test]
async fn a_rename_to_a_name_that_is_not_one_or_from_an_absent_pipeline_is_refused() {
    let workspace = scratch("rename_names");
    let etag = create(&workspace, "hybrid", HYBRID).await;
    create(&workspace, "Baseline", HYBRID_REFORMATTED).await;

    for to in ["a/b", "validate", ".hidden", "", "baseline"] {
        let response = rename(&workspace, "hybrid", to, Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{to}");
        assert_eq!(body_json(response).await["code"], "request_invalid", "{to}");
    }
    let absent = rename(&workspace, "absent", "other", Some(&etag)).await;
    assert_eq!(absent.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(absent).await["code"], "pipeline_not_found");
    // Renamed to itself: nothing to move, and nothing refused.
    let same = rename(&workspace, "hybrid", "hybrid", Some(&etag)).await;
    assert_eq!(same.status(), StatusCode::OK);
    assert_eq!(on_disk(&workspace, "hybrid"), HYBRID.as_bytes());
}

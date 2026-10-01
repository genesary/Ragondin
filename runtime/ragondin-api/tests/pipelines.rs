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
            "the configuration is written in a schema version this build cannot read: unsupported pipeline schema version 99: this build reads version 3",
            unlocated.clone(),
        ),
        (
            "pipeline:\n  inputs: [q\n  nodes: []\n".to_owned(),
            "could not parse configuration: did not find expected ',' or ']' at line 3 column 8, while parsing a flow sequence at line 2 column 11",
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

//! Negative decode tests: every message where the wire admits a value the
//! domain does not (ADR-C24).
//!
//! The round-trip property starts from a domain value, so it never produces
//! these messages. Each is refused by `from_proto`, and each test here proves
//! it: a required message left out, an `EmbedRole` of `UNSPECIFIED` or of a
//! number the enum does not name (ADR-C17), an empty identity (ADR-C31 § 1),
//! an empty `served_model` (ADR-C32 § 4), and a generator's empty
//! `served_model` or `template`, which is what an omitted one decodes as
//! (ADR-C31 § 2). Which `ComponentError` a refusal becomes depends on which
//! side decoded it, and is `status.rs`'s concern.

use ragondin_contracts::{
    ContextParams, EmbedParams, EmbeddedChunk, FusionParams, GenerateParams, RerankParams,
    RetrieveParams, SearchParams,
};
use ragondin_proto::v1;
use ragondin_remote::{DecodeError, FromProto};
use ragondin_types::{
    Answer, Chunk, Context, ContextChunk, Embedding, ModelIdentity, Query, ScoredChunk,
};

fn wire_chunk() -> v1::Chunk {
    v1::Chunk {
        id: "c".into(),
        text: "t".into(),
        document_id: "d".into(),
    }
}

fn wire_query() -> v1::Query {
    v1::Query {
        id: "q".into(),
        text: "t".into(),
    }
}

fn assert_missing<T: std::fmt::Debug>(result: Result<T, DecodeError>, want: (&str, &str)) {
    match result {
        Err(DecodeError::Missing { message, field }) => assert_eq!((message, field), want),
        other => panic!("expected {want:?} to be refused as missing, got {other:?}"),
    }
}

// --- a required message left out ---------------------------------------------

#[test]
fn a_scored_chunk_without_its_chunk_is_refused() {
    let wire = v1::ScoredChunk {
        chunk: None,
        score: 1.0,
    };
    assert_missing(ScoredChunk::from_proto(wire), ("ScoredChunk", "chunk"));
}

#[test]
fn an_embedded_chunk_without_its_chunk_or_embedding_is_refused() {
    let wire = v1::EmbeddedChunk {
        chunk: None,
        embedding: Some(v1::Embedding {
            components: vec![1.0],
        }),
    };
    assert_missing(EmbeddedChunk::from_proto(wire), ("EmbeddedChunk", "chunk"));

    let wire = v1::EmbeddedChunk {
        chunk: Some(wire_chunk()),
        embedding: None,
    };
    assert_missing(
        EmbeddedChunk::from_proto(wire),
        ("EmbeddedChunk", "embedding"),
    );
}

#[test]
fn a_missing_chunk_deep_in_a_list_is_refused() {
    let wire = v1::RetrieveResponse {
        chunks: vec![
            v1::ScoredChunk {
                chunk: Some(wire_chunk()),
                score: 1.0,
            },
            v1::ScoredChunk {
                chunk: None,
                score: 0.5,
            },
        ],
    };
    assert_missing(
        Vec::<ScoredChunk>::from_proto(wire),
        ("ScoredChunk", "chunk"),
    );
}

#[test]
fn a_retrieve_request_without_query_or_params_is_refused() {
    let wire = v1::RetrieveRequest {
        query: None,
        params: Some(v1::RetrieveParams { top_k: 1 }),
    };
    assert_missing(
        <(Query, RetrieveParams)>::from_proto(wire),
        ("RetrieveRequest", "query"),
    );
    let wire = v1::RetrieveRequest {
        query: Some(wire_query()),
        params: None,
    };
    assert_missing(
        <(Query, RetrieveParams)>::from_proto(wire),
        ("RetrieveRequest", "params"),
    );
}

#[test]
fn a_fuse_request_without_params_is_refused() {
    let wire = v1::FuseRequest {
        inputs: vec![],
        params: None,
    };
    assert_missing(
        <(Vec<Vec<ScoredChunk>>, FusionParams)>::from_proto(wire),
        ("FuseRequest", "params"),
    );
}

#[test]
fn a_rerank_request_without_query_or_params_is_refused() {
    let wire = v1::RerankRequest {
        query: None,
        chunks: vec![],
        params: Some(v1::RerankParams {
            top_k: 1,
            served_model: Some("m".into()),
        }),
    };
    assert_missing(
        <(Query, Vec<ScoredChunk>, RerankParams)>::from_proto(wire),
        ("RerankRequest", "query"),
    );
    let wire = v1::RerankRequest {
        query: Some(wire_query()),
        chunks: vec![],
        params: None,
    };
    assert_missing(
        <(Query, Vec<ScoredChunk>, RerankParams)>::from_proto(wire),
        ("RerankRequest", "params"),
    );
}

#[test]
fn an_embed_request_without_params_is_refused() {
    let wire = v1::EmbedRequest {
        texts: vec!["t".into()],
        params: None,
    };
    assert_missing(
        <(Vec<String>, EmbedParams)>::from_proto(wire),
        ("EmbedRequest", "params"),
    );
}

#[test]
fn a_search_request_without_embedding_or_params_is_refused() {
    let wire = v1::SearchRequest {
        embedding: None,
        params: Some(v1::SearchParams { top_k: 1 }),
    };
    assert_missing(
        <(Embedding, SearchParams)>::from_proto(wire),
        ("SearchRequest", "embedding"),
    );
    let wire = v1::SearchRequest {
        embedding: Some(v1::Embedding {
            components: vec![1.0],
        }),
        params: None,
    };
    assert_missing(
        <(Embedding, SearchParams)>::from_proto(wire),
        ("SearchRequest", "params"),
    );
}

#[test]
fn an_upsert_entry_without_its_embedding_is_refused() {
    let wire = v1::UpsertRequest {
        entries: vec![v1::EmbeddedChunk {
            chunk: Some(wire_chunk()),
            embedding: None,
        }],
    };
    assert_missing(
        Vec::<EmbeddedChunk>::from_proto(wire),
        ("EmbeddedChunk", "embedding"),
    );
}

#[test]
fn an_identity_response_without_its_identity_is_refused() {
    assert_missing(
        ModelIdentity::from_proto(v1::RerankerModelIdentityResponse { identity: None }),
        ("RerankerModelIdentityResponse", "identity"),
    );
    assert_missing(
        ModelIdentity::from_proto(v1::EmbedderModelIdentityResponse { identity: None }),
        ("EmbedderModelIdentityResponse", "identity"),
    );
}

// --- the embedding role: the reserved zero and unknown numbers (ADR-C17) ------

fn embed_params_with_role(role: i32) -> v1::EmbedParams {
    v1::EmbedParams {
        role,
        served_model: None,
    }
}

#[test]
fn embed_role_unspecified_is_refused() {
    let zero = v1::EmbedRole::Unspecified as i32;
    assert_eq!(zero, 0);
    assert!(matches!(
        EmbedParams::from_proto(embed_params_with_role(zero)),
        Err(DecodeError::Unspecified {
            message: "EmbedParams",
            field: "role"
        })
    ));
}

#[test]
fn an_omitted_embed_role_is_refused() {
    // proto3 decodes an omitted enum as its zero: the case the reserved value
    // exists for.
    assert!(matches!(
        EmbedParams::from_proto(v1::EmbedParams::default()),
        Err(DecodeError::Unspecified { .. })
    ));
}

#[test]
fn an_embed_role_the_enum_does_not_name_is_refused() {
    for number in [7, 3, -1, i32::MAX] {
        match EmbedParams::from_proto(embed_params_with_role(number)) {
            Err(DecodeError::Unknown {
                message: "EmbedParams",
                field: "role",
                value,
            }) => assert_eq!(value, number),
            other => panic!("role {number} must be refused as unknown, got {other:?}"),
        }
    }
}

#[test]
fn an_embed_request_carrying_an_unspecified_role_is_refused() {
    let wire = v1::EmbedRequest {
        texts: vec![],
        params: Some(embed_params_with_role(0)),
    };
    assert!(matches!(
        <(Vec<String>, EmbedParams)>::from_proto(wire),
        Err(DecodeError::Unspecified { .. })
    ));
}

// --- an empty identity (ADR-C31 § 1) ------------------------------------------

fn assert_empty<T: std::fmt::Debug>(result: Result<T, DecodeError>, want: (&str, &str)) {
    match result {
        Err(DecodeError::Empty { message, field }) => assert_eq!((message, field), want),
        other => panic!("expected {want:?} to be refused as empty, got {other:?}"),
    }
}

#[test]
fn an_empty_identity_is_refused() {
    assert_empty(
        ModelIdentity::from_proto(v1::ModelIdentity {
            identity: String::new(),
        }),
        ("ModelIdentity", "identity"),
    );
    assert_empty(
        ModelIdentity::from_proto(v1::EmbedderModelIdentityResponse {
            identity: Some(v1::ModelIdentity::default()),
        }),
        ("ModelIdentity", "identity"),
    );
    assert_empty(
        ModelIdentity::from_proto(v1::RerankerModelIdentityResponse {
            identity: Some(v1::ModelIdentity::default()),
        }),
        ("ModelIdentity", "identity"),
    );
}

// --- an empty served_model (ADR-C32 § 4) --------------------------------------

#[test]
fn an_empty_served_model_is_refused() {
    let empty = Some(String::new());
    assert_empty(
        EmbedParams::from_proto(v1::EmbedParams {
            role: v1::EmbedRole::Query as i32,
            served_model: empty.clone(),
        }),
        ("EmbedParams", "served_model"),
    );
    assert_empty(
        RerankParams::from_proto(v1::RerankParams {
            top_k: 1,
            served_model: empty.clone(),
        }),
        ("RerankParams", "served_model"),
    );
    assert_empty(
        Option::<String>::from_proto(v1::EmbedderModelIdentityRequest {
            served_model: empty.clone(),
        }),
        ("EmbedderModelIdentityRequest", "served_model"),
    );
    assert_empty(
        Option::<String>::from_proto(v1::RerankerModelIdentityRequest {
            served_model: empty,
        }),
        ("RerankerModelIdentityRequest", "served_model"),
    );
}

#[test]
fn an_absent_served_model_decodes_as_none() {
    // Absent is a valid domain value on the wire: it is the `Remote` service,
    // and the adapter before sending, that refuse it (ADR-C32 § 4), not the
    // conversion.
    let params = RerankParams::from_proto(v1::RerankParams {
        top_k: 1,
        served_model: None,
    })
    .unwrap();
    assert_eq!(params.served_model, None);
}

// --- a count wider than usize --------------------------------------------------

#[test]
fn a_count_is_carried_whole_or_refused() {
    // On a 64-bit target every u64 fits; the refusal exists for the targets
    // where it does not, and a truncated top_k would be a silent divergence.
    match RetrieveParams::from_proto(v1::RetrieveParams { top_k: u64::MAX }) {
        Ok(params) => assert_eq!(params.top_k as u64, u64::MAX),
        Err(DecodeError::TooLarge { value, .. }) => assert_eq!(value, u64::MAX),
        Err(other) => panic!("unexpected {other:?}"),
    }
}

#[test]
fn chunk_carries_no_required_field_and_always_decodes() {
    // Its fields are scalars: every message decodes.
    assert_eq!(
        Chunk::from_proto(v1::Chunk::default()).unwrap().text,
        String::new()
    );
}

// --- the generation families (ADR-C31 § 1–§ 2) ---------------------------------

fn wire_generate_params() -> v1::GenerateParams {
    v1::GenerateParams {
        served_model: "m".into(),
        template: "{query}".into(),
        ..Default::default()
    }
}

#[test]
fn a_build_request_without_query_or_params_is_refused() {
    let wire = v1::BuildRequest {
        query: None,
        chunks: vec![],
        params: Some(v1::ContextParams { budget: 1 }),
    };
    assert_missing(
        <(Query, Vec<ScoredChunk>, ContextParams)>::from_proto(wire),
        ("BuildRequest", "query"),
    );
    let wire = v1::BuildRequest {
        query: Some(wire_query()),
        chunks: vec![],
        params: None,
    };
    assert_missing(
        <(Query, Vec<ScoredChunk>, ContextParams)>::from_proto(wire),
        ("BuildRequest", "params"),
    );
}

#[test]
fn a_build_request_carrying_a_chunkless_scored_chunk_is_refused() {
    let wire = v1::BuildRequest {
        query: Some(wire_query()),
        chunks: vec![v1::ScoredChunk {
            chunk: None,
            score: 1.0,
        }],
        params: Some(v1::ContextParams { budget: 1 }),
    };
    assert_missing(
        <(Query, Vec<ScoredChunk>, ContextParams)>::from_proto(wire),
        ("ScoredChunk", "chunk"),
    );
}

#[test]
fn a_generate_request_without_query_context_or_params_is_refused() {
    let full = || v1::GenerateRequest {
        query: Some(wire_query()),
        context: Some(v1::Context::default()),
        params: Some(wire_generate_params()),
    };
    assert!(<(Query, Context, GenerateParams)>::from_proto(full()).is_ok());
    for (field, wire) in [
        (
            "query",
            v1::GenerateRequest {
                query: None,
                ..full()
            },
        ),
        (
            "context",
            v1::GenerateRequest {
                context: None,
                ..full()
            },
        ),
        (
            "params",
            v1::GenerateRequest {
                params: None,
                ..full()
            },
        ),
    ] {
        assert_missing(
            <(Query, Context, GenerateParams)>::from_proto(wire),
            ("GenerateRequest", field),
        );
    }
}

#[test]
fn a_response_without_its_context_or_answer_is_refused() {
    assert_missing(
        Context::from_proto(v1::BuildResponse { context: None }),
        ("BuildResponse", "context"),
    );
    assert_missing(
        Answer::from_proto(v1::GenerateResponse { answer: None }),
        ("GenerateResponse", "answer"),
    );
}

#[test]
fn a_generation_identity_response_without_or_with_an_empty_identity_is_refused() {
    assert_missing(
        ModelIdentity::from_proto(v1::GeneratorModelIdentityResponse { identity: None }),
        ("GeneratorModelIdentityResponse", "identity"),
    );
    assert_missing(
        ModelIdentity::from_proto(v1::ContextBuilderModelIdentityResponse { identity: None }),
        ("ContextBuilderModelIdentityResponse", "identity"),
    );
    assert_empty(
        ModelIdentity::from_proto(v1::GeneratorModelIdentityResponse {
            identity: Some(v1::ModelIdentity::default()),
        }),
        ("ModelIdentity", "identity"),
    );
    assert_empty(
        ModelIdentity::from_proto(v1::ContextBuilderModelIdentityResponse {
            identity: Some(v1::ModelIdentity::default()),
        }),
        ("ModelIdentity", "identity"),
    );
}

/// A proto3 `string` has no presence: an omitted `served_model` or `template`
/// decodes as the empty string, and ADR-C31 § 2 has a service refuse it on
/// receipt, as the adapter does before sending.
#[test]
fn a_generator_served_model_or_template_left_empty_is_refused() {
    assert_empty(
        GenerateParams::from_proto(v1::GenerateParams {
            served_model: String::new(),
            ..wire_generate_params()
        }),
        ("GenerateParams", "served_model"),
    );
    assert_empty(
        GenerateParams::from_proto(v1::GenerateParams {
            template: String::new(),
            ..wire_generate_params()
        }),
        ("GenerateParams", "template"),
    );
    assert!(matches!(
        GenerateParams::from_proto(v1::GenerateParams::default()),
        Err(DecodeError::Empty { .. })
    ));
    assert_empty(
        String::from_proto(v1::GeneratorModelIdentityRequest {
            served_model: String::new(),
        }),
        ("GeneratorModelIdentityRequest", "served_model"),
    );
}

#[test]
fn a_generate_request_carrying_an_empty_template_is_refused() {
    let wire = v1::GenerateRequest {
        query: Some(wire_query()),
        context: Some(v1::Context::default()),
        params: Some(v1::GenerateParams {
            template: String::new(),
            ..wire_generate_params()
        }),
    };
    assert_empty(
        <(Query, Context, GenerateParams)>::from_proto(wire),
        ("GenerateParams", "template"),
    );
}

#[test]
fn a_budget_or_max_tokens_is_carried_whole_or_refused() {
    match ContextParams::from_proto(v1::ContextParams { budget: u64::MAX }) {
        Ok(params) => assert_eq!(params.budget as u64, u64::MAX),
        Err(DecodeError::TooLarge { value, .. }) => assert_eq!(value, u64::MAX),
        Err(other) => panic!("unexpected {other:?}"),
    }
    let wire = v1::GenerateParams {
        max_tokens: Some(u64::MAX),
        ..wire_generate_params()
    };
    match GenerateParams::from_proto(wire) {
        Ok(params) => assert_eq!(params.max_tokens.map(|n| n as u64), Some(u64::MAX)),
        Err(DecodeError::TooLarge { value, .. }) => assert_eq!(value, u64::MAX),
        Err(other) => panic!("unexpected {other:?}"),
    }
}

#[test]
fn context_and_answer_carry_no_required_field_and_always_decode() {
    // Their fields are scalars and repeated scalar messages: every message
    // decodes, including a context chunk whose score is not finite, which is
    // the component's contract and not the wire's.
    assert_eq!(
        Context::from_proto(v1::Context::default()).unwrap(),
        Context {
            chunks: vec![],
            text: String::new()
        }
    );
    let chunk = ContextChunk::from_proto(v1::ContextChunk {
        score: f32::NAN,
        ..Default::default()
    })
    .unwrap();
    assert!(chunk.score.is_nan());
    assert_eq!(
        Answer::from_proto(v1::Answer::default()).unwrap().text,
        String::new()
    );
}

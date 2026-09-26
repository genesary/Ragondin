//! Field parity between the generated messages and the values they mirror.
//!
//! No conversion is written here — those are `ragondin-remote`'s. Most tests
//! build a domain value and the message a correct conversion would produce,
//! then compare them field by field. The rest do not: the role-number and
//! presence decode tests pin wire facts,
//! `requests_and_responses_carry_the_trait_arguments` checks message shapes at
//! compile time, and `fuse_request_keeps_the_order_of_its_legs` checks a wire
//! round trip. Every generated message, and every
//! domain struct whose fields are public and that is not `#[non_exhaustive]`,
//! is destructured **exhaustively**, so a field added to one of them and not to
//! its mirror stops this file compiling: the drift is caught here before the
//! round-trip tests of `ragondin-remote` ever run.
//!
//! That guard does **not** cover the params structs of `ragondin-contracts`.
//! They are `#[non_exhaustive]`, so a field added to one on the Rust side
//! compiles here unnoticed; `params_mirror_the_contracts_params` compares only
//! the fields it names.

use prost::Message;
use ragondin_contracts as contracts;
use ragondin_proto::v1;
use ragondin_types as types;

fn a_domain_chunk() -> types::Chunk {
    types::Chunk {
        id: types::ChunkId::new("chunk-1"),
        text: "the cat sat on the mat".to_string(),
        document_id: types::DocId::new("doc-1"),
    }
}

fn a_wire_chunk() -> v1::Chunk {
    v1::Chunk {
        id: "chunk-1".to_string(),
        text: "the cat sat on the mat".to_string(),
        document_id: "doc-1".to_string(),
    }
}

fn assert_chunk_parity(domain: &types::Chunk, wire: &v1::Chunk) {
    let types::Chunk {
        id,
        text,
        document_id,
    } = domain;
    let v1::Chunk {
        id: wire_id,
        text: wire_text,
        document_id: wire_document_id,
    } = wire;
    assert_eq!(id.as_str(), wire_id);
    assert_eq!(text, wire_text);
    assert_eq!(document_id.as_str(), wire_document_id);
}

fn assert_scored_chunk_parity(domain: &types::ScoredChunk, wire: &v1::ScoredChunk) {
    let types::ScoredChunk { chunk, score } = domain;
    let v1::ScoredChunk {
        chunk: wire_chunk,
        score: wire_score,
    } = wire;
    assert_chunk_parity(chunk, wire_chunk.as_ref().expect("chunk is required"));
    assert_eq!(score, wire_score);
}

fn assert_query_parity(domain: &types::Query, wire: &v1::Query) {
    let types::Query { id, text } = domain;
    let v1::Query {
        id: wire_id,
        text: wire_text,
    } = wire;
    assert_eq!(id.as_str(), wire_id);
    assert_eq!(text, wire_text);
}

fn assert_embedding_parity(domain: &types::Embedding, wire: &v1::Embedding) {
    let v1::Embedding { components } = wire;
    assert_eq!(domain.as_slice(), components.as_slice());
}

#[test]
fn query_mirrors_the_domain_query() {
    let domain = types::Query {
        id: types::QueryId::new("query-1"),
        text: "where did the cat sit?".to_string(),
    };
    let wire = v1::Query {
        id: "query-1".to_string(),
        text: "where did the cat sit?".to_string(),
    };
    assert_query_parity(&domain, &wire);
}

#[test]
fn chunk_mirrors_the_domain_chunk() {
    assert_chunk_parity(&a_domain_chunk(), &a_wire_chunk());
}

#[test]
fn scored_chunk_mirrors_the_domain_scored_chunk() {
    let domain = types::ScoredChunk {
        chunk: a_domain_chunk(),
        score: 0.87,
    };
    let wire = v1::ScoredChunk {
        chunk: Some(a_wire_chunk()),
        score: 0.87,
    };
    assert_scored_chunk_parity(&domain, &wire);
}

#[test]
fn embedding_mirrors_the_domain_embedding() {
    let domain = types::Embedding::new(vec![0.1, -0.2, 0.3]);
    let wire = v1::Embedding {
        components: vec![0.1, -0.2, 0.3],
    };
    assert_embedding_parity(&domain, &wire);
}

#[test]
fn embedded_chunk_mirrors_the_contracts_embedded_chunk() {
    let domain = contracts::EmbeddedChunk {
        chunk: a_domain_chunk(),
        embedding: types::Embedding::new(vec![0.5, 0.25]),
    };
    let wire = v1::EmbeddedChunk {
        chunk: Some(a_wire_chunk()),
        embedding: Some(v1::Embedding {
            components: vec![0.5, 0.25],
        }),
    };
    let contracts::EmbeddedChunk { chunk, embedding } = &domain;
    let v1::EmbeddedChunk {
        chunk: wire_chunk,
        embedding: wire_embedding,
    } = &wire;
    assert_chunk_parity(chunk, wire_chunk.as_ref().expect("chunk is required"));
    assert_embedding_parity(
        embedding,
        wire_embedding.as_ref().expect("embedding is required"),
    );
}

fn a_domain_context_chunk() -> types::ContextChunk {
    types::ContextChunk {
        id: types::ChunkId::new("chunk-1"),
        document_id: types::DocId::new("doc-1"),
        score: 0.87,
    }
}

fn a_wire_context_chunk() -> v1::ContextChunk {
    v1::ContextChunk {
        id: "chunk-1".to_string(),
        document_id: "doc-1".to_string(),
        score: 0.87,
    }
}

fn assert_context_chunk_parity(domain: &types::ContextChunk, wire: &v1::ContextChunk) {
    let types::ContextChunk {
        id,
        document_id,
        score,
    } = domain;
    let v1::ContextChunk {
        id: wire_id,
        document_id: wire_document_id,
        score: wire_score,
    } = wire;
    assert_eq!(id.as_str(), wire_id);
    assert_eq!(document_id.as_str(), wire_document_id);
    assert_eq!(score, wire_score);
}

#[test]
fn context_chunk_mirrors_the_domain_context_chunk() {
    assert_context_chunk_parity(&a_domain_context_chunk(), &a_wire_context_chunk());
}

#[test]
fn context_mirrors_the_domain_context() {
    let domain = types::Context {
        chunks: vec![a_domain_context_chunk()],
        text: "[1] the cat sat on the mat".to_string(),
    };
    let wire = v1::Context {
        chunks: vec![a_wire_context_chunk()],
        text: "[1] the cat sat on the mat".to_string(),
    };
    let types::Context { chunks, text } = &domain;
    let v1::Context {
        chunks: wire_chunks,
        text: wire_text,
    } = &wire;
    assert_eq!(chunks.len(), wire_chunks.len());
    for (chunk, wire_chunk) in chunks.iter().zip(wire_chunks) {
        assert_context_chunk_parity(chunk, wire_chunk);
    }
    assert_eq!(text, wire_text);
}

#[test]
fn answer_mirrors_the_domain_answer() {
    let types::Answer { text } = types::Answer {
        text: "on the mat".to_string(),
    };
    let v1::Answer { text: wire_text } = v1::Answer {
        text: "on the mat".to_string(),
    };
    assert_eq!(text, wire_text);
}

/// `ModelIdentity` is a newtype with a private field, so only the message is
/// destructured; its one field is named after `ModelIdentity::new`'s argument.
#[test]
fn model_identity_mirrors_the_domain_model_identity() {
    let domain = types::ModelIdentity::new(r#"{"id":"qwen"}"#);
    let v1::ModelIdentity { identity } = v1::ModelIdentity {
        identity: r#"{"id":"qwen"}"#.to_string(),
    };
    assert_eq!(domain.as_str(), identity);
}

/// The params structs are `#[non_exhaustive]`, so they cannot be destructured
/// from here; their public fields are compared instead, and the generated
/// messages — which are exhaustive — are destructured in full.
#[test]
fn params_mirror_the_contracts_params() {
    let retrieve = contracts::RetrieveParams::new(10);
    let v1::RetrieveParams { top_k } = v1::RetrieveParams { top_k: 10 };
    assert_eq!(retrieve.top_k as u64, top_k);

    let rerank = contracts::RerankParams::new(5).with_served_model("bge-reranker");
    let v1::RerankParams {
        top_k,
        served_model,
    } = v1::RerankParams {
        top_k: 5,
        served_model: Some("bge-reranker".to_string()),
    };
    assert_eq!(rerank.top_k as u64, top_k);
    assert_eq!(rerank.served_model, served_model);

    let search = contracts::SearchParams::new(3);
    let v1::SearchParams { top_k } = v1::SearchParams { top_k: 3 };
    assert_eq!(search.top_k as u64, top_k);

    // Empty on both faces, and mirrored all the same: a future knob is then
    // an added field number rather than a change to the rpc's signature.
    let _ = contracts::FusionParams::new();
    let v1::FusionParams {} = v1::FusionParams {};

    let embed =
        contracts::EmbedParams::new(contracts::EmbedRole::Passage).with_served_model("e5-large");
    let v1::EmbedParams { role, served_model } = v1::EmbedParams {
        role: v1::EmbedRole::Passage as i32,
        served_model: Some("e5-large".to_string()),
    };
    assert_eq!(mirror_role(embed.role) as i32, role);
    assert_eq!(embed.served_model, served_model);

    // No name on either face: absence, never the empty string.
    let unnamed = contracts::EmbedParams::new(contracts::EmbedRole::Query);
    let v1::EmbedParams {
        role: _,
        served_model,
    } = v1::EmbedParams {
        role: v1::EmbedRole::Query as i32,
        served_model: None,
    };
    assert_eq!(unnamed.served_model, served_model);

    let context = contracts::ContextParams::new(512);
    let v1::ContextParams { budget } = v1::ContextParams { budget: 512 };
    assert_eq!(context.budget as u64, budget);

    let generate = contracts::GenerateParams::new("qwen", "{context}\n\n{query}")
        .with_temperature(0.2)
        .with_seed(7)
        .with_max_tokens(256);
    let v1::GenerateParams {
        served_model,
        template,
        temperature,
        seed,
        max_tokens,
    } = v1::GenerateParams {
        served_model: "qwen".to_string(),
        template: "{context}\n\n{query}".to_string(),
        temperature: Some(0.2),
        seed: Some(7),
        max_tokens: Some(256),
    };
    assert_eq!(generate.served_model, served_model);
    assert_eq!(generate.template, template);
    assert_eq!(generate.temperature, temperature);
    assert_eq!(generate.seed, seed);
    assert_eq!(generate.max_tokens.map(|n| n as u64), max_tokens);
}

/// The three optional generation settings and every optional `served_model`
/// carry explicit presence (ADR-C31 § 2, ADR-C32 § 4): an omitted one decodes
/// as `None`, never as zero or the empty string. A plain proto3 `double`
/// would decode an omitted temperature as `0.0`, which is greedy decoding —
/// a setting nobody chose.
#[test]
fn an_omitted_optional_decodes_as_none() {
    let bytes = v1::GenerateParams {
        served_model: "qwen".to_string(),
        template: "{query}".to_string(),
        temperature: None,
        seed: None,
        max_tokens: None,
    }
    .encode_to_vec();
    let decoded = v1::GenerateParams::decode(bytes.as_slice()).expect("decodes");
    assert_eq!(decoded.temperature, None);
    assert_eq!(decoded.seed, None);
    assert_eq!(decoded.max_tokens, None);

    let embed = v1::EmbedParams::decode(
        v1::EmbedParams {
            role: v1::EmbedRole::Query as i32,
            served_model: None,
        }
        .encode_to_vec()
        .as_slice(),
    )
    .expect("decodes");
    assert_eq!(embed.served_model, None);

    let rerank = v1::RerankParams::decode(
        v1::RerankParams {
            top_k: 5,
            served_model: None,
        }
        .encode_to_vec()
        .as_slice(),
    )
    .expect("decodes");
    assert_eq!(rerank.served_model, None);

    let embedder_identity = v1::EmbedderModelIdentityRequest::decode(&[][..]).expect("decodes");
    assert_eq!(embedder_identity.served_model, None);
    let reranker_identity = v1::RerankerModelIdentityRequest::decode(&[][..]).expect("decodes");
    assert_eq!(reranker_identity.served_model, None);
}

/// The other half of presence: a value that is present and zero survives the
/// wire as that value. A temperature of zero asked for is not the same call as
/// no temperature.
#[test]
fn a_present_zero_decodes_as_that_zero() {
    let bytes = v1::GenerateParams {
        served_model: "qwen".to_string(),
        template: "{query}".to_string(),
        temperature: Some(0.0),
        seed: Some(0),
        max_tokens: Some(0),
    }
    .encode_to_vec();
    let decoded = v1::GenerateParams::decode(bytes.as_slice()).expect("decodes");
    assert_eq!(decoded.temperature, Some(0.0));
    assert_eq!(decoded.seed, Some(0));
    assert_eq!(decoded.max_tokens, Some(0));

    let embed = v1::EmbedParams::decode(
        v1::EmbedParams {
            role: v1::EmbedRole::Query as i32,
            served_model: Some(String::new()),
        }
        .encode_to_vec()
        .as_slice(),
    )
    .expect("decodes");
    assert_eq!(embed.served_model, Some(String::new()));
}

/// The generator's two required strings have no presence, the opposite shape
/// (ADR-C31 § 2): an omitted one decodes as the empty string, which is why an
/// empty `served_model` or `template` is an invalid request rather than an
/// absent one. This pins the wire fact; the refusal is the component's.
#[test]
fn an_omitted_required_string_decodes_as_empty() {
    let decoded = v1::GenerateParams::decode(&[][..]).expect("decodes");
    assert_eq!(decoded.served_model, "");
    assert_eq!(decoded.template, "");
    let identity = v1::GeneratorModelIdentityRequest::decode(&[][..]).expect("decodes");
    assert_eq!(identity.served_model, "");
}

/// Exhaustive over the closed Rust enum: a role added there and not here stops
/// this compiling.
fn mirror_role(role: contracts::EmbedRole) -> v1::EmbedRole {
    match role {
        contracts::EmbedRole::Query => v1::EmbedRole::Query,
        contracts::EmbedRole::Passage => v1::EmbedRole::Passage,
    }
}

#[test]
fn every_rust_role_has_a_distinct_nonzero_wire_number() {
    let query = mirror_role(contracts::EmbedRole::Query) as i32;
    let passage = mirror_role(contracts::EmbedRole::Passage) as i32;
    assert_eq!(query, 1);
    assert_eq!(passage, 2);
}

/// The zero is reserved (ADR-C17): an `EmbedParams` whose role was never set
/// decodes as `EMBED_ROLE_UNSPECIFIED`, a value no Rust variant mirrors. This
/// pins the wire fact the `Remote` adapter's refusal rests on; the refusal
/// itself is the adapter's, and so is its test.
#[test]
fn an_omitted_role_decodes_as_unspecified() {
    let bytes = v1::EmbedParams::default().encode_to_vec();
    let decoded = v1::EmbedParams::decode(bytes.as_slice()).expect("decodes");
    assert_eq!(decoded.role, 0);
    assert_eq!(
        v1::EmbedRole::try_from(decoded.role),
        Ok(v1::EmbedRole::Unspecified)
    );
}

/// A proto3 enum is open on the wire: a number the `.proto` does not name
/// still decodes, and only the attempt to name it fails. The Rust enum is
/// closed, so this is the second value the adapter must refuse.
#[test]
fn an_unknown_role_number_decodes_and_names_no_role() {
    // Field 1, varint wire type, value 7.
    let bytes = [0x08, 0x07];
    let decoded = v1::EmbedParams::decode(bytes.as_slice()).expect("decodes");
    assert_eq!(decoded.role, 7);
    assert!(v1::EmbedRole::try_from(decoded.role).is_err());
}

/// Its value is at compile time, as in `tests/stubs.rs`: each request and
/// response is destructured exhaustively, so a message that gains, loses or
/// renames a field stops this file compiling. The run itself asserts nothing.
#[test]
fn requests_and_responses_carry_the_trait_arguments() {
    let query = v1::Query {
        id: "query-1".to_string(),
        text: "where did the cat sit?".to_string(),
    };
    let scored = v1::ScoredChunk {
        chunk: Some(a_wire_chunk()),
        score: 0.5,
    };

    // Retriever::retrieve(query, params) -> Vec<ScoredChunk>
    let v1::RetrieveRequest {
        query: _,
        params: _,
    } = v1::RetrieveRequest {
        query: Some(query.clone()),
        params: Some(v1::RetrieveParams { top_k: 10 }),
    };
    let v1::RetrieveResponse { chunks: _ } = v1::RetrieveResponse {
        chunks: vec![scored.clone()],
    };

    // Reranker::rerank(query, chunks, params) -> Vec<ScoredChunk>
    let v1::RerankRequest {
        query: _,
        chunks: _,
        params: _,
    } = v1::RerankRequest {
        query: Some(query.clone()),
        chunks: vec![scored.clone()],
        params: Some(v1::RerankParams {
            top_k: 5,
            served_model: None,
        }),
    };
    let v1::RerankResponse { chunks: _ } = v1::RerankResponse {
        chunks: vec![scored.clone()],
    };

    let identity = v1::ModelIdentity {
        identity: "model@rev".to_string(),
    };

    // Reranker::model_identity(served_model) -> ModelIdentity
    let v1::RerankerModelIdentityRequest { served_model: _ } = v1::RerankerModelIdentityRequest {
        served_model: Some("bge-reranker".to_string()),
    };
    let v1::RerankerModelIdentityResponse { identity: _ } = v1::RerankerModelIdentityResponse {
        identity: Some(identity.clone()),
    };

    // Embedder::embed(texts, params) -> Vec<Embedding>
    let v1::EmbedRequest {
        texts: _,
        params: _,
    } = v1::EmbedRequest {
        texts: vec!["a passage".to_string()],
        params: Some(v1::EmbedParams {
            role: v1::EmbedRole::Passage as i32,
            served_model: None,
        }),
    };
    let v1::EmbedResponse { embeddings: _ } = v1::EmbedResponse {
        embeddings: vec![v1::Embedding {
            components: vec![1.0],
        }],
    };

    // Embedder::model_identity(served_model) -> ModelIdentity
    let v1::EmbedderModelIdentityRequest { served_model: _ } = v1::EmbedderModelIdentityRequest {
        served_model: Some("e5-large".to_string()),
    };
    let v1::EmbedderModelIdentityResponse { identity: _ } = v1::EmbedderModelIdentityResponse {
        identity: Some(identity.clone()),
    };

    let context = v1::Context {
        chunks: vec![a_wire_context_chunk()],
        text: "[1] the cat sat on the mat".to_string(),
    };

    // ContextBuilder::build(query, chunks, params) -> Context
    let v1::BuildRequest {
        query: _,
        chunks: _,
        params: _,
    } = v1::BuildRequest {
        query: Some(query.clone()),
        chunks: vec![scored.clone()],
        params: Some(v1::ContextParams { budget: 512 }),
    };
    let v1::BuildResponse { context: _ } = v1::BuildResponse {
        context: Some(context.clone()),
    };

    // ContextBuilder::model_identity() -> ModelIdentity
    let v1::ContextBuilderModelIdentityRequest {} = v1::ContextBuilderModelIdentityRequest {};
    let v1::ContextBuilderModelIdentityResponse { identity: _ } =
        v1::ContextBuilderModelIdentityResponse {
            identity: Some(identity.clone()),
        };

    // Generator::generate(query, context, params) -> Answer
    let v1::GenerateRequest {
        query: _,
        context: _,
        params: _,
    } = v1::GenerateRequest {
        query: Some(query),
        context: Some(context),
        params: Some(v1::GenerateParams {
            served_model: "qwen".to_string(),
            template: "{context}\n\n{query}".to_string(),
            temperature: None,
            seed: None,
            max_tokens: None,
        }),
    };
    let v1::GenerateResponse { answer: _ } = v1::GenerateResponse {
        answer: Some(v1::Answer {
            text: "on the mat".to_string(),
        }),
    };

    // Generator::model_identity(served_model) -> ModelIdentity
    let v1::GeneratorModelIdentityRequest { served_model: _ } = v1::GeneratorModelIdentityRequest {
        served_model: "qwen".to_string(),
    };
    let v1::GeneratorModelIdentityResponse { identity: _ } = v1::GeneratorModelIdentityResponse {
        identity: Some(identity),
    };

    // VectorStore::upsert(entries) -> ()
    let v1::UpsertRequest { entries: _ } = v1::UpsertRequest {
        entries: vec![v1::EmbeddedChunk {
            chunk: Some(a_wire_chunk()),
            embedding: Some(v1::Embedding {
                components: vec![1.0],
            }),
        }],
    };
    let v1::UpsertResponse {} = v1::UpsertResponse {};

    // VectorStore::search(embedding, params) -> Vec<ScoredChunk>
    let v1::SearchRequest {
        embedding: _,
        params: _,
    } = v1::SearchRequest {
        embedding: Some(v1::Embedding {
            components: vec![1.0],
        }),
        params: Some(v1::SearchParams { top_k: 3 }),
    };
    let v1::SearchResponse { chunks: _ } = v1::SearchResponse {
        chunks: vec![scored],
    };
}

/// `Fusion::fuse` takes `Vec<Vec<ScoredChunk>>`, and proto3 has no repeated
/// repeated field: each leg is wrapped in a `ScoredChunkList`. The legs'
/// order is the pipeline's wiring order and is significant, so it must survive
/// the wire.
#[test]
fn fuse_request_keeps_the_order_of_its_legs() {
    let leg = |id: &str| v1::ScoredChunkList {
        chunks: vec![v1::ScoredChunk {
            chunk: Some(v1::Chunk {
                id: id.to_string(),
                text: String::new(),
                document_id: "doc-1".to_string(),
            }),
            score: 1.0,
        }],
    };
    let request = v1::FuseRequest {
        inputs: vec![leg("from-bm25"), leg("from-dense")],
        params: Some(v1::FusionParams {}),
    };
    let decoded = v1::FuseRequest::decode(request.encode_to_vec().as_slice()).expect("decodes");
    let v1::FuseRequest { inputs, params } = decoded;
    assert!(params.is_some());
    let first_ids: Vec<&str> = inputs
        .iter()
        .map(|list| {
            let v1::ScoredChunkList { chunks } = list;
            chunks[0].chunk.as_ref().expect("chunk").id.as_str()
        })
        .collect();
    assert_eq!(first_ids, ["from-bm25", "from-dense"]);

    let v1::FuseResponse { chunks: _ } = v1::FuseResponse { chunks: vec![] };
}

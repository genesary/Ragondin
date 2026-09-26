//! Field parity between the generated messages and the values they mirror.
//!
//! No conversion is written here — those are `ragondin-remote`'s. Each test
//! builds a domain value and the message a correct conversion would produce,
//! then compares them field by field. Every generated message, and every
//! domain struct whose fields are public and that is not `#[non_exhaustive]`,
//! is destructured **exhaustively**, so a field added to one of them and not to
//! its mirror stops this file compiling: the drift is caught here before the
//! round-trip tests of `ragondin-remote` ever run.

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

/// The params structs are `#[non_exhaustive]`, so they cannot be destructured
/// from here; their public fields are compared instead, and the generated
/// messages — which are exhaustive — are destructured in full.
#[test]
fn params_mirror_the_contracts_params() {
    let retrieve = contracts::RetrieveParams::new(10);
    let v1::RetrieveParams { top_k } = v1::RetrieveParams { top_k: 10 };
    assert_eq!(retrieve.top_k as u64, top_k);

    let rerank = contracts::RerankParams::new(5);
    let v1::RerankParams { top_k } = v1::RerankParams { top_k: 5 };
    assert_eq!(rerank.top_k as u64, top_k);

    let search = contracts::SearchParams::new(3);
    let v1::SearchParams { top_k } = v1::SearchParams { top_k: 3 };
    assert_eq!(search.top_k as u64, top_k);

    // Empty on both faces, and mirrored all the same: a future knob is then
    // an added field number rather than a change to the rpc's signature.
    let _ = contracts::FusionParams::new();
    let v1::FusionParams {} = v1::FusionParams {};

    let embed = contracts::EmbedParams::new(contracts::EmbedRole::Passage);
    let v1::EmbedParams { role } = v1::EmbedParams {
        role: v1::EmbedRole::Passage as i32,
    };
    assert_eq!(mirror_role(embed.role) as i32, role);
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
        query: Some(query),
        chunks: vec![scored.clone()],
        params: Some(v1::RerankParams { top_k: 5 }),
    };
    let v1::RerankResponse { chunks: _ } = v1::RerankResponse {
        chunks: vec![scored.clone()],
    };

    // Embedder::embed(texts, params) -> Vec<Embedding>
    let v1::EmbedRequest {
        texts: _,
        params: _,
    } = v1::EmbedRequest {
        texts: vec!["a passage".to_string()],
        params: Some(v1::EmbedParams {
            role: v1::EmbedRole::Passage as i32,
        }),
    };
    let v1::EmbedResponse { embeddings: _ } = v1::EmbedResponse {
        embeddings: vec![v1::Embedding {
            components: vec![1.0],
        }],
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

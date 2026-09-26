//! The round-trip property: for every domain value `x` that crosses the wire,
//! `from_proto(to_proto(x)) == x` (ADR-C24, `docs/code-architecture.md` §7.2).
//!
//! This is what keeps the two faces of the contract in step. A field added to
//! a domain type and not carried by its conversion comes back missing, and the
//! value compares unequal.
//!
//! The values come from [`Gen`], a seeded xorshift generator written here
//! rather than `proptest`, which is not a workspace dependency (see
//! `ARCHITECTURE.md` § Tests). It is deterministic, so a failure
//! names the seed and case that reproduce it. Each value also crosses the
//! wire as bytes, `prost`-encoded and decoded, so the property covers what a
//! `Remote` service actually receives and not only the in-memory message.
//!
//! The generator draws the edges on purpose: empty collections, the empty
//! string, non-ASCII text, `-0.0` and the extreme finite floats, and `None`
//! and `Some` for every optional. It never draws a value the conversion
//! refuses, since the property starts from a valid domain value: an empty
//! `served_model` and an empty `ModelIdentity` are representable and not
//! valid, and their refusal is in `negative_decode.rs`.

use std::fmt::Debug;

use prost::Message;
use ragondin_contracts::{
    EmbedParams, EmbedRole, EmbeddedChunk, FusionParams, RerankParams, RetrieveParams, SearchParams,
};
use ragondin_proto::v1;
use ragondin_remote::{FromProto, IntoProto};
use ragondin_types::{
    Chunk, ChunkId, DocId, Embedding, ModelIdentity, Query, QueryId, ScoredChunk,
};

/// How many values of each type the property is checked over.
const CASES: u64 = 500;

/// A seeded xorshift64* generator over the domain types' fields.
struct Gen(u64);

impl Gen {
    fn new(seed: u64) -> Self {
        // xorshift has a fixed point at zero.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn coin(&mut self) -> bool {
        self.below(2) == 0
    }

    /// Text built from pieces that exercise UTF-8: ASCII, accents, CJK, an
    /// emoji outside the BMP, a combining mark, a NUL, a byte-order mark, a
    /// newline. Empty about one time in five.
    fn text(&mut self) -> String {
        const PIECES: &[&str] = &[
            "a",
            "the cat",
            " ",
            "é",
            "日本語",
            "🦝",
            "e\u{301}",
            "\0",
            "\u{FEFF}",
            "\n",
            "query: ",
        ];
        let len = if self.below(5) == 0 { 0 } else { self.below(6) };
        (0..len)
            .map(|_| PIECES[self.below(PIECES.len() as u64) as usize])
            .collect()
    }

    fn non_empty_text(&mut self) -> String {
        let text = self.text();
        if text.is_empty() {
            "m".to_string()
        } else {
            text
        }
    }

    /// A finite `f32`: an edge value half the time, any finite bit pattern the
    /// other half. Non-finite values are refused by the contracts' ranking
    /// rule, and `NaN` has no equality to round-trip under.
    fn float(&mut self) -> f32 {
        const EDGES: &[f32] = &[
            0.0,
            -0.0,
            1.0,
            -1.0,
            f32::MAX,
            f32::MIN,
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
            f32::EPSILON,
            1.0e-45, // the smallest subnormal
        ];
        if self.coin() {
            return EDGES[self.below(EDGES.len() as u64) as usize];
        }
        loop {
            let value = f32::from_bits(self.next() as u32);
            if value.is_finite() {
                return value;
            }
        }
    }

    /// A count: `0`, `1` and `usize::MAX` among the draws. Zero is a valid
    /// domain value; refusing it is the component's business, not the wire's.
    fn count(&mut self) -> usize {
        match self.below(4) {
            0 => 0,
            1 => 1,
            2 => usize::MAX,
            _ => self.next() as usize,
        }
    }

    fn vec<T>(&mut self, mut item: impl FnMut(&mut Self) -> T) -> Vec<T> {
        let len = if self.below(4) == 0 { 0 } else { self.below(5) };
        (0..len).map(|_| item(self)).collect()
    }

    fn served_model(&mut self) -> Option<String> {
        self.coin().then(|| self.non_empty_text())
    }

    fn query(&mut self) -> Query {
        Query {
            id: QueryId::new(self.text()),
            text: self.text(),
        }
    }

    fn chunk(&mut self) -> Chunk {
        Chunk {
            id: ChunkId::new(self.text()),
            text: self.text(),
            document_id: DocId::new(self.text()),
        }
    }

    fn scored_chunk(&mut self) -> ScoredChunk {
        ScoredChunk {
            chunk: self.chunk(),
            score: self.float(),
        }
    }

    fn scored_chunks(&mut self) -> Vec<ScoredChunk> {
        self.vec(Self::scored_chunk)
    }

    fn embedding(&mut self) -> Embedding {
        Embedding::new(self.vec(Self::float))
    }

    fn embedded_chunk(&mut self) -> EmbeddedChunk {
        EmbeddedChunk {
            chunk: self.chunk(),
            embedding: self.embedding(),
        }
    }

    fn identity(&mut self) -> ModelIdentity {
        ModelIdentity::new(self.non_empty_text())
    }

    fn role(&mut self) -> EmbedRole {
        if self.coin() {
            EmbedRole::Query
        } else {
            EmbedRole::Passage
        }
    }

    fn embed_params(&mut self) -> EmbedParams {
        let params = EmbedParams::new(self.role());
        match self.served_model() {
            Some(name) => params.with_served_model(name),
            None => params,
        }
    }

    fn rerank_params(&mut self) -> RerankParams {
        let params = RerankParams::new(self.count());
        match self.served_model() {
            Some(name) => params.with_served_model(name),
            None => params,
        }
    }
}

/// Checks the property for `CASES` values drawn by `draw`, each converted to
/// `P`, encoded to bytes, decoded, and converted back.
///
/// Equality is the domain's `PartialEq`, which is ADR-C24's property. For a
/// finite float that is equality of bits except for the sign of zero, which a
/// score does not keep on the wire: see `a_negative_zero_score_arrives_as_zero`.
fn assert_round_trips<D, P>(name: &str, seed: u64, mut draw: impl FnMut(&mut Gen) -> D)
where
    D: Clone + PartialEq + Debug + IntoProto<P> + FromProto<P>,
    P: Message + Default,
{
    let mut gen = Gen::new(seed);
    for case in 0..CASES {
        let value = draw(&mut gen);
        let proto: P = value.clone().into_proto();
        let bytes = proto.encode_to_vec();
        let decoded = P::decode(bytes.as_slice())
            .unwrap_or_else(|e| panic!("{name}, seed {seed}, case {case}: bytes decode: {e}"));
        let back = D::from_proto(decoded).unwrap_or_else(|e| {
            panic!("{name}, seed {seed}, case {case}: {value:?} did not convert back: {e}")
        });
        assert_eq!(back, value, "{name}, seed {seed}, case {case}");
    }
}

// --- values ---------------------------------------------------------------

#[test]
fn query_round_trips() {
    assert_round_trips::<Query, v1::Query>("Query", 1, Gen::query);
}

#[test]
fn chunk_round_trips() {
    assert_round_trips::<Chunk, v1::Chunk>("Chunk", 2, Gen::chunk);
}

#[test]
fn scored_chunk_round_trips() {
    assert_round_trips::<ScoredChunk, v1::ScoredChunk>("ScoredChunk", 3, Gen::scored_chunk);
}

#[test]
fn a_ranked_list_round_trips() {
    assert_round_trips::<Vec<ScoredChunk>, v1::ScoredChunkList>(
        "ScoredChunkList",
        4,
        Gen::scored_chunks,
    );
}

#[test]
fn embedding_round_trips() {
    assert_round_trips::<Embedding, v1::Embedding>("Embedding", 5, Gen::embedding);
}

#[test]
fn embedded_chunk_round_trips() {
    assert_round_trips::<EmbeddedChunk, v1::EmbeddedChunk>("EmbeddedChunk", 6, Gen::embedded_chunk);
}

#[test]
fn model_identity_round_trips() {
    assert_round_trips::<ModelIdentity, v1::ModelIdentity>("ModelIdentity", 7, Gen::identity);
}

// --- params -----------------------------------------------------------------

#[test]
fn retrieve_params_round_trip() {
    assert_round_trips::<RetrieveParams, v1::RetrieveParams>("RetrieveParams", 8, |g| {
        RetrieveParams::new(g.count())
    });
}

#[test]
fn fusion_params_round_trip() {
    assert_round_trips::<FusionParams, v1::FusionParams>("FusionParams", 9, |_| {
        FusionParams::new()
    });
}

#[test]
fn rerank_params_round_trip() {
    assert_round_trips::<RerankParams, v1::RerankParams>("RerankParams", 10, Gen::rerank_params);
}

#[test]
fn embed_params_round_trip() {
    assert_round_trips::<EmbedParams, v1::EmbedParams>("EmbedParams", 11, Gen::embed_params);
}

#[test]
fn search_params_round_trip() {
    assert_round_trips::<SearchParams, v1::SearchParams>("SearchParams", 12, |g| {
        SearchParams::new(g.count())
    });
}

// --- requests: a trait method's arguments -----------------------------------

#[test]
fn retrieve_request_round_trips() {
    assert_round_trips::<(Query, RetrieveParams), v1::RetrieveRequest>(
        "RetrieveRequest",
        13,
        |g| (g.query(), RetrieveParams::new(g.count())),
    );
}

#[test]
fn fuse_request_round_trips() {
    assert_round_trips::<(Vec<Vec<ScoredChunk>>, FusionParams), v1::FuseRequest>(
        "FuseRequest",
        14,
        |g| (g.vec(Gen::scored_chunks), FusionParams::new()),
    );
}

#[test]
fn rerank_request_round_trips() {
    assert_round_trips::<(Query, Vec<ScoredChunk>, RerankParams), v1::RerankRequest>(
        "RerankRequest",
        15,
        |g| (g.query(), g.scored_chunks(), g.rerank_params()),
    );
}

#[test]
fn embed_request_round_trips() {
    assert_round_trips::<(Vec<String>, EmbedParams), v1::EmbedRequest>("EmbedRequest", 16, |g| {
        (g.vec(Gen::text), g.embed_params())
    });
}

#[test]
fn upsert_request_round_trips() {
    assert_round_trips::<Vec<EmbeddedChunk>, v1::UpsertRequest>("UpsertRequest", 17, |g| {
        g.vec(Gen::embedded_chunk)
    });
}

#[test]
fn search_request_round_trips() {
    assert_round_trips::<(Embedding, SearchParams), v1::SearchRequest>("SearchRequest", 18, |g| {
        (g.embedding(), SearchParams::new(g.count()))
    });
}

#[test]
fn reranker_identity_request_round_trips() {
    assert_round_trips::<Option<String>, v1::RerankerModelIdentityRequest>(
        "RerankerModelIdentityRequest",
        19,
        Gen::served_model,
    );
}

#[test]
fn embedder_identity_request_round_trips() {
    assert_round_trips::<Option<String>, v1::EmbedderModelIdentityRequest>(
        "EmbedderModelIdentityRequest",
        20,
        Gen::served_model,
    );
}

// --- responses: a trait method's return value -------------------------------

#[test]
fn retrieve_response_round_trips() {
    assert_round_trips::<Vec<ScoredChunk>, v1::RetrieveResponse>(
        "RetrieveResponse",
        21,
        Gen::scored_chunks,
    );
}

#[test]
fn fuse_response_round_trips() {
    assert_round_trips::<Vec<ScoredChunk>, v1::FuseResponse>(
        "FuseResponse",
        22,
        Gen::scored_chunks,
    );
}

#[test]
fn rerank_response_round_trips() {
    assert_round_trips::<Vec<ScoredChunk>, v1::RerankResponse>(
        "RerankResponse",
        23,
        Gen::scored_chunks,
    );
}

#[test]
fn embed_response_round_trips() {
    assert_round_trips::<Vec<Embedding>, v1::EmbedResponse>("EmbedResponse", 24, |g| {
        g.vec(Gen::embedding)
    });
}

#[test]
fn upsert_response_round_trips() {
    assert_round_trips::<(), v1::UpsertResponse>("UpsertResponse", 25, |_| ());
}

#[test]
fn search_response_round_trips() {
    assert_round_trips::<Vec<ScoredChunk>, v1::SearchResponse>(
        "SearchResponse",
        26,
        Gen::scored_chunks,
    );
}

#[test]
fn reranker_identity_response_round_trips() {
    assert_round_trips::<ModelIdentity, v1::RerankerModelIdentityResponse>(
        "RerankerModelIdentityResponse",
        27,
        Gen::identity,
    );
}

#[test]
fn embedder_identity_response_round_trips() {
    assert_round_trips::<ModelIdentity, v1::EmbedderModelIdentityResponse>(
        "EmbedderModelIdentityResponse",
        28,
        Gen::identity,
    );
}

// --- the one bit the wire does not keep --------------------------------------

/// A proto3 scalar equal to its default is not encoded, and `-0.0 == 0.0`, so
/// `prost` leaves a score of `-0.0` off the wire and the other side reads
/// `0.0`. The round trip holds under `PartialEq`, ADR-C24's property, and a
/// list keeps its order, since order is position and not score. But the sign
/// is lost: `total_cmp`, which the rankings in this tree sort with, separates
/// `-0.0` from `0.0`, so a tie-break downstream of a `Remote` component could
/// differ from an all-`Local` composition. Pinned here so that it is known
/// rather than rediscovered. An embedding's components are a packed repeated
/// field, which encodes every element, and keep the sign.
#[test]
fn a_negative_zero_score_arrives_as_zero() {
    let chunk = ScoredChunk {
        chunk: Gen::new(1).chunk(),
        score: -0.0,
    };
    let proto: v1::ScoredChunk = chunk.clone().into_proto();
    let decoded = v1::ScoredChunk::decode(proto.encode_to_vec().as_slice()).unwrap();
    let back = ScoredChunk::from_proto(decoded).unwrap();
    assert_eq!(back, chunk);
    assert!(back.score.is_sign_positive());

    let embedding = Embedding::new(vec![-0.0]);
    let proto: v1::Embedding = embedding.into_proto();
    let decoded = v1::Embedding::decode(proto.encode_to_vec().as_slice()).unwrap();
    let back = Embedding::from_proto(decoded).unwrap();
    assert!(back.as_slice()[0].is_sign_negative());
}

// --- the generator reaches its edges -----------------------------------------

/// A generator that never drew the edges would make every test above pass
/// vacuously on them. Checked over the same case count the tests use.
#[test]
fn the_generator_draws_its_edges() {
    let mut gen = Gen::new(99);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..CASES {
        if gen.scored_chunks().is_empty() {
            seen.insert("empty collection");
        }
        let text = gen.text();
        if text.is_empty() {
            seen.insert("empty text");
        }
        if !text.is_ascii() {
            seen.insert("non-ASCII text");
        }
        let x = gen.float();
        if x == 0.0 && x.is_sign_negative() {
            seen.insert("-0.0");
        }
        if x == f32::MAX {
            seen.insert("f32::MAX");
        }
        if x == f32::MIN {
            seen.insert("f32::MIN");
        }
        if x != 0.0 && x.is_subnormal() {
            seen.insert("subnormal");
        }
        if gen.count() == usize::MAX {
            seen.insert("usize::MAX");
        }
        match gen.served_model() {
            None => seen.insert("None"),
            Some(_) => seen.insert("Some"),
        };
        match gen.role() {
            EmbedRole::Query => seen.insert("Query"),
            EmbedRole::Passage => seen.insert("Passage"),
        };
    }
    let wanted = [
        "empty collection",
        "empty text",
        "non-ASCII text",
        "-0.0",
        "f32::MAX",
        "f32::MIN",
        "subnormal",
        "usize::MAX",
        "None",
        "Some",
        "Query",
        "Passage",
    ];
    let missed: Vec<_> = wanted.iter().filter(|w| !seen.contains(*w)).collect();
    assert!(missed.is_empty(), "the generator never drew {missed:?}");
}

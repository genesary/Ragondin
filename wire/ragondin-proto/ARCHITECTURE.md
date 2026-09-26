# ARCHITECTURE — ragondin-proto

**Status: not an INV-1 boundary, and still a surface outsiders depend on.**
What a `Remote` author depends on is the `.proto` text, not this crate's Rust
API. ADR-C21 places it that way: its compatibility rule is the wire format's
own versioning, kept independent of the in-memory types, and tying it to INV-1
would couple the two again. A change to the `.proto` files is a change to
anything under `wire/` that a `Remote` service compiles against, so a choice
there that the documents do not settle escalates (`AGENTS.md` § Rules of
engagement).

## What lives here

Face 2 of the two-faced component contract (ADR-3): the protobuf services a
`Remote` component implements, in any language. Also the configuration-delivery
service, reserved.

| Piece | Role |
|---|---|
| `proto/ragondin/v1/*.proto` | The component services and the values they exchange, hand-maintained (ADR-C24) |
| `proto/ragondin/config/v1/config.proto` | The configuration-delivery service, reserved and empty |
| `build.rs` | Compiles the files above and generates the Rust stubs (ADR-C34) |
| `src/lib.rs` | Includes the generated code, as the `v1` and `config::v1` modules |

**Not here.** The conversions between a domain value and its message, and the
`Remote` adapters that call the generated clients, are `ragondin-remote`'s. The
configuration-delivery messages and rpcs, and anything that serves or calls
them, arrive with the `Stream` configuration source in M6.

## File layout

```
proto/
  ragondin/v1/
    types.proto            Query, Chunk, ScoredChunk, ScoredChunkList, Embedding, EmbeddedChunk,
                           ContextChunk, Context, Answer, ModelIdentity
    retriever.proto        service Retriever      + RetrieveParams, RetrieveRequest, RetrieveResponse
    fusion.proto           service Fusion         + FusionParams, FuseRequest, FuseResponse
    reranker.proto         service Reranker       + RerankParams, Rerank*, RerankerModelIdentity*
    embedder.proto         service Embedder       + EmbedRole, EmbedParams, Embed*, EmbedderModelIdentity*
    vector_store.proto     service VectorStore    + SearchParams, Upsert*/Search* messages
    context_builder.proto  service ContextBuilder + ContextParams, Build*, ContextBuilderModelIdentity*
    generator.proto        service Generator      + GenerateParams, Generate*, GeneratorModelIdentity*
  ragondin/config/v1/
    config.proto         service ConfigDelivery (no rpc)
```

One file per service, beside one file of shared values. A `Remote` author
implementing one family reads two files, and a service added later is a new
file rather than an edit to a shared one. The directory path matches the
package, the convention `protoc` users and `buf lint` expect of an import
path.

## Package and versioning

- **Component services: package `ragondin.v1`**, generated into
  `ragondin_proto::v1`. **Configuration delivery: package
  `ragondin.config.v1`**, generated into `ragondin_proto::config::v1`. The Rust
  module path follows the package, so a reader finds the generated type for a
  message by its fully qualified protobuf name.
- **The version is in the package, and it is the wire's own**, independent of
  the crate's version and of the Rust types it mirrors. Within `v1`, a change
  is additive: a new field takes a new number, and a field number or enum value
  once published is never reused or renumbered.
- The two packages version separately, because they have different readers: a
  component author and a data-plane controller.

## How the stubs are generated

`build.rs` compiles the files listed in its `FILES` to a `FileDescriptorSet`
with `protox`, then hands the set to `tonic_build::configure().compile_fds(..)`
(ADR-C34). **No `protoc` is used or looked up**, so `cargo build` from a clone
needs nothing but Cargo — which matters because `ragondin-config` depends on
this crate and `bin/ragondin` on `ragondin-config`. Both tools are
build-dependencies of this crate only; nothing they bring is linked into a
binary. `tonic-build` generates with its defaults: messages, a server trait and
a client for every service.

`FILES` lists every file explicitly. `protox` includes every imported file in
the descriptor set, and `tonic-build` generates code for every file in the set,
so a file left off `FILES` but imported by one on it is still generated. A file
left off it and imported by nothing is neither compiled nor generated, and
nothing says so. A new file is therefore added to `FILES` in the same diff,
with a line in `tests/stubs.rs` naming its generated server and client, which
fails to compile until the file is generated.

Generated code is never committed: it lives in `OUT_DIR` and is included by
`tonic::include_proto!`. Proto comments become the generated items' rustdoc,
so they are written as plain prose: no bracketed text and no indented lines,
which rustdoc would read as a link or as a doctest.

Clippy lints the generated code, since it is included into this crate. Two
lints are allowed on the generated modules only, each for a shape `tonic`
chose and not this crate: `result_large_err` on `v1`, because every rpc returns
`Result<_, tonic::Status>`; and `match_single_binding` on `config::v1`,
because a service with no rpc routes every path to one arm.

The generators bring a second `prettyplease` into the lock file: 0.2, used by
`tonic-build` 0.12 and by `prost-build` 0.13, beside the 0.3 already there
through `bon-macros` (tantivy's builder macros). Both copies are build-time
only, one in a build script and one in a proc macro, so neither is linked into
a binary. `deny.toml` sets `multiple-versions = "warn"`, so `just check-deny`
reports the duplicate as a warning and passes.

## Local invariants

- **The mirror is field for field, and hand-maintained (ADR-C24).** The domain
  types are the source of truth; a change to one owes an edit here, in the same
  diff. The translation rules:
  - an identifier newtype (`DocId`, `ChunkId`, `QueryId`) is a `string`, as it
    is in `ragondin-types`' serialized form;
  - a `usize` count (`top_k`, `budget`, `max_tokens`) is a `uint64`; an `f32`
    is a `float`, an `f64` a `double`;
  - `Embedding`'s components are `repeated float components`, named after
    `Embedding::new`'s argument, and `ModelIdentity`'s string is
    `string identity`, after `ModelIdentity::new`'s — a message rather than a
    bare `string`, unlike the identifier newtypes, because it is a value an
    rpc returns and a response carries it;
  - a trait method's arguments become one request message, in the method's
    argument order, and its return value one response message.
- **A message-typed field of a request is required**, though proto3 lets it be
  omitted: the request is refused as invalid (ADR-C24). Stated in
  `types.proto`, where a `Remote` author reads it.
- **`Fusion` cannot mirror its trait structurally.** `Vec<Vec<ScoredChunk>>`
  has no proto3 spelling, so each leg is a `ScoredChunkList`, and
  `FuseRequest.inputs` keeps the legs in the pipeline's wiring order, which is
  significant.
- **Every params struct is mirrored, including the empty `FusionParams`**, so a
  future knob is an added field rather than a change to an rpc's signature.
- **Every model-bearing service has a `GetModelIdentity` rpc** (ADR-C31
  § 4, ADR-C32 § 4): `Embedder`, `Reranker`, `ContextBuilder`, `Generator`.
  One package holds all four, and a message name is unique within a package,
  so each rpc's request and response are named after the service —
  `EmbedderModelIdentityRequest`, `…Response`, and so on — rather than
  `GetModelIdentityRequest`. Each response wraps a `ModelIdentity` message
  rather than being one, as every other rpc's response is its own message:
  the response can then gain a field without the value it mirrors changing.
  The context builder's request is empty and present all the same, under the
  rule `FusionParams` follows.
- **`served_model` rides in the params message, not beside it** — on
  `EmbedParams` and `RerankParams`, as field 2 of each. ADR-C32 § 4 puts it
  "on the Embed and Rerank requests" and, in the same section, makes it "a
  per-call parameter of the embedder and the reranker, on both faces", with
  `EmbedParams` and `RerankParams` gaining the field. The params message is
  the per-call parameters' face 2, and a request carries it, so the field is
  on the request by way of its params; a field on the request itself would
  split one Rust struct across two messages and break the rule that each
  params struct is mirrored field for field. The generator's required
  `served_model` and `template` sit in `GenerateParams` for the same reason
  (ADR-C31 § 2).
- **Optional means explicit presence; required strings have none.** Every
  `Option` in a mirrored struct — `served_model` on `EmbedParams` and
  `RerankParams` and on their identity requests, and `temperature`, `seed`
  and `max_tokens` on `GenerateParams` — is a proto3 `optional` field, so an
  omitted one decodes as `None` and never as zero or the empty string
  (ADR-C31 § 2): an omitted plain `double` would decode as `0.0`, which is
  greedy decoding. The generator's required `served_model` and `template` are
  plain strings, which have no presence; an omitted one decodes as empty, and
  an empty one is refused as an invalid request. Both shapes are pinned in
  `tests/mirror.rs`.
- **Refusals are named in `ComponentError` terms** ("an invalid request"),
  never as gRPC status codes: which status carries which refusal is not
  decided for face 2 as a whole. Decision issue #311 owns that gap, and its
  ADR adds the status codes; ADR-C33 fixes them for the reference generator
  service alone.
- **`EmbedRole` reserves the zero (ADR-C17).** `EMBED_ROLE_UNSPECIFIED = 0` is
  never valid, so the proto enum has three values where the Rust enum has two
  variants and decoding is not total. The comment in `embedder.proto` says so,
  so that nobody "fixes" the asymmetry.
- **The Embed rpc's text is final (ADR-C32 § 4).** A service must not prefix or
  otherwise transform the text by the role; it may use the role for anything
  that is not text. Stated on the rpc and beside the role field, because no
  conformance test can observe a double prefix.
- **The `VectorStore` service is defined and not yet reachable** (ADR-C32 § 5):
  nothing binds a `Remote` store until the contract gains a way to scope a
  store's content to a run.
- **No well-known type.** Using one (`google.protobuf.Timestamp`, say) would
  need a `prost-types` runtime dependency, which is a new
  `[workspace.dependencies]` entry and escalates (ADR-C34).

## Tests

- `tests/mirror.rs` — field parity. Each test builds a domain value and the
  message a correct conversion would give, and compares them. Both sides are
  destructured exhaustively where Rust allows it, so a field added to one face
  and not the other stops the test compiling. The params structs of
  `ragondin-contracts` are `#[non_exhaustive]` and cannot be destructured from
  here; their fields are compared instead. It also pins the two wire facts the
  `Remote` adapter's refusal of a role rests on: an omitted role decodes as
  `EMBED_ROLE_UNSPECIFIED`, and an unknown number decodes and names no role;
  an omitted optional field decodes as `None` and a present zero as that
  zero; and an omitted required string of the generator decodes as empty.
  The refusal itself, and its negative decode test, belong with the conversion
  in `ragondin-remote`.
- `tests/stubs.rs` — the stubs exist. It implements every generated server
  trait, naming each rpc's request and response, and names every generated
  client's constructor.

`ragondin-contracts` is a dev-dependency for those tests only: the params
structs and `EmbeddedChunk` live there, not in `ragondin-types`.

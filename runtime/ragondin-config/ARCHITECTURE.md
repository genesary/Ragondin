# ARCHITECTURE — ragondin-config

**Status: not an API boundary.** INV-1 names the three crates that are, and
this is not among them. Refactor it freely; the `ConfigSource` signature in
particular is expected to move when a second implementation arrives.

## What lives here

Where the data plane's configuration comes from, and nothing else.
`docs/system-architecture.md` §8.2 gives the abstraction its point: **the data
plane does not know who configures it.** §8.3 gives the file its status — the
YAML run locally **is** the Kubernetes custom resource, modulo the wire format.

| Piece | Role |
|---|---|
| `ConfigSource` | The trait a binary holds, as `Box<dyn ConfigSource>` |
| `LocalFile` | The one implementation: a YAML file on disk (standalone, P2) |
| `ConfigError` | Four typed diagnoses, one per thing the reader must do |

`Stream` — a configuration pushed from the controller over the
configuration-delivery service — is **M6 and deliberately absent**.
`docs/OPEN_QUESTIONS.md` #2 (the controller's language) is unresolved, and
nothing here presupposes an answer to it.

## Local invariants

- **The wire format is separate (INV-9).** The load path is
  `bytes → RawPipeline → validate → LogicalPipeline`, and this crate owns only
  the first arrow. The schema and the pass both live in `ragondin-pipeline` and
  are neither re-implemented nor paraphrased here. A file lands in the
  hand-maintained `Raw*` schema and reaches the in-memory model only through
  the pass — never through a deserializer pointed at an internal type.
- **This crate stops at `LogicalPipeline`.** Resolving implementations to
  components is physical planning; it needs an `EngineContext` and belongs to
  the engine. That separation is what lets `ragondin validate` check and
  content-address a configuration with no registry at all (ADR-C2).
- **The format lives at this edge.** `serde_yaml` is a normal dependency here
  and nowhere upstream: `ragondin-pipeline` carries no format implementation
  (INV-4) and does no I/O (INV-3), which is why its `peek_schema_version` is
  generic over the deserializer and why this crate is the one that supplies it.
  The decision to stay on `serde_yaml` despite its deprecation — the
  alternatives, the threat model, and the two triggers that reopen it — is
  recorded in `deny.toml`, beside the check that would fail if an advisory
  landed.
- **The version is read before the document is.** `SchemaVersion` refuses an
  unsupported version through `serde::de::Error::custom`, which keeps the
  wording and erases the type, so a plain parse reports *this build is too old*
  as a syntax error. Peeking first is what lets `ConfigError` keep apart the
  one fault no edit to the file can fix from the ones an edit can.
- **`tokio` is a dev-dependency, not a dependency.** `ConfigSource::load` is
  async and `async_trait`-declared (frozen: not RPITIT), but only a *caller*
  needs an executor. `docs/code-architecture.md` §11.2 selects the runtime at
  the binary level and keeps libraries runtime-agnostic as far as is practical.
  The consequence is a blocking `std::fs` read inside an `async fn`, which is
  the shape #198 is open on; that issue's subject is the component contract
  rather than this one, and a configuration is read once, at startup, off a
  local file — never per request on a serving path.
- **`ragondin-proto` is declared and unused.** The edge
  `docs/code-architecture.md` §4.3 draws, kept so that M6 adds a source rather
  than a dependency.

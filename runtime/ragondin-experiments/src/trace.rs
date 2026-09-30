//! The trace document's one definition: [`Trace`], read out of a
//! [`TraceDocument`] and written back into one.
//!
//! A [`TraceDocument`] is what the store moves, and it stays an opaque JSON
//! value to the store: [`RunStore::save`](crate::RunStore::save) and
//! [`RunStore::load`](crate::RunStore::load) never parse one (ADR-C28). This
//! module is for the code on either side of the store — the harness, which
//! renders the engine's `ExecutionTrace` into a [`Trace`] and the trace into a
//! document, and a reader, which asks for the typed shape back. Both compile
//! against the types below, so within one build the writer and the reader
//! cannot drift.
//!
//! # The three rules (ADR-C36 § 2)
//!
//! - **One definition.** The shape of a stored trace is these types and the
//!   two conversions beside them; nothing else in the workspace spells it.
//! - **Reported, never repaired.** A document these types do not describe
//!   fails [`Trace::try_from`] with a [`TraceError`] naming the node and the
//!   field. Nothing is defaulted, dropped or guessed: a missing field, a field
//!   the shape does not have, a summary in neither of its kind's shapes, and a
//!   count that disagrees with the chunks it counts are each refused, because
//!   accepting any of them would make the typed trace render a document other
//!   than the one it was read from.
//! - **The first incompatible change adds a version.** The document carries
//!   no version field today. A change to this shape that a stored trace could
//!   not satisfy adds one in the same change — and is a change to what the
//!   trace carries, which escalates (`AGENTS.md` § Rules of engagement).
//!
//! # The rendering
//!
//! ```text
//! {"nodes": [{"node", "inputs": [summary...], "output": summary | null,
//!             "duration_nanos", "error": string | null}, ...]}
//! ```
//!
//! and a summary is one of
//!
//! | kind | as an input port records it | as a node produced it |
//! |---|---|---|
//! | query | `{"query": {"id"}}` | the same |
//! | chunks | `{"chunks": {"count"}}` | `{"chunks": {"count", "ranked": [chunk...]}}` |
//! | context | `{"context": {"count", "text_bytes"}}` | `{"context": {"chunks": [chunk...], "text"}}` |
//! | answer | `{"answer": {"text_bytes"}}` | `{"answer": {"text"}}` |
//!
//! with a chunk `{"chunk", "document", "score"}`. The two columns are the
//! sides the harness renders each on (ADR-C28, ADR-C31 § 5), not a rule the
//! shape enforces: any summary may stand on either side, as the rendering
//! allows. The document is a `serde_json::Value`, whose objects are ordered
//! maps in this build, so the bytes a store writes do not depend on the order
//! a conversion inserts keys in.

use std::fmt;

use ragondin_pipeline::NodeId;
use ragondin_types::{ChunkId, DocId, QueryId};
use serde_json::{json, Map, Value};

use crate::run::TraceDocument;

/// One query's execution trace: every node that ran, in the order it ran.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// The nodes, in execution order.
    pub nodes: Vec<TraceNode>,
}

/// What one node received, produced and took.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceNode {
    /// The node, by its id in the pipeline.
    pub node: NodeId,
    /// One summary per input port, in port order.
    pub inputs: Vec<TraceSummary>,
    /// What the node produced, or `None` when it failed.
    pub output: Option<TraceSummary>,
    /// How long the node's component took, in nanoseconds — an integer, so a
    /// replay view reads the duration without rounding.
    pub duration_nanos: u64,
    /// The failure the node reported, rendered; `None` when it succeeded.
    pub error: Option<String>,
}

/// One named chunk, of a ranking or of a context — rendered alike.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceChunk {
    /// The chunk, by id.
    pub chunk: ChunkId,
    /// The document the chunk was derived from.
    pub document: DocId,
    /// The score the node gave it, on the node's own scale.
    ///
    /// An `f64`, where the engine records an `f32`: the harness widens the
    /// score losslessly on the way in, and an `f64` reads back any JSON
    /// number a stored document holds exactly, so a trace re-rendered from
    /// this type is the document it was read from.
    pub score: f64,
}

/// What travelled along one edge, as the trace records it.
///
/// Seven variants, because each kind a node produces is recorded two ways —
/// sized on an input port, named where it was produced — and a query one way.
#[derive(Clone, Debug, PartialEq)]
pub enum TraceSummary {
    /// `{"query": {"id"}}`: a query, by id.
    Query {
        /// The query's id.
        id: QueryId,
    },
    /// `{"chunks": {"count"}}`: a list of chunks, counted.
    Chunks {
        /// How many chunks the list held.
        count: u64,
    },
    /// `{"chunks": {"count", "ranked"}}`: the chunks a node produced, in its
    /// own order. The rendered `count` is the length of `chunks`.
    RankedChunks {
        /// The chunks, in the order the node returned them.
        chunks: Vec<TraceChunk>,
    },
    /// `{"context": {"count", "text_bytes"}}`: a context, sized.
    ContextSize {
        /// How many chunks the context held.
        count: u64,
        /// The length of its rendered text, in bytes of UTF-8.
        text_bytes: u64,
    },
    /// `{"context": {"chunks", "text"}}`: the context a node produced.
    Context {
        /// Its chunks, in the order the builder placed them.
        chunks: Vec<TraceChunk>,
        /// Its rendered text, whole.
        text: String,
    },
    /// `{"answer": {"text_bytes"}}`: an answer, sized.
    AnswerSize {
        /// The length of its text, in bytes of UTF-8.
        text_bytes: u64,
    },
    /// `{"answer": {"text"}}`: the answer a node produced.
    Answer {
        /// The answer's text, as the generator returned it.
        text: String,
    },
}

impl From<Trace> for TraceDocument {
    fn from(trace: Trace) -> Self {
        let nodes: Vec<Value> = trace
            .nodes
            .iter()
            .map(|node| {
                json!({
                    "node": node.node.as_str(),
                    "inputs": node.inputs.iter().map(render_summary).collect::<Vec<_>>(),
                    "output": node.output.as_ref().map(render_summary),
                    "duration_nanos": node.duration_nanos,
                    "error": node.error,
                })
            })
            .collect();
        TraceDocument::new(json!({ "nodes": nodes }))
    }
}

fn render_chunk(chunk: &TraceChunk) -> Value {
    json!({
        "chunk": chunk.chunk.as_str(),
        "document": chunk.document.as_str(),
        "score": chunk.score,
    })
}

fn render_chunks(chunks: &[TraceChunk]) -> Vec<Value> {
    chunks.iter().map(render_chunk).collect()
}

fn render_summary(summary: &TraceSummary) -> Value {
    match summary {
        TraceSummary::Query { id } => json!({"query": {"id": id.as_str()}}),
        TraceSummary::Chunks { count } => json!({"chunks": {"count": count}}),
        TraceSummary::RankedChunks { chunks } => json!({"chunks": {
            "count": chunks.len(),
            "ranked": render_chunks(chunks),
        }}),
        TraceSummary::ContextSize { count, text_bytes } => {
            json!({"context": {"count": count, "text_bytes": text_bytes}})
        }
        TraceSummary::Context { chunks, text } => json!({"context": {
            "chunks": render_chunks(chunks),
            "text": text,
        }}),
        TraceSummary::AnswerSize { text_bytes } => json!({"answer": {"text_bytes": text_bytes}}),
        TraceSummary::Answer { text } => json!({"answer": {"text": text}}),
    }
}

impl TryFrom<&TraceDocument> for Trace {
    type Error = TraceError;

    /// Reads the typed shape out of a stored document, or reports where the
    /// document departs from it. Never repairs: see the module's *The three
    /// rules*.
    fn try_from(document: &TraceDocument) -> Result<Self, TraceError> {
        let mut reader = Reader { node: None };
        let root = reader.object(document.as_value(), "")?;
        reader.exact_keys(root, &["nodes"], "")?;
        let nodes = reader.array(&root["nodes"], "nodes")?;
        let nodes = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| reader.node(node, &format!("nodes[{index}]")))
            .collect::<Result<_, _>>()?;
        Ok(Trace { nodes })
    }
}

/// Why a document is not a [`Trace`], and where.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.describe())]
pub struct TraceError {
    node: Option<NodeId>,
    field: String,
    problem: TraceProblem,
}

impl TraceError {
    /// The node the fault is in, when the document got far enough to name
    /// one; `None` for a fault outside any node, or in a node whose own id is
    /// the fault.
    pub fn node(&self) -> Option<&NodeId> {
        self.node.as_ref()
    }

    /// Where in the document the fault is, as a path from its root —
    /// `nodes[1].output.chunks`, say. Empty for the document itself.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// What is wrong there.
    pub fn problem(&self) -> &TraceProblem {
        &self.problem
    }

    fn describe(&self) -> String {
        let at = if self.field.is_empty() {
            "the document".to_owned()
        } else {
            format!("`{}`", self.field)
        };
        match &self.node {
            Some(node) => format!("trace node `{}`, at {at}: {}", node.as_str(), self.problem),
            None => format!("trace, at {at}: {}", self.problem),
        }
    }
}

/// What is wrong at the place a [`TraceError`] names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceProblem {
    /// A field the shape requires is absent.
    Missing,
    /// A field the shape does not have is present.
    Unexpected,
    /// The value is not of the type the shape requires there.
    WrongType {
        /// What the shape requires, in words: `a string`, `an array`, ….
        expected: &'static str,
    },
    /// A summary is in no shape the rendering writes: an unknown kind, or a
    /// known kind with neither of its two sets of fields.
    UnknownShape,
    /// A named chunk list's `count` is not the number of chunks it names.
    CountMismatch {
        /// The count the document states.
        count: u64,
        /// How many chunks it names.
        named: usize,
    },
}

impl fmt::Display for TraceProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("a required field is missing"),
            Self::Unexpected => f.write_str("a field the trace does not have"),
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::UnknownShape => f.write_str(
                "an unknown summary shape: expected one of query {id}, chunks {count} or \
                 {count, ranked}, context {count, text_bytes} or {chunks, text}, answer \
                 {text_bytes} or {text}",
            ),
            Self::CountMismatch { count, named } => {
                write!(f, "the count is {count} but {named} chunks are named")
            }
        }
    }
}

/// Walks a document, remembering the node it is in so that every error names
/// it.
struct Reader {
    node: Option<NodeId>,
}

fn join(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_owned()
    } else {
        format!("{at}.{key}")
    }
}

impl Reader {
    fn fail(&self, field: impl Into<String>, problem: TraceProblem) -> TraceError {
        TraceError {
            node: self.node.clone(),
            field: field.into(),
            problem,
        }
    }

    fn object<'a>(&self, value: &'a Value, at: &str) -> Result<&'a Map<String, Value>, TraceError> {
        value.as_object().ok_or_else(|| {
            self.fail(
                at,
                TraceProblem::WrongType {
                    expected: "an object",
                },
            )
        })
    }

    fn array<'a>(&self, value: &'a Value, at: &str) -> Result<&'a Vec<Value>, TraceError> {
        value.as_array().ok_or_else(|| {
            self.fail(
                at,
                TraceProblem::WrongType {
                    expected: "an array",
                },
            )
        })
    }

    fn string(&self, value: &Value, at: &str) -> Result<String, TraceError> {
        value.as_str().map(str::to_owned).ok_or_else(|| {
            self.fail(
                at,
                TraceProblem::WrongType {
                    expected: "a string",
                },
            )
        })
    }

    fn unsigned(&self, value: &Value, at: &str) -> Result<u64, TraceError> {
        value.as_u64().ok_or_else(|| {
            self.fail(
                at,
                TraceProblem::WrongType {
                    expected: "a non-negative integer",
                },
            )
        })
    }

    fn number(&self, value: &Value, at: &str) -> Result<f64, TraceError> {
        value.as_f64().ok_or_else(|| {
            self.fail(
                at,
                TraceProblem::WrongType {
                    expected: "a number",
                },
            )
        })
    }

    /// `map` holds exactly `keys`: the first missing one is reported, then the
    /// first one the shape does not have.
    fn exact_keys(
        &self,
        map: &Map<String, Value>,
        keys: &[&str],
        at: &str,
    ) -> Result<(), TraceError> {
        if let Some(missing) = keys.iter().find(|key| !map.contains_key(**key)) {
            return Err(self.fail(join(at, missing), TraceProblem::Missing));
        }
        if let Some(extra) = map.keys().find(|key| !keys.contains(&key.as_str())) {
            return Err(self.fail(join(at, extra), TraceProblem::Unexpected));
        }
        Ok(())
    }

    fn node(&mut self, value: &Value, at: &str) -> Result<TraceNode, TraceError> {
        self.node = None;
        let map = self.object(value, at)?;
        // The id first, so that every later fault in this node names it.
        if let Some(id) = map.get("node") {
            self.node = Some(NodeId::new(self.string(id, &join(at, "node"))?));
        }
        self.exact_keys(
            map,
            &["node", "inputs", "output", "duration_nanos", "error"],
            at,
        )?;
        let node = self.node.clone().expect("`exact_keys` found the node's id");

        let inputs_at = join(at, "inputs");
        let inputs = self
            .array(&map["inputs"], &inputs_at)?
            .iter()
            .enumerate()
            .map(|(index, input)| self.summary(input, &format!("{inputs_at}[{index}]")))
            .collect::<Result<_, _>>()?;
        let output = match &map["output"] {
            Value::Null => None,
            output => Some(self.summary(output, &join(at, "output"))?),
        };
        let duration_nanos = self.unsigned(&map["duration_nanos"], &join(at, "duration_nanos"))?;
        let error = match &map["error"] {
            Value::Null => None,
            error => Some(self.string(error, &join(at, "error"))?),
        };

        Ok(TraceNode {
            node,
            inputs,
            output,
            duration_nanos,
            error,
        })
    }

    fn summary(&self, value: &Value, at: &str) -> Result<TraceSummary, TraceError> {
        let map = self.object(value, at)?;
        let mut entries = map.iter();
        let (Some((kind, body)), None) = (entries.next(), entries.next()) else {
            return Err(self.fail(at, TraceProblem::UnknownShape));
        };
        let body_at = join(at, kind);
        let body = self.object(body, &body_at)?;
        let has =
            |keys: &[&str]| body.len() == keys.len() && keys.iter().all(|k| body.contains_key(*k));
        let field = |key: &str| join(&body_at, key);

        match kind.as_str() {
            "query" if has(&["id"]) => Ok(TraceSummary::Query {
                id: QueryId::new(self.string(&body["id"], &field("id"))?),
            }),
            "chunks" if has(&["count"]) => Ok(TraceSummary::Chunks {
                count: self.unsigned(&body["count"], &field("count"))?,
            }),
            "chunks" if has(&["count", "ranked"]) => {
                let count = self.unsigned(&body["count"], &field("count"))?;
                let chunks = self.chunks(&body["ranked"], &field("ranked"))?;
                if usize::try_from(count).ok() != Some(chunks.len()) {
                    return Err(self.fail(
                        field("count"),
                        TraceProblem::CountMismatch {
                            count,
                            named: chunks.len(),
                        },
                    ));
                }
                Ok(TraceSummary::RankedChunks { chunks })
            }
            "context" if has(&["count", "text_bytes"]) => Ok(TraceSummary::ContextSize {
                count: self.unsigned(&body["count"], &field("count"))?,
                text_bytes: self.unsigned(&body["text_bytes"], &field("text_bytes"))?,
            }),
            "context" if has(&["chunks", "text"]) => Ok(TraceSummary::Context {
                chunks: self.chunks(&body["chunks"], &field("chunks"))?,
                text: self.string(&body["text"], &field("text"))?,
            }),
            "answer" if has(&["text_bytes"]) => Ok(TraceSummary::AnswerSize {
                text_bytes: self.unsigned(&body["text_bytes"], &field("text_bytes"))?,
            }),
            "answer" if has(&["text"]) => Ok(TraceSummary::Answer {
                text: self.string(&body["text"], &field("text"))?,
            }),
            "query" | "chunks" | "context" | "answer" => {
                Err(self.fail(body_at, TraceProblem::UnknownShape))
            }
            _ => Err(self.fail(at, TraceProblem::UnknownShape)),
        }
    }

    fn chunks(&self, value: &Value, at: &str) -> Result<Vec<TraceChunk>, TraceError> {
        self.array(value, at)?
            .iter()
            .enumerate()
            .map(|(index, chunk)| {
                let at = format!("{at}[{index}]");
                let map = self.object(chunk, &at)?;
                self.exact_keys(map, &["chunk", "document", "score"], &at)?;
                Ok(TraceChunk {
                    chunk: ChunkId::new(self.string(&map["chunk"], &join(&at, "chunk"))?),
                    document: DocId::new(self.string(&map["document"], &join(&at, "document"))?),
                    score: self.number(&map["score"], &join(&at, "score"))?,
                })
            })
            .collect()
    }
}

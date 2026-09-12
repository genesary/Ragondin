//! The fixture cross-encoder, emitted as ONNX bytes.
//!
//! ONNX models are protobuf, and the sub-message shapes this file needs are
//! small enough to write directly. That is the point: a model built here is
//! readable, reproducible from a checkout, and needs neither a download nor a
//! committed binary nobody can inspect.

/// A protobuf message under construction.
///
/// Only the four wire shapes ONNX needs here: varint, length-delimited bytes,
/// a nested message, and a packed run of varints.
struct Buf(Vec<u8>);

impl Buf {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.0.push((value as u8) | 0x80);
            value >>= 7;
        }
        self.0.push(value as u8);
    }

    fn key(&mut self, field: u32, wire_type: u32) {
        self.varint(u64::from(field) << 3 | u64::from(wire_type));
    }

    /// A varint field. Repeating the call repeats the field, which is how an
    /// unpacked `repeated int64` — `TensorProto.dims` — is written.
    fn int(&mut self, field: u32, value: i64) {
        self.key(field, 0);
        self.varint(value as u64);
    }

    fn text(&mut self, field: u32, value: &str) {
        self.key(field, 2);
        self.varint(value.len() as u64);
        self.0.extend_from_slice(value.as_bytes());
    }

    fn nested(&mut self, field: u32, inner: Buf) {
        self.key(field, 2);
        self.varint(inner.0.len() as u64);
        self.0.extend_from_slice(&inner.0);
    }

    /// A packed `repeated int64` — `TensorProto.int64_data`.
    fn packed_ints(&mut self, field: u32, values: &[i64]) {
        let mut inner = Buf::new();
        for value in values {
            inner.varint(*value as u64);
        }
        self.nested(field, inner);
    }
}

/// `TensorProto.DataType.FLOAT`.
const FLOAT: i64 = 1;
/// `TensorProto.DataType.INT64`.
const INT64: i64 = 7;
/// `AttributeProto.AttributeType.INT`.
const ATTRIBUTE_INT: i64 = 2;

/// One dimension of a declared tensor shape.
enum Dim {
    /// A symbolic dimension, fixed only at run time.
    Param(&'static str),
    /// A dimension the graph fixes.
    Value(i64),
}

/// A 1-D `int64` initializer.
fn initializer(name: &str, values: &[i64]) -> Buf {
    let mut tensor = Buf::new();
    tensor.int(1, values.len() as i64); // dims
    tensor.int(2, INT64); // data_type
    tensor.packed_ints(7, values); // int64_data
    tensor.text(8, name); // name
    tensor
}

/// An `AttributeProto` carrying a single integer.
fn int_attribute(name: &str, value: i64) -> Buf {
    let mut attribute = Buf::new();
    attribute.text(1, name); // name
    attribute.int(3, value); // i
    attribute.int(20, ATTRIBUTE_INT); // type
    attribute
}

/// A `NodeProto`. Named after the value it produces, so a runtime error names
/// something findable in this file.
fn node(op_type: &str, inputs: &[&str], outputs: &[&str], attributes: Vec<Buf>) -> Buf {
    let mut node = Buf::new();
    for input in inputs {
        node.text(1, input);
    }
    for output in outputs {
        node.text(2, output);
    }
    node.text(3, &format!("{op_type}:{}", outputs[0])); // name
    node.text(4, op_type); // op_type
    for attribute in attributes {
        node.nested(5, attribute);
    }
    node
}

/// A `ValueInfoProto` for a tensor of `elem_type` and the given shape.
fn value_info(name: &str, elem_type: i64, dims: &[Dim]) -> Buf {
    let mut shape = Buf::new();
    for dim in dims {
        let mut entry = Buf::new();
        match dim {
            Dim::Value(value) => entry.int(1, *value), // dim_value
            Dim::Param(param) => entry.text(2, param), // dim_param
        }
        shape.nested(1, entry); // dim
    }

    let mut tensor = Buf::new();
    tensor.int(1, elem_type); // elem_type
    tensor.nested(2, shape); // shape

    let mut type_proto = Buf::new();
    type_proto.nested(1, tensor); // tensor_type

    let mut value_info = Buf::new();
    value_info.text(1, name); // name
    value_info.nested(2, type_proto); // type
    value_info
}

/// Builds the fixture cross-encoder.
///
/// It has the input and output signature of a BERT-style cross-encoder —
/// `input_ids`, `attention_mask` and `token_type_ids` of shape `[batch,
/// sequence]` in, `logits` of shape `[batch, 1]` out — and it scores a pair by
/// **lexical overlap**: how many (query token, passage token) positions hold
/// the same token id. The two segments are told apart by `token_type_ids`, and
/// padding is excluded by `attention_mask`, so the score of a pair does not
/// depend on what else was in its batch.
///
/// That is not what a trained cross-encoder computes, and it is not meant to
/// be. It is a relevance signal a test can predict exactly, which is what makes
/// "the obviously relevant chunk ranks first" an assertion rather than a hope.
pub fn cross_encoder() -> Vec<u8> {
    build(1)
}

/// The same model, emitting **two** scores per pair instead of one.
///
/// A cross-encoder with a two-way classification head has this shape, and it is
/// a different model: nothing on the boundary says which column means
/// *relevant*. The component must refuse it rather than read the columns as if
/// they were pairs, which is the one way a wrong model produces plausible
/// numbers instead of an error.
pub fn two_headed_cross_encoder() -> Vec<u8> {
    build(2)
}

fn build(scores_per_pair: i64) -> Vec<u8> {
    let mut nodes = vec![
        // Which positions are the passage, and which the query — padding
        // excluded by the attention mask.
        node(
            "Mul",
            &["token_type_ids", "attention_mask"],
            &["passage_positions"],
            vec![],
        ),
        node(
            "Sub",
            &["attention_mask", "passage_positions"],
            &["query_positions"],
            vec![],
        ),
        node(
            "Cast",
            &["query_positions"],
            &["query_weights"],
            vec![int_attribute("to", FLOAT)],
        ),
        node(
            "Cast",
            &["passage_positions"],
            &["passage_weights"],
            vec![int_attribute("to", FLOAT)],
        ),
        // Every position against every other: `same[b, i, j]` is 1 where
        // position i and position j hold the same token id.
        node("Unsqueeze", &["input_ids", "axis_2"], &["ids_rows"], vec![]),
        node(
            "Unsqueeze",
            &["input_ids", "axis_1"],
            &["ids_columns"],
            vec![],
        ),
        node("Equal", &["ids_rows", "ids_columns"], &["same"], vec![]),
        node(
            "Cast",
            &["same"],
            &["same_weights"],
            vec![int_attribute("to", FLOAT)],
        ),
        // Keep only the (query position, passage position) cells.
        node(
            "Unsqueeze",
            &["query_weights", "axis_2"],
            &["query_rows"],
            vec![],
        ),
        node(
            "Unsqueeze",
            &["passage_weights", "axis_1"],
            &["passage_columns"],
            vec![],
        ),
        node(
            "Mul",
            &["same_weights", "query_rows"],
            &["query_matches"],
            vec![],
        ),
        node(
            "Mul",
            &["query_matches", "passage_columns"],
            &["overlap_cells"],
            vec![],
        ),
        // Count them, leaving one score per pair.
        node(
            "ReduceSum",
            &["overlap_cells", "axis_2"],
            &["overlap_rows"],
            vec![int_attribute("keepdims", 0)],
        ),
        node(
            "ReduceSum",
            &["overlap_rows", "axis_1"],
            &["pair_score"],
            vec![int_attribute("keepdims", 1)],
        ),
    ];
    // The head. One column is a cross-encoder; more than one is the shape this
    // component refuses, and the only difference between the two fixtures.
    nodes.push(match scores_per_pair {
        1 => node("Identity", &["pair_score"], &["logits"], vec![]),
        columns => node(
            "Concat",
            &vec!["pair_score"; columns as usize],
            &["logits"],
            vec![int_attribute("axis", 1)],
        ),
    });

    let mut graph = Buf::new();
    for node in nodes {
        graph.nested(1, node); // node
    }
    graph.text(2, "lexical_overlap_cross_encoder"); // name
    graph.nested(5, initializer("axis_1", &[1])); // initializer
    graph.nested(5, initializer("axis_2", &[2]));
    for name in ["input_ids", "attention_mask", "token_type_ids"] {
        graph.nested(
            11, // input
            value_info(name, INT64, &[Dim::Param("batch"), Dim::Param("sequence")]),
        );
    }
    graph.nested(
        12, // output
        value_info(
            "logits",
            FLOAT,
            &[Dim::Param("batch"), Dim::Value(scores_per_pair)],
        ),
    );

    // Opset 13 is what the graph above is written against: `Unsqueeze` and
    // `ReduceSum` take their axes as an input there rather than as an
    // attribute. IR version 7 is the one that opset pairs with.
    let mut opset = Buf::new();
    opset.text(1, ""); // domain: the default ONNX domain
    opset.int(2, 13); // version

    let mut model = Buf::new();
    model.int(1, 7); // ir_version
    model.text(2, "ragondin-reranker-onnx tests"); // producer_name
    model.nested(7, graph); // graph
    model.nested(8, opset); // opset_import
    model.0
}

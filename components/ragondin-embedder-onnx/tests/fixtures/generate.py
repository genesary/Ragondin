#!/usr/bin/env python3
"""Regenerate the fixtures in this directory.

    python3 components/ragondin-embedder-onnx/tests/fixtures/generate.py

The tests need a real ONNX graph and a real tokenizer, and no model may be
fetched at build time (the rule this crate was created under, in #20). So the
fixtures are committed, and this script is how they came to be: the alternative
is eight opaque binaries nobody can regenerate or explain. Everything here is
fixed-seed, so a rerun reproduces the committed bytes.

The graphs are deliberately trivial -- a token-embedding table, a Gather, and at
most one node after it -- because what the tests exercise is the component
around the model: tokenization, prefixes, padding, batching, mean pooling,
normalization, and the six ways a model is refused. A real sentence
transformer would test ONNX Runtime instead, slowly.

Requires `onnx` and `numpy`.
"""

import json
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

HERE = Path(__file__).parent

HIDDEN = 8
OPSET = 13

# `[UNK]` first so that an out-of-vocabulary word still embeds rather than
# failing, and `[PAD]` second so that the component's default pad id of 0 is
# *not* silently the same token as `[UNK]`.
VOCAB = [
    "[UNK]",
    "[PAD]",
    ":",
    "query",
    "passage",
    "a",
    "an",
    "the",
    "short",
    "long",
    "of",
    "rather",
    "more",
    "words",
    "than",
    "first",
    "second",
    "one",
    "another",
    "entirely",
    "text",
    "both",
    "roles",
    "are",
    "asked",
    "about",
    "cat",
    "sat",
    "on",
    "mat",
    "dog",
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "zeta",
    "eta",
]


def token_embeddings() -> np.ndarray:
    """The token embedding table, `[vocab, hidden]`.

    Fixed seed, and deliberately not centred on zero: a table symmetric about
    the origin makes mean pooling of a long sequence tend to the zero vector,
    which would turn the normalization guard into the common case rather than
    the edge case it is.
    """
    rng = np.random.RandomState(20)
    return (rng.rand(len(VOCAB), HIDDEN).astype(np.float32) + 0.25) * 0.5


def write(model: onnx.ModelProto, name: str) -> None:
    onnx.checker.check_model(model)
    path = HERE / name
    path.write_bytes(model.SerializeToString())
    print(f"wrote {path.relative_to(Path.cwd()) if path.is_relative_to(Path.cwd()) else path}")


def tensor(name: str, elem_type: int, shape: list) -> onnx.ValueInfoProto:
    return helper.make_tensor_value_info(name, elem_type, shape)


def ids_input(name: str) -> onnx.ValueInfoProto:
    return tensor(name, TensorProto.INT64, ["batch", "sequence"])


def hidden_output(name: str) -> onnx.ValueInfoProto:
    return tensor(name, TensorProto.FLOAT, ["batch", "sequence", HIDDEN])


def model_of(graph: onnx.GraphProto) -> onnx.ModelProto:
    return helper.make_model(graph, opset_imports=[helper.make_opsetid("", OPSET)])


def tiny_embedder() -> onnx.ModelProto:
    """The ordinary case: two inputs, one `[batch, sequence, hidden]` output.

    `attention_mask` is declared and unused. That is on purpose -- the mask is
    what the component pools with, not something this stand-in model needs --
    and it also pins that a declared-but-unconsumed input is still fed.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    gather = helper.make_node(
        "Gather", ["token_embeddings", "input_ids"], ["last_hidden_state"], axis=0
    )
    graph = helper.make_graph(
        [gather],
        "tiny_embedder",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [hidden_output("last_hidden_state")],
        [weights],
    )
    return model_of(graph)


def tiny_embedder_token_types() -> onnx.ModelProto:
    """A model that also demands `token_type_ids`, as a BERT export does.

    Segment 0 contributes the zero vector and segment 1 contributes a large
    one, so this model answers *identically to* `tiny-embedder.onnx` exactly
    when the component supplies zeros -- which is what the test asserts.
    """
    segments = np.zeros((2, HIDDEN), dtype=np.float32)
    segments[1, :] = 100.0

    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    segment_weights = numpy_helper.from_array(segments, name="segment_embeddings")
    nodes = [
        helper.make_node("Gather", ["token_embeddings", "input_ids"], ["tokens"], axis=0),
        helper.make_node(
            "Gather", ["segment_embeddings", "token_type_ids"], ["segments"], axis=0
        ),
        helper.make_node("Add", ["tokens", "segments"], ["last_hidden_state"]),
    ]
    graph = helper.make_graph(
        nodes,
        "tiny_embedder_token_types",
        [ids_input("input_ids"), ids_input("attention_mask"), ids_input("token_type_ids")],
        [hidden_output("last_hidden_state")],
        [weights, segment_weights],
    )
    return model_of(graph)


def tiny_embedder_unknown_input() -> onnx.ModelProto:
    """A model demanding an input the component has no value for."""
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    gather = helper.make_node(
        "Gather", ["token_embeddings", "input_ids"], ["gathered"], axis=0
    )
    scale = helper.make_node("Mul", ["gathered", "temperature"], ["last_hidden_state"])
    graph = helper.make_graph(
        [gather, scale],
        "tiny_embedder_unknown_input",
        [
            ids_input("input_ids"),
            ids_input("attention_mask"),
            tensor("temperature", TensorProto.FLOAT, [1]),
        ],
        [hidden_output("last_hidden_state")],
        [weights],
    )
    return model_of(graph)


def tiny_embedder_no_ids() -> onnx.ModelProto:
    """A model that never asks for `input_ids`, and so encodes no text.

    Rejected at construction: every input it declares is one the component can
    fill, so the unknown-input check above passes it, and without this second
    check it would load happily and return one constant vector per call.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    gather = helper.make_node(
        "Gather", ["token_embeddings", "attention_mask"], ["last_hidden_state"], axis=0
    )
    graph = helper.make_graph(
        [gather],
        "tiny_embedder_no_ids",
        [ids_input("attention_mask")],
        [hidden_output("last_hidden_state")],
        [weights],
    )
    return model_of(graph)


def tiny_embedder_shrinking() -> onnx.ModelProto:
    """A model whose output sequence axis is shorter than its input's.

    It drops the first token, the way a graph that strips a `[CLS]` for itself
    would. The rank is right and the batch axis is right, so both of the other
    output checks pass it; only comparing the sequence axis against the width
    that was fed catches it. Slicing the output by the *input* width instead is
    an out-of-bounds read on a short answer and a read of the wrong row on a
    long one.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    starts = numpy_helper.from_array(np.array([1], dtype=np.int64), name="starts")
    ends = numpy_helper.from_array(np.array([2**31 - 1], dtype=np.int64), name="ends")
    axes = numpy_helper.from_array(np.array([1], dtype=np.int64), name="axes")
    nodes = [
        helper.make_node("Gather", ["token_embeddings", "input_ids"], ["tokens"], axis=0),
        helper.make_node(
            "Slice", ["tokens", "starts", "ends", "axes"], ["last_hidden_state"]
        ),
    ]
    graph = helper.make_graph(
        nodes,
        "tiny_embedder_shrinking",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [hidden_output("last_hidden_state")],
        [weights, starts, ends, axes],
    )
    return model_of(graph)


def tiny_embedder_float16() -> onnx.ModelProto:
    """A model whose hidden states come back as float16.

    The shape is exactly what this component wants; only the element type is
    not. Quantized exports ship this way routinely, so the failure has to name
    the dtype rather than report that the model crashed.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    nodes = [
        helper.make_node("Gather", ["token_embeddings", "input_ids"], ["tokens"], axis=0),
        helper.make_node("Cast", ["tokens"], ["last_hidden_state"], to=TensorProto.FLOAT16),
    ]
    graph = helper.make_graph(
        nodes,
        "tiny_embedder_float16",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [tensor("last_hidden_state", TensorProto.FLOAT16, ["batch", "sequence", HIDDEN])],
        [weights],
    )
    return model_of(graph)


def tiny_embedder_pooled() -> onnx.ModelProto:
    """A model that pools for itself, so its output is `[batch, hidden]`.

    Rejected by the component, which pools with the attention mask and so needs
    per-token hidden states. This fixture is what makes that rejection a test
    rather than a claim.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    nodes = [
        helper.make_node("Gather", ["token_embeddings", "input_ids"], ["tokens"], axis=0),
        helper.make_node("ReduceMean", ["tokens"], ["pooled"], axes=[1], keepdims=0),
    ]
    graph = helper.make_graph(
        nodes,
        "tiny_embedder_pooled",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [tensor("pooled", TensorProto.FLOAT, ["batch", HIDDEN])],
        [weights],
    )
    return model_of(graph)


def tiny_embedder_nan() -> onnx.ModelProto:
    """A model whose hidden states are `NaN` everywhere.

    A constant `NaN` bias is broadcast onto the gathered tokens, which is the
    cheapest way to reproduce what a real model does when a quantized weight
    overflows or a normalization divides by zero. The shape and the dtype are
    both what the component wants, so nothing before pooling rejects it: only
    checking the pooled components does.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    bias = numpy_helper.from_array(
        np.full((1, 1, HIDDEN), np.nan, dtype=np.float32), name="nan_bias"
    )
    nodes = [
        helper.make_node("Gather", ["token_embeddings", "input_ids"], ["tokens"], axis=0),
        helper.make_node("Add", ["tokens", "nan_bias"], ["last_hidden_state"]),
    ]
    graph = helper.make_graph(
        nodes,
        "tiny_embedder_nan",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [hidden_output("last_hidden_state")],
        [weights, bias],
    )
    return model_of(graph)


# The special tokens a BERT post-processor wraps a sequence in, appended rather
# than mixed into `VOCAB`: every graph above is generated from a table sized and
# seeded by `VOCAB`, so inserting a token would move every fixture's bytes.
BERT_VOCAB = VOCAB + ["[CLS]", "[SEP]"]


def tokenizer(vocab: list, post_processor: dict | None) -> dict:
    """A WordPiece tokenizer over `vocab`, in `tokenizer.json` form.

    Lowercasing and whitespace/punctuation splitting throughout; the
    post-processor is what the two fixtures differ by.
    """
    return {
        "version": "1.0",
        "truncation": None,
        "padding": None,
        "added_tokens": [
            {
                "id": index,
                "content": token,
                "single_word": False,
                "lstrip": False,
                "rstrip": False,
                "normalized": False,
                "special": True,
            }
            for index, token in enumerate(vocab)
            if token.startswith("[")
        ],
        "normalizer": {"type": "Lowercase"},
        "pre_tokenizer": {"type": "Whitespace"},
        "post_processor": post_processor,
        "decoder": None,
        "model": {
            "type": "WordPiece",
            "unk_token": "[UNK]",
            "continuing_subword_prefix": "##",
            "max_input_chars_per_word": 100,
            "vocab": {token: index for index, token in enumerate(vocab)},
        },
    }


def plain_tokenizer() -> dict:
    """The tokenizer every embedding test runs against: **no post-processor**.

    Nothing here adds `[CLS]` or `[SEP]`, so a text of no words tokenizes to no
    tokens. That is the empty-sequence case the component has to survive, and a
    fixture that quietly injected special tokens would hide it.
    """
    return tokenizer(VOCAB, None)


def bert_tokenizer() -> dict:
    """A tokenizer that *does* wrap a sequence, as a BERT export's does.

    It exists for one reason: `tokenizers` subtracts a post-processor's
    special-token count from the truncation limit without checking that
    anything is left, so the component refuses a `max_sequence_length` at or
    below that count. Proving the refusal needs a tokenizer whose count is not
    zero, and the plain fixture's is.

    Used at **construction only**. Its two special ids sit past the end of the
    38-row embedding table every graph here is built from, so it is never fed
    to one.
    """
    return tokenizer(
        BERT_VOCAB,
        {
            "type": "BertProcessing",
            "sep": ["[SEP]", BERT_VOCAB.index("[SEP]")],
            "cls": ["[CLS]", BERT_VOCAB.index("[CLS]")],
        },
    )


def main() -> None:
    write(tiny_embedder(), "tiny-embedder.onnx")
    write(tiny_embedder_token_types(), "tiny-embedder-token-types.onnx")
    write(tiny_embedder_unknown_input(), "tiny-embedder-unknown-input.onnx")
    write(tiny_embedder_no_ids(), "tiny-embedder-no-ids.onnx")
    write(tiny_embedder_pooled(), "tiny-embedder-pooled.onnx")
    write(tiny_embedder_shrinking(), "tiny-embedder-shrinking.onnx")
    write(tiny_embedder_float16(), "tiny-embedder-float16.onnx")
    write(tiny_embedder_nan(), "tiny-embedder-nan.onnx")

    for name, content in [
        ("tokenizer.json", plain_tokenizer()),
        ("tokenizer-bert.json", bert_tokenizer()),
    ]:
        path = HERE / name
        path.write_text(json.dumps(content, indent=1, ensure_ascii=False) + "\n")
        print(f"wrote {path}")


if __name__ == "__main__":
    main()

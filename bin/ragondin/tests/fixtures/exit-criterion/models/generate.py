#!/usr/bin/env python3
"""Regenerate the two models and the tokenizer in this directory.

    python3 bin/ragondin/tests/fixtures/exit-criterion/models/generate.py

`tests/exit_criterion.rs` spawns the built binary over these files, so they
have to exist on disk before the test runs, and no model may be fetched at
build time. So they are committed, and this script is how they came to be —
the convention `components/ragondin-embedder-onnx/tests/fixtures/generate.py`
set: three small binaries, and beside them the source that reproduces their
bytes exactly, so that whoever has to debug a failing run can read what the
models do. Nothing here is random, so a rerun reproduces the committed bytes.

Requires `onnx` and `numpy`.

# What the models compute, and why the gap is by construction

The exit criterion is *hybrid retrieval with reranking beats dense-only*. A
trained embedder and a trained cross-encoder would make that a fact about two
models, and neither fits a test that has to be fast, offline and deterministic.
So the corpus (`../dataset/`), the queries and these models were designed
together, and the gap is a property of the design:

- **The embedder** is a token-embedding table, gathered by `input_ids`; the
  component mean-pools the rows over the attention mask and L2-normalizes
  (`ragondin-embedder-onnx`'s convention). Each query's two words sit on two
  axes of their own, generic words all sit on one shared axis, and a
  distractor token sits on a query's own direction with more weight than
  either query word — the shape a near-synonym a model has over-learned takes.
- **The cross-encoder** scores a pair by **lexical overlap**: how many (query
  position, passage position) pairs hold the same token id, with the two
  segments told apart by `token_type_ids` and padding excluded by
  `attention_mask`. It is the graph `ragondin-reranker-onnx` builds for its own
  tests, written once more here because that crate's is emitted inside its test
  binary and this test needs a file.

Each query has one judged answer, and each is built to defeat a different
stage, so that removing any stage of the hybrid pipeline loses a query — which
is what makes the test a tripwire rather than a formality:

- `q-cat` (`cat mat`): `dog` sits on the cat-and-mat direction, so dense
  retrieval ranks `d-dog` above `d-cat`. BM25 ranks them the other way, and
  RRF then scores the two **exactly equal** — one first place and one second
  place each — so the fused order is decided by chunk id alone. The
  cross-encoder breaks the tie on content: `d-cat` overlaps the query at two
  positions, `d-dog` at one.
- `q-greek` (`alpha gamma`): `delta` sits on the alpha-and-gamma direction at
  twice the weight, and three passages carry it, so `d-alpha` falls out of the
  dense leg's `top_k` of 3 altogether. BM25 is what surfaces it; RRF ties it
  with the leading distractor, as above; the cross-encoder puts it first.
- `q-river` (`river banks`): `d-bank` says the query in three words, `d-river`
  says it twice in ten. BM25's length normalization prefers the short one, and
  so does the embedder, whose generic words dilute the long one — both legs
  and the fused list put the distractor first. The cross-encoder counts
  matching positions, and two mentions overlap more than one: it alone ranks
  `d-river` first.
- `q-short` (`short text`): no distractor. Every stage gets it right, which
  keeps the baseline a working retriever rather than a broken one.

So dense-only loses three of the four; BM25 alone loses `q-river`; the fused
list without the reranker loses `q-river`, and holds `q-cat` and `q-greek`
only by chunk-id order; dense with the reranker and no lexical leg never sees
`d-alpha`. Only the whole pipeline scores full marks. The numbers the test
compares are still real — nDCG over what each pipeline returned through the
real engine — and that is the condition the issue sets for a curated fixture.
"""

import json
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

HERE = Path(__file__).parent

OPSET = 13
HIDDEN = 8

# `[PAD]` first: the embedder pads with id 0, and the pad row below is zero.
SPECIALS = ["[PAD]", "[UNK]", "[CLS]", "[SEP]"]
WORDS = [
    "the", "cat", "sat", "on", "mat", "dog",
    "alpha", "beta", "gamma", "delta", "eta",
    "a", "short", "text", "long", "of", "more", "words", "than", "first", "one",
    "river", "banks", "build",
    "price", "tin", "fell", "last", "quarter",
]
VOCAB = SPECIALS + WORDS

# The axes of the embedding space, by what sits on them.
CAT, MAT, ALPHA, GAMMA, GENERIC, TEXT, RIVER, BANKS = range(HIDDEN)


def axis(*weights: tuple[int, float]) -> np.ndarray:
    row = np.zeros(HIDDEN, dtype=np.float32)
    for index, weight in weights:
        row[index] = weight
    return row


# Every word not named here is generic: half a unit on the shared axis, which
# is what dilutes a passage's query-bearing words in proportion to its length.
PLACED = {
    "cat": axis((CAT, 1.0)),
    "mat": axis((MAT, 1.0)),
    # The distractor for `q-cat`: on the cat-and-mat direction, at more weight
    # than `cat` and `mat` each carry.
    "dog": axis((CAT, 1.0), (MAT, 1.0)),
    "alpha": axis((ALPHA, 1.0)),
    "gamma": axis((GAMMA, 1.0)),
    # The distractor for `q-greek`: heavier still, so that three short
    # passages carrying it crowd the answer out of a `top_k` of 3.
    "delta": axis((ALPHA, 2.0), (GAMMA, 2.0)),
    "short": axis((TEXT, 1.0)),
    "text": axis((TEXT, 1.0)),
    "river": axis((RIVER, 1.0)),
    "banks": axis((BANKS, 1.0)),
}


def token_embeddings() -> np.ndarray:
    """The token embedding table, `[vocab, hidden]`."""
    table = np.zeros((len(VOCAB), HIDDEN), dtype=np.float32)
    for index, token in enumerate(VOCAB):
        if token in SPECIALS:
            continue  # zero: a special token moves no pooled vector
        table[index] = PLACED.get(token, axis((GENERIC, 0.5)))
    return table


def ids_input(name: str) -> onnx.ValueInfoProto:
    return helper.make_tensor_value_info(name, TensorProto.INT64, ["batch", "sequence"])


def model_of(graph: onnx.GraphProto) -> onnx.ModelProto:
    model = helper.make_model(
        graph,
        opset_imports=[helper.make_opsetid("", OPSET)],
        producer_name="ragondin exit-criterion fixtures",
    )
    model.ir_version = 7  # the IR version opset 13 pairs with
    return model


def embedder() -> onnx.ModelProto:
    """A token-embedding table and a Gather: `[batch, sequence, hidden]` out.

    `attention_mask` is declared and unused — the component pools with it, the
    model does not need it — exactly as the embedder crate's own fixture does.
    """
    weights = numpy_helper.from_array(token_embeddings(), name="token_embeddings")
    gather = helper.make_node(
        "Gather", ["token_embeddings", "input_ids"], ["last_hidden_state"], axis=0
    )
    graph = helper.make_graph(
        [gather],
        "exit_criterion_embedder",
        [ids_input("input_ids"), ids_input("attention_mask")],
        [
            helper.make_tensor_value_info(
                "last_hidden_state", TensorProto.FLOAT, ["batch", "sequence", HIDDEN]
            )
        ],
        [weights],
    )
    return model_of(graph)


def cross_encoder() -> onnx.ModelProto:
    """Lexical overlap as a cross-encoder: one logit per pair.

    Inputs `input_ids`, `attention_mask`, `token_type_ids` of `[batch,
    sequence]`; output `logits` of `[batch, 1]`. `same[b, i, j]` is 1 where
    positions i and j hold the same id; keeping the (query row, passage column)
    cells and summing them is the score.
    """
    axis_1 = numpy_helper.from_array(np.array([1], dtype=np.int64), name="axis_1")
    axis_2 = numpy_helper.from_array(np.array([2], dtype=np.int64), name="axis_2")
    nodes = [
        # Which positions are the passage, and which the query — padding
        # excluded by the attention mask.
        helper.make_node(
            "Mul", ["token_type_ids", "attention_mask"], ["passage_positions"]
        ),
        helper.make_node(
            "Sub", ["attention_mask", "passage_positions"], ["query_positions"]
        ),
        helper.make_node(
            "Cast", ["query_positions"], ["query_weights"], to=TensorProto.FLOAT
        ),
        helper.make_node(
            "Cast", ["passage_positions"], ["passage_weights"], to=TensorProto.FLOAT
        ),
        # Every position against every other.
        helper.make_node("Unsqueeze", ["input_ids", "axis_2"], ["ids_rows"]),
        helper.make_node("Unsqueeze", ["input_ids", "axis_1"], ["ids_columns"]),
        helper.make_node("Equal", ["ids_rows", "ids_columns"], ["same"]),
        helper.make_node("Cast", ["same"], ["same_weights"], to=TensorProto.FLOAT),
        # Keep only the (query position, passage position) cells.
        helper.make_node("Unsqueeze", ["query_weights", "axis_2"], ["query_rows"]),
        helper.make_node(
            "Unsqueeze", ["passage_weights", "axis_1"], ["passage_columns"]
        ),
        helper.make_node("Mul", ["same_weights", "query_rows"], ["query_matches"]),
        helper.make_node(
            "Mul", ["query_matches", "passage_columns"], ["overlap_cells"]
        ),
        # Count them, leaving one score per pair.
        helper.make_node(
            "ReduceSum", ["overlap_cells", "axis_2"], ["overlap_rows"], keepdims=0
        ),
        helper.make_node(
            "ReduceSum", ["overlap_rows", "axis_1"], ["pair_score"], keepdims=1
        ),
        helper.make_node("Identity", ["pair_score"], ["logits"]),
    ]
    graph = helper.make_graph(
        nodes,
        "exit_criterion_cross_encoder",
        [
            ids_input("input_ids"),
            ids_input("attention_mask"),
            ids_input("token_type_ids"),
        ],
        [helper.make_tensor_value_info("logits", TensorProto.FLOAT, ["batch", 1])],
        [axis_1, axis_2],
    )
    return model_of(graph)


def tokenizer() -> dict:
    """One BERT-style WordPiece tokenizer, shared by both models.

    Lowercasing, whitespace splitting, and the `[CLS] a [SEP]` / `[CLS] a [SEP]
    b [SEP]` post-processor a BERT export ships with: the cross-encoder reads
    `token_type_ids` to tell the pair apart, and the embedder, which reads no
    such thing, pools the zero rows of `[CLS]` and `[SEP]` along with the rest:
    they count in the mean's denominator and contribute nothing to its
    direction, which is all cosine similarity sees.
    """
    return {
        "version": "1.0",
        "truncation": None,
        "padding": None,
        "added_tokens": [
            {
                "id": VOCAB.index(token),
                "content": token,
                "single_word": False,
                "lstrip": False,
                "rstrip": False,
                "normalized": False,
                "special": True,
            }
            for token in SPECIALS
        ],
        "normalizer": {
            "type": "BertNormalizer",
            "clean_text": True,
            "handle_chinese_chars": True,
            "strip_accents": None,
            "lowercase": True,
        },
        "pre_tokenizer": {"type": "BertPreTokenizer"},
        "post_processor": {
            "type": "BertProcessing",
            "sep": ["[SEP]", VOCAB.index("[SEP]")],
            "cls": ["[CLS]", VOCAB.index("[CLS]")],
        },
        "decoder": None,
        "model": {
            "type": "WordPiece",
            "unk_token": "[UNK]",
            "continuing_subword_prefix": "##",
            "max_input_chars_per_word": 100,
            "vocab": {token: index for index, token in enumerate(VOCAB)},
        },
    }


def write_model(model: onnx.ModelProto, name: str) -> None:
    onnx.checker.check_model(model)
    (HERE / name).write_bytes(model.SerializeToString())
    print(f"wrote {HERE / name}")


def main() -> None:
    write_model(embedder(), "embedder.onnx")
    write_model(cross_encoder(), "cross-encoder.onnx")
    path = HERE / "tokenizer.json"
    path.write_text(json.dumps(tokenizer(), indent=1, ensure_ascii=False) + "\n")
    print(f"wrote {path}")


if __name__ == "__main__":
    main()

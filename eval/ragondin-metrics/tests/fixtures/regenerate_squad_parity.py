#!/usr/bin/env python3
"""Regenerate `squad_parity.tsv`, the frozen parity fixture for exact match and
token-F1 (ADR-C30 § 1).

ADR-C30 makes the official SQuAD v1.1 evaluation script the reference
implementation of `exact_match`, `token_f1` and the answer normalisation in
`ragondin-metrics`, as ADR-10 makes `trec_eval` the reference for retrieval. So
the values in the `.tsv` are *not* hand-computed and are not this project's
opinion: this script feeds each case to the script's own `normalize_answer`,
`exact_match_score`, `f1_score` and `metric_max_over_ground_truths`.

The script it calls, pinned by ADR-C30:

    URL:     https://worksheets.codalab.org/rest/bundles/0xbcd57bee090b421c982906709c8c27e1/contents/blob/
    Mirror:  https://raw.githubusercontent.com/allenai/bi-att-flow/master/squad/evaluate-v1.1.py
    SHA-256: f5a673dbbd173e29e9ea38f1b2091d883583b77b3a4c17144b223fb0f2f9bd09

It is downloaded at run time (the pinned URL first, then the byte-identical
mirror), refused unless its SHA-256 matches, and never committed. Nothing but
the Python standard library is needed:

    cd eval/ragondin-metrics/tests/fixtures
    python3 regenerate_squad_parity.py > squad_parity.tsv

Offline, pass a local copy instead; its SHA-256 is checked all the same:

    python3 regenerate_squad_parity.py path/to/evaluate-v1.1.py > squad_parity.tsv

Last run under **Python 3.14.7** (Unicode 16.0.0). The Python version matters:
`str.lower`, `str.split` and the regular expression's `\\b` read Unicode tables
that change between versions, and ADR-C30 pins Python 3 rather than a minor
version. The fixture's header records the version that produced it.

The output is deterministic: the random cases use a fixed seed and every case is
emitted in a fixed order, so regenerating under the same Python without editing
this script produces a byte-identical file. Regenerate only to *add* coverage —
a diff on an existing line is a finding, not a refresh.

What is NOT in here: a case with no reference at all. The script is undefined
over an empty reference list (`max` of an empty list raises), ADR-C30 makes such
a query unjudged, and so there is no reference value to freeze. Nor a case that
depends on a character whose Unicode properties Python and Rust disagree about;
`src/generation.rs` names that divergence and a unit test pins it.
"""

import hashlib
import importlib.util
import random
import sys
import unicodedata
import urllib.request

URLS = [
    "https://worksheets.codalab.org/rest/bundles/0xbcd57bee090b421c982906709c8c27e1/contents/blob/",
    "https://raw.githubusercontent.com/allenai/bi-att-flow/master/squad/evaluate-v1.1.py",
]
SHA256 = "f5a673dbbd173e29e9ea38f1b2091d883583b77b3a4c17144b223fb0f2f9bd09"

SEED = 20260924
N_RANDOM = 200

# Cases chosen by hand, each pinning a decision the script makes that a random
# case would hit only by luck. `label` is carried into the .tsv as a comment so
# a failure names what the case was protecting. The first ten are the edge
# cases ADR-C30 § 1 requires, in its order.
HAND_PICKED = [
    # ASCII punctuation beside non-ASCII punctuation: only `string.punctuation`
    # goes, so the curly quotes and the em dash survive as tokens or suffixes.
    ("ASCII punctuation removed, non-ASCII kept", "“Hello,” she said — twice!", ["hello she said twice"]),
    ("non-ASCII quote beside ASCII quote", "Rock ’n’ Roll's", ["rock n rolls", "rock ’n’ rolls"]),
    ("a typographic dash is not a hyphen", "1990–1995", ["19901995", "1990 1995"]),
    # A leading article.
    ("leading article", "The Eiffel Tower", ["Eiffel Tower"]),
    ("leading article, capitalised", "A Tale of Two Cities", ["tale of two cities"]),
    ("an is an article, and is not", "an apple and an orange", ["apple and orange"]),
    # An article directly before a non-ASCII quote: `\b` sees the quote as a
    # non-word character, so "the" is a whole word and goes.
    ("article before a non-ASCII quote", "the’s", ["’s", "the s"]),
    ("article after a non-ASCII quote", "‘the’ end", ["end", "‘ ’ end"]),
    # A non-breaking space splits like any other whitespace.
    ("non-breaking space", "New York", ["new york"]),
    ("non-breaking space beside an article", "the city", ["city"]),
    # A hyphenated name: punctuation goes before articles, so the hyphen is
    # removed rather than replaced, and "the-man" keeps its article.
    ("hyphenated name", "Bankman-Fried", ["bankmanfried", "Bankman Fried"]),
    ("hyphen hides an article", "the-man", ["theman", "man"]),
    # A Greek capital sigma in final position lowers to the final form.
    ("final capital sigma", "ΟΔΟΣ", ["οδος"]),
    ("final capital sigma against medial form", "ΟΔΟΣ", ["οδοσ"]),
    ("capital sigma final before punctuation", "ΑΣ.", ["ας"]),
    ("capital sigma after punctuation", "Α.Σ", ["ας"]),
    ("capital sigma medial", "ΣΑΣΑ", ["σασα"]),
    # An accented word adjacent to an article: accented letters are word
    # characters, so no boundary opens inside them.
    ("accented word after an article", "the élan", ["élan"]),
    ("accent glued to an article", "thé aé", ["thé aé"]),
    ("article glued to an accented word", "àthe café", ["àthe café", "café"]),
    ("decomposed accent after an article", "á café", ["́ café", "café"]),
    # An empty prediction.
    ("empty prediction", "", ["Paris"]),
    ("empty prediction, several references", "", ["Paris", "Lyon"]),
    # Several references, each metric taking its own maximum.
    ("several references, maximum taken", "the big red dog", ["red dog", "big red dog barks", "a cat"]),
    ("exact match from one reference, F1 from another", "red dog", ["cat", "Red  Dog!", "red dog red"]),
    ("F1 maximum not on the exact-match reference", "red big dog", ["blue", "big red dog", "red dog"]),
    # Both sides normalising to empty: exact match 1, F1 0 (v1.1, not v2.0).
    ("both normalise to empty (the reference '.')", ".", ["."]),
    ("empty prediction against an article-only reference", "", ["The"]),
    ("punctuation-only prediction against punctuation-only reference", "?!", ["..."]),
    ("empty-normalising reference beside a real one", "", [".", "Paris"]),
    # Punctuation inside a token.
    ("punctuation inside a token", "U.S.A.", ["USA", "U S A"]),
    ("thousands separator", "1,000", ["1000", "1 000"]),
    ("apostrophe inside a token", "rock'n'roll", ["rocknroll"]),
    ("underscore is punctuation", "snake_case", ["snakecase"]),
    # Unicode beyond the ADR's list.
    ("accented capitals lower", "ÉCOLE", ["école"]),
    ("sharp s does not fold", "Straße", ["STRASSE", "straße"]),
    ("dotted capital I lowers to two characters", "İstanbul", ["istanbul", "i̇stanbul"]),
    ("ideographic space", "Tokyo　Tower", ["tokyo tower"]),
    ("em space and line separator", "one two three", ["one two three"]),
    ("zero-width space is not whitespace", "one​two", ["one two", "one​two"]),
    ("information separators are whitespace", "new\u001fyork\u001cnow", ["new york now"]),
    ("tab and newline", "new\tyork\ncity", ["new york city"]),
    ("digits make an article part of a word", "the1 a2 3an", ["the1 a2 3an"]),
    ("non-Latin script", "東京 タワー", ["東京"]),
    # Arithmetic.
    ("repeated tokens count as a multiset", "the cat cat", ["cat"]),
    ("partial overlap", "the quick brown fox", ["a quick red fox jumps"]),
    ("no overlap", "Paris", ["London"]),
    ("identical", "Denver Broncos", ["Denver Broncos"]),
    ("identical after normalisation only", "  The DENVER, Broncos!  ", ["denver broncos"]),
]

# Fragments the random cases are drawn from: words, articles in every case, the
# punctuation and whitespace the hand-picked cases single out, and a few
# non-ASCII characters on each side of every boundary the normalisation has.
WORDS = [
    "the", "The", "THE", "a", "A", "an", "An", "and", "then", "at", "cat",
    "Cat's", "dog", "red", "Paris", "naïve", "café", "élan",
    "ΟΔΟΣ", "Σ", "İ", "Straße", "1,000",
    "U.S.", "Bankman-Fried", "rock'n'roll", "東京", "42", "_",
]
SEPARATORS = [" ", " ", " ", "  ", " ", "\t", "", "-", ".", ",", "’",
              "“", "—", "\u001f", "　", "​"]


def load_script(path):
    """The pinned SQuAD script as a module, refused unless its SHA-256 matches."""
    if path is None:
        source = None
        for url in URLS:
            try:
                with urllib.request.urlopen(url, timeout=30) as response:
                    source = response.read()
                break
            except OSError as error:
                print(f"could not fetch {url}: {error}", file=sys.stderr)
        if source is None:
            sys.exit("no copy of the SQuAD script could be downloaded")
    else:
        with open(path, "rb") as f:
            source = f.read()
    digest = hashlib.sha256(source).hexdigest()
    if digest != SHA256:
        sys.exit(f"SHA-256 mismatch: got {digest}, ADR-C30 pins {SHA256}")
    spec = importlib.util.spec_from_loader("squad_v1_1", loader=None)
    module = importlib.util.module_from_spec(spec)
    exec(compile(source, "evaluate-v1.1.py", "exec"), module.__dict__)
    return module


def escape(field):
    """One field, readable and TSV-safe.

    A backslash doubles; any character that is not printable (tab, newline,
    every space but the ASCII one, zero-width characters) is written
    `\\u{hex}`, as is a space at either end of the field, which an editor could
    otherwise strip. `squad_parity.rs` decodes exactly this.
    """
    out = []
    last = len(field) - 1
    for i, ch in enumerate(field):
        if ch == "\\":
            out.append("\\\\")
        elif not ch.isprintable() or (ch == " " and i in (0, last)):
            out.append(f"\\u{{{ord(ch):x}}}")
        else:
            out.append(ch)
    return "".join(out)


def random_cases():
    rng = random.Random(SEED)

    def phrase():
        parts = []
        for _ in range(rng.randint(0, 5)):
            parts.append(rng.choice(WORDS))
            parts.append(rng.choice(SEPARATORS))
        return "".join(parts)

    for _ in range(N_RANDOM):
        answer = phrase()
        references = [phrase() for _ in range(rng.randint(1, 3))]
        yield "", answer, references


def main():
    script = load_script(sys.argv[1] if len(sys.argv) > 1 else None)
    out = sys.stdout
    out.write(
        "# Expected values produced by the official SQuAD v1.1 evaluation\n"
        "# script (ADR-C30 § 1). Frozen CI regression fixture — see\n"
        "# regenerate_squad_parity.py, and do not edit a value by hand.\n"
        f"# Generated under Python {sys.version.split()[0]}"
        f" (Unicode {unicodedata.unidata_version}).\n"
        "#\n"
        "# exact_match <TAB> token_f1 <TAB> normalised answer <TAB> answer"
        " <TAB> reference [<TAB> reference ...]\n"
        "#\n"
        "# exact_match and token_f1 are metric_max_over_ground_truths over\n"
        "# every reference; the normalised answer is normalize_answer(answer).\n"
        "# Fields are escaped: `\\\\` is a backslash, `\\u{hex}` any\n"
        "# non-printable character or a space at either end of a field.\n"
    )
    for label, answer, references in list(HAND_PICKED) + list(random_cases()):
        if label:
            out.write(f"\n# {label}\n")
        em = script.metric_max_over_ground_truths(
            script.exact_match_score, answer, references)
        f1 = script.metric_max_over_ground_truths(
            script.f1_score, answer, references)
        fields = [
            "1" if em else "0",
            repr(float(f1)),
            escape(script.normalize_answer(answer)),
            escape(answer),
            *(escape(r) for r in references),
        ]
        out.write("\t".join(fields) + "\n")


if __name__ == "__main__":
    main()

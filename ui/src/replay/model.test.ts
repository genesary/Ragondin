import { describe, expect, it } from 'vitest';
import { DENSE, DENSE_GRAPH, DENSE_TRACE, FAILED_TRACE, HYBRID, HYBRID_GRAPH, HYBRID_QUERIES, HYBRID_TRACE, LISTING, withPassages } from './fixtures.ts';
import { candidates, counterpart, editorTarget, firstJudged, formatMs, listOf, matching, overlayOf, passagesBanner, runName, terminalOf, verdict } from './model.ts';

describe('overlayOf, building the canvas overlay from a trace', () => {
  const overlay = overlayOf({ graph: HYBRID_GRAPH, trace: HYBRID_TRACE, metric: 'ndcg@10' });

  it("gives every node that ran its duration and its share of the query's time", () => {
    // 9 + 40 + 1 + 349 + 1 + 600 = 1000 ms.
    expect(overlay['rerank']).toMatchObject({ durationMs: 349, share: 0.349 });
    expect(overlay['answer']).toMatchObject({ durationMs: 600, share: 0.6 });
  });

  it('gives a ranking node its metric for this query, to four decimals, and nothing to a node without one', () => {
    expect(overlay['rerank']?.metric).toEqual({ name: 'ndcg@10', value: '0.8610' });
    expect(overlay['context']?.metric).toBeUndefined();
  });

  it("maps the API's gold ranks onto the rank strip's cells, and draws no strip for a node with no ranking", () => {
    expect(overlay['bm25']?.ranks).toEqual([2, 4]);
    expect(overlay['answer']?.ranks).toBeUndefined();
  });

  it('counts what a node discarded of what its upstream ranked, and nothing for a retriever', () => {
    expect(overlay['rerank']?.discarded).toBe(4);
    expect(overlay['rrf']?.discarded).toBe(0);
    // One rule for every node fed chunks: the context builder's discards are counted too.
    expect(overlay['context']?.discarded).toBe(1);
    expect(overlay['bm25']?.discarded).toBeUndefined();
  });

  it('draws no overlay on a declared input', () => {
    expect(overlay['question']).toBeUndefined();
  });

  it('shows a failed node with its error, and every node after it as not run', () => {
    const failed = overlayOf({ graph: HYBRID_GRAPH, trace: FAILED_TRACE, metric: 'ndcg@10' });
    expect(failed['rerank']?.error).toBe('The service at 127.0.0.1:7001 did not answer within 30 s.');
    expect(failed['context']).toEqual({ notRun: true });
    expect(failed['answer']).toEqual({ notRun: true });
    expect(failed['rrf']?.ranks).toEqual([1, 2]);
  });

  it('tags the nodes absent from the run beside it with that run’s letter', () => {
    const beside = overlayOf({ graph: HYBRID_GRAPH, trace: HYBRID_TRACE, metric: 'ndcg@10', other: { graph: DENSE_GRAPH, letter: 'B' } });
    for (const id of ['bm25', 'rrf', 'rerank', 'context', 'answer']) expect(beside[id]?.onlyHere, id).toBe('only in A');
    expect(beside['dense']?.onlyHere).toBeUndefined();
    const dense = overlayOf({ graph: DENSE_GRAPH, trace: DENSE_TRACE, metric: 'ndcg@10', other: { graph: HYBRID_GRAPH, letter: 'A' }, letter: 'B' });
    expect(dense['dense']?.onlyHere).toBeUndefined();
  });
});

describe('formatMs', () => {
  it('rounds to a tenth below 10 ms and to a whole millisecond above', () => {
    expect(formatMs(349_400_000)).toBe(349);
    expect(formatMs(1_240_000)).toBe(1.2);
    expect(formatMs(40_000)).toBe(0);
  });
});

describe('listOf, what a node produced in rank order', () => {
  it('lists a retriever’s chunks in rank order with their grades and text, and no moves, since nothing was upstream', () => {
    const list = listOf(HYBRID_GRAPH, HYBRID_TRACE, 'bm25');
    expect(list?.kept.map((i) => [i.rank, i.chunk, i.grade, i.move])).toEqual([
      [1, 'c3', 0, null],
      [2, 'c2', 2, null],
      [3, 'c7', 0, null],
      [4, 'c5', 1, null],
      [5, 'c9', 0, null],
    ]);
    expect(list?.kept[0]?.text).toBe('Passage 3 of the corpus.');
    expect(list?.discarded).toEqual([]);
  });

  it("marks each of the reranker's chunks moved up or down against its upstream list, and its discarded chunks with their former rank", () => {
    const list = listOf(HYBRID_GRAPH, HYBRID_TRACE, 'rerank');
    expect(list?.kept.map((i) => [i.rank, i.chunk, i.move, i.former])).toEqual([
      [1, 'c5', 'up', 2],
      [2, 'c2', 'down', 1],
      [3, 'c4', 'up', 4],
      [4, 'c3', 'down', 3],
    ]);
    expect(list?.discarded.map((i) => [i.chunk, i.former])).toEqual([
      ['c7', 5],
      ['c6', 6],
      ['c9', 7],
      ['c8', 8],
    ]);
  });

  it('reads a fusion against the best rank each chunk had in any of its legs', () => {
    const list = listOf(HYBRID_GRAPH, HYBRID_TRACE, 'rrf');
    // c5 was 4th in bm25 and 3rd in dense: its best is 3, and rrf puts it 2nd.
    expect(list?.kept.find((i) => i.chunk === 'c5')).toMatchObject({ rank: 2, former: 3, move: 'up' });
    // c2 was 1st in dense and stays 1st.
    expect(list?.kept.find((i) => i.chunk === 'c2')).toMatchObject({ rank: 1, former: 1, move: 'same' });
  });

  it("lists a context's chunks in the builder's order, and what it left out of its upstream", () => {
    const list = listOf(HYBRID_GRAPH, HYBRID_TRACE, 'context');
    expect(list?.kept.map((i) => i.chunk)).toEqual(['c5', 'c2', 'c4']);
    expect(list?.discarded.map((i) => [i.chunk, i.former])).toEqual([['c3', 4]]);
  });

  it('is null for a node that produced no chunks: an answer, a failure', () => {
    expect(listOf(HYBRID_GRAPH, HYBRID_TRACE, 'answer')).toBeNull();
    expect(listOf(HYBRID_GRAPH, FAILED_TRACE, 'rerank')).toBeNull();
  });
});

describe('counterpart, the node beside the selected one', () => {
  it('is the node of the same id when the other run has it', () => {
    expect(counterpart('dense', { graph: DENSE_GRAPH, trace: DENSE_TRACE })).toEqual({ kind: 'same', node: 'dense' });
  });

  it("is the other run's final output when it has no such node, labelled as such", () => {
    expect(counterpart('rerank', { graph: DENSE_GRAPH, trace: DENSE_TRACE })).toEqual({ kind: 'final', node: 'dense' });
  });

  it("is the other run's last node that ran when it failed before its end", () => {
    expect(counterpart('generator-x', { graph: HYBRID_GRAPH, trace: FAILED_TRACE })).toEqual({ kind: 'final', node: 'rerank' });
  });
});

describe('terminalOf', () => {
  it('is the node no edge leaves', () => {
    expect(terminalOf(HYBRID_GRAPH)).toBe('answer');
    expect(terminalOf(DENSE_GRAPH)).toBe('dense');
  });
});

describe('verdict, the final node’s sentence', () => {
  it('says the query’s score and where the first gold passage landed, for one run', () => {
    expect(verdict({ metric: 'ndcg@10', a: { score: 0.861, gold: [1, 2] } })).toBe('ndcg@10 is 0.8610 on this query, with 2 gold documents in the top 10, the first at document rank 1.');
  });

  it('says when the ranking holds no gold passage, and when the query is not judged', () => {
    expect(verdict({ metric: 'ndcg@10', a: { score: 0, gold: [] } })).toBe('ndcg@10 is 0.0000 on this query, with no gold document in the ranking.');
    expect(verdict({ metric: 'ndcg@10', a: { score: undefined, gold: null } })).toBe('This query is not scored on ndcg@10, so there is no verdict.');
  });

  it('compares the two runs side by side: both scores, the difference, and both first gold ranks', () => {
    expect(verdict({ metric: 'ndcg@10', a: { score: 0.861, gold: [1, 2] }, b: { score: 0.6131, gold: [3] } })).toBe(
      'ndcg@10 is 0.8610 in A and 0.6131 in B, 0.2479 higher in A; the first gold document is at document rank 1 in A and 3 in B.',
    );
    expect(verdict({ metric: 'ndcg@10', a: { score: 0.5, gold: [2] }, b: { score: 0.5, gold: [] } })).toBe(
      'ndcg@10 is 0.5000 in A and 0.5000 in B, the same in both; the first gold document is at document rank 2 in A, and B ranks none.',
    );
    expect(verdict({ metric: 'ndcg@10', a: { score: 0.2, gold: [9] }, b: { score: undefined, gold: null } })).toBe('ndcg@10 is 0.2000 in A; B is not scored on it for this query.');
  });
});

describe('the toolbar’s choices', () => {
  it('offers beside a run only the other runs on its benchmark: a run on another benchmark is not offered', () => {
    expect(candidates(LISTING, HYBRID).map((r) => r.id)).toEqual([DENSE, 'f'.repeat(64)]);
  });

  it('opens on the first judged query', () => {
    expect(firstJudged([{ ...HYBRID_QUERIES.queries[2]! }, ...HYBRID_QUERIES.queries.slice(1, 2)])?.id).toBe('q2');
    expect(firstJudged([HYBRID_QUERIES.queries[2]!])?.id).toBe('q3');
    expect(firstJudged([])).toBeNull();
  });

  it('searches the queries by their text and their id, whatever the case', () => {
    expect(matching(HYBRID_QUERIES.queries, 'ENZYME').map((q) => q.id)).toEqual(['q2']);
    expect(matching(HYBRID_QUERIES.queries, 'q3').map((q) => q.id)).toEqual(['q3']);
    expect(matching(HYBRID_QUERIES.queries, '  ').map((q) => q.id)).toEqual(['q1', 'q2', 'q3']);
  });
});

describe('passagesBanner', () => {
  it('says nothing when the passages are verified', () => {
    expect(passagesBanner(HYBRID_TRACE.passages)).toBeNull();
  });

  it('names the absent dataset, and the digest the run expected', () => {
    const banner = passagesBanner(withPassages(HYBRID_TRACE, 'dataset_absent').passages);
    expect(banner?.title).toBe('Passage text is hidden: the dataset of this run is not on disk.');
    expect(banner?.digests).toBe(`The run expects dataset ${'5'.repeat(64)}, index ${'7'.repeat(64)}. Nothing on disk matched.`);
  });

  it('names a differing dataset, and both digests', () => {
    const banner = passagesBanner(withPassages(HYBRID_TRACE, 'dataset_differs').passages);
    expect(banner?.title).toBe('Passage text is hidden: the dataset on disk differs from the one this run was evaluated on.');
    expect(banner?.digests).toBe(`The run expects dataset ${'5'.repeat(64)}, index ${'7'.repeat(64)}. On disk: dataset ${'6'.repeat(64)}.`);
  });

  it('names a dataset that does not load, and a chunk set that differs', () => {
    const base = HYBRID_TRACE.passages;
    expect(passagesBanner({ ...base, status: 'dataset_unreadable', found: null })?.title).toBe('Passage text is hidden: the dataset on disk does not load.');
    expect(passagesBanner({ ...base, status: 'index_differs', found: { dataset_version: base.expected.dataset_version, index_version: '8'.repeat(64) } })?.title).toBe(
      'Passage text is hidden: the chunk set derived from the dataset differs from the one this run used.',
    );
  });
});

describe('runName, a run as Replay names it', () => {
  const run = LISTING.runs[0]!;
  it('names a run by its recorded name first, then its first hash match, else nothing (ADR-C39 § 4)', () => {
    expect(runName({ ...run, launched_as: { name: 'hybrid', prefix_of: null, held: 'exactly' }, pipeline_names: ['hybrid-fork'] })).toBe('hybrid');
    expect(runName({ ...run, launched_as: null, pipeline_names: ['hybrid-fork'] })).toBe('hybrid-fork');
    expect(runName({ ...run, launched_as: { name: null, prefix_of: null, held: null }, pipeline_names: [] })).toBeNull();
  });
});

describe('editorTarget, the stored document the editor opens a run on', () => {
  const run = LISTING.runs[0]!;
  const launched = (name: string, held: 'exactly' | 'gone') => ({ name, held, prefix_of: null });

  it('is a document whose content is what the run ran', () => {
    expect(editorTarget(run)).toEqual({ name: 'hybrid-rerank-gen' });
  });

  it('is the name the run was launched as when that document is among them, else the first', () => {
    expect(editorTarget({ ...run, pipeline_names: ['a-copy', 'hybrid'], launched_as: launched('hybrid', 'exactly') })).toEqual({ name: 'hybrid' });
    expect(editorTarget({ ...run, pipeline_names: ['a-copy', 'b-copy'], launched_as: launched('hybrid', 'exactly') })).toEqual({ name: 'a-copy' });
  });

  it('is never a name the API refuses to read', () => {
    expect(editorTarget({ ...run, pipeline_names: ['Hybrid', 'hybrid', 'z'], refused_pipeline_names: ['Hybrid', 'hybrid'] })).toEqual({ name: 'z' });
  });

  it('is none, with the reason, when no document can be opened on it', () => {
    expect(editorTarget({ ...run, pipeline_names: [] })).toEqual({ reason: 'No pipeline document in the workspace holds what this run ran. Fork this run to edit it.' });
    expect(editorTarget({ ...run, pipeline_names: [], launched_as: launched('hybrid', 'exactly') })).toEqual({ reason: 'hybrid has changed since this run, and no pipeline document holds what it ran. Fork this run to edit it.' });
    // A name gone from the workspace is no document at all.
    expect(editorTarget({ ...run, pipeline_names: [], launched_as: launched('hybrid', 'gone') })).toEqual({ reason: 'No pipeline document in the workspace holds what this run ran. Fork this run to edit it.' });
    expect(editorTarget({ ...run, pipeline_names: ['Hybrid', 'hybrid'], refused_pipeline_names: ['Hybrid', 'hybrid'] })).toEqual({
      reason: 'Hybrid and hybrid hold what this run ran, but each differs from another stored name only in case, so neither can be read. Fork this run to edit it.',
    });
  });
});

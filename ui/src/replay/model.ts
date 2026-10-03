// What the Replay screen draws from one query's trace: pure functions over
// the API's answers, so the screen only lays them out. Nothing here
// recomputes a metric or a gold rank — those are the API's (Scope — OUT of
// the screen's issue); what is computed is presentation: shares of time,
// moves between a node and its upstream, the counterpart beside a node, and
// the sentences. ARCHITECTURE.md § The Replay screen.
import type { DatasetCheck, Graph, QueryScores, QueryTrace, RunListing, RunSummary, TracePassage, TraceNodeView } from '../api/types.ts';
import type { NodeOverlay } from '../canvas/index.ts';

/**
 * The name a run goes by on this screen: the name its launch record gives,
 * else the first current document holding its hash, else none — ADR-C39
 * § 4's order, the Runs screen's grouping order.
 */
export const runName = (run: Pick<RunSummary, 'launched_as' | 'pipeline_names'>): string | null => run.launched_as?.name ?? run.pipeline_names[0] ?? null;

/** A metric value as every screen prints a ranking metric: four decimals. */
export const formatScore = (value: number) => value.toFixed(4);

/** A duration in milliseconds, as a card prints it: a tenth below 10 ms, whole above. */
export function formatMs(nanos: number): number {
  const ms = nanos / 1_000_000;
  return ms < 10 ? Math.round(ms * 10) / 10 : Math.round(ms);
}

/** The chunks a node produced, ranked or placed in a context; null for anything else. */
function chunksOf(node: TraceNodeView | undefined): TracePassage[] | null {
  const out = node?.output;
  if (out === undefined || out === null) return null;
  return out.kind === 'ranking' || out.kind === 'context' ? out.chunks : null;
}

/** The nodes feeding `id` chunks, in port order: what its list is read against. */
function upstreamOf(graph: Graph, id: string): string[] {
  return graph.edges.filter((e) => e.to === id && e.kind === 'chunks').sort((a, b) => a.port - b.port).map((e) => e.from);
}

/**
 * Each chunk's best rank, 1-based, across the upstream lists of `id` in this
 * trace; null when nothing upstream ranked chunks (a retriever).
 */
function formerRanks(graph: Graph, trace: QueryTrace, id: string): Map<string, number> | null {
  const lists = upstreamOf(graph, id).flatMap((from) => {
    const chunks = chunksOf(trace.nodes.find((n) => n.node === from));
    return chunks === null ? [] : [chunks];
  });
  if (lists.length === 0) return null;
  const best = new Map<string, number>();
  for (const list of lists) {
    list.forEach((p, i) => {
      const held = best.get(p.chunk);
      if (held === undefined || i + 1 < held) best.set(p.chunk, i + 1);
    });
  }
  return best;
}

export type Move = 'up' | 'down' | 'same' | 'new';

/** One chunk in a node's list: where it is, where it was upstream, and what it is. */
export type ListItem = TracePassage & {
  /** Its rank in this node's list, 1-based; for a discarded chunk, its former rank. */
  rank: number;
  /** Its best rank upstream; null when nothing was upstream. */
  former: number | null;
  /** How it moved against its upstream; null when nothing was upstream. */
  move: Move | null;
};

/** What a node produced, in rank order, and what it discarded of its upstream, by former rank. */
export type NodeList = { kept: ListItem[]; discarded: ListItem[] };

/**
 * What node `id` produced in rank order — its ranking, or its context's
 * chunks in the builder's order — each marked against its upstream list from
 * the same trace, and the upstream chunks it did not keep. Null when it
 * produced no chunks: an answer, a failure, a node that did not run.
 */
export function listOf(graph: Graph, trace: QueryTrace, id: string): NodeList | null {
  const chunks = chunksOf(trace.nodes.find((n) => n.node === id));
  if (chunks === null) return null;
  const former = formerRanks(graph, trace, id);
  const kept = chunks.map((p, i): ListItem => {
    const was = former?.get(p.chunk);
    const rank = i + 1;
    const move: Move | null = former === null ? null : was === undefined ? 'new' : was > rank ? 'up' : was < rank ? 'down' : 'same';
    return { ...p, rank, former: was ?? null, move };
  });
  if (former === null) return { kept, discarded: [] };
  const held = new Set(chunks.map((p) => p.chunk));
  const seen = new Set<string>();
  const discarded: ListItem[] = [];
  for (const from of upstreamOf(graph, id)) {
    for (const p of chunksOf(trace.nodes.find((n) => n.node === from)) ?? []) {
      if (held.has(p.chunk) || seen.has(p.chunk)) continue;
      seen.add(p.chunk);
      const was = former.get(p.chunk)!;
      discarded.push({ ...p, rank: was, former: was, move: null });
    }
  }
  discarded.sort((a, b) => a.rank - b.rank);
  return { kept, discarded };
}

/** The node no edge leaves: the pipeline's output. The last in node order when several are. */
export function terminalOf(graph: Graph): string | null {
  const feeding = new Set(graph.edges.map((e) => e.from));
  return graph.nodes.filter((n) => !feeding.has(n.id)).at(-1)?.id ?? null;
}

export type OverlayInput = {
  graph: Graph;
  trace: QueryTrace;
  /** The per-node metric chosen in the toolbar. */
  metric: string | null;
  /** This run's letter in side by side: A, or B for the run beside. */
  letter?: string;
  /** The run beside, whose nodes this run's are tagged against. */
  other?: { graph: Graph; letter: string };
};

/**
 * Per node, what its card shows for this query: duration and share of the
 * query's time, the chosen metric, the gold ranks as the rank strip's cells,
 * the discarded count, a failure; a node of the graph absent from the trace
 * did not run. A declared input has none. Side by side, a node the other run
 * lacks wears "only in <letter>".
 */
export function overlayOf({ graph, trace, metric, letter = 'A', other }: OverlayInput): Record<string, NodeOverlay> {
  const total = trace.nodes.reduce((sum, n) => sum + n.duration_nanos, 0);
  const elsewhere = other === undefined ? null : new Set([...other.graph.inputs.map((i) => i.id), ...other.graph.nodes.map((n) => n.id)]);
  const out: Record<string, NodeOverlay> = {};
  for (const node of graph.nodes) {
    const ran = trace.nodes.find((n) => n.node === node.id);
    const tag = elsewhere !== null && !elsewhere.has(node.id) ? { onlyHere: `only in ${letter}` } : {};
    if (ran === undefined) {
      out[node.id] = { notRun: true, ...tag };
      continue;
    }
    const value = metric === null ? undefined : ran.metrics?.[metric];
    const list = ran.error === null ? listOf(graph, trace, node.id) : null;
    const hasUpstream = upstreamOf(graph, node.id).length > 0;
    out[node.id] = {
      durationMs: formatMs(ran.duration_nanos),
      ...(total > 0 ? { share: ran.duration_nanos / total } : {}),
      ...(value === undefined || metric === null ? {} : { metric: { name: metric, value: formatScore(value) } }),
      ...(ran.gold_ranks === null ? {} : { ranks: ran.gold_ranks }),
      // One rule for every node fed chunks, a context builder included: what it did not keep.
      ...(list !== null && hasUpstream ? { discarded: list.discarded.length } : {}),
      ...(ran.error === null ? {} : { error: ran.error }),
      ...tag,
    };
  }
  return out;
}

/** The node beside the selected one: the same id when the other run has it, else its final output. */
export type Counterpart = { kind: 'same'; node: string } | { kind: 'final'; node: string };

/**
 * The counterpart of node `id` in the other run: the node of the same id, or
 * — when it has none — the other run's final output: the last node that ran,
 * which is its terminal node, or the node it failed at. Null when nothing ran.
 */
export function counterpart(id: string, other: { graph: Graph; trace: QueryTrace }): Counterpart | null {
  if (other.graph.nodes.some((n) => n.id === id) || other.graph.inputs.some((i) => i.id === id)) return { kind: 'same', node: id };
  const last = other.trace.nodes.at(-1);
  return last === undefined ? null : { kind: 'final', node: last.node };
}

/** One run's reading for the verdict: its score on the metric at its output, and its ranking node's gold ranks. */
export type Reading = { score: number | undefined; gold: readonly number[] | null };

const firstGold = (gold: readonly number[] | null) => (gold === null || gold.length === 0 ? null : Math.min(...gold));
const inTop = (gold: readonly number[]) => gold.filter((r) => r >= 1 && r <= 10).length;

/**
 * The final node's sentence: composed from numbers the API returned — the
 * query's score at each run's output, and the gold ranks at its ranking — in
 * plain English, counts and ranks rather than adjectives. The API's gold
 * ranks count documents, chunks folded by first occurrence, so the sentence
 * says "document rank": the list's ranks count chunks.
 */
export function verdict({ metric, a, b }: { metric: string; a: Reading; b?: Reading }): string {
  if (a.score === undefined) return `This query is not scored on ${metric}, so there is no verdict.`;
  if (b === undefined) {
    const first = firstGold(a.gold);
    if (first === null) return `${metric} is ${formatScore(a.score)} on this query, with no gold document in the ranking.`;
    const n = inTop(a.gold ?? []);
    return `${metric} is ${formatScore(a.score)} on this query, with ${n} gold document${n === 1 ? '' : 's'} in the top 10, the first at document rank ${first}.`;
  }
  if (b.score === undefined) return `${metric} is ${formatScore(a.score)} in A; B is not scored on it for this query.`;
  const diff = a.score - b.score;
  const compared = Math.abs(diff) < 5e-5 ? 'the same in both' : `${formatScore(Math.abs(diff))} higher in ${diff > 0 ? 'A' : 'B'}`;
  const [fa, fb] = [firstGold(a.gold), firstGold(b.gold)];
  const gold =
    fa === null && fb === null
      ? 'neither ranks a gold document'
      : fa === null
        ? `the first gold document is at document rank ${fb} in B, and A ranks none`
        : fb === null
          ? `the first gold document is at document rank ${fa} in A, and B ranks none`
          : `the first gold document is at document rank ${fa} in A and ${fb} in B`;
  return `${metric} is ${formatScore(a.score)} in A and ${formatScore(b.score)} in B, ${compared}; ${gold}.`;
}

/** The runs that can stand beside `run`: those of the listing on its benchmark, itself excluded. */
export function candidates(listing: RunListing, run: string): RunSummary[] {
  const dataset = listing.runs.find((r) => r.id === run)?.dataset_version;
  return dataset === undefined ? [] : listing.runs.filter((r) => r.id !== run && r.dataset_version === dataset);
}

/** The query Replay opens on when none is chosen: the first judged one, else the first. */
export function firstJudged(queries: readonly QueryScores[]): QueryScores | null {
  return queries.find((q) => Object.keys(q.scores).length > 0) ?? queries[0] ?? null;
}

/** The queries whose text or id holds `search`, whatever the case; all of them for a blank search. */
export function matching(queries: readonly QueryScores[], search: string): QueryScores[] {
  const needle = search.trim().toLowerCase();
  if (needle === '') return [...queries];
  return queries.filter((q) => q.id.toLowerCase().includes(needle) || (q.text ?? '').toLowerCase().includes(needle));
}

const HIDDEN: Record<Exclude<DatasetCheck['status'], 'verified'>, string> = {
  dataset_absent: 'the dataset of this run is not on disk',
  dataset_differs: 'the dataset on disk differs from the one this run was evaluated on',
  dataset_unreadable: 'the dataset on disk does not load',
  index_differs: 'the chunk set derived from the dataset differs from the one this run used',
};

/** The banner over a trace whose passages are not verified: the case, and the digests on hover; null when verified. */
export function passagesBanner(check: DatasetCheck): { title: string; detail: string; digests: string } | null {
  if (check.status === 'verified') return null;
  const expected = `The run expects dataset ${check.expected.dataset_version}, index ${check.expected.index_version}.`;
  const found =
    check.found === null
      ? 'Nothing on disk matched.'
      : `On disk: dataset ${check.found.dataset_version}${check.found.index_version === null ? '' : `, index ${check.found.index_version}`}.`;
  return { title: `Passage text is hidden: ${HIDDEN[check.status]}.`, detail: check.detail, digests: `${expected} ${found}` };
}

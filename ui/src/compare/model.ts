// What the Compare screen draws, read from one `POST /compare` answer. Every
// figure here is the API's — the bins, the deltas, the stages, the pairing,
// the best of each row — and this module only lays them out: which slot a
// run takes, which metrics share the bars' 0–1 scale, how a bound or a delta
// is written, and the verdict sentence. ARCHITECTURE.md § The Compare screen.
import { familyOfComponent, type Family, type HistogramBin, type RunSeries, type RunSlot, type StackSegment } from '../../design/index.ts';
import type { Comparison, ConfigurationMatrix, MetricDeltas, MetricDirection, MetricRow, NodePair, ParameterRow, ParameterValue, StageName, UnplacedPair } from '../api/types.ts';
import { formatParameter as formatValueOf } from '../parameters.ts';
import { shortHash } from '../runs/model.ts';

const SLOTS: readonly RunSlot[] = ['base', 'a', 'b', 'c', 'd'];

/** The slot of the run at `index` in the answer's order: the baseline first, then A to D. */
export const slotOf = (index: number): RunSlot => SLOTS[index] ?? 'd';

/** The letter written beside a run's mark: "base" for the baseline, A to D for the others. */
export const letterOf = (index: number) => (index === 0 ? 'base' : slotOf(index).toUpperCase());

/** A run's name: its workspace pipeline, or its short pipeline hash when no one document matches it (the API's `null`). */
export const pipelineName = (run: Comparison['runs'][number]) => run.pipeline ?? `pipeline ${shortHash(run.pipeline_hash)}`;

/** Each run as a chart series, in the answer's order. */
export function runSeries(c: Comparison): RunSeries[] {
  return c.runs.map((run, i) => ({ id: run.id, ink: slotOf(i), short: letterOf(i), label: `${i === 0 ? 'baseline' : letterOf(i)} · ${pipelineName(run)}` }));
}

/**
 * The metrics the grouped bars draw: those read on one 0–1 scale — higher is
 * better and every recorded value lies in [0, 1]. A latency is not one of
 * them; it stays in the table, so no chart needs a second axis.
 */
export function barMetrics(rows: readonly MetricRow[]): MetricRow[] {
  return rows.filter((r) => r.direction === 'higher' && r.values.every((v) => v === null || (v >= 0 && v <= 1)));
}

const MINUS = '−';

/**
 * A value as the table prints it: four decimals where higher is better, one
 * where lower is (a latency), and as stored for a metric the API gives no
 * direction — a name its catalogue does not know, which the Runs screen
 * prints as stored too: nothing says how to round it.
 */
export const formatValue = (direction: MetricDirection | null, value: number) => {
  if (direction === null) return String(value);
  return direction === 'lower' ? value.toFixed(1) : value.toFixed(4);
};

/**
 * A delta against the baseline: its sign, what it means by the metric's
 * direction, and the arrow. A metric with no direction moved, but neither
 * better nor worse: its meaning is null.
 */
export function deltaOf(direction: MetricDirection | null, delta: number): { text: string; meaning: 'better' | 'worse' | 'same' | null; direction: 'up' | 'down' | 'none' } {
  const magnitude = formatValue(direction, Math.abs(delta));
  // A delta that prints as zero reads as unchanged: a coloured "+0.0000"
  // would claim a change the number on screen does not show.
  if (Number(magnitude) === 0) return { text: magnitude, meaning: 'same', direction: 'none' };
  const up = delta > 0;
  const text = `${up ? '+' : MINUS}${magnitude}`;
  if (direction === null) return { text, meaning: null, direction: up ? 'up' : 'down' };
  return { text, meaning: up === (direction === 'higher') ? 'better' : 'worse', direction: up ? 'up' : 'down' };
}

/** A parameter's value as a configuration writes it, its kind never lost; an unset one in words. */
export const formatParameter = (value: ParameterValue | null): string => (value === null ? 'not set' : formatValueOf(value));

/** Which of a row's values differ from the baseline's — the first. */
export function departures(row: ParameterRow): boolean[] {
  const base = JSON.stringify(row.values[0] ?? null);
  return row.values.map((v, i) => i !== 0 && JSON.stringify(v ?? null) !== base);
}

/** A parameter's name as a configuration spells it. */
export const parameterName = (key: ParameterRow['key']) => (key.kind === 'param' ? key.name : key.kind);

/** The words for the runs holding a node: "only in baseline", "only in A and B". */
export function onlyIn(present: readonly boolean[]): string {
  const names = present.flatMap((held, i) => (held ? [i === 0 ? 'baseline' : letterOf(i)] : []));
  const words = names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names.at(-1) ?? ''}`;
  return `only in ${words}`;
}

/**
 * A row of the configuration table: a node some run lacks, as one row with
 * which runs hold it — the API's `partial_nodes` — or a parameter. A
 * parameter of such a node is kept only where the runs holding it disagree,
 * and carries which runs hold the node, so a run without it reads as lacking
 * the node rather than as leaving the parameter unset.
 */
export type ConfigurationRow = { kind: 'node'; node: string; present: boolean[] } | { kind: 'param'; row: ParameterRow; present: boolean[] | null };

export function configurationRows(matrix: Extract<ConfigurationMatrix, { kind: 'compared' }>): ConfigurationRow[] {
  const partial = new Map(matrix.partial_nodes.map((p) => [p.node, p.present]));
  const out: ConfigurationRow[] = [];
  for (const row of matrix.parameters) {
    const present = partial.get(row.node) ?? null;
    if (present === null) {
      out.push({ kind: 'param', row, present: null });
      continue;
    }
    if (!out.some((r) => r.kind === 'node' && r.node === row.node)) out.push({ kind: 'node', node: row.node, present });
    const held = new Set(row.values.filter((_, i) => present[i]).map((v) => JSON.stringify(v ?? null)));
    if (held.size > 1) out.push({ kind: 'param', row, present });
  }
  return out;
}

const STAGE_LABEL: Record<StageName, string> = {
  retrieval_legs: 'retrieval legs',
  after_fusion: 'after fusion',
  after_rerank: 'after rerank',
  final_ranking: 'final ranking',
  answer: 'answer',
};
export const stageLabel = (stage: StageName) => STAGE_LABEL[stage];

/** The words for a run without the stage: the design's, the same as an absent node in Replay. */
export const NO_STAGE = 'no stage here';

/**
 * The per-stage line for one metric: each run's best value at each stage the
 * API kept, null where it has none, and every node of a stage holding several
 * — the retrieval legs — as its own mark.
 */
export function stageLine(c: Comparison, metric: string) {
  const x = c.stages.map((row) => ({ id: row.stage, label: row.label ?? stageLabel(row.stage) }));
  const values = c.runs.map((_, r) =>
    c.stages.map((row) => {
      const cell = row.cells[r];
      return cell?.kind === 'present' ? (cell.best[metric]?.value ?? null) : null;
    }),
  );
  const dots = c.runs.flatMap((_, r) =>
    c.stages.flatMap((row, s) => {
      const cell = row.cells[r];
      if (cell?.kind !== 'present' || cell.nodes.length < 2) return [];
      return cell.nodes.flatMap((n) => {
        const value = n.metrics?.[metric];
        return value === undefined ? [] : [{ series: r, x: s, value, label: n.node }];
      });
    }),
  );
  /** Why a run has no point at a stage: no stage at all, or a stage with no figure for this metric. */
  const gap = (series: number, at: number) => (c.stages[at]?.cells[series]?.kind === 'present' ? 'no figure' : NO_STAGE);
  return { x, values, dots, gap };
}

/** The metrics the stage line can draw: those any stage carries a figure for, in name order. */
export function stageMetrics(c: Comparison): string[] {
  const names = new Set<string>();
  for (const row of c.stages) for (const cell of row.cells) if (cell.kind === 'present') for (const m of Object.keys(cell.best)) names.add(m);
  return [...names].sort();
}

/** Each run's latency stacked node by node, in milliseconds, each node by its family's pigment. */
export function latencyBars(c: Comparison) {
  const series = runSeries(c);
  const families: Family[] = [];
  const others: string[] = [];
  const segments: StackSegment[][] = c.runs.map((run) => {
    const nodes = c.latency.find((l) => l.run === run.id)?.nodes ?? [];
    return nodes.map((n) => {
      const family = familyOfComponent(n.family);
      if (family === null) {
        if (!others.includes(n.family)) others.push(n.family);
      } else if (!families.includes(family)) families.push(family);
      return { id: n.node, label: n.node, value: n.median_nanos / 1e6, family };
    });
  });
  return { bars: series.map((s) => ({ id: s.id, label: s.label })), segments, families, others };
}

/** A bound as the bins' ranges write it, with a true minus sign. */
const bound = (n: number) => (n < 0 ? `${MINUS}${String(-n)}` : String(n));

/** The API's bins as the histogram draws them: its names, its bounds, its counts. */
export function binsOf(md: MetricDeltas): HistogramBin[] {
  return md.bins.map((b) => ({
    id: b.bin,
    label: b.bin.replace(/_/g, ' '),
    range: b.lower === null ? `below ${bound(b.upper ?? 0)}` : b.upper === null ? `above ${bound(b.lower)}` : b.lower === b.upper ? bound(b.lower) : `${bound(b.lower)} to ${bound(b.upper)}`,
    count: b.count,
    tone: b.bin.endsWith('worse') ? 'worse' : b.bin.endsWith('better') ? 'better' : 'zero',
  }));
}

const isWorse = (bin: string) => bin.endsWith('worse');
const isBetter = (bin: string) => bin.endsWith('better');
const sum = (md: MetricDeltas, keep: (bin: string) => boolean) => md.bins.filter((b) => keep(b.bin)).reduce((n, b) => n + b.count, 0);
const n = (count: number) => count.toLocaleString('en-US');

/**
 * The verdict: one sentence of counts, never adjectives — how many queries
 * improve, are unchanged and get worse on one metric, and how many of those
 * fall in the most extreme worse bin that holds any, by that bin's bound.
 */
export function verdict(md: MetricDeltas, run: string): string {
  if (md.judged_queries === 0) return `On ${md.metric}, no query of ${run} could be compared with the baseline.`;
  const better = sum(md, isBetter);
  const same = sum(md, (b) => b === 'unchanged');
  const worse = sum(md, isWorse);
  const counts = `${n(better)} ${better === 1 ? 'query improves' : 'queries improve'}, ${n(same)} ${same === 1 ? 'is' : 'are'} unchanged, ${n(worse)} ${worse === 1 ? 'gets' : 'get'} worse`;
  // The bins come worst first: the first worse bin holding a query, with a
  // bound away from zero, is the largest regression the sentence can name.
  const extreme = md.bins.find((b) => isWorse(b.bin) && b.count > 0);
  const edge = extreme?.upper ?? 0;
  const clause = extreme === undefined || edge === 0 ? '' : ` — ${n(extreme.count)} by more than ${String(Math.abs(edge))}`;
  return `On ${md.metric}, ${run} against the baseline: ${counts}${clause}.`;
}

/**
 * Why no verdict can be read for `run`: either the ground truth is verified
 * and the two runs share no ranking metric, or it is not, and its detail says
 * why.
 */
export function noVerdict(c: Comparison, run: string): string {
  if (c.ground_truth.status === 'verified') return `${run} and the baseline share no ranking metric, so no query can be compared.`;
  return `No query of ${run} can be compared with the baseline: ${c.ground_truth.detail}.`;
}

/** Whether any query compared changed at all: with none, there is nothing to open in Replay. */
export const hasChange = (md: MetricDeltas) => sum(md, (b) => isWorse(b) || isBetter(b)) > 0;

const natural = new Intl.Collator('en', { numeric: true });

/** A bin's queries, the largest change first, ties in natural order (q9 before q10). */
export function queriesByDelta(queries: readonly string[], deltas: readonly { query: string; delta: number }[]): string[] {
  const size = new Map(deltas.map((d) => [d.query, Math.abs(d.delta)]));
  return [...queries].sort((a, b) => (size.get(b) ?? 0) - (size.get(a) ?? 0) || natural.compare(a, b));
}

/**
 * The queries that get worse — those the API's worse bins hold — the lowest
 * delta first, ties in the API's order: the set Replay steps through, opening
 * on the worst.
 */
export function regressions(md: MetricDeltas): { count: number; worst: string | null; queries: string[] } {
  const ids = new Set(md.bins.filter((b) => isWorse(b.bin)).flatMap((b) => b.queries));
  const queries = md.deltas
    .filter((d) => ids.has(d.query))
    .sort((a, b) => a.delta - b.delta)
    .map((d) => d.query);
  return { count: ids.size, worst: queries[0] ?? null, queries };
}

/** The stages a pair drawn by hand can move a node into: those a node's kind decides. */
const PAIRABLE: readonly StageName[] = ['retrieval_legs', 'after_fusion', 'after_rerank'];
const STAGE_FAMILY: Partial<Record<StageName, string>> = { retrieval_legs: 'retriever', after_fusion: 'fusion', after_rerank: 'reranker' };

export type PairableNode = { node: string; stage: StageName; family: Family | null };

/** A run's nodes the pairing panel offers — its retrievers, fusion and reranker — read from the stages the API derived. */
export function pairableNodes(c: Comparison, run: number): PairableNode[] {
  const ran = c.latency.find((l) => l.run === c.runs[run]?.id)?.nodes ?? [];
  const out: PairableNode[] = [];
  for (const row of c.stages) {
    if (!PAIRABLE.includes(row.stage)) continue;
    const cell = row.cells[run];
    if (cell?.kind !== 'present') continue;
    for (const n of cell.nodes) {
      if (out.some((o) => o.node === n.node)) continue;
      const family = ran.find((r) => r.node === n.node)?.family ?? STAGE_FAMILY[row.stage] ?? '';
      out.push({ node: n.node, stage: row.stage, family: familyOfComponent(family) });
    }
  }
  return out;
}

/** The pairs the automatic pairing makes between the baseline and `other`: every node of a stage both have, none placed by hand. */
export function automaticLinks(c: Comparison, other: number): NodePair[] {
  return c.stages
    .filter((row) => PAIRABLE.includes(row.stage))
    .flatMap((row) => {
      const base = row.cells[0];
      const them = row.cells[other];
      if (base?.kind !== 'present' || them?.kind !== 'present') return [];
      return base.nodes.filter((b) => !b.paired_by_hand).flatMap((b) => them.nodes.filter((o) => !o.paired_by_hand).map((o) => ({ node: b.node, other: o.node })));
    });
}

/**
 * The pairs drawn by hand between the baseline's pipeline and `other`'s, as
 * the answer lists them: the whole pairing kept, placed or not — the one a
 * change posts back, so a pair a run of earlier content lacks is not lost.
 */
export function manualPairs(c: Comparison, other: number): NodePair[] {
  const base = c.runs[0]?.pipeline ?? null;
  const them = c.runs[other]?.pipeline ?? null;
  if (base === null || them === null) return [];
  return c.pairings.find((p) => p.pipeline === base && p.other === them)?.pairs ?? [];
}

/** Whether the API reports `pair` as one run `other` could not place. */
const isUnplaced = (c: Comparison, other: number, pair: NodePair) =>
  c.unplaced_pairs.some((u) => u.run === c.runs[other]?.id && u.pair.node === pair.node && u.pair.other === pair.other);

/** The pairs drawn by hand that run `other` placed: `manualPairs` less those the API lists in `unplaced_pairs` for it. */
export function placedPairs(c: Comparison, other: number): NodePair[] {
  return manualPairs(c, other).filter((pair) => !isUnplaced(c, other, pair));
}

/**
 * The stage section's subtitle: how many pairs drawn by hand some run
 * placed. A pair no run could place is not counted; `unplacedLabel` says why.
 */
export function pairsByHandLabel(c: Comparison): string {
  const placed = new Set<string>();
  for (let other = 1; other < c.runs.length; other += 1) {
    for (const pair of placedPairs(c, other)) placed.add(JSON.stringify([c.runs[0]?.pipeline, c.runs[other]?.pipeline, pair.node, pair.other]));
  }
  const count = placed.size;
  if (count === 0) return 'Paired automatically';
  return `${n(count)} pair${count === 1 ? '' : 's'} by hand`;
}

/**
 * A pair the API could not place, in words: the pair, then which run lacks
 * its node — "dense ↔ splade: not in run B". Never moved onto another node,
 * never left unsaid (decided in #402).
 */
export function unplacedLabel(c: Comparison, entry: UnplacedPair): string {
  const index = c.runs.findIndex((r) => r.id === entry.run);
  const run = `run ${index < 0 ? shortHash(entry.run) : letterOf(index)}`;
  const where = entry.absent_from === 'baseline' ? 'the baseline' : entry.absent_from === 'run' ? run : `the baseline nor in ${run}`;
  return `${entry.pair.node} ↔ ${entry.pair.other}: not in ${where}`;
}

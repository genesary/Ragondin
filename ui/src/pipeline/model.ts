// What the Pipeline screen draws from one `GET /pipelines/{name}/matrix`:
// the metrics a row can read, the best gain of each row, the words for a
// column, a ground truth and a feeding run, and the verdict sentence. Every
// figure and every gain is the API's; this only lays them out.
// ARCHITECTURE.md § The Pipeline screen.
import type { FeedingRun, GroundTruth, MatrixCell, MatrixColumn, PipelineMatrix } from '../api/types.ts';
import { shortHash } from '../runs/model.ts';

/** The metric names the measured cells of the rows producing `produces` carry, in name order. */
function metricsOf(m: PipelineMatrix, produces: 'chunks' | 'answer'): string[] {
  const names = new Set<string>();
  m.rows.forEach((row, i) => {
    if (row.produces !== produces) return;
    for (const column of m.columns) {
      const cell = column.cells[i];
      if (cell?.kind === 'measured') Object.keys(cell.metrics).forEach((name) => names.add(name));
    }
  });
  return [...names].sort();
}

/** The ranking metrics a ranking row can read, in name order: the choices of the "Ranking metric" selector. */
export const rankingMetrics = (m: PipelineMatrix) => metricsOf(m, 'chunks');

/** The answer metrics the generator row reads, in name order. */
export const answerMetrics = (m: PipelineMatrix) => metricsOf(m, 'answer');

/** The gain a cell's API answer gives on `metric`, or null when it gives none. */
export function gainOn(cell: MatrixCell | undefined, metric: string): number | null {
  if (cell?.kind !== 'measured' || cell.gain.kind !== 'over_previous_stage') return null;
  return cell.gain.values[metric] ?? null;
}

/**
 * The columns holding the greatest gain over the previous stage on `metric`
 * in row `row` — every one of a tie — set in bold; never the greatest value,
 * because benchmarks differ in difficulty and the best value would almost
 * always sit in the same column. The API serves a gain only for a metric
 * its catalogue gives a direction, and serves no direction with it: the
 * greatest is read as the best, as `gain` is defined as a node's value minus
 * the best value before it. None when fewer than two cells have a gain:
 * one gain is not best against anything.
 */
export function bestGainColumns(m: PipelineMatrix, row: number, metric: string): number[] {
  const gains = m.columns.map((column) => gainOn(column.cells[row], metric));
  const present = gains.filter((g): g is number => g !== null);
  if (present.length < 2) return [];
  const best = Math.max(...present);
  return gains.flatMap((g, i) => (g === best ? [i] : []));
}

const MINUS = '−';

/** A gain as its cell prints it: signed, four decimals, a true minus; one that prints as zero is unsigned. */
export function signedGain(gain: number): string {
  const magnitude = Math.abs(gain).toFixed(4);
  if (Number(magnitude) === 0) return magnitude;
  return `${gain > 0 ? '+' : MINUS}${magnitude}`;
}

/** Whether row `row` says one thing for every benchmark: every cell not scored — a context builder. */
export function spansRow(m: PipelineMatrix, row: number): boolean {
  return m.columns.length > 0 && m.columns.every((column) => column.cells[row]?.kind === 'not_scored');
}

/** A column in words: every benchmark pinned to its digest, else its short digest. */
export const columnLabel = (column: MatrixColumn) => (column.benchmark_names.length === 0 ? `dataset ${shortHash(column.dataset_version)}` : column.benchmark_names.join(', '));

const GROUND_TRUTH: Record<GroundTruth, string> = { none: 'no ground truth', qrels: 'qrels', reference_answers: 'reference answers', both: 'qrels, reference answers' };

/** What a benchmark's ground truth carries; the API knows it only once a run measured the benchmark. */
export const groundTruthLabel = (gt: GroundTruth | null) => (gt === null ? 'ground truth read once run' : GROUND_TRUTH[gt]);

/** How many cells a run of the whole pipeline would fill, by the API's `missing`. */
export const missingCount = (m: PipelineMatrix) => m.missing.reduce((sum, column) => sum + column.nodes.length, 0);

const count = (n: number, one: string, many: string) => `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;

/**
 * The verdict: one sentence of counts, never adjectives — on how many
 * benchmarks the pipeline is measured, how many of those only by a prefix
 * run, and how many cells wait for a run of the whole pipeline.
 */
export function verdict(m: PipelineMatrix): string {
  const measured = m.columns.filter((c) => c.run !== null);
  const prefixes = measured.filter((c) => c.up_to !== null);
  const stops = new Set(prefixes.map((c) => c.up_to));
  const prefixClause = prefixes.length === 0 ? '' : stops.size === 1 ? `, ${prefixes.length} of them only up to ${[...stops][0]}` : `, ${prefixes.length} of them by a prefix run`;
  const missing = missingCount(m);
  const missingClause = missing === 0 ? 'No cell waits for a run.' : `${count(missing, 'cell waits', 'cells wait')} for a run of the whole pipeline.`;
  return `Measured on ${measured.length} of ${count(m.columns.length, 'benchmark', 'benchmarks')}${prefixClause}. ${missingClause}`;
}

/** ADR-C39 § 4's first fact: what the run's launch record says it was launched as. */
export function launchFact(run: FeedingRun): string {
  const record = run.launched_as;
  if (record === null) return 'No launch record';
  if (record.prefix_of !== null) return `Launched as a prefix of ${record.name ?? 'a pipeline'}, up to ${record.prefix_of.up_to}`;
  return record.name === null ? 'Launched under no name' : `Launched as ${record.name}`;
}

/** ADR-C39 § 4's second fact: which current documents the run's content is — never resolved with the first into one name. */
export const contentFact = (run: FeedingRun) => `Content: ${run.pipeline_names.length === 0 ? 'no current pipeline document' : run.pipeline_names.join(', ')}`;

/** For a run launched as `pipeline` whose content has since changed, what its record says it was; null for a run that fills a cell. */
export function sinceChangedLabel(run: FeedingRun, pipeline: string): string | null {
  const changed = run.content_since_changed;
  if (changed === null) return null;
  return changed.launched === 'as_prefix' ? `A prefix of an earlier version of ${pipeline}` : `Launched as ${pipeline}; content since changed`;
}

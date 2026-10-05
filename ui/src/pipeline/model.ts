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
 * always sit in the same column. None when fewer than two cells have a gain:
 * one gain is not best against anything.
 *
 * **This assumes a higher gain is better.** The matrix serves no direction
 * with a gain, and the browser keeps no copy of the catalogue. It holds
 * because the API serves a gain only for a metric its catalogue gives a
 * direction, and every entry of that catalogue improves upward — which
 * `eval/ragondin-metrics/src/catalogue.rs`'s test
 * `answers_and_ranking_metrics_carry_their_family_and_direction` asserts, so
 * a lower-is-better metric fails it first. Adding one means serving the
 * direction here too, as `POST /compare` does, and reading it.
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

/**
 * The reasons a cell holds no figure because nothing measured it there, which
 * say nothing of the node itself — the pipeline that cannot be scored on the
 * benchmark included, which is the whole pipeline's matter, not the node's.
 */
const UNMEASURED: ReadonlySet<MatrixCell['kind']> = new Set(['not_run_yet', 'prefix_stops', 'not_run_on_this_version', 'not_scorable']);

/**
 * Whether row `row` says one thing for every benchmark: a node no metric
 * reads — a context builder. Not scored is structural, so a row with one
 * `not_scored` cell is not scored everywhere; a cell that only says the node
 * was not measured there — not run yet, beyond a prefix run, run on earlier
 * content, on a benchmark the pipeline cannot be scored on — does not
 * contradict it. Any other cell does.
 */
export function spansRow(m: PipelineMatrix, row: number): boolean {
  const kinds = m.columns.map((column) => column.cells[row]?.kind);
  return kinds.includes('not_scored') && kinds.every((kind) => kind === 'not_scored' || (kind !== undefined && UNMEASURED.has(kind)));
}

/** A column in words: every benchmark pinned to its digest, else its short digest. */
export const columnLabel = (column: MatrixColumn) => (column.benchmark_names.length === 0 ? `dataset ${shortHash(column.dataset_version)}` : column.benchmark_names.join(', '));

const GROUND_TRUTH: Record<GroundTruth, string> = { none: 'no ground truth', qrels: 'qrels', reference_answers: 'reference answers', both: 'qrels, reference answers' };

/** What a benchmark's ground truth carries; the API knows it only once a run measured the benchmark. */
export const groundTruthLabel = (gt: GroundTruth | null) => (gt === null ? 'ground truth read once run' : GROUND_TRUTH[gt]);

/** How many cells a run of the whole pipeline would fill, by the API's `missing`. */
export const missingCount = (m: PipelineMatrix) => m.missing.reduce((sum, column) => sum + column.nodes.length, 0);

/** Of those, how many a launch can fill: a column no benchmark name is pinned to has nothing to launch on. */
export const launchableCount = (m: PipelineMatrix) => m.missing.reduce((sum, column) => sum + (column.benchmark === null ? 0 : column.nodes.length), 0);

/**
 * The benchmarks a launch can fill, each name once: one run of the whole
 * pipeline per benchmark fills every missing cell of its column, so the
 * primary action counts these, never cells. Two columns may carry one name
 * (two digests pinned under it) and are one run; a column no name is pinned
 * to has nothing to launch on.
 */
export const launchableBenchmarks = (m: PipelineMatrix) => [...new Set(m.missing.flatMap((column) => (column.benchmark === null ? [] : [column.benchmark])))];

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
  const stuck = missing - launchableCount(m);
  const stuckClause = stuck === 0 ? '' : `, ${stuck} of them on a dataset no benchmark name is pinned to, which cannot be launched`;
  const missingClause = missing === 0 ? 'No cell waits for a run.' : `${count(missing, 'cell waits', 'cells wait')} for a run of the whole pipeline${stuckClause}.`;
  return `Measured on ${measured.length} of ${count(m.columns.length, 'benchmark', 'benchmarks')}${prefixClause}. ${missingClause}`;
}

/** ADR-C39 § 4's first fact: what the run's launch record says it was launched as. */
export function launchFact(run: FeedingRun): string {
  const record = run.launched_as;
  if (record === null) return 'No record of the name it ran under';
  if (record.prefix_of !== null) return `Run as a prefix of ${record.name ?? 'a pipeline'}, up to ${record.prefix_of.up_to}`;
  return record.name === null ? 'Run under no name' : `Run under the name ${record.name}`;
}

/** ADR-C39 § 4's second fact: which current documents the run's content is — never resolved with the first into one name. */
export const contentFact = (run: FeedingRun) => `Configuration matches ${run.pipeline_names.length === 0 ? 'no pipeline in the workspace' : run.pipeline_names.join(', ')} now`;

/** For a run launched as `pipeline` whose content has since changed, what its record says it was; null for a run that fills a cell. */
export function sinceChangedLabel(run: FeedingRun, pipeline: string): string | null {
  const changed = run.content_since_changed;
  if (changed === null) return null;
  return changed.launched === 'as_prefix' ? `A prefix of an earlier version of ${pipeline}` : `Launched as ${pipeline}; content since changed`;
}

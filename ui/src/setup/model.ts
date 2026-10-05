// What the Setup screen reads off the API's answers, as pure functions:
// sizes and digests in words, the first-launch rule, the build identity's
// parts, where a benchmark's download stands. ARCHITECTURE.md § The Setup
// screen.
import { FAMILY_LABEL, familyOfComponent } from '../../design/index.ts';
import type { ApiProblem } from '../api/client.ts';
import type { Jobs } from '../api/jobs.ts';
import type { BenchmarkEntry, GroundTruth, JobSummary, Scorable, ServiceStatus } from '../api/types.ts';

const UNITS = ['B', 'kB', 'MB', 'GB', 'TB'] as const;

/** A size in decimal units, as a download page states one: one decimal under ten, whole above. */
export function formatSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const shown = unit === 0 || value >= 100 ? Math.round(value).toString() : (Math.round(value * 10) / 10).toString();
  return `${shown} ${UNITS[unit]}`;
}

/** A digest shortened as the rest of the UI shortens a hash; the full value goes in its title. */
export const shortDigest = (digest: string) => digest.slice(0, 12);

const GROUND_TRUTH: Record<GroundTruth, string> = {
  qrels: 'qrels',
  reference_answers: 'reference answers',
  both: 'qrels and reference answers',
  none: 'no ground truth',
};

/** What a benchmark's ground truth lets a run measure, in words. */
export const groundTruthLabel = (truth: GroundTruth) => GROUND_TRUTH[truth];

/** Whether a pipeline ending (or not) in an answer can be scored on `truth`: in the list `GET /benchmarks`' `scorable` serves for it. */
const scoredBy = (scorable: Scorable, truth: GroundTruth, endsInAnswer: boolean) => scorable[endsInAnswer ? 'ending_in_answer' : 'ending_elsewhere'].includes(truth);

/**
 * Which pipelines a benchmark carrying `truth` scores, in words: read off the
 * lists `GET /benchmarks` serves — `CarriedPieces::scorable` asked of each
 * ground truth — never decided here. `none` is in neither list, so a
 * benchmark carrying nothing scores no pipeline.
 */
export function scorableLabel(scorable: Scorable, truth: GroundTruth): string {
  const answer = scoredBy(scorable, truth, true);
  const elsewhere = scoredBy(scorable, truth, false);
  if (answer && elsewhere) return 'any pipeline';
  if (answer) return 'only a pipeline that ends in an answer';
  if (elsewhere) return 'only a pipeline that does not end in an answer';
  return 'no pipeline';
}

/**
 * A family as a person reads it — design/'s `FAMILY_LABEL` of the tile
 * `familyOfComponent` gives it, so `context_builder` is "context builder" —
 * or its own name for a family no tile draws (`embedder`). The value sent
 * stays the configuration's.
 */
export function familyLabel(family: string): string {
  const tile = familyOfComponent(family);
  return tile === null ? family : FAMILY_LABEL[tile];
}

/**
 * Whether the workspace is at its first launch: no benchmark on disk — an
 * `available` entry is the manifest's offer, not something held — and no
 * service bound. The screen is then an invitation rather than four sections.
 */
export function isFirstLaunch(benchmarks: readonly BenchmarkEntry[], services: readonly ServiceStatus[]): boolean {
  return services.length === 0 && benchmarks.every((b) => b.state.kind === 'available');
}

type Available = BenchmarkEntry & { state: { kind: 'available'; size_bytes: number } };

const isAvailable = (b: BenchmarkEntry): b is Available => b.state.kind === 'available';

/** The available benchmark with the fewest bytes — a good first run, read from the manifest's sizes — or null. */
export function smallestAvailable(benchmarks: readonly BenchmarkEntry[]): Available | null {
  return benchmarks.filter(isAvailable).reduce<Available | null>((best, b) => (best === null || b.state.size_bytes < best.state.size_bytes ? b : best), null);
}

/**
 * The benchmark the first launch recommends. The front-end design's first-run
 * journey (§ 3) downloads a benchmark, then launches the Editor's example
 * pipeline, which is retrieval-only: so the smallest available benchmark a
 * pipeline that does not end in an answer is scored on, read off the ground
 * truth the listing serves before a download; otherwise the smallest
 * available; null when none is.
 */
export function firstBenchmark(benchmarks: readonly BenchmarkEntry[], scorable: Scorable): { entry: Available; why: 'first-run' | 'smallest' } | null {
  const retrievalOnly = smallestAvailable(benchmarks.filter((b) => b.ground_truth !== null && scoredBy(scorable, b.ground_truth, false)));
  if (retrievalOnly !== null) return { entry: retrievalOnly, why: 'first-run' };
  const smallest = smallestAvailable(benchmarks);
  return smallest === null ? null : { entry: smallest, why: 'smallest' };
}

/** The build identity's two parts, `<version>+<commit>` (ARCHITECTURE.md § The build identity handshake). */
export function splitBuild(build: string): { version: string; commit: string | null } {
  const at = build.indexOf('+');
  return at < 0 ? { version: build, commit: null } : { version: build.slice(0, at), commit: build.slice(at + 1) };
}

/** A binding's key, `<family>/<name>`, as `--remote` and `workspace.toml` spell it. */
export const serviceKey = (s: { family: string; name: string }) => `${s.family}/${s.name}`;

/** This page's request for a benchmark's download: in flight, refused, or answered with its job. */
export type Submission = { kind: 'submitting' } | { kind: 'refused'; problem: ApiProblem } | { kind: 'accepted'; jobId: string };

/** Where a benchmark's download stands, as its row says it. */
export type DownloadView =
  | { kind: 'idle' }
  | { kind: 'submitting' }
  | { kind: 'refused'; problem: ApiProblem }
  | { kind: 'queued'; jobId: string }
  | { kind: 'running'; jobId: string; done: number; total: number | null }
  | { kind: 'verifying' }
  | { kind: 'failed'; error: string }
  | { kind: 'cancelled' };

const isDownloadOf = (name: string) => (j: JobSummary) => j.work.kind === 'download' && j.work.benchmark === name;

function viewOf(job: JobSummary): DownloadView {
  const state = job.state;
  switch (state.kind) {
    case 'queued':
      return { kind: 'queued', jobId: job.id };
    case 'running':
      return { kind: 'running', jobId: job.id, done: state.done, total: state.total };
    case 'done':
      return { kind: 'verifying' };
    case 'failed':
      return { kind: 'failed', error: state.error };
    case 'cancelled':
      return { kind: 'cancelled' };
  }
}

/**
 * Where a benchmark's download stands. This page's submission decides while
 * it has one — its job once the stream has carried it, queued until then,
 * never an older job of the same benchmark. Without one, the benchmark's last
 * download job in the queue does, so a download started in another tab, or
 * before the page was opened, is shown — but a done one, which the listing
 * already says as `ready`.
 */
export function downloadView(name: string, submission: Submission | undefined, jobs: Jobs): DownloadView {
  if (submission !== undefined) {
    if (submission.kind !== 'accepted') return submission;
    const job = jobs.get(submission.jobId);
    return job === undefined ? { kind: 'queued', jobId: submission.jobId } : viewOf(job);
  }
  const last = [...jobs.values()].filter(isDownloadOf(name)).at(-1);
  return last === undefined || last.state.kind === 'done' ? { kind: 'idle' } : viewOf(last);
}

/**
 * The benchmarks whose download ended done between two readings of the
 * queue, so the listing is read again for their digests: a job seen before
 * and not done then, or one this page submitted (`submitted`), whose earlier
 * events the stream may have carried before the submission was answered.
 */
export function finishedDownloads(before: Jobs, after: Jobs, submitted: ReadonlySet<string>): string[] {
  const names: string[] = [];
  for (const job of after.values()) {
    if (job.work.kind !== 'download' || job.state.kind !== 'done') continue;
    const was = before.get(job.id);
    if (was === undefined ? submitted.has(job.id) : was.state.kind !== 'done') names.push(job.work.benchmark);
  }
  return names;
}

// The job queue's run jobs as rows of the Runs screen: the same `RunRow` a
// run of the store is, its source a job. The rows are derived from the queue
// the stream gave, never kept apart from it. ARCHITECTURE.md § The Runs screen.
import type { Jobs } from '../api/jobs.ts';
import type { JobStatus, JobSummary } from '../api/types.ts';
import type { RowStatus, RunRow } from './model.ts';

type RunJob = JobSummary & { work: Extract<JobSummary['work'], { kind: 'run' }> };

const isRunJob = (job: JobSummary): job is RunJob => job.work.kind === 'run';

/** The queued run jobs' ids, in the order the worker takes them. */
export function queuedOrder(jobs: Jobs): string[] {
  return [...jobs.values()]
    .filter((j) => isRunJob(j) && j.state.kind === 'queued')
    .sort((a, b) => a.position - b.position)
    .map((j) => j.id);
}

function status(state: JobStatus): RowStatus {
  switch (state.kind) {
    case 'queued':
      return { state: 'queued' };
    case 'running':
      return { state: 'running', done: state.done, total: state.total };
    case 'done':
      return { state: 'done' };
    case 'failed':
      return { state: 'failed', node: state.at_node, error: state.error };
    case 'cancelled':
      return { state: 'cancelled' };
  }
}

/** Live work first — the running job, then the queued ones as the worker takes them — then the ended, newest first. */
function rank(job: JobSummary): number {
  return job.state.kind === 'running' ? 0 : job.state.kind === 'queued' ? 1 : 2;
}
function byQueue(a: JobSummary, b: JobSummary): number {
  return rank(a) - rank(b) || (rank(a) === 1 ? a.position - b.position : (b.created_at_ms ?? 0) - (a.created_at_ms ?? 0)) || (a.id < b.id ? -1 : 1);
}

/**
 * The runs of the store, each given the id its job announced when the job
 * reports it filed the run under another (`id_mismatch`), so the run's row
 * shows both and says why. The client compares nothing: the job says it.
 */
export function withAnnounced(runs: readonly RunRow[], jobs: Jobs): RunRow[] {
  const announced = new Map<string, string>();
  for (const job of jobs.values()) {
    if (job.state.kind === 'done' && job.state.id_mismatch !== null) announced.set(job.state.id_mismatch.decided, job.state.id_mismatch.announced);
  }
  return runs.map((r) => (r.source.kind === 'run' && announced.has(r.source.id) ? { ...r, announced: announced.get(r.source.id) ?? null } : r));
}

/** Whether `a` was accepted after `b`: by its time, then by its id, which counts within one millisecond. */
const acceptedAfter = (a: JobSummary, b: JobSummary) => (a.created_at_ms ?? 0) > (b.created_at_ms ?? 0) || ((a.created_at_ms ?? 0) === (b.created_at_ms ?? 0) && a.id > b.id);

/**
 * The run jobs as rows. A done job keeps its row until the store holds the
 * run it filed, then hands over to that run's row. A failed or cancelled job
 * keeps its row — a failed run is a first-class object — until the store
 * holds its announced run or a later job announced the same id, which took
 * it over (a resubmission), or a person dismisses it. A download is not a
 * run, and has no row here.
 * A benchmark name a run of the store pins takes that run's digest and
 * names, so one filter chip holds both.
 */
export function rowsFromJobs(jobs: Jobs, runs: readonly RunRow[]): RunRow[] {
  const stored = new Set(runs.flatMap((r) => (r.source.kind === 'run' ? [r.source.id] : [])));
  const all = [...jobs.values()].filter(isRunJob);
  const queued = queuedOrder(jobs);
  const takenOver = (job: RunJob) => stored.has(job.work.run_id) || all.some((other) => other.work.run_id === job.work.run_id && acceptedAfter(other, job));

  return all
    .filter((job) => {
      const { state } = job;
      // Dismissed: a person is done with it, and it leaves the rows. The queue still lists it.
      if (job.dismissed_at_ms !== null) return false;
      if (state.kind === 'done') return state.run_id === null || !stored.has(state.run_id);
      if (state.kind === 'failed' || state.kind === 'cancelled') return !takenOver(job);
      return true;
    })
    .sort(byQueue)
    .map((job) => {
      const { work, state } = job;
      const pinned = runs.find((r) => r.source.kind === 'run' && r.benchmarkNames.includes(work.benchmark));
      const place = queued.indexOf(job.id);
      return {
        source: { kind: 'job', id: job.id, runId: work.run_id },
        // A job knows the name it was launched as, never its canonical hash.
        pipeline: '',
        pipelineNames: [],
        refusedNames: [],
        launchedAs: work.pipeline,
        launchedHeld: null,
        launchRecorded: true,
        benchmark: pinned?.benchmark ?? `name:${work.benchmark}`,
        benchmarkNames: pinned?.benchmarkNames ?? [work.benchmark],
        status: status(state),
        metrics: [],
        latencyMs: null,
        startedAt: null,
        prefix: work.up_to === null ? null : { parents: [work.pipeline], upTo: work.up_to },
        contentPrefix: null,
        job: {
          submission: { pipeline: work.pipeline, benchmark: work.benchmark, up_to: work.up_to },
          place: place === -1 ? null : place,
          queued: queued.length,
          startedAtMs: state.kind === 'running' ? state.started_at_ms : null,
          medianMs: state.kind === 'running' && state.median_latency_nanos !== null ? state.median_latency_nanos / 1e6 : null,
          filed: state.kind === 'done' ? state.run_id : null,
          mismatch: state.kind === 'done' ? state.id_mismatch : null,
          faults: job.faults.map((f) => f.reason),
        },
        announced: null,
      } satisfies RunRow;
    });
}

// One job of the queue, at its own address (`#runs/job/<id>`): what was
// submitted, the identity it announced, where it stands, and — failed or
// cancelled — how many queries' traces it kept, where, and the way to Replay
// over them. The job is the stream's when the queue holds it, else
// `GET /jobs/{id}`'s answer.
// ARCHITECTURE.md § The Runs screen.
import { useEffect, useState, type Ref } from 'react';
import { ButtonLink, Section, StatusChip } from '../../design/index.ts';
import type { ApiClient } from '../api/client.ts';
import type { JobSummary } from '../api/types.ts';
import { useJobs } from '../jobs/queue.tsx';
import { formatHash } from '../routes.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { shortHash } from './model.ts';

export type JobPanelProps = {
  client: ApiClient;
  id: string;
  /** Where Close leads: Runs, keeping its selection. */
  closeHref: string;
  /** The section, which takes focus when the address opens another job. */
  anchor?: Ref<HTMLElement>;
};

const when = (ms: number | null) => (ms === null ? null : new Date(ms).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' }));

function Facts({ job }: { job: JobSummary }) {
  const { work, state } = job;
  if (work.kind !== 'run') {
    return <p>A download of {work.benchmark}, followed in Setup.</p>;
  }
  const accepted = when(job.created_at_ms);
  return (
    <>
      <dl className="rg-job__facts">
        <dt>Pipeline</dt>
        <dd>{work.up_to === null ? work.pipeline : `${work.pipeline}, up to ${work.up_to}`}</dd>
        <dt>Benchmark</dt>
        <dd>{work.benchmark}</dd>
        <dt>Announced run</dt>
        <dd>
          <code title={work.run_id}>{shortHash(work.run_id)}</code>
        </dd>
        {accepted === null ? null : (
          <>
            <dt>Queued</dt>
            <dd>{accepted}</dd>
          </>
        )}
        <dt>State</dt>
        <dd>
          {state.kind === 'running' ? (
            <StatusChip state="running" fraction={state.total !== null && state.total > 0 ? state.done / state.total : 0}>
              {state.total === null ? 'starting' : `running ${state.done.toLocaleString('en-US')} / ${state.total.toLocaleString('en-US')}`}
            </StatusChip>
          ) : state.kind === 'failed' ? (
            <StatusChip state="failed">
              {state.at_node === null ? (
                'failed'
              ) : (
                <>
                  failed at <code>{state.at_node}</code>
                </>
              )}
            </StatusChip>
          ) : (
            <StatusChip state={state.kind} />
          )}
        </dd>
      </dl>
      {state.kind === 'failed' ? <p className="rg-runs__failure">{state.at_node === null ? `The run failed: ${state.error}` : `${state.at_node} failed: ${state.error}`}</p> : null}
      {state.kind === 'failed' || state.kind === 'cancelled' ? (
        state.partial_traces === 0 ? (
          <p className="rg-job__partial">It kept no trace: a run keeps the traces of the queries it completed when it fails or is cancelled, and a crash keeps none.</p>
        ) : (
          <p className="rg-job__partial">
            <span>
              It kept the traces of the {state.partial_traces === 1 ? 'query' : `${state.partial_traces.toLocaleString('en-US')} queries`} it completed before it {state.kind === 'failed' ? 'failed' : 'was cancelled'}, under jobs/{job.id}/partial/ in the workspace, never in the store.
            </span>{' '}
            <ButtonLink size="s" href={formatHash({ screen: 'replay', job: job.id })}>
              Replay the partial traces
            </ButtonLink>
          </p>
        )
      ) : null}
      {state.kind === 'done' && state.run_id !== null ? (
        <p>
          {state.id_mismatch === null ? 'Filed under the id it announced.' : `Filed under ${shortHash(state.id_mismatch.decided)}, not the announced ${shortHash(state.id_mismatch.announced)}: what ran differs from what was announced.`}{' '}
          <ButtonLink size="s" href={formatHash({ screen: 'replay', run: state.run_id })}>
            Open in Replay
          </ButtonLink>
        </p>
      ) : null}
    </>
  );
}

export function JobPanel({ client, id, closeHref, anchor }: JobPanelProps) {
  const { jobs } = useJobs();
  const streamed = jobs.get(id);
  const [read, setRead] = useState<RequestState<JobSummary>>({ status: 'loading' });

  // The address may name a job before the stream has given the queue, or one it never will: ask the API.
  useEffect(() => {
    const controller = new AbortController();
    setRead({ status: 'loading' });
    void client.get('/jobs/{id}', { id }, { signal: controller.signal }).then((result) => {
      if (controller.signal.aborted) return;
      setRead(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
    });
    return () => controller.abort();
  }, [client, id]);

  return (
    <Section heading={`Job ${id}`} {...(anchor === undefined ? {} : { anchor })}>
      <div className="rg-job">
        {streamed !== undefined ? (
          <Facts job={streamed} />
        ) : read.status === 'loaded' ? (
          <Facts job={read.value} />
        ) : read.status === 'error' ? (
          <ErrorState problem={read.problem} />
        ) : (
          <Loading label="Reading the job" />
        )}
        <ButtonLink size="s" href={closeHref}>
          Close
        </ButtonLink>
      </div>
    </Section>
  );
}

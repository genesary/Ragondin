// The launch panel: a pipeline of the workspace on a benchmark, queued as a
// run. Its fields are exactly `ragondin bench`'s arguments (the front-end
// design, § 2, Parity) — the pipeline, the benchmark, the bindings in force
// and the store — the last two shown, not chosen: the API snapshots the
// workspace's bindings itself, and the store is the workspace's. It shows the
// pipeline's hash once the pipeline validates, and the run id the API
// announces once it is launched; it never computes one (INV-8).
// ARCHITECTURE.md § The Runs screen.
import { useCallback, useEffect, useId, useRef, useState, type Ref } from 'react';
import { Button, ButtonLink, InlineMessage, Section, Select } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import type { BenchmarkListing, PipelineListing, ServiceListing } from '../api/types.ts';
import { formatHash, type Route } from '../routes.ts';
import { groundTruthLabel } from '../setup/model.ts';
import { ErrorState, Loading, type RequestState } from '../shell/states.tsx';
import { shortHash } from './model.ts';

export type LaunchPanelProps = {
  client: ApiClient;
  /** The workspace's root, where the store is; null while the shell has not read it. */
  store: string | null;
  /** The panel's section, which takes focus when the panel opens. */
  anchor?: Ref<HTMLElement>;
};

/** Where a launch stands: not asked, in flight, queued under an announced id, or refused. */
type Launch = { kind: 'idle' } | { kind: 'sending' } | { kind: 'queued'; job: string; run: string } | { kind: 'refused'; problem: ApiProblem };

/** Where a refusal is shown: on the field or the list it names, or for the whole panel. */
const PIPELINE_CODES = new Set(['pipeline_invalid', 'impl_not_in_build', 'pipeline_not_found']);
const BENCHMARK_CODES = new Set(['benchmark_not_found', 'dataset_absent', 'dataset_differs']);
const BINDING_CODES = new Set(['service_unreachable', 'binding_refused', 'service_not_found']);

/** Where `run_exists`'s link leads in this UI: a stored run to Replay, a job to its row in Runs. */
export function conflictRoute(link: string | undefined): { route: Route; job: boolean } | null {
  const match = /^\/api\/v1\/(runs|jobs)\/([^/?#]+)$/.exec(link ?? '');
  if (match === null) return null;
  const id = decodeURIComponent(match[2] as string);
  return match[1] === 'runs' ? { route: { screen: 'replay', run: id }, job: false } : { route: { screen: 'runs', job: id }, job: true };
}

/** A refusal in the API's words: where, what, how to fix it, and its code. */
function refusalWords(problem: ApiProblem): string {
  const node = problem.location?.node;
  return `${problem.message}${node == null ? '' : ` At node ${node}.`} ${problem.hint} (${problem.code})`;
}

/**
 * `run_exists`: never an error. Something already holds the identity — a run
 * of the store or a job of the queue — and the answer is to open it.
 */
export function Conflict({ problem }: { problem: ApiProblem }) {
  const to = conflictRoute(problem.link);
  return (
    <InlineMessage
      tone="info"
      title={to?.job === true ? 'A run with this identity is already queued or running.' : 'A run with this identity already exists.'}
      action={
        to === null ? undefined : (
          <ButtonLink size="s" href={formatHash(to.route)}>
            Open
          </ButtonLink>
        )
      }
    >
      {problem.message}
    </InlineMessage>
  );
}

/** Reads one listing on mount; only the last answer asked for lands, and the screen going cancels it. */
function useListing<T>(read: (signal: AbortSignal) => Promise<{ ok: true; value: T } | { ok: false; problem: ApiProblem }>) {
  const [state, setState] = useState<RequestState<T>>({ status: 'loading' });
  const reading = useRef(read);
  const load = useCallback(() => {
    const controller = new AbortController();
    setState({ status: 'loading' });
    void reading.current(controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      setState(result.ok ? { status: 'loaded', value: result.value } : { status: 'error', problem: result.problem });
    });
    return controller;
  }, []);
  const current = useRef<AbortController | null>(null);
  const reload = useCallback(() => {
    current.current?.abort();
    current.current = load();
  }, [load]);
  useEffect(() => {
    reload();
    return () => current.current?.abort();
  }, [reload]);
  return [state, reload] as const;
}

export function LaunchPanel({ client, store, anchor }: LaunchPanelProps) {
  const [pipelines, retryPipelines] = useListing<PipelineListing>((signal) => client.get('/pipelines', { signal }));
  const [benchmarks, retryBenchmarks] = useListing<BenchmarkListing>((signal) => client.get('/benchmarks', { signal }));
  const [services, retryServices] = useListing<ServiceListing>((signal) => client.get('/services', { signal }));
  const [chosenPipeline, setPipeline] = useState<string | null>(null);
  const [chosenBenchmark, setBenchmark] = useState<string | null>(null);
  const [launch, setLaunch] = useState<Launch>({ kind: 'idle' });
  const ids = { pipeline: useId(), benchmark: useId() };

  const docs = pipelines.status === 'loaded' ? pipelines.value.pipelines : [];
  // Ready ones only: a benchmark on disk whose digest is the one expected of it.
  const ready = benchmarks.status === 'loaded' ? benchmarks.value.benchmarks.filter((b) => b.state.kind === 'ready' || b.state.kind === 'local') : [];
  const pipeline = docs.find((p) => p.name === chosenPipeline) ?? docs.find((p) => p.hash !== null) ?? docs[0] ?? null;
  const benchmark = ready.find((b) => b.name === chosenBenchmark) ?? ready[0] ?? null;

  // A change of what would be launched is a new launch: the last answer no longer describes it.
  const choose = (set: (v: string) => void) => (value: string) => {
    set(value);
    setLaunch({ kind: 'idle' });
  };

  const send = async () => {
    if (pipeline === null || benchmark === null) return;
    setLaunch({ kind: 'sending' });
    const result = await client.post('/runs', { pipeline: pipeline.name, benchmark: benchmark.name });
    setLaunch(result.ok ? { kind: 'queued', job: result.value.job_id, run: result.value.run_id } : { kind: 'refused', problem: result.problem });
  };

  const refused = launch.kind === 'refused' ? launch.problem : null;
  const on = (codes: Set<string>) => (refused !== null && codes.has(refused.code) ? refusalWords(refused) : undefined);
  const pipelineError = pipeline?.error != null ? `${pipeline.error.detail} Correct the document in the Editor, then launch.` : on(PIPELINE_CODES);
  const benchmarkError = on(BENCHMARK_CODES);
  const bindingError = on(BINDING_CODES);
  const elsewhere = refused !== null && refused.code !== 'run_exists' && pipelineError === undefined && benchmarkError === undefined && bindingError === undefined ? refused : null;

  const why =
    pipeline === null
      ? 'No pipeline in this workspace: build one in the Editor.'
      : pipeline.hash === null
        ? 'This pipeline does not validate.'
        : benchmark === null
          ? 'No benchmark is ready: download or import one in Setup.'
          : launch.kind === 'queued'
            ? 'Queued. Choose another pipeline or benchmark to launch another run.'
            : null;

  return (
    <Section heading="Launch a run" caption="A run is one pipeline on one benchmark. The run's id is announced when it is queued." {...(anchor === undefined ? {} : { anchor })}>
      <div className="rg-launch">
        {pipelines.status === 'loading' ? <Loading label="Reading pipelines" /> : null}
        {pipelines.status === 'error' ? <ErrorState problem={pipelines.problem} onRetry={retryPipelines} /> : null}
        {pipelines.status === 'loaded' ? (
          <div className="rg-launch__field">
            <Select
              id={ids.pipeline}
              label="Pipeline"
              value={pipeline?.name ?? ''}
              options={docs.map((p) => ({ value: p.name, label: p.hash === null ? `${p.name} — does not validate` : p.name }))}
              onChange={(e) => choose(setPipeline)(e.target.value)}
              {...(pipelineError === undefined ? {} : { error: pipelineError })}
            />
            {/* The identity the API gives, never one computed here: the pipeline's hash now, the run's once queued. */}
            <p className="rg-launch__identity">
              {pipeline?.hash == null ? null : <code title={pipeline.hash}>{`pipeline ${shortHash(pipeline.hash)}`}</code>}
            </p>
          </div>
        ) : null}

        {benchmarks.status === 'loading' ? <Loading label="Reading benchmarks" /> : null}
        {benchmarks.status === 'error' ? <ErrorState problem={benchmarks.problem} onRetry={retryBenchmarks} /> : null}
        {benchmarks.status === 'loaded' ? (
          <div className="rg-launch__field">
            <Select
              id={ids.benchmark}
              label="Benchmark"
              value={benchmark?.name ?? ''}
              options={ready.map((b) => ({ value: b.name, label: b.ground_truth === null ? b.name : `${b.name} — ${groundTruthLabel(b.ground_truth)}` }))}
              onChange={(e) => choose(setBenchmark)(e.target.value)}
              {...(benchmarkError === undefined ? { help: 'Ready benchmarks only, with the ground truth each carries.' } : { error: benchmarkError })}
            />
            {ready.length === 0 ? (
              <ButtonLink size="s" href={formatHash({ screen: 'setup', section: 'benchmarks' })}>
                Open Setup
              </ButtonLink>
            ) : null}
          </div>
        ) : null}

        <div className="rg-launch__bindings">
          <span className="rg-launch__label">Bindings in force</span>
          {services.status === 'loading' ? <Loading label="Reading services" /> : null}
          {services.status === 'error' ? <ErrorState problem={services.problem} onRetry={retryServices} /> : null}
          {services.status === 'loaded' ? (
            services.value.services.length === 0 ? (
              <p className="rg-launch__note">None: the pipeline runs on the components this build carries.</p>
            ) : (
              <ul>
                {services.value.services.map((s) => (
                  <li key={`${s.family}/${s.name}`}>
                    <span>{`${s.family}/${s.name}`}</span> <code>{s.uri}</code>
                  </li>
                ))}
              </ul>
            )
          ) : null}
          <p className="rg-launch__note">
            Snapshotted when the run is queued; connect a service in <a href={formatHash({ screen: 'setup', section: 'services' })}>Setup</a>.
          </p>
          {bindingError === undefined ? null : <InlineMessage tone="critical" title={bindingError} />}
        </div>

        <div className="rg-launch__store">
          <span className="rg-launch__label">Store</span>
          {store === null ? <span className="rg-launch__note">this workspace’s</span> : <code>{store}</code>}
        </div>

        <div className="rg-launch__action">
          {launch.kind === 'queued' ? (
            <>
              <Button kind="primary" disabled disabledReason={why ?? 'Queued.'}>
                Queued
              </Button>
              <span className="rg-launch__identity">
                <code title={launch.run}>{`run ${shortHash(launch.run)}`}</code> <span className="rg-launch__note">announced</span>{' '}
                <ButtonLink size="s" href={formatHash({ screen: 'runs', job: launch.job })}>
                  Open
                </ButtonLink>
              </span>
            </>
          ) : why !== null ? (
            <Button kind="primary" disabled disabledReason={why}>
              Launch
            </Button>
          ) : (
            <Button kind="primary" busy={launch.kind === 'sending'} busyLabel="Launching…" onClick={() => void send()}>
              Launch
            </Button>
          )}
        </div>
        {refused?.code === 'run_exists' ? <Conflict problem={refused} /> : null}
        {elsewhere === null ? null : <ErrorState problem={elsewhere} />}
      </div>
    </Section>
  );
}

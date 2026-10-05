// The launch panel: a pipeline of the workspace on a benchmark, queued as a
// run. Its fields are exactly `ragondin bench`'s arguments (the front-end
// design, § 2, Parity) — the pipeline, the benchmark, the bindings in force
// and the store — the last two shown, not chosen: the API snapshots the
// workspace's bindings itself, and the store is the workspace's. It shows the
// pipeline's hash once the pipeline validates, and the run id the API
// announces once it is launched; it never computes one (INV-8). It offers a
// pipeline only the benchmarks it can be scored on, by what it ends in, as the
// API serves the rule (`GET /benchmarks`' `scorable`), and names the others
// with why. Opened up to a node — the editor's "Run up to this node" — it
// launches the pipeline cut there, judged by what the cut ends in, and leaves
// the identity to the API, since the cut is a pipeline of its own. Opened on
// a benchmark — the Pipeline screen's Run — it launches on that one, or says
// why it cannot, never on another in its place. Opened on several — "Launch
// the N missing runs" — it lists them, each with why it cannot be launched if it
// cannot, and one confirmation sends one `POST /runs` per launchable one, each
// outcome said beside its benchmark.
// ARCHITECTURE.md § The Runs screen.
import { useCallback, useEffect, useId, useRef, useState, type Ref } from 'react';
import { Button, ButtonLink, InlineMessage, PrefixLabel, Section, Select } from '../../design/index.ts';
import type { ApiClient, ApiProblem } from '../api/client.ts';
import { useJobs } from '../jobs/queue.tsx';
import type { BenchmarkEntry, BenchmarkListing, GroundTruth, PipelineDetail, PipelineListing, RunRequest, Scorable, ServiceListing } from '../api/types.ts';
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
  /** The pipeline the panel opens on, when the address names one. */
  pipeline?: string | undefined;
  /** The node a prefix run stops at: the pipeline is launched cut there. Null for the whole pipeline. */
  upTo?: string | null;
  /** The benchmarks the panel opens on, when the address names any: one is chosen in the field, several are launched together. */
  benchmarks?: readonly string[] | undefined;
  /** Back to the whole pipeline, from a panel opened up to a node. */
  onWhole?: (() => void) | undefined;
  /** Closes the panel. */
  onClose?: (() => void) | undefined;
};

/** Where a launch stands: not asked, in flight, queued under an announced id, or refused. */
type Launch = { kind: 'idle' } | { kind: 'sending' } | { kind: 'queued'; job: string; run: string } | { kind: 'refused'; problem: ApiProblem };

/** One benchmark's submission among several: in flight, queued under an announced id, or refused. */
type Submission = { kind: 'sending' } | { kind: 'queued'; job: string; run: string } | { kind: 'refused'; problem: ApiProblem };

/** Why a benchmark the address named is not offered: the workspace does not know it, it is not on disk as expected, or the cut cannot be scored on it. */
type Standing = 'unknown' | 'not ready' | 'not offered';

/** A benchmark that is not launched, in a few words, for the list of several. */
const notLaunched = (standing: Standing, what: 'pipeline' | 'prefix'): string =>
  ({ unknown: 'not a benchmark of this workspace', 'not ready': 'not ready — download or import it in Setup', 'not offered': `cannot score this ${what}` })[standing];

const runsLabel = (n: number) => `Launch ${n} run${n === 1 ? '' : 's'}`;

/** The outcomes of several submissions as one sentence of counts. */
function outcomeCounts(outcomes: readonly Submission[]): string {
  const queued = outcomes.filter((o) => o.kind === 'queued').length;
  const held = outcomes.filter((o) => o.kind === 'refused' && o.problem.code === 'run_exists').length;
  const refused = outcomes.filter((o) => o.kind === 'refused').length - held;
  const parts = [...(queued === 0 ? [] : [`${queued} queued`]), ...(held === 0 ? [] : [`${held} already held by a run or a job`]), ...(refused === 0 ? [] : [`${refused} refused`])];
  return `${parts.join(', ')}.`;
}

/** Where a refusal is shown: on the field or the list it names, or for the whole panel. */
const PIPELINE_CODES = new Set(['pipeline_invalid', 'impl_not_in_build', 'pipeline_not_found', 'prefix_node_not_found', 'prefix_is_whole_pipeline', 'prefix_ends_in_context']);
const BENCHMARK_CODES = new Set(['benchmark_not_found', 'dataset_absent', 'dataset_differs', 'prefix_not_scorable']);

/** The ground truths `GET /benchmarks`' `scorable` lists for what is launched, by whether it ends in an answer. */
const scorableFor = (scorable: Scorable, endsInAnswer: boolean): readonly GroundTruth[] => scorable[endsInAnswer ? 'ending_in_answer' : 'ending_elsewhere'];

/**
 * Whether a pipeline can be scored on a benchmark, given whether it ends in an
 * answer: the API lists the benchmark's ground truth among those such a
 * pipeline can be scored on — `CarriedPieces::scorable`, served in
 * `GET /benchmarks`' `scorable`, which leaves out a benchmark that carries
 * nothing to score — and nothing is restated here.
 */
const scoredOn = (scorable: Scorable, endsInAnswer: boolean) => (b: BenchmarkEntry) => (scorableFor(scorable, endsInAnswer) as readonly (GroundTruth | null)[]).includes(b.ground_truth);

/** Words joined as a sentence lists them: "a", "a or b", "a, b or c". */
const either = (words: readonly string[]) => (words.length < 2 ? words.join('') : `${words.slice(0, -1).join(', ')} or ${words[words.length - 1] as string}`);

/**
 * Why the benchmarks a pipeline — or a prefix — cannot be scored on are not
 * offered, in one sentence; null when every one is. Worded from what the API
 * serves alone: each benchmark's ground truth, and the ground truths what is
 * launched can be scored on, by what it ends in.
 */
function notOffered(absent: readonly BenchmarkEntry[], prefix: boolean, endsInAnswer: boolean, scorable: Scorable): string | null {
  if (absent.length === 0) return null;
  const which = absent.map((b) => `${b.name}, which carries ${b.ground_truth === null ? 'a ground truth not yet read' : groundTruthLabel(b.ground_truth)}`).join('; ');
  const what = `a ${prefix ? 'prefix' : 'pipeline'} that ${endsInAnswer ? 'ends' : 'does not end'} in an answer`;
  const on = scorableFor(scorable, endsInAnswer).map(groundTruthLabel);
  return `Not offered: ${which}: ${what} can be scored ${on.length === 0 ? 'on no benchmark' : `only on a benchmark whose ground truth is ${either(on)}`}.`;
}
const BINDING_CODES = new Set(['service_unreachable', 'binding_refused', 'service_not_found']);

/** Where `run_exists`'s link leads in this UI: a stored run to Replay, a job to its row in Runs. */
export function conflictRoute(link: string | undefined): { route: Route; job: boolean } | null {
  const match = /^\/api\/v1\/(runs|jobs)\/([^/?#]+)$/.exec(link ?? '');
  if (match === null) return null;
  const id = decodeURIComponent(match[2] as string);
  return match[1] === 'runs' ? { route: { screen: 'replay', run: id }, job: false } : { route: { screen: 'runs', job: id }, job: true };
}

/** A text with every full run id — 64 hex digits — cut to the twelve the screen prints. */
const shortIds = (text: string) => text.replace(/\b[0-9a-f]{64}\b/g, (id) => shortHash(id));

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
      {shortIds(problem.message)}
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

/** What one of several benchmarks says: why it is not launched, its ground truth before the launch, then its outcome. */
function SeveralLine({ name, standing, entry, submission, what }: { what: 'pipeline' | 'prefix'; name: string; standing: Standing | null; entry: BenchmarkEntry | undefined; submission: Submission | undefined }) {
  if (standing !== null) return <span className="rg-launch__note">{`not launched: ${notLaunched(standing, what)}`}</span>;
  if (submission === undefined) return <span className="rg-launch__note">{entry?.ground_truth == null ? '' : groundTruthLabel(entry.ground_truth)}</span>;
  if (submission.kind === 'sending') return <span className="rg-launch__note">launching…</span>;
  if (submission.kind === 'queued') {
    return (
      <span className="rg-launch__identity">
        <code title={submission.run}>{`run ${shortHash(submission.run)}`}</code> <span className="rg-launch__note">announced</span>{' '}
        <ButtonLink size="s" href={formatHash({ screen: 'runs', job: submission.job })} aria-label={`Open the job on ${name}`}>
          Open
        </ButtonLink>
      </span>
    );
  }
  if (submission.problem.code === 'run_exists') {
    const to = conflictRoute(submission.problem.link);
    return (
      <span className="rg-launch__note">
        {to?.job === true ? 'A run with this identity is already queued or running.' : 'A run with this identity already exists.'}{' '}
        {to === null ? null : (
          <ButtonLink size="s" href={formatHash(to.route)} aria-label={`Open what holds the run on ${name}`}>
            Open
          </ButtonLink>
        )}
      </span>
    );
  }
  return <span className="rg-launch__error">{`refused: ${refusalWords(submission.problem)}`}</span>;
}

export function LaunchPanel({ client, store, anchor, pipeline: opened, upTo = null, benchmarks: names, onWhole, onClose }: LaunchPanelProps) {
  const named = names?.length === 1 ? names[0] : undefined;
  const several = names !== undefined && names.length > 1 ? names : null;
  const [pipelines, retryPipelines] = useListing<PipelineListing>((signal) => client.get('/pipelines', { signal }));
  const [benchmarks, retryBenchmarks] = useListing<BenchmarkListing>((signal) => client.get('/benchmarks', { signal }));
  const [services, retryServices] = useListing<ServiceListing>((signal) => client.get('/services', { signal }));
  // Up to a node, the API says what the cut there ends in (`GET /pipelines/{name}`' `ends_in_answer_up_to`).
  const [detail] = useListing<PipelineDetail | null>((signal) =>
    upTo === null || opened === undefined ? Promise.resolve({ ok: true as const, value: null }) : client.get('/pipelines/{name}', { name: opened }, { signal }),
  );
  const [chosenPipeline, setPipeline] = useState<string | null>(opened ?? null);
  const [chosenBenchmark, setBenchmark] = useState<string | null>(named ?? null);
  const [launch, setLaunch] = useState<Launch>({ kind: 'idle' });
  // Several benchmarks: each one's submission, by name, in the order they are sent.
  const [batch, setBatch] = useState<Readonly<Record<string, Submission>> | null>(null);
  const ids = { pipeline: useId(), benchmark: useId(), absent: useId(), list: useId() };
  const { jobs } = useJobs();

  // Up to a node, the pipeline is the one the node is in: no other is offered.
  const listed = pipelines.status === 'loaded' ? pipelines.value.pipelines : [];
  const docs = upTo === null ? listed : listed.filter((p) => p.name === opened);
  const pipeline = docs.find((p) => p.name === chosenPipeline) ?? docs.find((p) => p.hash !== null) ?? docs[0] ?? null;
  // Ready ones only: a benchmark on disk whose digest is the one expected of it, and that what is launched can be scored on.
  const onDisk = benchmarks.status === 'loaded' ? benchmarks.value.benchmarks.filter((b) => b.state.kind === 'ready' || b.state.kind === 'local') : [];
  // What is launched ends in an answer, as the API says it: `GET /pipelines`' `ends_in_answer` of a whole pipeline,
  // `GET /pipelines/{name}`' `ends_in_answer_up_to` of a cut. A whole pipeline that does not validate says nothing,
  // and is refused whatever the benchmark; until the cut's answer is read — or when it cannot be — the cut is taken
  // to end elsewhere, the narrower offer.
  const endsInAnswer = upTo === null ? (pipeline?.ends_in_answer ?? null) : detail.status === 'loaded' ? (detail.value?.ends_in_answer_up_to?.[upTo] ?? false) : false;
  const scores = benchmarks.status !== 'loaded' || endsInAnswer === null ? () => true : scoredOn(benchmarks.value.scorable, endsInAnswer);
  const ready = onDisk.filter(scores);
  const absent = benchmarks.status !== 'loaded' || endsInAnswer === null ? null : notOffered(onDisk.filter((b) => !scores(b)), upTo !== null, endsInAnswer, benchmarks.value.scorable);
  // The benchmark the address named, while it is still the one chosen and is not offered: refused, never replaced.
  // Why it is not offered: the workspace does not know it, it is not on disk as expected, or the cut cannot be scored on it.
  const standing = (name: string): Standing | null =>
    benchmarks.status !== 'loaded' || ready.some((b) => b.name === name)
      ? null
      : !benchmarks.value.benchmarks.some((b) => b.name === name)
        ? 'unknown'
        : onDisk.some((b) => b.name === name)
          ? 'not offered'
          : 'not ready';
  const unoffered = named !== undefined && chosenBenchmark === named && standing(named) !== null ? named : null;
  const unofferedBecause = unoffered === null ? null : standing(unoffered);
  // Several: the ones that can be launched, in the address's order.
  const launchable = several === null || benchmarks.status !== 'loaded' ? [] : several.filter((name) => standing(name) === null);
  const submitted = batch !== null && launchable.every((name) => batch[name] !== undefined && batch[name].kind !== 'sending');
  const sending = batch !== null && Object.values(batch).some((o) => o.kind === 'sending');
  // What a retry sends: the refused alone — a run or job already holding the identity is not refused, it is there.
  const refusedNames = batch === null ? [] : launchable.filter((name) => batch[name]?.kind === 'refused' && (batch[name] as { problem: ApiProblem }).problem.code !== 'run_exists');
  const benchmark = unoffered !== null ? null : (ready.find((b) => b.name === chosenBenchmark) ?? ready[0] ?? null);

  // A change of what would be launched is a new launch: the last answer no longer describes it.
  const choose = (set: (v: string) => void) => (value: string) => {
    set(value);
    setLaunch({ kind: 'idle' });
    setBatch(null);
  };

  const send = async () => {
    if (pipeline === null || benchmark === null) return;
    setLaunch({ kind: 'sending' });
    const request: RunRequest = upTo === null ? { pipeline: pipeline.name, benchmark: benchmark.name } : { pipeline: pipeline.name, benchmark: benchmark.name, up_to: upTo };
    const result = await client.post('/runs', request);
    setLaunch(result.ok ? { kind: 'queued', job: result.value.job_id, run: result.value.run_id } : { kind: 'refused', problem: result.problem });
  };

  // One confirmation, then one `POST /runs` per launchable benchmark, one after another, each answer kept beside its benchmark.
  const sendAll = async (names: readonly string[]) => {
    if (pipeline === null) return;
    for (const name of names) {
      setBatch((b) => ({ ...b, [name]: { kind: 'sending' } }));
      const request: RunRequest = upTo === null ? { pipeline: pipeline.name, benchmark: name } : { pipeline: pipeline.name, benchmark: name, up_to: upTo };
      const result = await client.post('/runs', request);
      setBatch((b) => ({ ...b, [name]: result.ok ? { kind: 'queued', job: result.value.job_id, run: result.value.run_id } : { kind: 'refused', problem: result.problem } }));
    }
  };

  // A launch queued is done with once its job has ended, as the stream says: Launch is offered again.
  const queuedJob = launch.kind === 'queued' ? jobs.get(launch.job) : undefined;
  const ended = queuedJob !== undefined && queuedJob.state.kind !== 'queued' && queuedJob.state.kind !== 'running';
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
        : several !== null
          ? launchable.length === 0
            ? 'None of these benchmarks can be launched: each says why.'
            : submitted && refusedNames.length === 0
              ? 'Submitted: each benchmark says its outcome.'
              : null
        : unoffered !== null
          ? unofferedBecause === 'unknown'
            ? `${unoffered} is not a benchmark of this workspace: choose another benchmark.`
            : unofferedBecause === 'not offered'
              ? `${unoffered} cannot score this ${upTo === null ? 'pipeline' : 'prefix'}: choose another benchmark.`
              : `${unoffered} is not ready: download or import it in Setup, or choose another benchmark.`
          : benchmark === null
          ? onDisk.length > 0
            ? `No ready benchmark can score this ${upTo === null ? 'pipeline' : 'prefix'}: each is named under the benchmark field with why.`
            : 'No benchmark is ready: download or import one in Setup.'
          : launch.kind === 'queued' && !ended
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
              options={docs.length === 0 ? [{ value: '', label: 'No pipeline yet' }] : docs.map((p) => ({ value: p.name, label: p.hash === null ? `${p.name} — does not validate` : p.name }))}
              onChange={(e) => choose(setPipeline)(e.target.value)}
              // Several runs in flight are the chosen pipeline's: another chosen now would have their answers written over it.
              disabled={sending}
              {...(pipelineError === undefined ? {} : { error: pipelineError })}
            />
            {/* The identity the API gives, never one computed here: the pipeline's hash now, the run's once queued. Up to a
                node the cut is a pipeline of its own, whose hash the parent's would only stand in for: the API announces it. */}
            {upTo === null ? (
              <p className="rg-launch__identity">
                {/* The content hash of the document as it is now: a run's identity is derived from it, so it changes when the pipeline does. */}
                {pipeline?.hash == null ? null : <span title={pipeline.hash}>{`Content hash ${shortHash(pipeline.hash)}`}</span>}
              </p>
            ) : (
              <div className="rg-launch__prefix">
                <PrefixLabel parents={opened === undefined ? [] : [opened]} upTo={upTo} />
                <p className="rg-launch__note">The prefix is a pipeline of its own: its identity is announced when it is queued.</p>
                {onWhole === undefined ? null : (
                  <Button size="s" kind="quiet" onClick={onWhole}>
                    Run the whole pipeline
                  </Button>
                )}
              </div>
            )}
          </div>
        ) : null}

        {benchmarks.status === 'loading' ? <Loading label="Reading benchmarks" /> : null}
        {benchmarks.status === 'error' ? <ErrorState problem={benchmarks.problem} onRetry={retryBenchmarks} /> : null}
        {benchmarks.status === 'loaded' && several !== null ? (
          <div className="rg-launch__field">
            <span className="rg-launch__label" id={ids.list}>
              Benchmarks
            </span>
            <ul className="rg-launch__several" aria-labelledby={ids.list}>
              {several.map((name) => (
                <li key={name}>
                  <span className="rg-launch__bench">{name}</span> <SeveralLine what={upTo === null ? 'pipeline' : 'prefix'} name={name} standing={standing(name)} entry={ready.find((b) => b.name === name)} submission={batch?.[name]} />
                </li>
              ))}
            </ul>
            {launchable.length < several.length && several.some((name) => standing(name) === 'not ready') ? (
              <ButtonLink size="s" href={formatHash({ screen: 'setup', section: 'benchmarks' })}>
                Open Setup
              </ButtonLink>
            ) : null}
          </div>
        ) : null}
        {benchmarks.status === 'loaded' && several === null ? (
          <div className="rg-launch__field">
            <Select
              id={ids.benchmark}
              label="Benchmark"
              value={unoffered ?? benchmark?.name ?? ''}
              options={
                unoffered === null && ready.length === 0
                  ? [{ value: '', label: 'No ready benchmark' }]
                  : [
                      ...(unoffered === null ? [] : [{ value: unoffered, label: `${unoffered} — ${unofferedBecause ?? 'not ready'}` }]),
                      ...ready.map((b) => ({ value: b.name, label: b.ground_truth === null ? b.name : `${b.name} — ${groundTruthLabel(b.ground_truth)}` })),
                    ]
              }
              onChange={(e) => choose(setBenchmark)(e.target.value)}
              {...(benchmarkError === undefined ? { help: unoffered === null ? 'Ready benchmarks only, with the ground truth each carries.' : `Ready benchmarks, with the ground truth each carries, and ${unoffered}, which the address named.` } : { error: benchmarkError })}
              {...(absent === null ? {} : { 'aria-describedby': ids.absent })}
            />
            {absent === null ? null : (
              <p id={ids.absent} className="rg-launch__note">
                {absent}
              </p>
            )}
            {ready.length === 0 || unofferedBecause === 'not ready' ? (
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
          {several !== null ? (
            <>
              {why !== null ? (
                <Button kind="primary" disabled disabledReason={why} showReason>
                  {launchable.length === 0 ? 'Nothing to launch' : submitted ? 'Submitted' : runsLabel(launchable.length)}
                </Button>
              ) : submitted ? (
                <Button kind="primary" onClick={() => void sendAll(refusedNames)}>
                  {`Retry the ${refusedNames.length} refused`}
                </Button>
              ) : (
                <Button kind="primary" busy={sending} busyLabel="Launching…" onClick={() => void sendAll(launchable)}>
                  {runsLabel(launchable.length)}
                </Button>
              )}
              <span className="rg-launch__note" role="status">
                {submitted && batch !== null ? outcomeCounts(launchable.map((name) => batch[name] as Submission)) : ''}
              </span>
              {sending ? <span className="rg-launch__note">Closing this panel does not stop the runs not yet sent: they are sent all the same.</span> : null}
            </>
          ) : launch.kind === 'queued' && !ended ? (
            <Button kind="primary" disabled disabledReason={why ?? 'Queued.'} showReason>
              Queued
            </Button>
          ) : why !== null ? (
            <Button kind="primary" disabled disabledReason={why} showReason>
              Launch
            </Button>
          ) : (
            <Button kind="primary" busy={launch.kind === 'sending'} busyLabel="Launching…" onClick={() => void send()}>
              Launch
            </Button>
          )}
          {/* The identity the API announced stays once the job ended, with the way to it: Launch is offered again beside it. */}
          {several === null && launch.kind === 'queued' ? (
            <span className="rg-launch__identity">
              <code title={launch.run}>{`run ${shortHash(launch.run)}`}</code> <span className="rg-launch__note">announced</span>{' '}
              <ButtonLink size="s" href={formatHash({ screen: 'runs', job: launch.job })}>
                Open
              </ButtonLink>
            </span>
          ) : null}
          {onClose === undefined ? null : (
            <Button kind="quiet" onClick={onClose}>
              Close
            </Button>
          )}
        </div>
        {refused?.code === 'run_exists' ? <Conflict problem={refused} /> : null}
        {elsewhere === null ? null : <ErrorState problem={elsewhere} />}
      </div>
    </Section>
  );
}

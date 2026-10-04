/** @vitest-environment happy-dom */
// The launch flow on the Runs screen, against the API's mocks and a fake job
// stream: the launch panel, the queue's rows, cancel and reorder, the stream
// down and back, and the outcomes as toasts. Each test is one the issue that
// built the flow requires, under its name.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { FakeEventSource, installFakeEventSource, mockApi, type MockRoutes } from '../api/testing.ts';
import type { BenchmarkEntry, JobListing, JobSummary, PipelineSummary, Problem, RunListing, RunSummary } from '../api/types.ts';
import { CANCELLED, connect, doneAs, failedAt, hex, QUEUED, runJob, running, send } from '../jobs/fixtures.ts';
import { JobQueueProvider } from '../jobs/queue.tsx';
import { JobToasts } from '../jobs/Toasts.tsx';
import { useRoute } from '../routes.ts';
import { parseRules } from '../../design/testing/css.ts';
import toastsCss from '../jobs/Toasts.css?raw';
import runsCss from './Runs.css?raw';
import { RunsScreen } from './RunsScreen.tsx';

const SCIFACT = hex('5');
const HYBRID = hex('c');
const ANNOUNCED = hex('a');
const short = (id: string) => id.slice(0, 12);

const pipeline = (name: string, over: Partial<PipelineSummary> = {}): PipelineSummary => ({ name, etag: 'e', hash: HYBRID, error: null, modified_ms: 1, ...over });
const PIPELINES = { pipelines: [pipeline('hybrid'), pipeline('broken', { hash: null, error: { detail: 'node `rerank` reads `fused`, which no node writes', location: { node: 'rerank', edge: null } } })] };

const bench = (name: string, state: BenchmarkEntry['state'], truth: BenchmarkEntry['ground_truth'] = 'qrels'): BenchmarkEntry => ({ name, format: 'beir', ground_truth: truth, licence: null, licence_url: null, state });
const BENCHMARKS = {
  benchmarks: [bench('beir/scifact', { kind: 'ready', dataset_version: SCIFACT }), bench('beir/fiqa', { kind: 'available', size_bytes: 1 }, null), bench('mine', { kind: 'local', dataset_version: hex('7') }, 'both')],
};
const SERVICES = { services: [{ family: 'generator', name: 'qwen', uri: 'http://127.0.0.1:8080', connected: true, identity: 'qwen2.5-7b' }] };

const run = (id: string, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline: HYBRID,
  pipeline_names: ['hybrid'],
  refused_pipeline_names: [],
  launched_as: { name: 'hybrid', held: 'exactly', prefix_of: null },
  dataset_version: SCIFACT,
  benchmark_names: ['beir/scifact'],
  index_version: hex('0'),
  engine_version: '0.0.0',
  started_at_ms: 1,
  finished_at_ms: 2,
  metrics: { 'ndcg@10': 0.65 },
  metric_families: { 'ndcg@10': 'ranking' },
  median_query_latency_nanos: null,
  ...over,
});
const OLD = hex('1');
const LISTING: RunListing = { runs: [run(OLD)], unreadable: [], shapes: {} };

const problem = (code: Problem['code'], status: number, detail: string, hint: string, over: Partial<Problem> = {}): { problem: Problem } => ({
  problem: { type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint, ...over },
});

const routes = (over: MockRoutes = {}): MockRoutes => ({
  'GET /runs': { body: LISTING },
  'GET /pipelines': { body: PIPELINES },
  'GET /benchmarks': { body: BENCHMARKS },
  'GET /services': { body: SERVICES },
  ...over,
});

/** What the shell does: follows the job stream for the page, hands Runs its route, and raises the toasts. */
function Shell() {
  const [client] = useState(() => createApiClient());
  const route = useRoute();
  const runs = route?.screen === 'runs' ? route : { screen: 'runs' as const };
  return (
    <JobQueueProvider>
      <RunsScreen client={client} sel={runs.sel ?? []} job={runs.job} store="/work/ws" />
      <JobToasts />
    </JobQueueProvider>
  );
}

async function show(hash = '#runs', mocks: MockRoutes = routes()) {
  window.history.replaceState(null, '', `/${hash}`);
  const api = mockApi(mocks);
  render(<Shell />);
  await screen.findByRole('row', { name: new RegExp(`^Run ${short(OLD)} on `) });
  // The screen's effects run after the commit the row appeared in: let them, before the test drives the stream.
  await act(async () => {});
  return { api, stream: FakeEventSource.latest() };
}

const panel = () => screen.getByRole('region', { name: 'Launch a run' });
async function openPanel() {
  fireEvent.click(screen.getByRole('button', { name: 'Launch…' }));
  await within(panel()).findByText(`pipeline ${short(HYBRID)}`);
  return panel();
}
const jobRowOf = (id = ANNOUNCED) => screen.getByRole('row', { name: new RegExp(`^Run ${short(id)} on beir/scifact, `) });
const notifications = () => screen.getByRole('region', { name: 'Notifications' });

beforeEach(() => {
  installFakeEventSource();
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  window.history.replaceState(null, '', '/');
});

describe('the launch panel', () => {
  it('launch_panel_shows_the_announced_identity_and_queues_on_launch', async () => {
    const { api, stream } = await show('#runs', routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    const at = await openPanel();
    // Before launching: the pipeline's hash, once it validates; the benchmarks that are ready, with their ground truth;
    // the bindings in force, read-only; the store, shown not chosen.
    expect(within(at).getByText(`pipeline ${short(HYBRID)}`)).toBeTruthy();
    const benchmark = within(at).getByLabelText('Benchmark') as HTMLSelectElement;
    expect([...benchmark.options].map((o) => o.textContent)).toEqual(['beir/scifact — qrels', 'mine — qrels and reference answers']);
    expect(within(at).getByText('generator/qwen')).toBeTruthy();
    expect(within(at).getByText('/work/ws')).toBeTruthy();

    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const queued = await within(at).findByRole('button', { name: 'Queued' });
    expect(api.requests).toContain('POST /api/v1/runs');
    expect(api.bodies[api.requests.indexOf('POST /api/v1/runs')]).toEqual({ pipeline: 'hybrid', benchmark: 'beir/scifact' });
    expect(queued.getAttribute('aria-disabled')).toBe('true');
    expect(within(at).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
    expect(within(at).getByText('announced')).toBeTruthy();

    // The queued row appears in the pipeline's group within one render of the event.
    send(stream, { event: 'queued', data: runJob('j1', QUEUED) });
    const row = jobRowOf();
    expect(within(row).getByText('queued, next')).toBeTruthy();
    const group = row.closest('tbody') as HTMLElement;
    expect(within(group).getByRole('row', { name: new RegExp(`^Run ${short(OLD)} on `) })).toBeTruthy();
  });

  it('a_conflict_is_shown_as_an_existing_run_or_job_to_open_not_as_an_error', async () => {
    const conflicts = [
      problem('run_exists', 409, `Run ${OLD} is already in the store.`, 'Open it.', { link: `/api/v1/runs/${OLD}` }),
      problem('run_exists', 409, 'Job j7 is already queued under this run id.', 'Open it.', { link: '/api/v1/jobs/j7' }),
    ];
    const { stream } = await show('#runs', routes({ 'POST /runs': conflicts }));
    connect(stream, [runJob('j7', QUEUED)]);
    const at = await openPanel();

    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const exists = await within(at).findByText('A run with this identity already exists.');
    expect(within(at).queryByRole('alert')).toBeNull();
    const open = within(exists.closest('.rg-inline') as HTMLElement).getByRole('link', { name: 'Open' });
    expect(open.getAttribute('href')).toBe(`#replay/${OLD}`);

    fireEvent.change(within(at).getByLabelText('Benchmark'), { target: { value: 'mine' } });
    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const running = await within(at).findByText('A run with this identity is already queued or running.');
    const openJob = within(running.closest('.rg-inline') as HTMLElement).getByRole('link', { name: 'Open' });
    expect(openJob.getAttribute('href')).toBe('#runs/job/j7');
    fireEvent.click(openJob);
    window.location.hash = '#runs/job/j7';
    await waitFor(() => expect(jobRowOf().getAttribute('aria-current')).toBe('true'));
  });

  it('invalid_pipeline_and_unreachable_service_answers_are_shown_inline_with_the_hint', async () => {
    const answers = [
      problem('pipeline_invalid', 400, 'node `rerank` expects chunks on input 1 and gets a query', 'Connect a chunks output to rerank input 1.', { location: { node: 'rerank', edge: null } }),
      problem('service_unreachable', 502, 'generator/qwen at http://127.0.0.1:8080 did not answer: connection refused', 'Start the service, or connect another in Setup.'),
      problem('impl_not_in_build', 400, 'node `dense` names `embedder/onnx`, which this build does not carry', 'Rebuild with the feature, or bind a Remote under this name.'),
    ];
    const { stream } = await show('#runs', routes({ 'POST /runs': answers }));
    connect(stream);
    const at = await openPanel();
    const pipelineField = within(at).getByLabelText('Pipeline');

    // A document the listing already says does not validate is refused before anything is sent.
    fireEvent.change(pipelineField, { target: { value: 'broken' } });
    expect(within(at).getByText(/node `rerank` reads `fused`/)).toBeTruthy();
    expect(within(at).getByRole('button', { name: 'Launch' }).getAttribute('aria-disabled')).toBe('true');
    fireEvent.change(pipelineField, { target: { value: 'hybrid' } });

    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const invalid = await within(at).findByText(/expects chunks on input 1/);
    expect(invalid.textContent).toContain('At node rerank.');
    expect(invalid.textContent).toContain('Connect a chunks output to rerank input 1.');
    expect(pipelineField.getAttribute('aria-describedby')).toBe(invalid.closest('[id]')?.id);

    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const unreachable = await within(at).findByText(/did not answer: connection refused/);
    expect(unreachable.closest('.rg-launch__bindings')).toBeTruthy();
    expect(unreachable.textContent).toContain('Start the service, or connect another in Setup.');

    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const missing = await within(at).findByText(/which this build does not carry/);
    expect(missing.textContent).toContain('Rebuild with the feature, or bind a Remote under this name.');
    expect(pipelineField.getAttribute('aria-invalid')).toBe('true');
  });
});

describe('the queue’s rows', () => {
  it('running_ticks_update_progress_median_and_elapsed', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(1_700_000_010_000);
    const { stream } = await show();
    connect(stream, [runJob('j1', QUEUED)]);
    send(stream, { event: 'running', data: runJob('j1', running(0, null)) });
    expect(within(jobRowOf()).getByText('starting')).toBeTruthy();
    for (const [done, median] of [
      [1, 2_000_000],
      [2, 1_900_000],
      [3, 1_840_000],
    ] as const) {
      send(stream, { event: 'running', data: runJob('j1', running(done, 10, median)) });
    }
    const row = jobRowOf();
    expect(within(row).getByText('running 3 / 10')).toBeTruthy();
    expect((row.querySelector('.rg-status__meter i') as HTMLElement).style.width).toBe('30%');
    expect(within(row).getByText('1.8 ms / query')).toBeTruthy();
    expect(within(row).getByText('0:10 elapsed')).toBeTruthy();
    act(() => vi.advanceTimersByTime(5000));
    expect(within(jobRowOf()).getByText('0:15 elapsed')).toBeTruthy();
  });

  it('done_and_failed_events_replace_the_row_and_raise_one_toast_each', async () => {
    const DONE = ANNOUNCED;
    const FAILED = hex('b');
    const later: RunListing = { ...LISTING, runs: [run(DONE, { started_at_ms: 5 }), ...LISTING.runs] };
    const { api, stream } = await show('#runs', routes({ 'GET /runs': [{ body: LISTING }, { body: later }] }));
    connect(stream, [runJob('j1', running(9, 10)), runJob('j2', QUEUED, { runId: FAILED, position: 1 })]);

    send(stream, { event: 'done', data: runJob('j1', doneAs(DONE)) });
    // The store is read again, and the run's row takes the job's place.
    await screen.findByRole('row', { name: new RegExp(`^Run ${short(DONE)} on beir/scifact$`) });
    expect(api.requests.filter((r) => r === 'GET /api/v1/runs')).toHaveLength(2);
    expect(screen.queryByRole('row', { name: new RegExp(`^Run ${short(DONE)} on beir/scifact, `) })).toBeNull();

    send(stream, { event: 'running', data: runJob('j2', running(1, 10), { runId: FAILED }) });
    send(stream, { event: 'failed', data: runJob('j2', failedAt('rerank', 'the reranker answered 503'), { runId: FAILED }) });
    const failedRow = jobRowOf(FAILED);
    expect(within(failedRow).getByText('rerank failed: the reranker answered 503')).toBeTruthy();

    const toasts = within(notifications());
    expect(toasts.getByRole('status').textContent).toContain('Run done');
    expect(toasts.getByRole('alert').textContent).toContain('Run failed at rerank');
    expect(notifications().querySelectorAll('.rg-toast')).toHaveLength(2);
    fireEvent.click(within(toasts.getByRole('status')).getByRole('button', { name: 'Open' }));
    expect(window.location.hash).toBe(`#replay/${DONE}`);
  });

  it('a_failed_job_opens_with_its_partial_traces', async () => {
    const { stream } = await show('#runs/job/j2');
    connect(stream, [runJob('j2', failedAt('rerank', 'the reranker answered 503'), { runId: hex('b') })]);
    const job = await screen.findByRole('region', { name: 'Job j2' });
    expect(job.querySelector('.rg-status')?.textContent).toBe('failed at rerank');
    expect(within(job).getByText('rerank failed: the reranker answered 503')).toBeTruthy();
    // Where the partial traces are, said as what it is: kept by the job, not in the store, and not served yet.
    expect(within(job).getByText(/traces of the queries it executed before it failed are kept under jobs\/j2\/partial\/traces\.json/)).toBeTruthy();
    expect(within(job).getByText(/does not serve them yet, so Replay cannot open them/)).toBeTruthy();
    expect(within(job).getByText(short(hex('b')))).toBeTruthy();
    expect(jobRowOf(hex('b')).getAttribute('aria-current')).toBe('true');
  });

  it('cancel_and_resubmit_round_trip_through_the_api', async () => {
    const cancelledQueued = runJob('j2', CANCELLED, { position: 1 });
    const { api, stream } = await show(
      '#runs',
      routes({
        'DELETE /jobs/{id}': [{ body: cancelledQueued }, { body: runJob('j1', running(4, 10)) }],
        'POST /runs': { body: { job_id: 'j3', run_id: ANNOUNCED } },
      }),
    );
    connect(stream, [runJob('j1', running(3, 10), { runId: hex('d') }), runJob('j2', QUEUED, { position: 1 })]);

    // A queued row: DELETE, then Cancelled once the queue says so, focus kept on the row's next control.
    const cancel = within(jobRowOf()).getByRole('button', { name: `Cancel run ${short(ANNOUNCED)}` });
    cancel.focus();
    fireEvent.click(cancel);
    await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/jobs/j2'));
    send(stream, { event: 'cancelled', data: cancelledQueued });
    expect(within(jobRowOf()).getByText('cancelled')).toBeTruthy();
    const resubmit = within(jobRowOf()).getByRole('button', { name: `Resubmit run ${short(ANNOUNCED)}` });
    expect(document.activeElement).toBe(resubmit);

    // Resubmitting sends the same submission again.
    fireEvent.click(resubmit);
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/runs'));
    expect(api.bodies[api.requests.indexOf('POST /api/v1/runs')]).toEqual({ pipeline: 'hybrid', benchmark: 'beir/scifact', up_to: null });
    send(stream, { event: 'queued', data: runJob('j3', QUEUED, { position: 2 }) });
    expect(within(jobRowOf()).getByText('queued, next')).toBeTruthy();
    expect(screen.getAllByRole('row', { name: new RegExp(`^Run ${short(ANNOUNCED)} on `) })).toHaveLength(1);
    // The Resubmit that had focus went with its row: focus is on the new job's Cancel.
    expect(document.activeElement).toBe(within(jobRowOf()).getByRole('button', { name: `Cancel run ${short(ANNOUNCED)}` }));

    // The running row says "cancelling…" until the cancelled event arrives.
    const current = jobRowOf(hex('d'));
    fireEvent.click(within(current).getByRole('button', { name: `Cancel run ${short(hex('d'))}` }));
    await waitFor(() => expect(api.requests).toContain('DELETE /api/v1/jobs/j1'));
    expect(within(jobRowOf(hex('d'))).getByText('cancelling…')).toBeTruthy();
    send(stream, { event: 'running', data: runJob('j1', running(4, 10), { runId: hex('d') }) });
    expect(within(jobRowOf(hex('d'))).getByText('cancelling…')).toBeTruthy();
    send(stream, { event: 'cancelled', data: runJob('j1', CANCELLED, { runId: hex('d') }) });
    expect(within(jobRowOf(hex('d'))).getByText('cancelled')).toBeTruthy();
  });

  it('reorder_follows_the_api_answer', async () => {
    const A = hex('a');
    const B = hex('b');
    const C = hex('e');
    // The API answers an order of its own — the drag asked for B first, and the queue put B second.
    const answer: JobListing = { faults: [], jobs: [runJob('ja', QUEUED, { runId: A, position: 0 }), runJob('jb', QUEUED, { runId: B, position: 1 }), runJob('jc', QUEUED, { runId: C, position: 2 })] };
    const { api, stream } = await show('#runs', routes({ 'PATCH /jobs/{id}': { body: answer } }));
    connect(stream, [runJob('jc', QUEUED, { runId: C, position: 0 }), runJob('ja', QUEUED, { runId: A, position: 1 }), runJob('jb', QUEUED, { runId: B, position: 2 })]);
    const order = () => screen.getAllByRole('row', { name: /, queued, / }).map((r) => r.getAttribute('aria-label')?.slice(4, 16));
    expect(order()).toEqual([short(C), short(A), short(B)]);

    const up = within(jobRowOf(B)).getByRole('button', { name: `Move run ${short(B)} up` });
    up.focus();
    fireEvent.click(up);
    await waitFor(() => expect(order()).toEqual([short(A), short(B), short(C)]));
    expect(api.requests).toContain('PATCH /api/v1/jobs/jb');
    expect(api.bodies[api.requests.indexOf('PATCH /api/v1/jobs/jb')]).toEqual({ position: 1 });
    // Focus follows the row it moved.
    expect(document.activeElement).toBe(within(jobRowOf(B)).getByRole('button', { name: `Move run ${short(B)} up` }));

    // The stream's own word on the order is the last: a reorder made elsewhere is followed.
    for (const j of [runJob('jb', QUEUED, { runId: B, position: 0 }), runJob('ja', QUEUED, { runId: A, position: 1 }), runJob('jc', QUEUED, { runId: C, position: 2 })]) {
      send(stream, { event: 'reordered', data: j });
    }
    expect(order()).toEqual([short(B), short(A), short(C)]);
  });

  it('follows the stream, not a reorder’s answer that the stream’s own events overtook', async () => {
    const A = hex('a');
    const B = hex('b');
    const C = hex('e');
    const q = (id: string, runId: string, position: number) => runJob(id, QUEUED, { runId, position });
    // Two quick moves; the stream says both, then the answers arrive, the older last.
    const answers: ((reply: { body: JobListing }) => void)[] = [];
    const { stream } = await show('#runs', routes({ 'PATCH /jobs/{id}': () => new Promise((resolve) => answers.push(resolve)) }));
    connect(stream, [q('ja', A, 0), q('jb', B, 1), q('jc', C, 2)]);
    const order = () => screen.getAllByRole('row', { name: /, queued/ }).map((r) => r.getAttribute('aria-label')?.slice(4, 16));

    fireEvent.click(within(jobRowOf(B)).getByRole('button', { name: `Move run ${short(B)} up` }));
    fireEvent.click(within(jobRowOf(C)).getByRole('button', { name: `Move run ${short(C)} up` }));
    await waitFor(() => expect(answers).toHaveLength(2));
    // The queue publishes each reorder's events before it answers it.
    for (const j of [q('jb', B, 0), q('ja', A, 1)]) send(stream, { event: 'reordered', data: j });
    for (const j of [q('jc', C, 1), q('ja', A, 2)]) send(stream, { event: 'reordered', data: j });
    expect(order()).toEqual([short(B), short(C), short(A)]);
    await act(async () => answers[1]?.({ body: { faults: [], jobs: [q('jb', B, 0), q('jc', C, 1), q('ja', A, 2)] } }));
    await act(async () => answers[0]?.({ body: { faults: [], jobs: [q('jb', B, 0), q('ja', A, 1), q('jc', C, 2)] } }));
    expect(order()).toEqual([short(B), short(C), short(A)]);
  });

  it('follows a resync that arrives while a reorder is in flight, not the answer that comes after it', async () => {
    const A = hex('a');
    const B = hex('b');
    const C = hex('e');
    const q = (id: string, runId: string, position: number) => runJob(id, QUEUED, { runId, position });
    let answer: (reply: { body: JobListing }) => void = () => {};
    const { stream } = await show('#runs', routes({ 'PATCH /jobs/{id}': () => new Promise((resolve) => (answer = resolve)) }));
    connect(stream, [q('ja', A, 0), q('jb', B, 1), q('jc', C, 2)]);
    const order = () => screen.getAllByRole('row', { name: /, queued/ }).map((r) => r.getAttribute('aria-label')?.slice(4, 16));

    fireEvent.click(within(jobRowOf(C)).getByRole('button', { name: `Move run ${short(C)} up` }));
    // The stream starts again meanwhile, and its whole queue already has a later order than this move's.
    send(stream, { event: 'resync', data: { faults: [], jobs: [q('jc', C, 0), q('ja', A, 1), q('jb', B, 2)] } });
    await act(async () => answer({ body: { faults: [], jobs: [q('ja', A, 0), q('jc', C, 1), q('jb', B, 2)] } }));
    expect(order()).toEqual([short(C), short(A), short(B)]);
  });

  it('shows both ids on the run filed under another id than its job announced', async () => {
    const DECIDED = hex('d');
    const later: RunListing = { ...LISTING, runs: [run(DECIDED, { started_at_ms: 5 }), ...LISTING.runs] };
    const { stream } = await show('#runs', routes({ 'GET /runs': [{ body: LISTING }, { body: later }] }));
    connect(stream, [runJob('j1', running(9, 10))]);
    send(stream, { event: 'done', data: runJob('j1', doneAs(DECIDED, { announced: ANNOUNCED, decided: DECIDED })) });
    const row = await screen.findByRole('row', { name: new RegExp(`^Run ${short(DECIDED)} on beir/scifact$`) });
    expect(within(row).getByText(`announced as ${short(ANNOUNCED)}; filed under this id because what ran differs from what was announced`)).toBeTruthy();
  });

  it('names a running row by where it stands, not by its count, so a tick does not re-announce it', async () => {
    const { stream } = await show();
    connect(stream, [runJob('j1', running(1, 10))]);
    const label = jobRowOf().getAttribute('aria-label');
    send(stream, { event: 'running', data: runJob('j1', running(2, 10)) });
    expect(jobRowOf().getAttribute('aria-label')).toBe(label);
    expect(label).toBe(`Run ${short(ANNOUNCED)} on beir/scifact, running`);
    expect(within(jobRowOf()).getByText('running 2 / 10')).toBeTruthy();
  });

  it('says a done job that names no run as such, rather than reading the store for it forever', async () => {
    const { api, stream } = await show();
    connect(stream, [runJob('j1', running(9, 10))]);
    send(stream, { event: 'done', data: runJob('j1', { kind: 'done', run_id: null, id_mismatch: null, finished_at_ms: 2 }) });
    expect(within(jobRowOf()).getByText('Done; the queue names no run filed.')).toBeTruthy();
    await act(async () => {});
    expect(api.requests.filter((r) => r === 'GET /api/v1/runs')).toHaveLength(1);
  });
});

describe('the stream', () => {
  it('a_dropped_stream_is_shown_and_missed_events_replay_once_on_reconnect', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { stream } = await show();
    connect(stream, [runJob('j1', running(3, 10))]);
    const line = screen.getByTestId('job-stream');
    expect(line.textContent).toBe('');

    act(() => stream.fail());
    expect(line.textContent).toBe('Job stream disconnected, retrying: progress shown is the last known.');
    expect(within(jobRowOf()).getByText('running 3 / 10 · last known')).toBeTruthy();

    // The browser's own retry resumes after the last id it saw: the server replays what was missed, once.
    act(() => stream.open());
    send(stream, { event: 'running', data: runJob('j1', running(9, 10)) });
    send(stream, { event: 'done', data: runJob('j1', doneAs(ANNOUNCED)) });
    expect(line.textContent).toBe('');
    expect(within(jobRowOf()).getByText('done')).toBeTruthy();
    expect(notifications().querySelectorAll('.rg-toast')).toHaveLength(1);

    // Given up on, the stream is opened anew and begins with the whole queue: nothing is said twice.
    act(() => stream.drop());
    act(() => vi.advanceTimersByTime(1000));
    connect(FakeEventSource.latest(), [runJob('j1', doneAs(ANNOUNCED))]);
    expect(notifications().querySelectorAll('.rg-toast')).toHaveLength(1);
  });
});

describe('the keyboard', () => {
  it('every_control_is_operable_by_keyboard', async () => {
    const { api, stream } = await show(
      '#runs',
      routes({
        'POST /runs': { body: { job_id: 'j9', run_id: hex('9') } },
        'PATCH /jobs/{id}': { body: { faults: [], jobs: [] as JobSummary[] } },
        'DELETE /jobs/{id}': { body: runJob('j2', CANCELLED) },
      }),
    );
    connect(stream, [runJob('j1', QUEUED, { runId: hex('d'), position: 0 }), runJob('j2', QUEUED, { position: 1 })]);

    // The panel: opened from a button, every field a native control, Launch a button.
    const toggle = screen.getByRole('button', { name: 'Launch…' });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    const at = panel();
    await within(at).findByText(`pipeline ${short(HYBRID)}`);
    // Focus moves to the panel, so the keyboard starts from it: its fields come next in tab order.
    expect(document.activeElement).toBe(at);
    for (const control of [within(at).getByLabelText('Pipeline'), within(at).getByLabelText('Benchmark'), within(at).getByRole('button', { name: 'Launch' })]) {
      expect(control.tabIndex).toBe(0);
      expect(['SELECT', 'BUTTON']).toContain(control.tagName);
    }

    // A row: it takes the table's keys, and each of its controls is a native button in the tab order.
    const row = jobRowOf();
    for (const button of within(row).getAllByRole('button')) {
      expect(button.tagName).toBe('BUTTON');
      expect(button.tabIndex).toBe(0);
    }
    row.focus();
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(window.location.hash).toBe('#runs/job/j2');
    fireEvent.click(within(row).getByRole('button', { name: `Move run ${short(ANNOUNCED)} up` }));
    await waitFor(() => expect(api.requests).toContain('PATCH /api/v1/jobs/j2'));
  });
});

describe('the layout', () => {
  const rule = (css: string, selector: string, atRule: string | null = null) => parseRules(css).find((r) => r.selector === selector && r.atRule === atRule);

  it('holds the stream line one line high, and two at a phone’s width where its longer sentence wraps, so no row moves as the stream drops', () => {
    expect(rule(runsCss, '.rg-runs__stream')?.declarations.get('min-height')).toBe('var(--space-4)');
    expect(rule(runsCss, '.rg-runs__stream', '@media (max-width: 640px)')?.declarations.get('min-height')).toBe('calc(2 * var(--space-4))');
  });

  it('holds every job row one small control high, a button in it or not, as a block box', () => {
    expect(rule(runsCss, '.rg-runs__job')?.declarations.get('min-height')).toBe('var(--size-control-s)');
    expect(rule(runsCss, '.rg-runs__job')?.declarations.get('display')).toBe('flex');
  });

  it('gives a running job’s figures and Cancel one line, so the median arriving with the first tick wraps nothing and grows no row', () => {
    // Measured in Chrome: "1,840 ms / query", "12:34 elapsed" and Cancel side by side take 37 ch of the body face.
    expect(rule(runsCss, '.rg-runs__job')?.declarations.get('min-width')).toBe('40ch');
  });

  it('floats the toasts over the page’s corner, within a phone’s width, and lets a click through where there is none', () => {
    const region = rule(toastsCss, '.rg-toasts');
    expect(region?.declarations.get('position')).toBe('fixed');
    expect(region?.declarations.get('pointer-events')).toBe('none');
    expect(rule(toastsCss, '.rg-toasts > *')?.declarations.get('pointer-events')).toBe('auto');
    expect(rule(toastsCss, '.rg-toasts', '@media (max-width: 640px)')?.declarations.get('left')).toBe('var(--space-4)');
  });
});

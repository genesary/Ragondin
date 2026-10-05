/** @vitest-environment happy-dom */
// The launch flow on the Runs screen, against the API's mocks and a fake job
// stream: the launch panel, the queue's rows, cancel and reorder, the stream
// down and back, and the outcomes as toasts. Each test is one the issue that
// built the flow requires, under its name.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createApiClient } from '../api/client.ts';
import { FakeEventSource, installFakeEventSource, mockApi, SCORABLE, type MockRoutes } from '../api/testing.ts';
import type { BenchmarkEntry, JobListing, JobSummary, PipelineDetail, PipelineSummary, Problem, RunListing, RunSummary } from '../api/types.ts';
import { CANCELLED, connect, doneAs, failedAt, hex, QUEUED, runJob, running, send } from '../jobs/fixtures.ts';
import { JobQueueProvider } from '../jobs/queue.tsx';
import { JobToasts } from '../jobs/Toasts.tsx';
import { navigate, useRoute } from '../routes.ts';
import { parseRules } from '../../design/testing/css.ts';
import toastsCss from '../jobs/Toasts.css?raw';
import runsCss from './Runs.css?raw';
import { RunsScreen } from './RunsScreen.tsx';

const SCIFACT = hex('5');
const HYBRID = hex('c');
const ANNOUNCED = hex('a');
const short = (id: string) => id.slice(0, 12);

const pipeline = (name: string, over: Partial<PipelineSummary> = {}): PipelineSummary => ({ name, etag: 'e', hash: HYBRID, error: null, ends_in_answer: false, modified_ms: 1, ...over });
// `hybrid` ends in a generator (HYBRID_DETAIL); `lexical` is retrieval only.
const PIPELINES = { pipelines: [pipeline('hybrid', { ends_in_answer: true }), pipeline('lexical', { hash: hex('9') }), pipeline('broken', { hash: null, ends_in_answer: null, error: { detail: 'node `rerank` reads `fused`, which no node writes', location: { node: 'rerank', edge: null } } })] };

const bench = (name: string, state: BenchmarkEntry['state'], truth: BenchmarkEntry['ground_truth'] = 'qrels'): BenchmarkEntry => ({ name, format: 'beir', ground_truth: truth, licence: null, licence_url: null, state });
const BENCHMARKS = {
  scorable: SCORABLE,
  benchmarks: [bench('beir/scifact', { kind: 'ready', dataset_version: SCIFACT }), bench('beir/fiqa', { kind: 'available', size_bytes: 1 }, null), bench('mine', { kind: 'local', dataset_version: hex('7') }, 'both')],
};
const SERVICES = { services: [{ family: 'generator', name: 'qwen', uri: 'http://127.0.0.1:8080', connected: true, identity: 'qwen2.5-7b' }] };

const run = (id: string, over: Partial<RunSummary> = {}): RunSummary => ({
  id,
  pipeline: HYBRID,
  pipeline_names: ['hybrid'],
  refused_pipeline_names: [], prefix_of_documents: [],
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

/**
 * `hybrid` as `GET /pipelines/{name}` serves it: a reranker, then a generator
 * whose answer a judge reads — so a cut at the generator ends in an answer.
 */
const node = (id: string, component: string, inputs: string[]) => ({ id, component, impl: component, inputs, params: {} });
const HYBRID_DETAIL: PipelineDetail = {
  name: 'hybrid',
  document: 'pipeline: …',
  etag: 'e',
  hash: HYBRID,
  error: null,
  canonical: false,
  // What each cut ends in, as the API serves it: only the generator's ends in an answer.
  ends_in_answer_up_to: { bm25: false, rerank: false, context: false, generate: true, judge: false },
  typed: {
    pipeline: {
      inputs: ['question'],
      nodes: [
        node('bm25', 'retriever', ['question']),
        node('rerank', 'reranker', ['question', 'bm25']),
        node('context', 'context_builder', ['question', 'rerank']),
        node('generate', 'generator', ['question', 'context']),
        node('judge', 'extension', ['generate']),
      ],
    },
  },
};

const routes = (over: MockRoutes = {}): MockRoutes => ({
  'GET /runs': { body: LISTING },
  'GET /pipelines/{name}': { body: HYBRID_DETAIL },
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
      <RunsScreen client={client} sel={runs.sel ?? []} job={runs.job} launch={runs.launch} bench={runs.bench} store="/work/ws" />
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
  await within(panel()).findByText(`Content hash ${short(HYBRID)}`);
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
    expect(within(at).getByText(`Content hash ${short(HYBRID)}`)).toBeTruthy();
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

    // The queued row appears in the Queue within one render of the event, naming its pipeline.
    send(stream, { event: 'queued', data: runJob('j1', QUEUED) });
    const row = jobRowOf();
    expect(within(row).getByText('queued, next')).toBeTruthy();
    expect(row.closest('table')?.getAttribute('aria-label') ?? row.closest('table')?.querySelector('caption')?.textContent).toBe('Queue');
    expect(within(row).getByText('hybrid')).toBeTruthy();
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

describe('the launch panel, as the UX audit left it', () => {
  it('offers a whole pipeline only the benchmarks it can be scored on, judged by what it ends in, and says why the others are absent', async () => {
    const { stream } = await show();
    connect(stream);
    const at = await openPanel();
    const benchmark = within(at).getByLabelText('Benchmark') as HTMLSelectElement;
    // `hybrid` ends in an answer: every ready benchmark with a ground truth.
    expect([...benchmark.options].map((o) => o.value)).toEqual(['beir/scifact', 'mine']);
    expect(within(at).queryByText(/Not offered/)).toBeNull();

    fireEvent.change(within(at).getByLabelText('Pipeline'), { target: { value: 'lexical' } });
    expect([...benchmark.options].map((o) => o.value)).toEqual(['beir/scifact']);
    // Worded from what the API serves — the benchmark's ground truth, and the ones this output can be scored on — never a rule of its own.
    const absent = within(at).getByText('Not offered: mine, which carries qrels and reference answers: a pipeline that does not end in an answer can be scored only on a benchmark whose ground truth is qrels.');
    expect(benchmark.getAttribute('aria-describedby')?.split(' ')).toContain(absent.id);
  });

  it('offers exactly the ground truths the API lists, and applies no rule of its own to a benchmark that carries none', async () => {
    const withNone = [...BENCHMARKS.benchmarks, bench('empty', { kind: 'ready', dataset_version: hex('6') }, 'none')];
    // As the API serves it, a benchmark with nothing to score is in neither list: it is not offered, and the note says why.
    const { stream } = await show('#runs', routes({ 'GET /benchmarks': { body: { ...BENCHMARKS, benchmarks: withNone } } }));
    connect(stream);
    const at = await openPanel();
    const benchmark = within(at).getByLabelText('Benchmark') as HTMLSelectElement;
    expect([...benchmark.options].map((o) => o.value)).toEqual(['beir/scifact', 'mine']);
    expect(within(at).getByText(/Not offered: empty, which carries no ground truth: a pipeline that ends in an answer can be scored only on a benchmark whose ground truth is qrels, reference answers or qrels and reference answers\./)).toBeTruthy();
    cleanup();

    // Were the API to list it, the panel would offer it: the list is the rule, read and never restated.
    const lists = { ...BENCHMARKS, benchmarks: withNone, scorable: { ending_in_answer: [...SCORABLE.ending_in_answer, 'none' as const], ending_elsewhere: SCORABLE.ending_elsewhere } };
    const again = await show('#runs', routes({ 'GET /benchmarks': { body: lists } }));
    connect(again.stream);
    const offered = within(await openPanel()).getByLabelText('Benchmark') as HTMLSelectElement;
    expect([...offered.options].map((o) => o.value)).toEqual(['beir/scifact', 'mine', 'empty']);
  });

  it('says why Launch is refused in words on the page, not only to a screen reader', async () => {
    const { stream } = await show();
    connect(stream);
    const at = await openPanel();
    fireEvent.change(within(at).getByLabelText('Pipeline'), { target: { value: 'broken' } });
    const launch = within(at).getByRole('button', { name: 'Launch' });
    expect(launch.getAttribute('aria-disabled')).toBe('true');
    const reason = within(at).getByText('This pipeline does not validate.');
    expect(reason.closest('.rg-visually-hidden')).toBeNull();
    // Said once to a screen reader: the visible reason is the one that describes the button.
    expect(launch.getAttribute('aria-describedby')).toBe(reason.id);
  });

  it('names an empty pipeline or benchmark field rather than leaving it blank', async () => {
    const { stream } = await show('#runs', routes({ 'GET /pipelines': { body: { pipelines: [] } }, 'GET /benchmarks': { body: { ...BENCHMARKS, benchmarks: [] } } }));
    connect(stream);
    fireEvent.click(screen.getByRole('button', { name: 'Launch…' }));
    const pipelineField = (await within(panel()).findByLabelText('Pipeline')) as HTMLSelectElement;
    expect([...pipelineField.options].map((o) => o.textContent)).toEqual(['No pipeline yet']);
    const benchmarkField = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    expect([...benchmarkField.options].map((o) => o.textContent)).toEqual(['No ready benchmark']);
  });

  it('offers Launch again once the run it queued has ended', async () => {
    const { stream } = await show('#runs', routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    const at = await openPanel();
    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    await within(at).findByRole('button', { name: 'Queued' });
    send(stream, { event: 'queued', data: runJob('j1', QUEUED) });
    expect(within(at).getByRole('button', { name: 'Queued' })).toBeTruthy();
    send(stream, { event: 'failed', data: runJob('j1', failedAt('rerank', 'boom')) });
    const again = within(at).getByRole('button', { name: 'Launch' });
    expect(again.getAttribute('aria-disabled')).toBeNull();
    // What was announced stays, with the way to it.
    expect(within(at).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
  });

  it('closes from a control of its own, taking the launch out of the address', async () => {
    const { stream } = await show('#runs?launch=hybrid');
    connect(stream);
    await within(panel()).findByText(`Content hash ${short(HYBRID)}`);
    fireEvent.click(within(panel()).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('region', { name: 'Launch a run' })).toBeNull();
    await waitFor(() => expect(window.location.hash).toBe('#runs'));
    expect(screen.getByRole('button', { name: 'Launch…' }).getAttribute('aria-expanded')).toBe('false');
    // The control that closed it went with the panel: focus is back on the one that opens it.
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Launch…' }));
  });

  it('labels the hash as the content hash, and a conflict names the run by its short id', async () => {
    const conflict = problem('run_exists', 409, `Run ${OLD} is already in the store.`, 'Open it.', { link: `/api/v1/runs/${OLD}` });
    const { stream } = await show('#runs', routes({ 'POST /runs': conflict }));
    connect(stream);
    const at = await openPanel();
    expect(within(at).queryByText(`pipeline ${short(HYBRID)}`)).toBeNull();
    fireEvent.click(within(at).getByRole('button', { name: 'Launch' }));
    const exists = await within(at).findByText('A run with this identity already exists.');
    const message = exists.closest('.rg-inline') as HTMLElement;
    expect(message.textContent).not.toContain(OLD);
    expect(message.textContent).toContain(short(OLD));
  });
});

describe('the launch panel up to a node', () => {
  const PREFIX = '#runs?launch=hybrid&up_to=rerank';

  it('opens on the pipeline cut at the node, offers only benchmarks the prefix can be scored on, and says why the others are absent', async () => {
    const { api, stream } = await show(PREFIX, routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    const at = await within(panel()).findByText('prefix of hybrid, up to rerank');
    const sheet = at.closest('section') as HTMLElement;
    const benchmark = within(sheet).getByLabelText('Benchmark') as HTMLSelectElement;
    // `mine` carries reference answers: a prefix ending before the generator produces no answer to score.
    expect([...benchmark.options].map((o) => o.textContent)).toEqual(['beir/scifact — qrels']);
    const absent = within(sheet).getByText('Not offered: mine, which carries qrels and reference answers: a prefix that does not end in an answer can be scored only on a benchmark whose ground truth is qrels.');
    // The note describes the field it explains.
    expect(benchmark.getAttribute('aria-describedby')?.split(' ')).toContain(absent.id);
    // The identity is the cut's, announced by the API: the parent's hash is not shown as if it were the run's.
    expect(within(sheet).queryByText(`Content hash ${short(HYBRID)}`)).toBeNull();
    expect(within(sheet).getByText(/identity is announced when it is queued/)).toBeTruthy();

    fireEvent.click(within(sheet).getByRole('button', { name: 'Launch' }));
    await within(sheet).findByRole('button', { name: 'Queued' });
    expect(api.bodies[api.requests.indexOf('POST /api/v1/runs')]).toEqual({ pipeline: 'hybrid', benchmark: 'beir/scifact', up_to: 'rerank' });
    expect(within(sheet).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
  });

  it('offers a benchmark with reference answers to a cut that ends in an answer', async () => {
    const { stream } = await show('#runs?launch=hybrid&up_to=generate');
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to generate');
    const benchmark = within(panel()).getByLabelText('Benchmark') as HTMLSelectElement;
    await waitFor(() => expect([...benchmark.options].map((o) => o.textContent)).toEqual(['beir/scifact — qrels', 'mine — qrels and reference answers']));
    expect(within(panel()).queryByText(/Not offered/)).toBeNull();
  });

  it('judges a cut by what the API says it ends in, never by the family of the node it stops at', async () => {
    // A cut the API says ends in an answer, at a node whose family is no generator: the served fact is the one read.
    const served: PipelineDetail = { ...HYBRID_DETAIL, ends_in_answer_up_to: { ...HYBRID_DETAIL.ends_in_answer_up_to, rerank: true } };
    const { stream } = await show(PREFIX, routes({ 'GET /pipelines/{name}': { body: served } }));
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    const benchmark = within(panel()).getByLabelText('Benchmark') as HTMLSelectElement;
    await waitFor(() => expect([...benchmark.options].map((o) => o.value)).toEqual(['beir/scifact', 'mine']));
  });

  it('goes back to the whole pipeline, every ready benchmark offered again', async () => {
    const { stream } = await show(PREFIX);
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    fireEvent.click(within(panel()).getByRole('button', { name: 'Run the whole pipeline' }));
    await waitFor(() => expect(window.location.hash).toBe('#runs?launch=hybrid'));
    const benchmark = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    await waitFor(() => expect(benchmark.options).toHaveLength(2));
    expect(within(panel()).queryByText('prefix of hybrid, up to rerank')).toBeNull();
  });

  it('shows a refused cut on the pipeline field, naming the node', async () => {
    const refused = problem('prefix_is_whole_pipeline', 422, '`rerank` is the pipeline’s output: the prefix would be the whole pipeline', 'Launch the whole pipeline instead.', {
      location: { node: 'rerank', edge: null },
    });
    const { stream } = await show(PREFIX, routes({ 'POST /runs': refused }));
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    fireEvent.click(within(panel()).getByRole('button', { name: 'Launch' }));
    const words = await within(panel()).findByText(/the prefix would be the whole pipeline/);
    expect(words.textContent).toContain('At node rerank.');
    expect((within(panel()).getByLabelText('Pipeline') as HTMLElement).getAttribute('aria-invalid')).toBe('true');
  });
});

describe('the launch panel on a benchmark', () => {
  it('opens on the pipeline and the benchmark the address names — the Pipeline screen’s Run — and launches them', async () => {
    const { api, stream } = await show('#runs?launch=hybrid&benchmark=mine', routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    await within(panel()).findByText(`Content hash ${short(HYBRID)}`);
    expect((within(panel()).getByLabelText('Pipeline') as HTMLSelectElement).value).toBe('hybrid');
    const benchmark = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    expect(benchmark.value).toBe('mine');
    fireEvent.click(within(panel()).getByRole('button', { name: 'Launch' }));
    await within(panel()).findByRole('button', { name: 'Queued' });
    expect(api.bodies[api.requests.indexOf('POST /api/v1/runs')]).toEqual({ pipeline: 'hybrid', benchmark: 'mine' });
  });

  it('refuses to launch on a benchmark the address names that is not ready, rather than another in its place', async () => {
    const { api, stream } = await show('#runs?launch=hybrid&benchmark=beir%2Ffiqa');
    connect(stream);
    await within(panel()).findByText(`Content hash ${short(HYBRID)}`);
    const benchmark = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    expect(benchmark.value).toBe('beir/fiqa');
    expect(benchmark.selectedOptions[0]?.textContent).toBe('beir/fiqa — not ready');
    const launch = within(panel()).getByRole('button', { name: 'Launch' });
    expect(launch.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(launch.getAttribute('aria-describedby') as string)?.textContent).toBe('beir/fiqa is not ready: download or import it in Setup, or choose another benchmark.');
    // The way to make it ready, though other benchmarks are; and a help line that says the field holds more than ready ones.
    expect(within(panel()).getByRole('link', { name: 'Open Setup' }).getAttribute('href')).toBe('#setup/benchmarks');
    expect(within(panel()).getByText('Ready benchmarks, with the ground truth each carries, and beir/fiqa, which the address named.')).toBeTruthy();
    expect(within(panel()).queryByText(/Ready benchmarks only/)).toBeNull();
    // Choosing a ready one lifts the refusal.
    fireEvent.change(benchmark, { target: { value: 'beir/scifact' } });
    expect(within(panel()).getByRole('button', { name: 'Launch' }).hasAttribute('aria-disabled')).toBe(false);
    expect(api.requests).not.toContain('POST /api/v1/runs');
  });
});

describe('the launch panel on a benchmark it does not offer, or keeps', () => {
  it('names a benchmark the workspace does not know as such, with no way to Setup', async () => {
    const { stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fnope');
    connect(stream);
    await within(panel()).findByText(`Content hash ${short(HYBRID)}`);
    const benchmark = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    expect(benchmark.selectedOptions[0]?.textContent).toBe('beir/nope — unknown');
    const launch = within(panel()).getByRole('button', { name: 'Launch' });
    expect(document.getElementById(launch.getAttribute('aria-describedby') as string)?.textContent).toBe('beir/nope is not a benchmark of this workspace: choose another benchmark.');
    expect(within(panel()).queryByRole('link', { name: 'Open Setup' })).toBeNull();
  });

  it('says a ready benchmark a prefix cannot be scored on is not offered, with its reason and no way to Setup', async () => {
    const { stream } = await show('#runs?launch=hybrid&up_to=rerank&benchmark=mine');
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    const benchmark = (await within(panel()).findByLabelText('Benchmark')) as HTMLSelectElement;
    expect(benchmark.value).toBe('mine');
    expect(benchmark.selectedOptions[0]?.textContent).toBe('mine — not offered');
    const launch = within(panel()).getByRole('button', { name: 'Launch' });
    expect(launch.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(launch.getAttribute('aria-describedby') as string)?.textContent).toBe('mine cannot score this prefix: choose another benchmark.');
    expect(within(panel()).queryByRole('link', { name: 'Open Setup' })).toBeNull();
  });

  it('keeps the benchmark when going back to the whole pipeline', async () => {
    const { stream } = await show('#runs?launch=hybrid&up_to=rerank&benchmark=mine');
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    fireEvent.click(within(panel()).getByRole('button', { name: 'Run the whole pipeline' }));
    await waitFor(() => expect(window.location.hash).toBe('#runs?launch=hybrid&benchmark=mine'));
    await waitFor(() => expect((within(panel()).getByLabelText('Benchmark') as HTMLSelectElement).value).toBe('mine'));
  });

  it('opens afresh on another benchmark when the address names one', async () => {
    const { stream } = await show('#runs?launch=hybrid&benchmark=mine');
    connect(stream);
    await waitFor(() => expect((within(panel()).getByLabelText('Benchmark') as HTMLSelectElement).value).toBe('mine'));
    act(() => navigate({ screen: 'runs', launch: { pipeline: 'hybrid', benchmarks: ['beir/scifact'] } }));
    await waitFor(() => expect((within(panel()).getByLabelText('Benchmark') as HTMLSelectElement).value).toBe('beir/scifact'));
  });

  it('keeps the benchmark in the address when a run is selected', async () => {
    const { stream } = await show('#runs?launch=hybrid&benchmark=mine');
    connect(stream);
    fireEvent.click(screen.getByRole('checkbox', { name: `Select run ${short(OLD)} on beir/scifact` }));
    await waitFor(() => expect(window.location.hash).toBe(`#runs?sel=${OLD}&launch=hybrid&benchmark=mine`));
  });
});

describe('the launch panel on several benchmarks', () => {
  const SEVERAL = '#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine&benchmark=beir%2Ffiqa&benchmark=beir%2Fnope';
  const rowOf = (name: string) => within(screen.getByRole('list', { name: 'Benchmarks' })).getByText(name).closest('li') as HTMLElement;

  it('shows the hash and the bindings once, lists every benchmark with what will happen on it, and offers one confirmation', async () => {
    const { stream } = await show(SEVERAL);
    connect(stream);
    const launch = await within(panel()).findByRole('button', { name: 'Launch 2 runs' });
    expect(within(panel()).getAllByText(`Content hash ${short(HYBRID)}`)).toHaveLength(1);
    expect(within(panel()).getAllByText('generator/qwen')).toHaveLength(1);
    // No benchmark picker: the benchmarks are the address's, each said for itself.
    expect(within(panel()).queryByLabelText('Benchmark')).toBeNull();
    expect(rowOf('beir/scifact').textContent).toContain('qrels');
    expect(rowOf('mine').textContent).toContain('qrels and reference answers');
    expect(rowOf('beir/fiqa').textContent).toContain('not launched: not ready — download or import it in Setup');
    expect(rowOf('beir/nope').textContent).toContain('not launched: not a benchmark of this workspace');
    expect(launch.hasAttribute('aria-disabled')).toBe(false);
  });

  it('sends one POST /runs per launchable benchmark, in order, and says each outcome — queued or already held', async () => {
    const replies = (body: { benchmark: string }) =>
      body.benchmark === 'beir/scifact'
        ? { body: { job_id: 'j1', run_id: ANNOUNCED } }
        : problem('run_exists', 409, 'A run with this identity exists.', 'Open it.', { link: `/api/v1/runs/${OLD}` });
    const { api, stream } = await show(SEVERAL, routes({ 'POST /runs': replies }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    await within(panel()).findByText('1 queued, 1 already held by a run or a job.');
    const posts = api.requests.flatMap((r, i) => (r === 'POST /api/v1/runs' ? [api.bodies[i]] : []));
    expect(posts).toEqual([
      { pipeline: 'hybrid', benchmark: 'beir/scifact' },
      { pipeline: 'hybrid', benchmark: 'mine' },
    ]);
    expect(within(rowOf('beir/scifact')).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
    expect(within(rowOf('beir/scifact')).getByRole('link', { name: 'Open the job on beir/scifact' }).getAttribute('href')).toBe('#runs/job/j1');
    expect(rowOf('mine').textContent).toContain('A run with this identity already exists.');
    expect(within(rowOf('mine')).getByRole('link', { name: 'Open what holds the run on mine' }).getAttribute('href')).toBe(`#replay/${OLD}`);
    const done = within(panel()).getByRole('button', { name: 'Submitted' });
    expect(done.getAttribute('aria-disabled')).toBe('true');
  });

  it('says a partial failure as it is: what was queued, and what was refused with its reason', async () => {
    const replies = (body: { benchmark: string }) =>
      body.benchmark === 'beir/scifact' ? { body: { job_id: 'j1', run_id: ANNOUNCED } } : problem('dataset_differs', 409, 'The dataset on disk is not the pinned one.', 'Download it again in Setup.');
    const { stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine', routes({ 'POST /runs': replies }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    const counts = await within(panel()).findByText('1 queued, 1 refused.');
    // The count is announced: a status line.
    expect(counts.getAttribute('role')).toBe('status');
    expect(within(rowOf('beir/scifact')).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
    expect(rowOf('mine').textContent).toContain('refused: The dataset on disk is not the pinned one. Download it again in Setup. (dataset_differs)');
  });

  it('sends the rest when the first is refused', async () => {
    const replies = (body: { benchmark: string }) =>
      body.benchmark === 'beir/scifact' ? problem('dataset_differs', 409, 'The dataset on disk is not the pinned one.', 'Download it again in Setup.') : { body: { job_id: 'j2', run_id: ANNOUNCED } };
    const { api, stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine', routes({ 'POST /runs': replies }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    await within(panel()).findByText('1 queued, 1 refused.');
    expect(api.requests.filter((r) => r === 'POST /api/v1/runs')).toHaveLength(2);
    expect(within(rowOf('mine')).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
  });

  it('retries the refused alone, with one confirmation, keeping what was queued', async () => {
    let refuseMine = true;
    const replies = (body: { benchmark: string }) =>
      body.benchmark === 'beir/scifact'
        ? { body: { job_id: 'j1', run_id: ANNOUNCED } }
        : refuseMine
          ? problem('service_unreachable', 503, 'The generator does not answer.', 'Start it, then retry.')
          : { body: { job_id: 'j2', run_id: hex('b') } };
    const { api, stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine', routes({ 'POST /runs': replies }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    const retry = await within(panel()).findByRole('button', { name: 'Retry the 1 refused' });
    refuseMine = false;
    fireEvent.click(retry);
    await within(panel()).findByText('2 queued.');
    const posts = api.requests.flatMap((r, i) => (r === 'POST /api/v1/runs' ? [api.bodies[i]] : []));
    expect(posts).toEqual([
      { pipeline: 'hybrid', benchmark: 'beir/scifact' },
      { pipeline: 'hybrid', benchmark: 'mine' },
      { pipeline: 'hybrid', benchmark: 'mine' },
    ]);
    expect(within(rowOf('beir/scifact')).getByText(`run ${short(ANNOUNCED)}`)).toBeTruthy();
    expect(within(rowOf('mine')).getByText(`run ${short(hex('b'))}`)).toBeTruthy();
    const done = within(panel()).getByRole('button', { name: 'Submitted' });
    expect(document.getElementById(done.getAttribute('aria-describedby') as string)?.textContent).toBe('Submitted: each benchmark says its outcome.');
  });

  it('is busy while the runs are sent, locks the pipeline, and says closing does not stop them', async () => {
    let answer: (reply: { body: { job_id: string; run_id: string } }) => void = () => {};
    const replies = () => new Promise<{ body: { job_id: string; run_id: string } }>((resolve) => (answer = resolve));
    const { api, stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine', routes({ 'POST /runs': replies }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    const busy = await within(panel()).findByRole('button', { name: 'Launching…' });
    expect(busy.getAttribute('aria-busy')).toBe('true');
    // A pipeline chosen now would have the old batch's answers written over it: the field waits.
    expect((within(panel()).getByLabelText('Pipeline') as HTMLSelectElement).disabled).toBe(true);
    expect(within(panel()).getByText('Closing this panel does not stop the runs not yet sent: they are sent all the same.')).toBeTruthy();
    // A second press sends nothing more.
    fireEvent.click(busy);
    await act(async () => answer({ body: { job_id: 'j1', run_id: ANNOUNCED } }));
    await act(async () => answer({ body: { job_id: 'j2', run_id: hex('b') } }));
    await within(panel()).findByText('2 queued.');
    expect(api.requests.filter((r) => r === 'POST /api/v1/runs')).toHaveLength(2);
    expect((within(panel()).getByLabelText('Pipeline') as HTMLSelectElement).disabled).toBe(false);
  });

  it('forgets the outcomes when another pipeline is chosen', async () => {
    const { stream } = await show('#runs?launch=hybrid&benchmark=beir%2Fscifact&benchmark=mine', routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 2 runs' }));
    await within(panel()).findByRole('button', { name: 'Submitted' });
    fireEvent.change(within(panel()).getByLabelText('Pipeline'), { target: { value: 'broken' } });
    expect(within(rowOf('beir/scifact')).queryByText(`run ${short(ANNOUNCED)}`)).toBeNull();
    expect(rowOf('beir/scifact').textContent).toContain('qrels');
  });

  it('names a benchmark a whole pipeline cannot be scored on as not launched, for that pipeline', async () => {
    const { stream } = await show('#runs?launch=lexical&benchmark=beir%2Fscifact&benchmark=mine');
    connect(stream);
    await within(panel()).findByRole('button', { name: 'Launch 1 run' });
    expect(rowOf('mine').textContent).toContain('not launched: cannot score this pipeline');
  });

  it('sends up_to with every run of a cut, and offers Setup only for a benchmark that is not ready', async () => {
    const { api, stream } = await show('#runs?launch=hybrid&up_to=rerank&benchmark=beir%2Fscifact&benchmark=mine&benchmark=beir%2Fnope', routes({ 'POST /runs': { body: { job_id: 'j1', run_id: ANNOUNCED } } }));
    connect(stream);
    await within(panel()).findByText('prefix of hybrid, up to rerank');
    expect(rowOf('mine').textContent).toContain('not launched: cannot score this prefix');
    // Neither an unknown benchmark nor one the cut cannot score is made ready in Setup.
    expect(within(panel()).queryByRole('link', { name: 'Open Setup' })).toBeNull();
    fireEvent.click(await within(panel()).findByRole('button', { name: 'Launch 1 run' }));
    await within(panel()).findByText('1 queued.');
    expect(api.bodies[api.requests.indexOf('POST /api/v1/runs')]).toEqual({ pipeline: 'hybrid', benchmark: 'beir/scifact', up_to: 'rerank' });
  });

  it('offers Setup for a benchmark among several that is not ready', async () => {
    const { stream } = await show(SEVERAL);
    connect(stream);
    await within(panel()).findByRole('button', { name: 'Launch 2 runs' });
    expect(within(panel()).getByRole('link', { name: 'Open Setup' }).getAttribute('href')).toBe('#setup/benchmarks');
  });

  it('refuses to launch when none of the benchmarks can be, saying why', async () => {
    const { stream } = await show('#runs?launch=hybrid&benchmark=beir%2Ffiqa&benchmark=beir%2Fnope');
    connect(stream);
    const launch = await within(panel()).findByRole('button', { name: 'Nothing to launch' });
    expect(launch.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(launch.getAttribute('aria-describedby') as string)?.textContent).toBe('None of these benchmarks can be launched: each says why.');
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
    expect(within(failedRow).getByText('the reranker answered 503')).toBeTruthy();

    const toasts = within(notifications());
    expect(toasts.getByRole('status').textContent).toContain('Run done');
    expect(toasts.getByRole('alert').textContent).toContain('Run failed at rerank');
    expect(notifications().querySelectorAll('.rg-toast')).toHaveLength(2);
    fireEvent.click(within(toasts.getByRole('status')).getByRole('button', { name: 'Open' }));
    expect(window.location.hash).toBe(`#replay/${DONE}`);
  });

  it('a_failed_job_opens_with_its_partial_traces', async () => {
    const { stream } = await show('#runs/job/j2');
    connect(stream, [runJob('j2', failedAt('rerank', 'the reranker answered 503', 2), { runId: hex('b') })]);
    const job = await screen.findByRole('region', { name: 'Job j2' });
    expect(job.querySelector('.rg-status')?.textContent).toBe('failed at rerank');
    expect(within(job).getByText('the reranker answered 503')).toBeTruthy();
    // The count, said as what it is: the traces the job kept, not a run in the store — and the way to Replay them.
    expect(within(job).getByText('It kept the traces of the 2 queries it executed before it failed, under jobs/j2/partial/ in the workspace, never in the store.')).toBeTruthy();
    expect(within(job).queryByText(/does not serve/)).toBeNull();
    expect(within(job).getByRole('link', { name: 'Replay the partial traces' }).getAttribute('href')).toBe('#replay/job/j2');
    expect(within(job).getByText(short(hex('b')))).toBeTruthy();
    expect(jobRowOf(hex('b')).getAttribute('aria-current')).toBe('true');

    // A job that kept none says so, and offers nothing to replay.
    send(stream, { event: 'failed', data: runJob('j2', failedAt(null, 'interrupted', 0), { runId: hex('b') }) });
    expect(within(job).getByText('It kept no trace: a run keeps the traces of the queries it executed when it fails or is cancelled, and a crash keeps none it can vouch for.')).toBeTruthy();
    expect(within(job).queryByRole('link', { name: 'Replay the partial traces' })).toBeNull();
  });

  it('offers nothing to replay when no query completed before the failure', async () => {
    const { stream } = await show('#runs/job/j2');
    // One trace kept, of the query a node failed on: no query completed.
    connect(stream, [runJob('j2', failedAt('rerank', 'the reranker answered 503', 1), { runId: hex('b') })]);
    const job = await screen.findByRole('region', { name: 'Job j2' });
    expect(within(job).getByText(/No query completed before it failed/)).toBeTruthy();
    expect(within(job).queryByRole('link', { name: 'Replay the partial traces' })).toBeNull();
  });

  it('dismisses a failed job: the API is asked, and the row goes once the stream says so', async () => {
    const failed = runJob('j2', failedAt('rerank', 'boom'), { runId: hex('b') });
    const { api, stream } = await show('#runs', routes({ 'POST /jobs/{id}/dismiss': { body: { ...failed, dismissed_at_ms: 1_700_000_300_000 } } }));
    connect(stream, [failed]);
    fireEvent.click(within(jobRowOf(hex('b'))).getByRole('button', { name: `Dismiss run ${short(hex('b'))}` }));
    await waitFor(() => expect(api.requests).toContain('POST /api/v1/jobs/j2/dismiss'));
    send(stream, { event: 'dismissed', data: { ...failed, dismissed_at_ms: 1_700_000_300_000 } });
    expect(screen.queryByRole('row', { name: new RegExp(`^Run ${short(hex('b'))} on `) })).toBeNull();
  });

  it('hands focus to the next row when the row whose Dismiss had it goes', async () => {
    const failed = runJob('j2', failedAt('rerank', 'boom'), { runId: hex('b') });
    const { stream } = await show('#runs', routes({ 'POST /jobs/{id}/dismiss': { body: { ...failed, dismissed_at_ms: 1 } } }));
    connect(stream, [failed]);
    const table = screen.getByRole('table', { name: 'Runs, grouped by pipeline' });
    const rows = [...table.querySelectorAll<HTMLElement>('tr[tabindex]')];
    const at = rows.indexOf(jobRowOf(hex('b')));
    const next = rows[at + 1] as HTMLElement;
    expect(next).toBeTruthy();
    const dismiss = within(jobRowOf(hex('b'))).getByRole('button', { name: `Dismiss run ${short(hex('b'))}` });
    dismiss.focus();
    fireEvent.click(dismiss);
    await act(async () => {});
    send(stream, { event: 'dismissed', data: { ...failed, dismissed_at_ms: 1 } });
    expect(document.activeElement).toBe(next);
  });

  it('hands focus to the table heading when the row whose Dismiss had it was the last', async () => {
    const failed = runJob('j9', failedAt('rerank', 'boom'), { runId: hex('b'), benchmark: 'nf/corpus' });
    const { stream } = await show('#runs', routes({ 'POST /jobs/{id}/dismiss': { body: { ...failed, dismissed_at_ms: 1 } } }));
    connect(stream, [failed]);
    // Filtered to its benchmark, the job's row is the table's last.
    fireEvent.click(screen.getByRole('button', { name: /^nf\/corpus/ }));
    await waitFor(() => expect(screen.queryByRole('row', { name: new RegExp(`^Run ${short(OLD)} on `) })).toBeNull());
    const row = screen.getByRole('row', { name: new RegExp(`^Run ${short(hex('b'))} on nf/corpus, `) });
    const table = screen.getByRole('table', { name: 'Runs, grouped by pipeline' });
    expect([...table.querySelectorAll<HTMLElement>('tr[tabindex]')].at(-1)).toBe(row);
    const dismiss = within(row).getByRole('button', { name: `Dismiss run ${short(hex('b'))}` });
    dismiss.focus();
    fireEvent.click(dismiss);
    await act(async () => {});
    send(stream, { event: 'dismissed', data: { ...failed, dismissed_at_ms: 1 } });
    expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Runs' }));
  });

  it('shows the live jobs in a Queue above the runs, in the order the worker takes them', async () => {
    const { stream } = await show();
    connect(stream, [
      runJob('j3', QUEUED, { runId: hex('8'), position: 7 }),
      runJob('j1', running(2, 10), { runId: hex('6') }),
      runJob('j2', QUEUED, { runId: hex('7'), position: 3 }),
      runJob('j4', failedAt(null, 'boom'), { runId: hex('4') }),
    ]);
    const queue = screen.getByRole('table', { name: 'Queue' });
    expect(queue.closest('.rg-tablewrap')?.getAttribute('role')).toBe('region');
    expect(queue.closest('.rg-tablewrap')?.getAttribute('aria-label')).toBe('Queue');
    const names = within(queue)
      .getAllByRole('row')
      .map((r) => r.getAttribute('aria-label'))
      .filter((n) => n !== null);
    expect(names.map((n) => n?.slice(0, 16))).toEqual([`Run ${short(hex('6'))}`, `Run ${short(hex('7'))}`, `Run ${short(hex('8'))}`]);
    const runsTable = screen.getByRole('table', { name: 'Runs, grouped by pipeline' });
    // The Queue comes first; an ended job stays with its pipeline's runs.
    expect(queue.compareDocumentPosition(runsTable) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(runsTable).queryByRole('row', { name: new RegExp(`^Run ${short(hex('6'))}`) })).toBeNull();
    expect(within(runsTable).getByRole('row', { name: new RegExp(`^Run ${short(hex('4'))}`) })).toBeTruthy();
  });

  it('a_fault_reported_during_a_job_shows_on_its_row_and_in_its_view_without_a_reload', async () => {
    const { api, stream } = await show('#runs/job/j1');
    connect(stream, [runJob('j1', running(3, 10))]);
    const job = await screen.findByRole('region', { name: 'Job j1' });
    // The polite region is there, empty, before any fault: one inserted already filled may go unannounced.
    const live = job.querySelector('.rg-job__faults') as HTMLElement;
    expect(live.getAttribute('role')).toBe('status');
    // Not atomic: a new fault is announced as what was added, never the whole list again.
    expect(live.getAttribute('aria-atomic')).toBe('false');
    expect(live.textContent).toBe('');
    const reads = api.requests.length;

    const reason = 'the layout of pipeline hybrid could not be copied at launch, so a fork of its run starts without one';
    send(stream, { event: 'fault', data: runJob('j1', running(3, 10), { faults: [reason] }) });

    // Said politely, beside the job, which goes on: a warning, never an alert.
    expect(job.querySelector('.rg-job__faults')).toBe(live);
    expect(live.textContent).toContain('1 fault beside this job; it did not stop it');
    expect(live.textContent).toContain(reason);
    expect(within(live).getByRole('img', { name: 'Warning' })).toBeTruthy();
    // One live region, so a fault is announced once: the warning inside it is not a second.
    expect(within(live).queryByRole('status')).toBeNull();
    // Each fault says when it was reported.
    const at = new Date(1_700_000_050_000).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });
    expect(live.querySelector('.rg-job__fault')?.textContent).toBe(`${reason} (${at})`);
    expect(within(job).queryByRole('alert')).toBeNull();
    expect(job.querySelector('.rg-status')?.textContent).toBe('running 3 / 10');
    expect(jobRowOf().getAttribute('aria-label')).toBe(`Run ${short(ANNOUNCED)} on beir/scifact, running, 1 fault`);
    expect(within(jobRowOf()).getByText('1 fault')).toBeTruthy();
    // Nothing was read again: the stream carried it.
    expect(api.requests.length).toBe(reads);
    expect(notifications().querySelectorAll('.rg-toast')).toHaveLength(0);
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

  it('still follows a reorder’s answer after a fault event, which says no order', async () => {
    const A = hex('a');
    const B = hex('b');
    const C = hex('e');
    const q = (id: string, runId: string, position: number, faults: string[] = []) => runJob(id, QUEUED, { runId, position, faults });
    let answer: (reply: { body: JobListing }) => void = () => {};
    const { stream } = await show('#runs', routes({ 'PATCH /jobs/{id}': () => new Promise((resolve) => (answer = resolve)) }));
    connect(stream, [q('jc', C, 0), q('ja', A, 1), q('jb', B, 2)]);
    const order = () => screen.getAllByRole('row', { name: /, queued/ }).map((r) => r.getAttribute('aria-label')?.slice(4, 16));

    fireEvent.click(within(jobRowOf(B)).getByRole('button', { name: `Move run ${short(B)} up` }));
    // A fault reported beside a job while the move is in flight moves no job among the queued.
    send(stream, { event: 'fault', data: q('jc', C, 0, ['the layout could not be copied']) });
    await act(async () => answer({ body: { faults: [], jobs: [q('ja', A, 0), q('jb', B, 1), q('jc', C, 2, ['the layout could not be copied'])] } }));
    expect(order()).toEqual([short(A), short(B), short(C)]);
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
    await within(at).findByText(`Content hash ${short(HYBRID)}`);
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

  it('lets a row’s identity and its labels wrap, so the table fits a 1440 px screen with every column in view', async () => {
    // The table's cells do not wrap (design/'s Table); a run's hash, its prefix label and its facts would otherwise make one long line.
    expect(rule(runsCss, '.rg-runs__run')?.declarations.get('white-space')).toBe('normal');
    const { stream } = await show();
    connect(stream, [runJob('j9', failedAt(null, 'boom'), { runId: hex('d') })]);
    expect(jobRowOf(hex('d')).querySelector('.rg-runs__run')).toBeTruthy();
    expect(screen.getByRole('row', { name: new RegExp(`^Run ${short(OLD)} on `) }).querySelector('.rg-runs__run')).toBeTruthy();
  });

  it('floats the toasts over the page’s corner, within a phone’s width, and lets a click through where there is none', () => {
    const region = rule(toastsCss, '.rg-toasts');
    expect(region?.declarations.get('position')).toBe('fixed');
    expect(region?.declarations.get('pointer-events')).toBe('none');
    expect(rule(toastsCss, '.rg-toasts > *')?.declarations.get('pointer-events')).toBe('auto');
    expect(rule(toastsCss, '.rg-toasts', '@media (max-width: 640px)')?.declarations.get('left')).toBe('var(--space-4)');
  });
});

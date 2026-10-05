/** @vitest-environment happy-dom */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { declared, rulesFor } from '../../design/testing/css.ts';
import { createApiClient, type ApiClient } from '../api/client.ts';
import { mockApi, type MockReply, type MockRoutes } from '../api/testing.ts';
import type { CompareRequest, Comparison, Problem, RunListing, RunSummary } from '../api/types.ts';
import { useRoute } from '../routes.ts';
import { CompareScreen } from './CompareScreen.tsx';
import css from './Compare.css?raw';
import { COMPARISON, DENSE, HYBRID, RERANK, SCIFACT } from './fixtures.ts';

const hex = (c: string) => c.repeat(64);
const short = (id: string) => id.slice(0, 12);
const OTHER_BENCH = hex('f');
const R4 = hex('4');
const R5 = hex('6');
const R6 = hex('7');
const FIQA_RUN = hex('8');

const summary = (id: string, dataset: string): RunSummary => ({ id, pipeline: hex('9'), dataset_version: dataset, index_version: hex('0'), engine_version: '0.0.0', metrics: {}, pipeline_names: [], refused_pipeline_names: [], prefix_of_documents: [], launched_as: null, benchmark_names: [], started_at_ms: null, finished_at_ms: null, metric_families: {}, median_query_latency_nanos: null });
const LISTING: RunListing = {
  runs: [summary(DENSE, SCIFACT), summary(HYBRID, SCIFACT), summary(RERANK, SCIFACT), summary(R4, SCIFACT), summary(R5, SCIFACT), summary(R6, SCIFACT), summary(FIQA_RUN, OTHER_BENCH)],
  unreadable: [],
  shapes: {},
};

const problem = (code: Problem['code'], detail: string, hint: string, status = 409): Problem => ({ type: `urn:ragondin:problem:${code}`, title: code, status, detail, code, hint });
const CEILING = problem('runs_not_comparable', '6 runs: a comparison holds a baseline and at most four runs, 5 in all', 'Compare runs of one benchmark, a baseline and at most four others.');

/** What `POST /compare` answers: the fixture for up to five runs, keeping the pairing it was sent — as the API does. */
function answer(body: CompareRequest) {
  if (body.run_ids.length > 5) return { problem: CEILING };
  const pairings = body.pairing === undefined || body.pairing === null || body.pairing.pairs.length === 0 ? [] : [body.pairing];
  const stages = COMPARISON.stages.map((row) =>
    pairings.length > 0 && row.stage === 'after_rerank' ? { ...row, source: 'manual' as const } : row,
  );
  return { body: { ...COMPARISON, pairings, stages } satisfies Comparison };
}

const routes = (compare: MockRoutes['POST /compare'] = answer): MockRoutes => ({ 'POST /compare': compare, 'GET /runs': { body: LISTING } });

/** What the shell does: hands the screen the runs and the baseline the address carries. */
function Shell({ client }: { client: ApiClient }) {
  const route = useRoute();
  if (route?.screen !== 'compare') return <p>elsewhere {window.location.hash}</p>;
  return <CompareScreen client={client} ids={route.ids} baseline={route.baseline} />;
}

function show(hash: string, mocks: MockRoutes = routes()) {
  window.history.replaceState(null, '', `/${hash}`);
  const api = mockApi(mocks);
  render(<Shell client={createApiClient()} />);
  return api;
}

const THREE = `#compare/${DENSE}+${HYBRID}+${RERANK}?baseline=${DENSE}`;
const loaded = () => screen.findByRole('heading', { name: 'Metrics' });

beforeEach(() => window.history.replaceState(null, '', '/'));
afterEach(() => {
  window.history.replaceState(null, '', '/');
  vi.unstubAllGlobals();
});

describe('the states', () => {
  it('with fewer than two runs, says so and leads back to Runs with what was chosen', () => {
    show(`#compare/${DENSE}`);
    expect(screen.getByRole('heading', { name: 'Choose at least two runs' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe(`#runs?sel=${DENSE}`);
  });

  it('with none, leads back to Runs', () => {
    show('#compare');
    expect(screen.getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe('#runs');
  });

  it('says so while comparing', () => {
    show(THREE);
    expect(screen.getByRole('status').textContent).toBe('Comparing 3 runs');
  });

  it('fills in the baseline the address left out, in place, with the first run', async () => {
    show(`#compare/${HYBRID}+${DENSE}`);
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${HYBRID}+${DENSE}?baseline=${HYBRID}`));
  });

  it('says why runs are not comparable, in the API\'s words, and leads back to Runs', async () => {
    const other = problem('runs_not_comparable', `runs ${short(DENSE)} and ${short(FIQA_RUN)} were evaluated on different datasets`, 'Compare runs of one benchmark, a baseline and at most four others.');
    show(`#compare/${DENSE}+${FIQA_RUN}?baseline=${DENSE}`, routes({ problem: other }));
    expect(await screen.findByRole('heading', { name: 'These runs cannot be compared' })).toBeTruthy();
    expect(screen.getByText(other.detail)).toBeTruthy();
    expect(screen.getByText(other.hint)).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open Runs' }).getAttribute('href')).toBe(`#runs?sel=${DENSE},${FIQA_RUN}`);
  });

  it('shows another failure inline with Retry, and Retry compares again', async () => {
    show(THREE, routes([{ network: 'down' }, answer({ run_ids: [DENSE, HYBRID, RERANK], baseline: DENSE })]));
    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));
    await loaded();
  });
});

describe('the address', () => {
  it('reloads into the same view: the runs and the baseline it names are what is compared', async () => {
    const api = show(THREE);
    await loaded();
    expect(api.bodies[0]).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: DENSE });
  });

  it('writes a change of baseline in place, and compares again against it', async () => {
    const api = show(THREE);
    await loaded();
    fireEvent.change(screen.getByLabelText('Baseline'), { target: { value: HYBRID } });
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${DENSE}+${HYBRID}+${RERANK}?baseline=${HYBRID}`));
    await waitFor(() => expect(api.bodies.at(-1)).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: HYBRID }));
  });
});

describe('the run bar', () => {
  it('names each run by its slot, its pipeline and its short id', async () => {
    show(THREE);
    await loaded();
    const items = within(screen.getByRole('list', { name: 'Runs compared' })).getAllByRole('listitem');
    expect(items.map((li) => li.querySelector('.rg-swatch')?.getAttribute('data-run'))).toEqual(['base', 'a', 'b']);
    expect(items[1]?.textContent).toContain('hybrid');
    expect(within(items[0] as HTMLElement).getByRole('button', { name: /^Copy \w{6}, the hash of the baseline$/ })).toBeTruthy();
    expect(within(items[1] as HTMLElement).getByRole('button', { name: `Copy ${HYBRID.slice(0, 6)}, the hash of run A` }).getAttribute('title')).toBe(HYBRID);
    expect(screen.getByText('beir/scifact')).toBeTruthy();
  });

  it('offers to add only runs of the same benchmark not yet compared', async () => {
    show(THREE);
    await loaded();
    const select = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBeGreaterThan(1));
    const offered = [...select.options].map((o) => o.value).filter((v) => v !== '');
    expect(offered).toEqual([R4, R5, R6]);
  });

  it('labels a prefix run in both run selectors, by its record or by structure', async () => {
    const recorded = { ...summary(R4, SCIFACT), launched_as: { name: 'hybrid', prefix_of: { up_to: 'rrf', parent_pipeline_hash: hex('e') }, held: 'exactly' as const } };
    const structural = { ...summary(HYBRID, SCIFACT), prefix_of_documents: [{ pipeline: 'hybrid-rerank', up_to: 'rrf' }] };
    const listing = { ...LISTING, runs: LISTING.runs.map((r) => (r.id === R4 ? recorded : r.id === HYBRID ? structural : r)) };
    show(THREE, { ...routes(), 'GET /runs': { body: listing } });
    await loaded();
    const add = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(add.options.length).toBeGreaterThan(1));
    const option = (select: HTMLSelectElement, id: string) => [...select.options].find((o) => o.value === id)?.textContent;
    expect(option(add, R4)).toContain('prefix of hybrid, up to rrf');
    expect(option(add, R5)).not.toContain('prefix');
    const baseline = screen.getByLabelText('Baseline') as HTMLSelectElement;
    expect(option(baseline, HYBRID)).toContain('prefix of hybrid-rerank, up to rrf');
    expect(option(baseline, DENSE)).not.toContain('prefix');
  });

  it('adds a run: compares with it, then writes it to the address', async () => {
    const api = show(THREE);
    await loaded();
    const select = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(4));
    fireEvent.change(select, { target: { value: R4 } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${DENSE}+${HYBRID}+${RERANK}+${R4}?baseline=${DENSE}`));
    expect(api.bodies.some((b) => JSON.stringify(b) === JSON.stringify({ run_ids: [DENSE, HYBRID, RERANK, R4], baseline: DENSE }))).toBe(true);
  });

  it('refuses a sixth run with the API\'s message, and keeps the comparison on screen', async () => {
    show(`#compare/${DENSE}+${HYBRID}+${RERANK}+${R4}+${R5}?baseline=${DENSE}`);
    await loaded();
    const select = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(2));
    fireEvent.change(select, { target: { value: R6 } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect((await screen.findByRole('alert')).textContent).toContain(CEILING.detail);
    expect(window.location.hash).toBe(`#compare/${DENSE}+${HYBRID}+${RERANK}+${R4}+${R5}?baseline=${DENSE}`);
    expect(screen.getByRole('heading', { name: 'Metrics' })).toBeTruthy();
  });

  it('removes a run from the address, but never below two', async () => {
    show(THREE);
    await loaded();
    fireEvent.click(screen.getByRole('button', { name: 'Remove run A' }));
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${DENSE}+${RERANK}?baseline=${DENSE}`));
  });

  it('shows "Replay side by side" with the reason it waits for a query', async () => {
    show(THREE);
    await loaded();
    const replay = screen.getByRole('button', { name: 'Replay side by side' });
    expect(replay.getAttribute('aria-disabled')).toBe('true');
    expect(document.getElementById(replay.getAttribute('aria-describedby') ?? '')?.textContent).toBe('Choose a query in the histogram: Replay opens one query beside the baseline.');
  });
});

describe('the charts', () => {
  it('draws the grouped bars: three metrics, three runs, one 0–1 scale, the best labelled', async () => {
    show(THREE);
    await loaded();
    const figure = screen.getByRole('figure', { name: 'Each metric per run, on one 0–1 scale' });
    expect(figure.querySelectorAll('rect.rg-chart__bar')).toHaveLength(9);
    expect([...figure.querySelectorAll('.rg-chart__group')].map((g) => g.textContent)).toEqual(['mrr@10', 'ndcg@10', 'recall@100']);
    expect([...figure.querySelectorAll('.rg-chart__best')].map((g) => g.textContent)).toEqual(['★ 0.6790', '★ 0.7032', '★ 0.9310', '★ 0.9310']);
    expect([...figure.querySelectorAll('.rg-chart__tick')].map((t) => t.textContent)).toEqual(['0', '0.2', '0.4', '0.6', '0.8', '1']);
  });

  it('counts only the pairs placed, and lists in words each pair a run could not place', async () => {
    const earlier: Comparison = {
      ...COMPARISON,
      pairings: [{ pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }, { node: 'dense', other: 'splade' }] }],
      unplaced_pairs: [{ run: RERANK, pair: { node: 'dense', other: 'splade' }, absent_from: 'run' }],
    };
    show(THREE, routes(() => ({ body: earlier })));
    await loaded();
    expect(screen.getByText('1 pair by hand')).toBeTruthy();
    const list = screen.getByRole('list', { name: 'Pairs not placed' });
    expect(within(list).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['dense ↔ splade: not in run B']);
  });

  it('draws no list of unplaced pairs when every pair was placed', async () => {
    show(THREE);
    await loaded();
    expect(screen.queryByRole('list', { name: 'Pairs not placed' })).toBeNull();
  });

  it('announces the unplaced pairs through a live region that is there before they are, so a re-compare that brings them is heard', async () => {
    const earlier = (body: CompareRequest): MockReply<Comparison> => {
      const reply = answer(body);
      if (!('body' in reply) || body.baseline !== HYBRID) return reply;
      return { body: { ...reply.body, unplaced_pairs: [{ run: RERANK, pair: { node: 'dense', other: 'splade' }, absent_from: 'run' }] } };
    };
    show(THREE, routes(earlier));
    await loaded();
    const region = screen.getByRole('status', { name: 'Pairs drawn by hand not placed' });
    expect(region.textContent).toBe('');
    fireEvent.change(screen.getByLabelText('Baseline'), { target: { value: HYBRID } });
    const list = await within(region).findByRole('list', { name: 'Pairs not placed' });
    // The same node, now holding the count message and the list.
    expect(screen.getByRole('status', { name: 'Pairs drawn by hand not placed' })).toBe(region);
    expect(region.textContent).toContain('1 pair drawn by hand not placed');
    expect(list.closest('[role="status"]')).toBe(region);
  });

  it('takes the empty live region out of flow, so it adds no gap to the stages and moves nothing', () => {
    // A grid's row gap cannot be taken back by a margin: out of flow, the region makes no row at all.
    // It stays in the accessibility tree, as `display: none` would not. Measured in a browser in the PR.
    expect(declared(css, '.rg-compare__unplaced:empty', 'position')).toBe('absolute');
    expect(declared(css, '.rg-compare__stack > .rg-compare__unplaced:empty', 'margin-top')).toBeUndefined();
  });

  it('draws the stage line: the legs, after fusion, after rerank; the dense-only line breaks where it has no stage', async () => {
    show(THREE);
    await loaded();
    const figure = screen.getByRole('figure', { name: 'ndcg@10 at each stage' });
    expect([...figure.querySelectorAll('.rg-chart__group')].map((g) => g.textContent)).toEqual(['retrieval legs', 'after fusion', 'after rerank', 'final ranking']);
    // The baseline has a leg and a final ranking only: two points, no segment.
    expect(figure.querySelectorAll('path.rg-chart__line[data-ink="base"]')).toHaveLength(0);
    expect(figure.querySelectorAll('circle.rg-chart__point[data-ink="base"]')).toHaveLength(2);
    // The two legs of each hybrid run are marks of their own.
    expect(figure.querySelectorAll('circle.rg-chart__dot')).toHaveLength(4);
  });

  it('stacks the latency of each run\'s nodes by family, the reranked run\'s four nodes', async () => {
    show(THREE);
    await loaded();
    const figure = screen.getByRole('figure', { name: 'Median latency per node, in milliseconds' });
    const stacks = figure.querySelectorAll('g.rg-chart__stack');
    expect([...(stacks[2] as Element).querySelectorAll('rect')].map((r) => r.getAttribute('data-family'))).toEqual(['retriever', 'retriever', 'reranker', 'fusion']);
    expect([...within(figure).getByRole('list', { name: 'Legend' }).querySelectorAll('li')].map((li) => li.textContent)).toEqual(['retriever', 'fusion', 'reranker']);
  });

  it('draws the histogram\'s seven bins with their counts and its worse and better halves', async () => {
    show(THREE);
    await loaded();
    fireEvent.change(screen.getByLabelText('Per-query metric'), { target: { value: 'mrr@10' } });
    const group = screen.getByRole('group', { name: 'Per-query change in mrr@10, A · hybrid against the baseline' });
    expect(within(group).getAllByRole('button')).toHaveLength(7);
    expect(screen.getAllByText('worse', { selector: '.rg-hist__half' })).toHaveLength(1);
    fireEvent.click(screen.getByRole('radio', { name: 'B · hybrid-rerank' }));
    const b = screen.getByRole('group', { name: 'Per-query change in mrr@10, B · hybrid-rerank against the baseline' });
    expect([...b.querySelectorAll('.rg-hist__count')].map((c) => c.textContent)).toEqual(['6', '20', '35', '108', '54', '0', '77']);
  });

  it('lists a bar\'s queries on Enter or click, each opening Replay beside the baseline', async () => {
    show(THREE);
    await loaded();
    fireEvent.change(screen.getByLabelText('Per-query metric'), { target: { value: 'mrr@10' } });
    fireEvent.click(screen.getByRole('radio', { name: 'B · hybrid-rerank' }));
    fireEvent.click(screen.getByRole('button', { name: 'much worse, 6 queries, below −0.3' }));
    const list = screen.getByRole('region', { name: '6 queries much worse, below −0.3' });
    const links = within(list).getAllByRole('link');
    expect(links).toHaveLength(6);
    expect(links[0]?.textContent).toBe('q1−0.5200');
    expect(links[0]?.getAttribute('href')).toBe(`#replay/${RERANK}/q/q1?with=${DENSE}`);
  });

  it('says in the bars\' table which values the stars mark, every run of a tie', async () => {
    show(THREE);
    await loaded();
    fireEvent.click(screen.getAllByRole('button', { name: 'Show as a table' })[0] as HTMLElement);
    const table = screen.getByRole('table', { name: 'Each metric per run, as a table' });
    const recall = within(table).getByText('recall@100').closest('tr') as HTMLElement;
    expect([...recall.querySelectorAll('td[data-best]')].map((td) => td.textContent)).toEqual(['0.9310 (best)', '0.9310 (best)']);
    const ndcg = within(table).getByText('ndcg@10').closest('tr') as HTMLElement;
    expect([...ndcg.querySelectorAll('td[data-best]')].map((td) => td.textContent)).toEqual(['0.7032 (best)']);
  });

  it('names its two metric choices apart', async () => {
    show(THREE);
    await loaded();
    // The one rule every view opens on (src/metrics.ts): ndcg@10 when the stages carry it.
    expect((screen.getByLabelText('Stage metric') as HTMLSelectElement).value).toBe('ndcg@10');
    // The same rule for the per-query deltas, never the API's first (mrr@10 in the fixture).
    expect((screen.getByLabelText('Per-query metric') as HTMLSelectElement).value).toBe('ndcg@10');
    expect(screen.getByRole('group', { name: 'Per-query change in ndcg@10, A · hybrid against the baseline' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Verdict' }).textContent).toContain('On ndcg@10, A · hybrid against the baseline');
  });

  it('gives every chart a table, one keyboard stop away', async () => {
    show(THREE);
    await loaded();
    const toggles = screen.getAllByRole('button', { name: 'Show as a table' });
    expect(toggles).toHaveLength(4);
    fireEvent.click(toggles[1] as HTMLElement);
    const table = screen.getByRole('table', { name: 'ndcg@10 at each stage, as a table' });
    expect(within(table).getAllByText('no stage here')).toHaveLength(3);
  });
});

describe('at phone width', () => {
  // The shell's main column is a grid track sized by its item: an item that
  // keeps the default `min-width: auto` grows the page to its widest table
  // (about 613 px at a 390 px viewport, #428). Measured in a browser in the PR.
  it('lets the screen shrink below its widest content, so the page never scrolls sideways', () => {
    expect(declared(css, '.rg-compare', 'min-width')).toBe('0');
  });

  it('keeps a chart\'s shown table and the pairing panel\'s node columns from widening the screen', () => {
    // A chart is a grid whose one column a shown table would size: the table scrolls in its region instead.
    expect(declared(css, '.rg-compare .rg-chart__table', 'min-width')).toBe('0');
    // A node's name is cut with an ellipsis rather than pushing its column past the panel.
    expect(declared(css, '.rg-pair__column', 'grid-template-columns')).toBe('minmax(0, 1fr)');
    // At phone width the stage's words give way to the node's name, which is what tells the node;
    // the shell's breakpoint, and nothing changes above it.
    const narrow = rulesFor(css, '.rg-pair__stage').filter((r) => r.atRule === '@media (max-width: 640px)');
    expect(narrow.map((r) => r.declarations.get('display'))).toEqual(['none']);
  });

  it('scrolls every table inside a region named by its caption, one tab stop, rather than widening the page', async () => {
    show(THREE);
    await loaded();
    for (const toggle of screen.getAllByRole('button', { name: 'Show as a table' })) fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'much worse, 1 query, below −0.3' }));
    const tables = screen.getAllByRole('table');
    expect(tables.length).toBeGreaterThanOrEqual(7);
    for (const table of tables) {
      const caption = table.getAttribute('aria-label') ?? '';
      expect(caption).not.toBe('');
      const region = table.closest('[role="region"]') as HTMLElement | null;
      expect(region?.getAttribute('aria-label'), caption).toBe(caption);
      expect(region?.tabIndex, caption).toBe(0);
    }
  });
});

describe('the tables', () => {
  it('emphasises the best of each row by its direction, and signs each delta against the baseline', async () => {
    show(THREE);
    await loaded();
    const table = screen.getByRole('table', { name: 'Metrics of 3 runs against the baseline' });
    const latency = within(table).getByText('latency_p50_ms').closest('tr') as HTMLElement;
    // Lower is better for a latency: the baseline holds the best.
    expect([...latency.querySelectorAll('td[data-best]')].map((td) => td.textContent)).toEqual(['12.0 (best)']);
    expect([...latency.querySelectorAll('.rg-delta[data-meaning="worse"]')].map((d) => d.textContent)).toEqual(['+7.0 worse', '+129.0 worse']);
    const recall = within(table).getByText('recall@100').closest('tr') as HTMLElement;
    expect(recall.querySelectorAll('td[data-best]')).toHaveLength(2);
    expect((recall.querySelector('.rg-delta[data-meaning="better"]') as HTMLElement).textContent).toBe('+0.0290 better');
  });

  it('marks no best for a metric the API gives no direction, and signs its deltas without calling them better or worse', async () => {
    const unknown = { name: 'foo_score', direction: null, values: [2, 3.5, 1], deltas: [0, 1.5, -1], best: [] };
    show(THREE, routes((body) => {
      const reply = answer(body);
      return 'body' in reply ? { body: { ...reply.body, metrics: [...reply.body.metrics, unknown] } } : reply;
    }));
    await loaded();
    const table = screen.getByRole('table', { name: 'Metrics of 3 runs against the baseline' });
    const row = within(table).getByText('foo_score').closest('tr') as HTMLElement;
    expect(row.querySelectorAll('td[data-best]')).toHaveLength(0);
    expect(row.querySelectorAll('.rg-delta')).toHaveLength(0);
    expect([...row.querySelectorAll('.rg-compare__delta')].map((d) => d.textContent)).toEqual(['+1.5', '−1']);
    // Printed as stored, as the Runs screen prints a metric of unknown family.
    expect(within(row).getByText('3.5')).toBeTruthy();
  });

  it('shows only the parameters that differ, departures from the baseline marked', async () => {
    show(THREE);
    await loaded();
    const table = screen.getByRole('table', { name: 'Parameters that differ across the runs' });
    const rows = within(table).getAllByRole('row').slice(1);
    expect(rows).toHaveLength(4);
    const dense = rows[1] as HTMLElement;
    expect(dense.textContent).toContain('top_k');
    expect([...dense.querySelectorAll('mark')].map((m) => m.textContent)).toEqual(['50 (differs from the baseline)']);
  });

  it('says "no stage here" in the stage table where a run lacks a stage', async () => {
    show(THREE);
    await loaded();
    const table = screen.getByRole('table', { name: 'Stages of each run' });
    const fusion = within(table).getByText('after fusion').closest('tr') as HTMLElement;
    expect(fusion.textContent).toContain('no stage here');
    expect(fusion.textContent).toContain('rrf');
  });
});

describe('the verdict', () => {
  it('ends the page: the sentence, then the one primary action, to the worst regression beside the baseline', async () => {
    show(THREE);
    await loaded();
    fireEvent.change(screen.getByLabelText('Per-query metric'), { target: { value: 'mrr@10' } });
    fireEvent.click(screen.getByRole('radio', { name: 'B · hybrid-rerank' }));
    const section = screen.getByRole('region', { name: 'Verdict' });
    expect(section.textContent).toContain('On mrr@10, B · hybrid-rerank against the baseline: 131 queries improve, 108 are unchanged, 61 get worse — 6 by more than 0.3.');
    const primary = document.querySelectorAll('.rg-btn--primary');
    expect(primary).toHaveLength(1);
    expect(primary[0]?.textContent).toBe('Replay the 61 regressions');
    expect(primary[0]?.getAttribute('href')).toBe(`#replay/${RERANK}/q/q1?with=${DENSE}`);
    // Nothing follows the action on the page.
    const sheet = section.closest('.rg-sheet') as HTMLElement;
    expect(sheet.lastElementChild).toBe(section);
    const sentence = section.querySelector('.rg-compare__verdict') as HTMLElement;
    expect(sentence.nextElementSibling?.contains(primary[0] as Node)).toBe(true);
    expect(section.lastElementChild).toBe(sentence.nextElementSibling);
  });
});

describe('the pairing', () => {
  it('points Pair nodes… at its panel only while the panel is on the page', async () => {
    show(THREE);
    await loaded();
    const toggle = screen.getByRole('button', { name: 'Pair nodes…' });
    expect(toggle.hasAttribute('aria-controls')).toBe(false);
    // No control on the screen names an element that is not in the document.
    for (const el of document.querySelectorAll('[aria-controls]')) expect(document.getElementById(el.getAttribute('aria-controls')!), el.outerHTML.slice(0, 80)).not.toBeNull();
    fireEvent.click(toggle);
    expect(document.getElementById(toggle.getAttribute('aria-controls') ?? '')).toBe(screen.getByRole('region', { name: 'Pair nodes' }));
  });

  it('posts a pair drawn by hand, then the subtitle counts it; Reset to automatic clears it', async () => {
    const api = show(THREE);
    await loaded();
    expect(screen.getByText('Paired automatically')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Pair nodes…' }));
    const panel = screen.getByRole('region', { name: 'Pair nodes' });
    fireEvent.change(within(panel).getByLabelText('Pair the baseline with'), { target: { value: RERANK } });
    fireEvent.click(within(panel).getByRole('button', { name: 'dense, baseline' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'rerank, B' }));
    expect(await screen.findByText('1 pair by hand')).toBeTruthy();
    expect(api.bodies.at(-1)).toEqual({
      run_ids: [DENSE, HYBRID, RERANK],
      baseline: DENSE,
      pairing: { pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [{ node: 'dense', other: 'rerank' }] },
    });
    fireEvent.click(within(screen.getByRole('region', { name: 'Pair nodes' })).getByRole('button', { name: 'Reset to automatic' }));
    expect(await screen.findByText('Paired automatically')).toBeTruthy();
    expect(api.bodies.at(-1)).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: DENSE, pairing: { pipeline: 'dense-only', other: 'hybrid-rerank', pairs: [] } });
  });
});

/**
 * A `POST /compare` whose answers the test releases one by one, in any order.
 * Each answer names the benchmark it is released with, so the screen shows
 * which answer it holds.
 */
function held() {
  const calls: { body: CompareRequest; release: (benchmark: string) => Promise<void> }[] = [];
  const route = (body: CompareRequest) =>
    new Promise<MockReply<Comparison>>((resolve) => {
      calls.push({
        body,
        release: async (benchmark) => {
          const reply = answer(body);
          await act(async () => {
            resolve('body' in reply ? { body: { ...reply.body, ground_truth: { ...reply.body.ground_truth, benchmark } } } : reply);
            await new Promise((r) => setTimeout(r, 0));
          });
        },
      });
    });
  const call = async (n: number) => {
    await waitFor(() => expect(calls.length).toBeGreaterThan(n));
    return calls[n] as (typeof calls)[number];
  };
  return { calls, route, call };
}

const baselineTo = (id: string) => fireEvent.change(screen.getByLabelText('Baseline'), { target: { value: id } });
const busyNote = () => document.querySelector('.rg-compare__busy') as HTMLElement;

describe('answers that arrive out of order', () => {
  it('lands only the comparison asked for last', async () => {
    const h = held();
    show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    baselineTo(HYBRID);
    const second = await h.call(1);
    baselineTo(RERANK);
    const third = await h.call(2);
    await third.release('third');
    expect(screen.getByText('third')).toBeTruthy();
    await second.release('second');
    expect(screen.queryByText('second')).toBeNull();
    expect(screen.getByText('third')).toBeTruthy();
  });

  it('cancels the comparison a new baseline overtakes, and shows no error for it', async () => {
    const h = held();
    const api = show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    baselineTo(HYBRID);
    await h.call(1);
    const compares = () => api.signals.filter((_, i) => api.requests[i]?.startsWith('POST '));
    expect(compares()[1]?.aborted).toBe(false);
    baselineTo(RERANK);
    const third = await h.call(2);
    expect(compares()[1]?.aborted).toBe(true);
    // While the third is still held, the aborted second has settled: it must show nothing.
    await act(async () => new Promise((r) => setTimeout(r, 0)));
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(screen.queryByText('request_aborted')).toBeNull();
    expect(screen.getByText('first')).toBeTruthy();
    await third.release('third');
    expect(screen.getByText('third')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });

  it('compares the address again when a pairing that overtook a comparison in flight is refused, rather than staying busy', async () => {
    const calls: { body: CompareRequest; resolve: (reply: MockReply<Comparison>) => void }[] = [];
    const route = (body: CompareRequest) => new Promise<MockReply<Comparison>>((resolve) => calls.push({ body, resolve }));
    const call = async (n: number) => {
      await waitFor(() => expect(calls.length).toBeGreaterThan(n));
      return calls[n]!;
    };
    const named = (body: CompareRequest, benchmark: string): MockReply<Comparison> => {
      const reply = answer(body);
      return 'body' in reply ? { body: { ...reply.body, ground_truth: { ...reply.body.ground_truth, benchmark } } } : reply;
    };
    show(THREE, routes(route));
    const first = await call(0);
    await act(async () => first.resolve(named(first.body, 'first')));
    await screen.findByText('first');
    baselineTo(HYBRID);
    await call(1);
    expect(busyNote().textContent).toBe('Comparing again…');
    fireEvent.click(screen.getByRole('button', { name: 'Pair nodes…' }));
    const panel = screen.getByRole('region', { name: 'Pair nodes' });
    fireEvent.change(within(panel).getByLabelText('Pair the baseline with'), { target: { value: RERANK } });
    fireEvent.click(within(panel).getByRole('button', { name: 'dense, baseline' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'rerank, B' }));
    const pairing = await call(2);
    expect(pairing.body.pairing).toBeTruthy();
    await act(async () => pairing.resolve({ problem: problem('request_invalid', 'node dense is paired twice', 'Pair retrievers.', 400) }));
    // The comparison the pairing overtook is asked for again, for the address as it stands.
    const again = await call(3);
    expect(again.body).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: HYBRID });
    await act(async () => again.resolve(named(again.body, 'again')));
    expect(await screen.findByText('again')).toBeTruthy();
    expect(busyNote().textContent).toBe('');
  });

  it('never cancels a pairing a newer comparison overtakes: the API may have kept it', async () => {
    const h = held();
    const api = show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    fireEvent.click(screen.getByRole('button', { name: 'Pair nodes…' }));
    const panel = screen.getByRole('region', { name: 'Pair nodes' });
    fireEvent.change(within(panel).getByLabelText('Pair the baseline with'), { target: { value: RERANK } });
    fireEvent.click(within(panel).getByRole('button', { name: 'dense, baseline' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'rerank, B' }));
    await h.call(1);
    baselineTo(HYBRID);
    await h.call(2);
    const compares = api.signals.filter((_, i) => api.requests[i]?.startsWith('POST '));
    expect(compares[1]?.aborted ?? false).toBe(false);
  });

  it('keeps the comparison on screen while a newer one is read, every section busy, and says so in a status that stays', async () => {
    const h = held();
    show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    expect(busyNote().getAttribute('role')).toBe('status');
    expect(busyNote().textContent).toBe('');
    expect(document.querySelector('.rg-sheet')?.closest('[aria-busy="true"]')).toBeNull();
    baselineTo(HYBRID);
    const second = await h.call(1);
    expect(screen.getByText('first')).toBeTruthy();
    expect(document.querySelector('.rg-sheet')?.closest('[aria-busy="true"]')).toBeTruthy();
    expect(busyNote().textContent).toBe('Comparing again…');
    await second.release('second');
    expect(screen.getByText('second')).toBeTruthy();
    expect(document.querySelector('.rg-sheet')?.closest('[aria-busy="true"]')).toBeNull();
    expect(busyNote().textContent).toBe('');
  });

  it('writes a new baseline in place: Back does not step through baselines', async () => {
    show(THREE);
    await loaded();
    const before = window.history.length;
    baselineTo(HYBRID);
    await waitFor(() => expect(window.location.hash).toContain(`baseline=${HYBRID}`));
    expect(window.history.length).toBe(before);
  });

  it('keeps the screen on a newer comparison over a pairing it overtook, yet says the pair was kept, asks again, and serves nothing stale', async () => {
    const h = held();
    show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    fireEvent.click(screen.getByRole('button', { name: 'Pair nodes…' }));
    const panel = screen.getByRole('region', { name: 'Pair nodes' });
    fireEvent.change(within(panel).getByLabelText('Pair the baseline with'), { target: { value: RERANK } });
    fireEvent.click(within(panel).getByRole('button', { name: 'dense, baseline' }));
    fireEvent.click(within(panel).getByRole('button', { name: 'rerank, B' }));
    const pairing = await h.call(1);
    expect(pairing.body.pairing?.pairs).toEqual([{ node: 'dense', other: 'rerank' }]);
    baselineTo(HYBRID);
    const newer = await h.call(2);
    await newer.release('newer');
    await pairing.release('paired');
    // The pairing's own answer never lands over the newer comparison…
    expect(screen.getByText('newer')).toBeTruthy();
    expect(screen.queryByText('paired')).toBeNull();
    expect(within(screen.getByRole('region', { name: 'Pair nodes' })).queryByRole('alert')).toBeNull();
    // …but the API kept the pair, and the panel says so.
    expect(within(screen.getByRole('region', { name: 'Pair nodes' })).getByRole('status').textContent).toBe(
      'dense paired with rerank (kept; the comparison shown was asked for after it).',
    );
    // The newer comparison may have been read before the pair was written: it is asked for again.
    const again = await h.call(3);
    expect(again.body).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: HYBRID });
    await again.release('again');
    expect(screen.getByText('again')).toBeTruthy();
    expect(document.querySelector('.rg-sheet')?.closest('[aria-busy="true"]')).toBeNull();
    // An answer cached before the pair was kept is never served again.
    baselineTo(DENSE);
    const back = await h.call(4);
    expect(back.body).toEqual({ run_ids: [DENSE, HYBRID, RERANK], baseline: DENSE });
  });

  it('adds a run to the address as it stands when the answer arrives, not as it was when Add was pressed', async () => {
    const h = held();
    show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    await screen.findByText('first');
    const select = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(4));
    fireEvent.change(select, { target: { value: R4 } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    const adding = await h.call(1);
    baselineTo(HYBRID);
    await waitFor(() => expect(window.location.hash).toContain(`baseline=${HYBRID}`));
    await (await h.call(2)).release('newer');
    await adding.release('added');
    await waitFor(() => expect(window.location.hash).toBe(`#compare/${DENSE}+${HYBRID}+${RERANK}+${R4}?baseline=${HYBRID}`));
  });

  it('keeps the Add button\'s words while it adds, so its width never moves the button beside it', async () => {
    const h = held();
    show(THREE, routes(h.route));
    await (await h.call(0)).release('first');
    const select = (await screen.findByLabelText('Add a run')) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(4));
    fireEvent.change(select, { target: { value: R4 } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await h.call(1);
    const add = screen.getByRole('button', { name: 'Add' });
    expect(add.getAttribute('aria-busy')).toBe('true');
    expect(add.textContent).toBe('Add');
  });
});

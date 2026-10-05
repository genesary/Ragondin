// The Pipeline screen's Run buttons, end to end: a `not run yet` cell's
// "Run on <benchmark>" opens Runs' launch panel with that benchmark chosen,
// the launch files a run, and the cell fills; "Run the N missing cells" opens
// it with every missing benchmark, and one confirmation launches one run per
// column the panel can launch.
//
// The fixture workspace measures `hybrid-rerank` on its one ready benchmark,
// and the manifest's two are not on disk, so this test imports three more —
// subsets of the fixture corpus's queries, which digest differently and so
// are columns of their own — through the API, from files it writes. The manifest's
// columns stay missing: launching on them needs a download, the network.
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { Page } from '@playwright/test';
import { expect, test, type Fixture } from './support/fixture.ts';

/** The fixture corpus with only `queries`, and their qrels: a benchmark of its own. */
function subset(fixture: Fixture, queries: readonly string[]): string {
  const dir = mkdtempSync(join(tmpdir(), 'ragondin-e2e-corpus-'));
  mkdirSync(join(dir, 'qrels'));
  writeFileSync(join(dir, 'corpus.jsonl'), readFileSync(join(fixture.corpus, 'corpus.jsonl')));
  const lines = (file: string) => readFileSync(join(fixture.corpus, file), 'utf8').split('\n').filter((line) => line !== '');
  const kept = lines('queries.jsonl').filter((line) => queries.includes((JSON.parse(line) as { _id: string })._id));
  writeFileSync(join(dir, 'queries.jsonl'), `${kept.join('\n')}\n`);
  const [header, ...qrels] = lines('qrels/test.tsv');
  writeFileSync(join(dir, 'qrels', 'test.tsv'), `${[header, ...qrels.filter((line) => queries.includes(line.split('\t')[0] ?? ''))].join('\n')}\n`);
  return dir;
}

/** Imports `path` as the local benchmark `name`, as Setup's form does. */
async function importBenchmark(page: Page, origin: string, name: string, path: string): Promise<void> {
  const answer = await page.request.post('/api/v1/benchmarks/import', { data: { name, path }, headers: { Origin: origin } });
  expect(answer.status(), await answer.text()).toBe(200);
  // The import copied what it read into the workspace.
  rmSync(path, { recursive: true, force: true });
}

const column = (page: Page, benchmark: string) => page.getByRole('columnheader', { name: new RegExp(`^${benchmark.replace('/', '\\/')} `) });

test('the Pipeline screen launches the missing cells through Runs’ launch panel', async ({ page, fixture, ragondin }) => {
  const origin = new URL(ragondin.url).origin;
  await importBenchmark(page, origin, 'first-half', subset(fixture, ['q-cat', 'q-greek']));
  await importBenchmark(page, origin, 'second-half', subset(fixture, ['q-river', 'q-short', 'q-wren']));
  await importBenchmark(page, origin, 'odd-ones', subset(fixture, ['q-cat', 'q-river', 'q-wren']));

  // One cell: Run on beir/first-half.
  await page.goto('/#pipeline/hybrid-rerank');
  await expect(column(page, 'beir/first-half')).toContainText('not run yet');
  await page.getByRole('button', { name: 'Run on beir/first-half' }).click();
  await expect(page).toHaveURL(/#runs\?launch=hybrid-rerank&benchmark=beir%2Ffirst-half$/);
  const panel = page.getByRole('region', { name: 'Launch a run' });
  await expect(panel.getByRole('combobox', { name: 'Benchmark' })).toHaveValue('beir/first-half');
  await panel.getByRole('button', { name: 'Launch', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Notifications' })).toContainText('Run done · hybrid-rerank on beir/first-half');

  await page.goto('/#pipeline/hybrid-rerank');
  await expect(column(page, 'beir/first-half')).toContainText('measured');
  await expect(column(page, 'beir/second-half')).toContainText('not run yet');
  await expect(column(page, 'beir/odd-ones')).toContainText('not run yet');

  // Every missing cell: the panel lists each benchmark, says why the two not
  // on disk are not launched, and one confirmation launches the other two.
  await page.getByRole('button', { name: /^Run the \d+ missing cells$/ }).click();
  await expect(page).toHaveURL(/#runs\?launch=hybrid-rerank(&benchmark=[^&]+){4}$/);
  const list = panel.getByRole('list', { name: 'Benchmarks' });
  await expect(list.getByRole('listitem')).toHaveText([
    /^beir\/odd-ones/,
    /^beir\/scifact not launched: not ready/,
    /^beir\/second-half/,
    /^squad\/dev not launched: not ready/,
  ]);
  await panel.getByRole('button', { name: 'Launch 2 runs' }).click();
  const notifications = page.getByRole('region', { name: 'Notifications' });
  await expect(notifications).toContainText('Run done · hybrid-rerank on beir/odd-ones');
  await expect(notifications).toContainText('Run done · hybrid-rerank on beir/second-half');

  await page.goto('/#pipeline/hybrid-rerank');
  for (const benchmark of ['beir/exit-criterion', 'beir/first-half', 'beir/odd-ones', 'beir/second-half']) {
    await expect(column(page, benchmark)).toContainText('measured');
  }
  for (const benchmark of ['beir/scifact', 'squad/dev']) await expect(column(page, benchmark)).toContainText('not run yet');
});

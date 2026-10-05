// The first-run journey (the front-end design, § 3): an empty workspace →
// Setup's two-step invitation → the fixture benchmark imported and ready →
// the Editor on the first-launch example → launched from Runs → the run
// appears and completes.
//
// "Obtain SciFact" becomes "import the fixture corpus": the download path
// needs the network, and is exercised by the ignored SciFact journey
// (bin/ragondin/tests/journey_scifact.rs).
import { expect, test } from './support/fixture.ts';
import { expectSameViewWhenOpenedFresh } from './support/views.ts';

test.use({ workspaceKind: 'empty' });

test('first run: an empty workspace to a completed run', async ({ page, fixture }) => {
  await page.goto('/');
  await page.getByRole('navigation', { name: 'Screens' }).getByRole('link', { name: 'Setup' }).click();

  // The two-step invitation, and nothing else to configure first.
  const start = page.getByRole('region', { name: 'Get started' });
  await expect(start.getByRole('heading', { name: 'Add a benchmark' })).toBeVisible();
  await expect(start.getByRole('heading', { name: /^Connect a generator/ })).toBeVisible();

  // The fixture benchmark, imported from the server's disk.
  const form = start.getByRole('form', { name: 'Import a local corpus' });
  await form.getByRole('textbox', { name: 'Corpus directory' }).fill(fixture.corpus);
  await form.getByRole('textbox', { name: 'Import as' }).fill(fixture.benchmark_name);
  await form.getByRole('button', { name: 'Import' }).click();
  const benchmarks = page.getByRole('region', { name: 'Benchmarks' });
  await expect(benchmarks.getByRole('row', { name: new RegExp(fixture.benchmark) })).toContainText('imported, verified');

  // The Editor opens on the first-launch example, and keeps it.
  await page.getByRole('navigation', { name: 'Screens' }).getByRole('link', { name: 'Editor' }).click();
  await expect(page.getByRole('application', { name: /^Pipeline/ }).getByRole('group', { name: /lexical, retriever\/bm25/ })).toBeVisible();
  await page.getByRole('button', { name: 'Keep this pipeline' }).click();
  // Its first write asks its name, offering the one proposed.
  const naming = page.getByRole('region', { name: 'Name this pipeline' });
  await expect(naming.getByRole('textbox', { name: 'Pipeline name' })).toHaveValue('example');
  await naming.getByRole('button', { name: 'Save' }).click();
  await expect(page).toHaveURL(/#editor\/example$/);
  await expect(page.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible();

  // Launched from Runs, where the launch panel lives.
  await page.getByRole('navigation', { name: 'Screens' }).getByRole('link', { name: 'Runs' }).click();
  await page.getByRole('button', { name: 'Launch…' }).click();
  const panel = page.getByRole('region', { name: 'Launch a run' });
  await panel.getByRole('combobox', { name: 'Pipeline' }).selectOption('example');
  await panel.getByRole('combobox', { name: 'Benchmark' }).selectOption(fixture.benchmark);
  await panel.getByRole('button', { name: 'Launch', exact: true }).click();
  // Exactly the note beside the queued run's id: the panel's caption says the id "is announced when it is queued" too.
  await expect(panel.getByText('announced', { exact: true })).toBeVisible();

  // It appears, and completes: the toast, then the run filed in the table.
  await expect(page.getByRole('region', { name: 'Notifications' })).toContainText(`Run done · example on ${fixture.benchmark}`);
  const table = page.getByRole('table', { name: 'Runs, grouped by pipeline' });
  const group = table.getByRole('rowgroup').filter({ has: page.getByRole('rowheader', { name: /^example/ }) });
  await expect(group.getByRole('row', { name: new RegExp(`^Run [0-9a-f]{12} on ${fixture.benchmark}`) })).toContainText('done');
  await expect(group).toContainText('ndcg@10');

  await expectSameViewWhenOpenedFresh(page, (p) => p.getByRole('table', { name: 'Runs, grouped by pipeline' }));
});

// The iterate journey (the front-end design, § 3): Runs → fork hybrid-rerank's
// run → change one parameter → launch → Compare against the original, the
// delta visible.
//
// The parameter is the dense leg's `top_k`, three to one. On the fixture the
// cross-encoder recovers every query from what is left, by construction
// (bin/ragondin/tests/exit_criterion.rs), so the final metrics are unchanged
// and the delta shows where the change acts: the ranking after fusion, in
// Compare's stage line, and the parameter itself in its configuration table.
import { expect, test } from './support/fixture.ts';
import { expectSameViewWhenOpenedFresh } from './support/views.ts';

test('iterate: fork a run, change top_k, launch, compare with the original', async ({ page, fixture }) => {
  const original = fixture.runs.hybrid_rerank;
  const short = (id: string) => id.slice(0, 12);
  const screens = page.getByRole('navigation', { name: 'Screens' });

  // Runs → Fork.
  await page.goto('/#runs');
  const table = page.getByRole('table', { name: 'Runs, grouped by pipeline' });
  await table.getByRole('checkbox', { name: `Select run ${short(original)} on ${fixture.benchmark}` }).check();
  await page.getByRole('button', { name: 'Fork this run' }).click();

  // One parameter.
  await expect(page).toHaveURL(/#editor\/hybrid-rerank-fork$/);
  await expect(page.getByText('Forked from run')).toBeVisible();
  await page.getByRole('button', { name: 'More actions for vectors' }).click();
  await page.getByRole('menuitem', { name: /^Open parameters/ }).click();
  const topK = page.getByRole('complementary', { name: 'vectors' }).getByRole('textbox', { name: 'top_k' });
  await topK.fill('1');
  await topK.press('Enter');
  // The fork is the run's configuration byte for byte, comments included, so
  // its first save asks before the server's rendering replaces it.
  await page.getByRole('button', { name: 'Rewrite this file' }).click();
  await expect(page.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible();

  // Launch.
  await screens.getByRole('link', { name: 'Runs' }).click();
  await page.getByRole('button', { name: 'Launch…' }).click();
  const panel = page.getByRole('region', { name: 'Launch a run' });
  await panel.getByRole('combobox', { name: 'Pipeline' }).selectOption('hybrid-rerank-fork');
  await panel.getByRole('combobox', { name: 'Benchmark' }).selectOption(fixture.benchmark);
  await panel.getByRole('button', { name: 'Launch', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Notifications' })).toContainText(`Run done · hybrid-rerank-fork on ${fixture.benchmark}`);

  // Compare, against the original.
  const fork = table.getByRole('rowgroup').filter({ has: page.getByRole('rowheader', { name: /^hybrid-rerank-fork/ }) });
  const forkRow = fork.getByRole('row', { name: new RegExp(`^Run [0-9a-f]{12} on ${fixture.benchmark}`) });
  await expect(forkRow).toContainText('done');
  await table.getByRole('checkbox', { name: `Select run ${short(original)} on ${fixture.benchmark}` }).check();
  await forkRow.getByRole('checkbox').check();
  await page.getByRole('button', { name: 'Compare 2 selected' }).click();
  await expect(page).toHaveURL(new RegExp(`#compare/.*baseline=${original}`));
  await expect(page.getByRole('list', { name: 'Runs compared' }).getByRole('listitem').first()).toHaveText(new RegExp(`^baseline\\s*hybrid-rerank\\s*${original.slice(0, 6)}$`));

  // The delta: the parameter, and the ranking after fusion.
  const configuration = page.getByRole('table', { name: 'Parameters that differ across the runs' });
  await expect(configuration.getByRole('row', { name: 'vectors top_k 3 1 (differs from the baseline)' })).toBeVisible();
  const afterFusion = page.getByRole('table', { name: 'Stages of each run' }).getByRole('row', { name: /^after fusion/ });
  // The fixture's figures: with one dense passage, the fused ranking loses
  // ground on the queries the dense leg alone answered.
  await expect(afterFusion.getByRole('cell')).toHaveText(['after fusion', /^fused\s*0\.8262$/, /^fused\s*0\.7786$/]);

  await expectSameViewWhenOpenedFresh(page, (p) => p.getByRole('main'));
});

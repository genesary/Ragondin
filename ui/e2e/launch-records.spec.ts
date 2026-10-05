// The two runs decision #390 leaves to be shown rather than guessed, as the
// fixture workspace holds them (bin/ragondin/tests/support/workspace.rs): a
// run without a launch record, and a run whose recorded name's content has
// changed since. Runs and the Pipeline screen show both facts ADR-C39 § 4
// keeps apart — what the record says, and which documents hold the content —
// and resolve neither into the other.
import { expect, test } from './support/fixture.ts';

test('Runs groups the run without a record by its content, and the changed run by its recorded name', async ({ page, fixture }) => {
  const { unrecorded, changed } = fixture.runs;
  await page.goto('/#runs');
  const table = page.getByRole('table', { name: 'Runs, grouped by pipeline' });
  const group = (name: string) => table.getByRole('rowgroup').filter({ has: page.getByRole('rowheader', { name: new RegExp(`^${name}\\b`) }) });
  const row = (id: string) => table.getByRole('row', { name: new RegExp(`^Run ${id.slice(0, 12)} on `) });

  // No record: under the document whose hash it has, saying so.
  await expect(group('bm25-only').getByRole('row', { name: new RegExp(`^Run ${unrecorded.slice(0, 12)} on `) })).toBeVisible();
  await expect(row(unrecorded)).toContainText('launch not recorded');

  // Recorded as hybrid-rerank: under that name, though no document holds its content.
  await expect(group('hybrid-rerank').getByRole('row', { name: new RegExp(`^Run ${changed.slice(0, 12)} on `) })).toBeVisible();
  await expect(row(changed)).toContainText('no current document has this content');
});

test('the Pipeline screen lists the changed run among the feeding runs, filling no cell, with what differs', async ({ page, fixture }) => {
  const { changed, hybrid_rerank: hybrid } = fixture.runs;
  await page.goto('/#pipeline/hybrid-rerank');
  const feeding = page.getByRole('list', { name: 'Runs that feed the matrix' });
  const entry = feeding.getByRole('listitem').filter({ has: page.getByRole('link', { name: `run ${changed.slice(0, 12)}` }) });
  await expect(entry).toContainText('Launched as hybrid-rerank; content since changed');
  await expect(entry).toContainText('fills no cell');
  const difference = entry.getByRole('table', { name: `What differs between hybrid-rerank now and run ${changed.slice(0, 12)}` });
  await expect(difference.getByRole('row', { name: `reranked top_k 10 ${fixture.changed_top_k}` })).toBeVisible();

  // The column is the current content's run, not the changed one.
  const current = feeding.getByRole('listitem').filter({ has: page.getByRole('link', { name: `run ${hybrid.slice(0, 12)}` }) });
  await expect(current).toContainText('fills its column');
});

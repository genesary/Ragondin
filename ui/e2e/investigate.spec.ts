// The investigate journey (the front-end design, § 3): Compare → the
// histogram's regressions → Replay side by side → the node that differs,
// marked, and what each run produced there.
//
// dense-only against hybrid-rerank as the baseline, so the queries dense-only
// loses are the regressions; each of the regressed bin's queries opens Replay
// side by side. Of the nodes on screen, the ones B has and A lacks are marked
// "only in B"; the reranker, the one that ranks the gold passage first, is
// the node selected.
import { expect, test } from './support/fixture.ts';
import { expectSameViewWhenOpenedFresh } from './support/views.ts';

test('investigate: from the histogram to the node that differs, side by side', async ({ page, fixture }) => {
  const { hybrid_rerank: hybrid, dense_only: dense } = fixture.runs;
  await page.goto(`/#compare/${hybrid}+${dense}?baseline=${hybrid}`);

  // The histogram's regressions.
  const perQuery = page.getByRole('region', { name: 'Per query' });
  const worst = perQuery.getByRole('button', { name: /^much worse, [1-9][\d,]* quer(y|ies), change below −0\.3$/ });
  await worst.click();
  await expect(worst).toHaveAttribute('aria-expanded', 'true');
  const regressions = perQuery.getByRole('region', { name: /queries much worse/ }).getByRole('link');
  await expect(regressions).not.toHaveCount(0);
  const query = /\/q\/([^?]+)/.exec((await regressions.first().getAttribute('href')) ?? '')?.[1] ?? '';
  expect(query).not.toBe('');

  // Replay side by side.
  await regressions.first().click();
  await expect(page).toHaveURL(new RegExp(`#replay/${dense}/q/${query}\\?with=${hybrid}$`));
  await expect(page.getByRole('radio', { name: 'Side by side' })).toBeChecked();
  const a = page.getByRole('application', { name: `Run A, dense-only, query ${query}` });
  const b = page.getByRole('application', { name: `Run B, hybrid-rerank, query ${query}` });
  await expect(a.getByRole('group', { name: /^retriever vectors/ })).toBeVisible();

  // The nodes that differ are marked; the reranker is the one selected.
  for (const node of ['retriever lexical', 'fusion fused', 'reranker reranked']) {
    await expect(b.getByRole('group', { name: new RegExp(`^${node}, .*only in B$`) })).toBeVisible();
  }
  await b.getByRole('group', { name: /^reranker reranked/ }).click();
  await expect(page).toHaveURL(new RegExp(`#replay/${dense}/q/${query}/node/reranked\\?with=${hybrid}$`));

  // Its two outputs: A's final ranking where B reranks, and B's reranked one.
  const inspector = page.getByRole('complementary', { name: 'reranked' });
  const inA = inspector.getByRole('region', { name: 'A, dense-only' });
  const inB = inspector.getByRole('region', { name: 'B, hybrid-rerank' });
  await expect(inA).toContainText('No such node in A.');
  await expect(inA.getByRole('list', { name: /^Ranked by vectors/ }).getByRole('listitem')).not.toHaveCount(0);
  await expect(inB.getByRole('list', { name: /^Ranked by reranked/ }).getByRole('listitem').first()).toContainText('gold');
  await expect(inspector.getByRole('heading', { name: 'Verdict' })).toBeVisible();

  await expectSameViewWhenOpenedFresh(page, (p) => p.getByRole('complementary', { name: 'reranked' }));
});

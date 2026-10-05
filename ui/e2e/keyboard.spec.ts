// The keyboard-only journey (the front-end design, § 9): from Runs, select two
// runs, open Compare, open Replay, select a node on the canvas and open its
// menu — Tab, the arrow keys, Space, Enter and Shift+F10, and no pointer.
import { expect, test } from './support/fixture.ts';
import { isFocused, pressUntilFocused, tabTo } from './support/keyboard.ts';

test('keyboard only: Runs → Compare → Replay → a node and its menu', async ({ page, fixture }) => {
  const { hybrid_rerank: hybrid, dense_only: dense } = fixture.runs;
  const row = (id: string) => page.getByRole('row', { name: new RegExp(`^Run ${id.slice(0, 12)} on `) });
  await page.goto('/#runs');
  await expect(row(hybrid)).toBeVisible();

  // Runs: the table is one tab stop; the arrows move, Space selects.
  await tabTo(page, page.getByRole('row').filter({ has: page.getByRole('checkbox') }).first(), 'the runs table');
  await page.keyboard.press('Home');
  for (const id of [hybrid, dense]) {
    await pressUntilFocused(page, row(id), 'ArrowDown', `run ${id.slice(0, 12)}`);
    await page.keyboard.press('Space');
    await expect(row(id)).toHaveAccessibleName(/, selected$/);
  }
  await tabTo(page, page.getByRole('button', { name: 'Compare 2 selected' }), 'Compare 2 selected');
  await page.keyboard.press('Enter');

  // Compare: a regressed bin, then one of its queries, into Replay.
  // Each run by its 12-character prefix, which names one run of the workspace.
  await expect(page).toHaveURL(new RegExp(`#compare/${hybrid.slice(0, 12)}\\+${dense.slice(0, 12)}\\?baseline=${hybrid.slice(0, 12)}$`));
  const worst = page.getByRole('button', { name: /^much worse, [1-9][\d,]* quer(y|ies), change below −0\.3$/ });
  await tabTo(page, worst, 'the regressed bin');
  await page.keyboard.press('Enter');
  await tabTo(page, page.getByRole('region', { name: /queries much worse/ }).getByRole('link').first(), 'its first query');
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(new RegExp(`#replay/${dense}/q/[^/?]+\\?with=${hybrid}$`));

  // Replay: a node selected on the canvas, then its menu.
  const node = page.getByRole('application', { name: /^Run B/ }).getByRole('group', { name: /^reranker reranked/ });
  await tabTo(page, node, 'the reranker on run B');
  await page.keyboard.press('Enter');
  await expect(page).toHaveURL(/\/node\/reranked\?/);
  await expect(page.getByRole('complementary', { name: 'reranked' })).toBeVisible();
  await page.keyboard.press('Shift+F10');
  const menu = page.getByRole('menu', { name: 'Node reranked' });
  await expect(menu).toBeVisible();
  expect(await isFocused(menu.getByRole('menuitem').first()), 'focus moves into the menu').toBe(true);
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  await expect.poll(() => isFocused(node), { message: 'focus returns to the node' }).toBe(true);
});

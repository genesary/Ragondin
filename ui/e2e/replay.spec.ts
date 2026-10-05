// Replay as the UX audit of 2026-10-05 read it (#490): Compare's "Replay the
// N regressions" keeps the set, stepped through with Previous and Next; side
// by side, the two canvases share one zoom and line up; at phone width the
// query list folds so the trace comes first; and `#replay` alone offers a
// run to replay.
import { expect, test } from './support/fixture.ts';

test('Replay the N regressions keeps the set in the address, stepped through with Previous and Next', async ({ page, fixture }) => {
  const { hybrid_rerank: hybrid, dense_only: dense } = fixture.runs;
  await page.goto(`/#compare/${hybrid}+${dense}?baseline=${hybrid}`);
  await page.getByRole('link', { name: /^Replay the [1-9][\d,]* regressions?$/ }).click();
  await expect(page).toHaveURL(new RegExp(`#replay/${dense}/q/[^?]+\\?with=${hybrid}&set=regressions&metric=`));
  const steps = page.getByRole('navigation', { name: 'Step through the regressions' });
  await expect(steps).toContainText(/Regression 1 of [1-9]/);
  const total = Number(/of ([\d,]+)/.exec((await steps.textContent()) ?? '')?.[1]?.replace(/,/g, ''));
  const first = page.url();
  if (total > 1) {
    await steps.getByRole('button', { name: 'Next regression' }).click();
    await expect(steps).toContainText(`Regression 2 of ${total}`);
    expect(page.url()).not.toBe(first);
    await expect(page).toHaveURL(/&set=regressions&metric=/);
  }
  await expect(page.getByRole('listbox', { name: 'Queries' }).getByRole('option')).toHaveCount(total);
});

test('side by side, the two canvases share one zoom and line up', async ({ page, fixture }) => {
  const { hybrid_rerank: hybrid, dense_only: dense } = fixture.runs;
  await page.goto(`/#replay/${dense}`);
  await expect(page).toHaveURL(/\/q\//);
  const query = /\/q\/([^?/]+)/.exec(page.url())?.[1] ?? '';
  await page.goto(`/#replay/${dense}/q/${query}?with=${hybrid}`);
  // Replay names each run by its name, never a letter (#489): the run replayed
  // is the first canvas, the run beside the second.
  const side = { A: 0, B: 1 } as const;
  const toolbar = (letter: 'A' | 'B') => page.getByRole('toolbar', { name: /^Canvas, .+, query / }).nth(side[letter]);
  await expect(toolbar('A')).toBeVisible();
  await expect(toolbar('B')).toBeVisible();
  const zoom = async (letter: 'A' | 'B') => (await toolbar(letter).locator('.rg-canvas__zoom').textContent()) ?? '';
  await expect.poll(async () => (await zoom('A')) === (await zoom('B')) && (await zoom('A')) !== '').toBe(true);
  // The query input, the first rank of both graphs, stands at the same place across.
  const input = (letter: 'A' | 'B') => page.getByRole('application', { name: /, query / }).nth(side[letter]).locator('.react-flow__node').first();
  const a = await input('A').boundingBox();
  const b = await input('B').boundingBox();
  expect(a).not.toBeNull();
  expect(Math.abs((a?.x ?? 0) - (b?.x ?? 0))).toBeLessThanOrEqual(1);
});

test('at phone width the query list folds, so the trace comes first', async ({ page, fixture }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/#replay/${fixture.runs.dense_only}`);
  await expect(page).toHaveURL(/\/q\//);
  const toggle = page.getByRole('button', { name: /^Queries · / });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByRole('listbox', { name: 'Queries' })).toBeHidden();
  await expect(page.getByRole('application', { name: /query/ })).toBeVisible();
  await toggle.click();
  await expect(page.getByRole('listbox', { name: 'Queries' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test('Replay alone offers a run to replay', async ({ page, fixture }) => {
  await page.goto('/#replay');
  await expect(page.getByRole('heading', { name: 'Choose a run to replay' })).toBeVisible();
  await page.getByLabel('Run').selectOption(fixture.runs.dense_only);
  await page.getByRole('link', { name: 'Replay this run' }).click();
  await expect(page).toHaveURL(new RegExp(`#replay/${fixture.runs.dense_only}/q/`));
});

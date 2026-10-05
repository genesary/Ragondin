// "Every view has URL state, so a link pasted into an issue reproduces it"
// (the front-end design, § 3), asserted at the end of each journey: the
// address the journey ended on, opened in a fresh browser context — no
// history, no session or local storage — shows the same view, read as the
// accessibility tree of what the journey names as the view.
import type { Locator, Page } from '@playwright/test';
import { expect } from './fixture.ts';

export async function expectSameViewWhenOpenedFresh(page: Page, view: (page: Page) => Locator): Promise<void> {
  const url = page.url();
  const heading = await page.getByRole('heading', { level: 1 }).textContent();
  const shown = await view(page).ariaSnapshot();
  const browser = page.context().browser();
  if (browser === null) throw new Error('the page belongs to no browser');
  const context = await browser.newContext();
  const origin = new URL(url).origin;
  const refused: string[] = [];
  await context.route('**/*', async (route) => {
    const requested = route.request().url();
    if (new URL(requested).origin === origin) await route.continue();
    else {
      refused.push(requested);
      await route.abort('blockedbyclient');
    }
  });
  try {
    const fresh = await context.newPage();
    await fresh.goto(url);
    await expect(fresh.getByRole('heading', { level: 1 })).toHaveText(heading ?? '');
    await expect(view(fresh)).toMatchAriaSnapshot(shown);
    expect(refused, 'the fresh page reached only the origin that served it').toEqual([]);
  } finally {
    await context.close();
  }
}

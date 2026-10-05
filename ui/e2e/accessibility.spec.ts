// The accessibility pass (the front-end design, § 9): every screen, in both
// themes, under the automated check (support/accessibility.ts), and with
// `prefers-reduced-motion` set, no animation running on any of them.
import type { Page } from '@playwright/test';
import { accessibilityViolations, visibleAnimations, withAccessibilityEngine } from './support/accessibility.ts';
import { expect, test, type Fixture } from './support/fixture.ts';

/** The six screens, each at an address that shows it with content. */
function screens(fixture: Fixture): { name: string; hash: string; ready: (page: Page) => Promise<void> }[] {
  const { hybrid_rerank: hybrid, dense_only: dense } = fixture.runs;
  return [
    { name: 'Runs', hash: '#runs', ready: (p) => expect(p.getByRole('table', { name: 'Runs, grouped by pipeline' })).toBeVisible() },
    { name: 'Pipeline', hash: '#pipeline/hybrid-rerank', ready: (p) => expect(p.getByRole('table', { name: 'hybrid-rerank: each node on each benchmark' })).toBeVisible() },
    { name: 'Compare', hash: `#compare/${hybrid}+${dense}?baseline=${hybrid}`, ready: (p) => expect(p.getByRole('region', { name: 'Verdict' })).toBeVisible() },
    {
      name: 'Replay',
      hash: `#replay/${dense}/q/q-greek/node/vectors?with=${hybrid}`,
      ready: (p) => expect(p.getByRole('complementary', { name: 'vectors' }).getByRole('region', { name: 'hybrid-rerank' })).toBeVisible(),
    },
    { name: 'Editor', hash: '#editor/hybrid-rerank', ready: (p) => expect(p.getByRole('status').filter({ hasText: /^No changes since it was opened$/ })).toBeVisible() },
    { name: 'Setup', hash: '#setup', ready: (p) => expect(p.getByRole('region', { name: 'Benchmarks' })).toBeVisible() },
  ];
}

for (const theme of ['Light', 'Dark'] as const) {
  test(`every screen, in the ${theme.toLowerCase()} theme, has no accessibility violation`, async ({ page, context, fixture }, testInfo) => {
    await withAccessibilityEngine(context);
    await page.goto('/#runs');
    await page.getByRole('radio', { name: theme }).check();
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
    const found: string[] = [];
    const details: Record<string, unknown[]> = {};
    for (const screen of screens(fixture)) {
      await page.goto(`/${screen.hash}`);
      await screen.ready(page);
      await expect(page.locator('html'), `${screen.name} keeps the theme chosen`).toHaveAttribute('data-theme', theme.toLowerCase());
      const violations = await accessibilityViolations(page);
      if (violations.length > 0) details[screen.name] = violations;
      for (const { ruleId, message } of violations) found.push(`${screen.name}: ${ruleId}: ${message}`);
    }
    // Every element and the engine's message for it, beside the result.
    await testInfo.attach('violations.json', { body: JSON.stringify(details, null, 2), contentType: 'application/json' });
    expect(found, `WCAG 2.2 A and AA violations in the ${theme.toLowerCase()} theme`).toEqual([]);
  });
}

test('with prefers-reduced-motion set, no animation runs on any screen', async ({ page, fixture }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const found: Record<string, string[]> = {};
  for (const screen of screens(fixture)) {
    await page.goto(`/${screen.hash}`);
    await screen.ready(page);
    const visible = await visibleAnimations(page);
    if (visible.length > 0) found[screen.name] = visible;
  }
  expect(found).toEqual({});
});

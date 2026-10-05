// The accessibility pass (the front-end design, § 9): every screen, in both
// themes, under the automated check (support/accessibility.ts), and with
// `prefers-reduced-motion` set, no animation running on any of them.
import type { Page } from '@playwright/test';
import { accessibilityViolations, visibleAnimations, withAccessibilityEngine } from './support/accessibility.ts';
import { expect, test, type Fixture } from './support/fixture.ts';

/**
 * The violations the check found when it was written, each a defect of the
 * screen it is on, kept here under the issue that removes it — not an
 * allowance: the test fails when what it finds differs from this list in
 * either direction, so a new violation fails it, and so does a fix until its
 * entry goes. Empty once #476 is done.
 */
const KNOWN_VIOLATIONS: readonly { screen: string; rule: string; issue: string }[] = [
  { screen: 'Compare', rule: 'aria_id_unique', issue: '#476' },
  { screen: 'Compare', rule: 'label_name_visible', issue: '#476' },
  { screen: 'Editor', rule: 'svg_graphics_labelled', issue: '#476' },
  { screen: 'Replay', rule: 'aria_toolbar_label_unique', issue: '#476' },
  { screen: 'Replay', rule: 'label_name_visible', issue: '#476' },
  { screen: 'Replay', rule: 'svg_graphics_labelled', issue: '#476' },
];

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
      ready: (p) => expect(p.getByRole('complementary', { name: 'vectors' }).getByRole('region', { name: 'B, hybrid-rerank' })).toBeVisible(),
    },
    { name: 'Editor', hash: '#editor/hybrid-rerank', ready: (p) => expect(p.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible() },
    { name: 'Setup', hash: '#setup', ready: (p) => expect(p.getByRole('region', { name: 'Benchmarks' })).toBeVisible() },
  ];
}

for (const theme of ['Light', 'Dark'] as const) {
  test(`every screen, in the ${theme.toLowerCase()} theme, has no accessibility violation but the known ones`, async ({ page, context, fixture }, testInfo) => {
    await withAccessibilityEngine(context);
    await page.goto('/#runs');
    await page.getByRole('radio', { name: theme }).check();
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
    const found = new Set<string>();
    const details: Record<string, unknown[]> = {};
    for (const screen of screens(fixture)) {
      await page.goto(`/${screen.hash}`);
      await screen.ready(page);
      await expect(page.locator('html'), `${screen.name} keeps the theme chosen`).toHaveAttribute('data-theme', theme.toLowerCase());
      const violations = await accessibilityViolations(page);
      if (violations.length > 0) details[screen.name] = violations;
      for (const { ruleId } of violations) found.add(`${screen.name}: ${ruleId}`);
    }
    // Every element and the engine's message for it, beside the result.
    await testInfo.attach('violations.json', { body: JSON.stringify(details, null, 2), contentType: 'application/json' });
    const known = KNOWN_VIOLATIONS.map(({ screen, rule }) => `${screen}: ${rule}`);
    expect([...found].sort(), `WCAG 2.2 A and AA violations in the ${theme.toLowerCase()} theme, against the known ones`).toEqual([...known].sort());
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

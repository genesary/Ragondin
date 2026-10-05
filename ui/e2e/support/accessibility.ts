// The accessibility check applied to a screen: an automated rules engine run
// inside the page over the rendered document, failing on every violation of
// WCAG 2.2 at levels A and AA — AA being the level the design system holds
// its contrast to (ARCHITECTURE.md § The design system: 4.5:1 for text).
//
// The engine is `accessibility-checker-engine` (ui/DEPENDENCIES.md), injected
// as an init script: the content security policy the binary sends refuses an
// inline script, and an init script is the browser's own, not the page's.
// Only definite violations fail; what the engine reports as needing review
// ("potential" or "manual") is a judgment, and a check that fails on a
// judgment cannot be green.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import type { BrowserContext, Page } from '@playwright/test';

const require = createRequire(import.meta.url);
const ENGINE = require.resolve('accessibility-checker-engine/ace.js');
const RULESET = 'WCAG_2_2';

/** What the engine reports of one rule on one element. */
export type Violation = { ruleId: string; reasonId: string; message: string; path: string };

/** Makes the engine available in every page the context opens. */
export async function withAccessibilityEngine(context: BrowserContext): Promise<void> {
  // The bundle declares `var ace` at its top level; an init script is run
  // inside a function, so the binding is handed to the page's global.
  await context.addInitScript({ content: `${readFileSync(ENGINE, 'utf8')}\nglobalThis.ace = ace;` });
}

/** Every definite violation of the ruleset on the page as it stands. */
export async function accessibilityViolations(page: Page): Promise<Violation[]> {
  // A colour read mid-transition — the theme applied after mount, a hover —
  // is no colour the page settles on: wait for every transition to end.
  // An infinite animation never ends, so it is not waited on; and the wait is
  // bounded, so a page that never settles fails here, naming why.
  await page.waitForFunction(
    () =>
      document.getAnimations().every((animation) => animation.playState !== 'running' || animation.effect?.getComputedTiming().iterations === Infinity),
    undefined,
    { timeout: 5_000 },
  );
  return page.evaluate(async (ruleset) => {
    type Result = { ruleId: string; reasonId: string; value: [string, string]; message: string; path: { dom: string } };
    type Engine = { Checker: new () => { check: (doc: Document, rulesets: string[]) => Promise<{ results: Result[] }> } };
    const engine = (globalThis as unknown as { ace?: Engine }).ace;
    if (engine === undefined) throw new Error('the accessibility engine was not injected');
    const report = await new engine.Checker().check(document, [ruleset]);
    return report.results
      .filter((result) => result.value[0] === 'VIOLATION' && result.value[1] === 'FAIL')
      .map((result) => ({ ruleId: result.ruleId, reasonId: result.reasonId, message: result.message, path: result.path.dom }));
  }, RULESET);
}

/**
 * The animations and transitions on the page that last long enough to be
 * seen. Under `prefers-reduced-motion` the design system collapses every
 * duration to 1 ms rather than to none (design/base.css), so a transition
 * still ends and fires its events; one that lasts longer is motion.
 */
export async function visibleAnimations(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    document
      .getAnimations()
      .filter((animation) => {
        const timing = animation.effect?.getComputedTiming();
        const duration = typeof timing?.duration === 'number' ? timing.duration : 0;
        return duration * (timing?.iterations ?? 1) > 1;
      })
      .map((animation) => {
        const target = (animation.effect as KeyframeEffect | null)?.target;
        const name = 'animationName' in animation ? String(animation.animationName) : 'transitionProperty' in animation ? String(animation.transitionProperty) : animation.id;
        return `${name} on ${target instanceof Element ? target.className || target.tagName : 'nothing'}`;
      }),
  );
}

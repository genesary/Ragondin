// Which value a token takes in each theme, read from the generated
// tokens.css: what a chart's "in both themes" test checks a mark's token
// against. Test-only; nothing in the bundle imports it.
import tokens from '../tokens.css?raw';
import { parseRules, selectors } from './css.ts';

export const THEMES = ['light', 'dark'] as const;
export type Theme = (typeof THEMES)[number];

/** The block a theme set on an element reads its colours from. */
const BLOCK: Record<Theme, string> = { light: '[data-theme="light"]', dark: '[data-theme="dark"]' };

/** The value `token` (with its `--`) takes under `data-theme="<theme>"`, or undefined when that theme does not define it. */
export function tokenIn(theme: Theme, token: string): string | undefined {
  return parseRules(tokens)
    .filter((rule) => rule.atRule === null && selectors(rule).includes(BLOCK[theme]))
    .map((rule) => rule.declarations.get(token))
    .find((value) => value !== undefined);
}

import { describe, expect, it } from 'vitest';
import base from './base.css?raw';
import { declared, parseRules, selectors } from './testing/css.ts';
import { THEMES, tokenIn } from './testing/themes.ts';

// The token a declaration names, or null when it names a value instead.
const tokenOf = (value: string | undefined) => /^var\((--[a-z0-9-]+)\)$/.exec(value ?? '')?.[1] ?? null;

describe('links, wherever a screen writes one bare', () => {
  it('take their ink and their underline from tokens, never the browser\'s blue and purple', () => {
    const ink = declared(base, 'a', 'color');
    const underline = declared(base, 'a', 'text-decoration-color');
    expect(tokenOf(ink)).not.toBeNull();
    expect(tokenOf(underline)).not.toBeNull();
    expect(declared(base, 'a', 'text-decoration-line')).toBe('underline');
    // Underlined, so a link in a line of text is told by its mark, not by its colour alone.
    for (const theme of THEMES) {
      expect(tokenIn(theme, tokenOf(ink)!), `${theme} ink`).toBeDefined();
      expect(tokenIn(theme, tokenOf(underline)!), `${theme} underline`).toBeDefined();
    }
  });

  it('yields to a component\'s own rule for its links: no colour on a selector a class could lose to', () => {
    // `a:visited` would outrank `.rg-workspace`, and a hash link is visited once followed.
    expect(parseRules(base).filter((r) => r.declarations.has('color') && selectors(r).some((s) => /^a\W/.test(s)))).toEqual([]);
    expect(declared(base, 'a:hover', 'text-decoration-color')).toBe('currentColor');
  });
});

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { parseRules, selectors, type Rule } from '../design/testing/css.ts';
import { renderTokensCss } from '../scripts/tokens.mjs';

const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8');
const css = read('../design/tokens.css');
const source = JSON.parse(read('../design/tokens.json'));
const rules = parseRules(css);

const LIGHT = ':root, [data-theme="light"]';
const DARK_MEDIA = '@media (prefers-color-scheme: dark)';

const lightBlock = () => rules.find((r) => r.atRule === null && r.selector === LIGHT);
const mediaDark = () => rules.find((r) => r.atRule === DARK_MEDIA && r.selector === ':root:not([data-theme="light"])');
const attrDark = () => rules.find((r) => r.atRule === null && r.selector === ':root[data-theme="dark"], [data-theme="dark"]');
const customProperties = (rule: Rule | undefined) => [...(rule?.declarations.keys() ?? [])].filter((k) => k.startsWith('--')).sort();

describe('tokens.css', () => {
  it('is generated from tokens.json and is current (npm run tokens regenerates it)', () => {
    expect(css).toBe(renderTokensCss(source));
  });

  it('defines the light palette on bare :root, the dark one under the media query and under [data-theme="dark"]', () => {
    expect(lightBlock()).toBeDefined();
    expect(mediaDark()).toBeDefined();
    expect(attrDark()).toBeDefined();
  });

  it('redefines in each dark block exactly the set the light block defines, no more and no fewer', () => {
    const light = customProperties(lightBlock());
    expect(light.length).toBeGreaterThan(40);
    expect(customProperties(mediaDark())).toEqual(light);
    expect(customProperties(attrDark())).toEqual(light);
  });

  it('sets color-scheme wherever a palette applies', () => {
    expect(lightBlock()?.declarations.get('color-scheme')).toBe('light');
    expect(mediaDark()?.declarations.get('color-scheme')).toBe('dark');
    expect(attrDark()?.declarations.get('color-scheme')).toBe('dark');
  });

  it('defines every token on bare :root before any media or [data-theme] block redefines it', () => {
    const seen = new Set<string>();
    for (const rule of rules) {
      const onRoot = rule.atRule === null && selectors(rule).includes(':root');
      for (const name of customProperties(rule)) {
        if (onRoot) seen.add(name);
        else expect(seen, `${name} is redefined in "${rule.selector}" before bare :root defines it`).toContain(name);
      }
    }
  });

  it('names no token by value (role names only)', () => {
    const names = [...new Set(rules.flatMap(customProperties))];
    expect(names.filter((n) => /gr[ae]y-?\d|blue|red|green|#|\d{3}$/.test(n))).toEqual([]);
  });
});

describe('the palettes are the design system’s values, in both themes', () => {
  const value = (rule: Rule | undefined, name: string) => rule?.declarations.get(name);
  const expectPair = (name: string, light: string, dark: string) => {
    expect(value(lightBlock(), `--${name}`)).toBe(light);
    expect(value(mediaDark(), `--${name}`)).toBe(dark);
    expect(value(attrDark(), `--${name}`)).toBe(dark);
  };

  it.each([
    ['family-retriever', '#47a9df', '#247dad'],
    ['family-fusion', '#dfa635', '#ad7c1d'],
    ['family-reranker', '#cd5ea2', '#9d3772'],
    ['family-context', '#5aca94', '#29996d'],
    ['family-generator', '#9777d5', '#6e4da7'],
    ['family-judge', '#bcba4e', '#8b8c27'],
    ['family-query', '#7c8088', '#82868e'],
  ])('node family pigment %s', expectPair);

  it.each([
    ['run-base', '#7c8088', '#82868e'],
    ['run-a', '#1161bb', '#5a94e0'],
    ['run-b', '#9a7c00', '#af8f15'],
    ['run-c', '#bf2f77', '#d45c91'],
    ['run-d', '#62359c', '#8470d6'],
  ])('run ink %s', expectPair);

  it.each([
    ['better', '#007277', '#50bebf'],
    ['worse', '#b83b07', '#f67d51'],
    ['good', '#1d7635', '#5fcc74'],
    ['warning', '#d79700', '#f2bf4e'],
    ['critical', '#b7162d', '#f57377'],
    ['accent', '#242e40', '#d5dfef'],
  ])('reserved pair %s', expectPair);

  it('raises the segmented thumb: the surface in light, line-strong in dark, so it reads above its track', () => {
    expectPair('seg-thumb', 'var(--surface)', 'var(--line-strong)');
  });

  it('draws the focus ring in the accent of the surface it sits on, in every theme block', () => {
    expectPair('focus-ring', 'var(--accent)', 'var(--accent)');
  });
});

describe('the scales', () => {
  const root = rules.find((r) => r.atRule === null && r.selector === ':root');
  const get = (name: string) => root?.declarations.get(`--${name}`);

  it('has twelve type styles, each a font shorthand on one of the three families', () => {
    const styles = [...(root?.declarations.keys() ?? [])].filter((k) => /^--type-[a-z]+$/.test(k));
    expect(styles).toHaveLength(12);
    for (const style of styles) expect(root?.declarations.get(style)).toMatch(/^\d{3} \d+px\/\d+px var\(--font-(display|sans|mono)\)$/);
  });

  it('gives each family a real fallback stack ending on a generic family', () => {
    expect(get('font-display')).toMatch(/^"Wix Madefor Display",.*system-ui.*sans-serif$/);
    expect(get('font-sans')).toMatch(/^"Wix Madefor Text",.*system-ui.*sans-serif$/);
    expect(get('font-mono')).toMatch(/^"Atkinson Hyperlegible Mono",.*ui-monospace.*monospace$/);
  });

  it('keeps every space on the 4 px grid (the 2 px surface gap aside)', () => {
    const spaces = [...(root?.declarations.entries() ?? [])].filter(([k]) => k.startsWith('--space-'));
    expect(spaces.length).toBeGreaterThanOrEqual(10);
    for (const [name, v] of spaces) if (name !== '--space-0-5') expect(Number.parseInt(v, 10) % 4, name).toBe(0);
    expect(get('space-0-5')).toBe('2px');
  });

  it.each([
    'radius-xs', 'radius-s', 'radius-track', 'radius-m', 'radius-l', 'radius-full',
    'duration-0', 'duration-press', 'duration-micro', 'duration-standard', 'duration-emphasis', 'duration-data',
    'ease-standard', 'ease-exit', 'ease-snap',
    'size-grid', 'size-grid-major', 'size-control', 'size-topbar', 'size-inspector', 'size-rank-cell',
  ])('defines %s', (name) => {
    expect(get(name)).toBeDefined();
  });

  it('carries the elevation model in both themes', () => {
    for (const name of ['shadow-contact', 'shadow-lift', 'shadow-float', 'shadow-modal']) {
      expect(lightBlock()?.declarations.get(`--${name}`)).toBeDefined();
      expect(attrDark()?.declarations.get(`--${name}`)).not.toBe(lightBlock()?.declarations.get(`--${name}`));
    }
  });

  it('collapses every duration but duration-0 under prefers-reduced-motion', () => {
    const reduced = rules.find((r) => r.atRule === '@media (prefers-reduced-motion: reduce)' && r.selector === ':root');
    expect(reduced?.declarations.get('--duration-0')).toBeUndefined();
    for (const name of ['press', 'micro', 'standard', 'emphasis', 'data']) expect(reduced?.declarations.get(`--duration-${name}`)).toBe('1ms');
  });
});

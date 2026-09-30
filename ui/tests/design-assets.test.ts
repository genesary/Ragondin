import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { declared, parseRules, stripComments } from '../design/testing/css.ts';

const UI = fileURLToPath(new URL('..', import.meta.url));
const DESIGN = join(UI, 'design');
const read = (path: string) => readFileSync(path, 'utf8');

/** Every file under `dir` whose extension is one of `exts`, test files and test helpers aside. */
function sources(dir: string, exts: string[]): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (name !== 'testing' && name !== 'node_modules') out.push(...sources(path, exts));
    } else if (exts.some((e) => name.endsWith(e)) && !/\.test\.[jt]sx?$/.test(name)) {
      out.push(path);
    }
  }
  return out;
}

const SCHEME_OR_HOST = /^\s*['"]?\s*(?:[a-z][a-z0-9+.-]*:|\/\/)/i;
/** The one absolute URL a committed file may carry: an SVG namespace, which nothing fetches. */
const NAMESPACE = /xmlns(?::\w+)?\s*=\s*["']http:\/\/www\.w3\.org\/[^"']*["']/g;

/** Every reference in `text` to a URL with a scheme or a host. */
function externalReferences(text: string, isCss: boolean): string[] {
  const found: string[] = [];
  const body = isCss ? stripComments(text) : text;
  if (isCss) {
    for (const m of body.matchAll(/url\(([^)]*)\)/gi)) if (SCHEME_OR_HOST.test(m[1] ?? '')) found.push(m[0]);
    for (const m of body.matchAll(/@import\s+(?!url\()([^;]+);/gi)) if (SCHEME_OR_HOST.test(m[1] ?? '')) found.push(m[0]);
  }
  const rest = body.replace(NAMESPACE, '');
  // Any scheme followed by `//`, whatever the host: a name, an IP, localhost.
  for (const m of rest.matchAll(/\b[a-z][a-z0-9+.-]*:\/\/[^\s'"()<>]*/gi)) found.push(m[0]);
  // A protocol-relative address: `//host` opening a quoted string or a url(). A
  // line comment's `// ` is not preceded by a quote or a parenthesis.
  for (const m of rest.matchAll(/["'(`]\s*\/\/[^\s/'"()<>][^\s'"()<>]*/g)) found.push(m[0]);
  return found;
}

describe('externalReferences', () => {
  it.each([
    ['@import url("https://fonts.example.com/css");', true],
    ["@import 'https://example.com/x.css';", true],
    ['src: url(//cdn.example.com/a.woff2);', true],
    ['src: url(https://example.com/a.woff2) format("woff2");', true],
    ['const href = "https://example.com/logo.svg";', false],
    ['<svg><image href="http://127.0.0.1/x.png"/></svg>', false],
    ["const api = 'http://localhost:8080/api';", false],
    ['<link href="//cdn/x.css">', false],
    ["const ws = 'ws://10.0.0.2:9000';", false],
    ['{ "logo": "https://example.com/l.svg" }', false],
  ])('flags %j', (text, isCss) => {
    expect(externalReferences(text, isCss)).not.toEqual([]);
  });

  it.each([
    ['src: url("./fonts/A.woff2") format("woff2");', true],
    ["@import './tokens.css';", true],
    ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>', false],
    ['/* see https://example.com in a comment */ .a { color: red; }', true],
    ['// a line comment, and a path: import x from "./a.ts";', false],
  ])('passes %j', (text, isCss) => {
    expect(externalReferences(text, isCss)).toEqual([]);
  });
});

describe('one origin: no stylesheet or component names another host (ADR-C36 § 5)', () => {
  const files = [...sources(DESIGN, ['.css', '.ts', '.tsx', '.html', '.svg', '.json']), ...sources(join(UI, 'src'), ['.css', '.ts', '.tsx']), join(UI, 'index.html')];

  it('scans the design system, the application and the entry page', () => {
    expect(files.some((f) => f.endsWith('fonts.css'))).toBe(true);
    expect(files.some((f) => f.endsWith('.tsx'))).toBe(true);
    expect(files.some((f) => f.endsWith('.svg'))).toBe(true);
    expect(files.some((f) => f.endsWith('tokens.json'))).toBe(true);
  });

  it.each(files.map((f) => [relative(UI, f), f]))('%s references no URL with a scheme or a host', (_name, path) => {
    expect(externalReferences(read(path), path.endsWith('.css'))).toEqual([]);
  });
});

describe('fonts.css', () => {
  const path = join(DESIGN, 'fonts.css');
  const css = existsSync(path) ? read(path) : '';

  it('declares a face for every committed font file, and only those', () => {
    const referenced = [...css.matchAll(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g)].map((m) => resolve(DESIGN, m[1] as string)).sort();
    const committed = readdirSync(join(DESIGN, 'fonts')).filter((f) => f.endsWith('.woff2')).map((f) => join(DESIGN, 'fonts', f)).sort();
    expect(committed.length).toBe(9);
    expect(referenced).toEqual(committed);
  });

  it('points every face at a file under ui/design/fonts/, as woff2, swapping in the fallback while it loads', () => {
    expect(css).toMatch(/@font-face/);
    for (const m of css.matchAll(/@font-face\s*\{([^}]*)\}/g)) {
      expect(m[1]).toMatch(/src:\s*url\("\.\/fonts\/[A-Za-z-]+\.woff2"\) format\("woff2"\);/);
      expect(m[1]).toMatch(/font-display:\s*swap;/);
    }
  });
});

describe('base.css', () => {
  const css = read(join(DESIGN, 'base.css'));

  it('pulls in the tokens and the faces, relatively', () => {
    expect(css).toMatch(/@import ['"]\.\/tokens\.css['"];/);
    expect(css).toMatch(/@import ['"]\.\/fonts\.css['"];/);
  });

  it('takes the body ground, ink and type from tokens, with tabular figures', () => {
    expect(declared(css, 'body', 'background-color')).toBe('var(--ground)');
    expect(declared(css, 'body', 'color')).toBe('var(--ink)');
    expect(declared(css, 'body', 'font')).toBe('var(--type-body)');
    expect(declared(css, 'body', 'font-variant-numeric')).toBe('tabular-nums');
  });

  it('repaints a themed subtree: its ground and its bare text take the theme it chose', () => {
    expect(declared(css, '[data-theme]', 'background-color')).toBe('var(--ground)');
    expect(declared(css, '[data-theme]', 'color')).toBe('var(--ink)');
  });

  it('draws a visible focus ring on everything that takes keyboard focus', () => {
    expect(declared(css, ':focus-visible', 'outline')).toBe('2px solid var(--focus-ring)');
    expect(declared(css, ':focus-visible', 'outline-offset')).toBe('2px');
  });

  it('honours prefers-reduced-motion: animations and transitions become instant', () => {
    const reduced = parseRules(css).filter((r) => r.atRule === '@media (prefers-reduced-motion: reduce)');
    const all = reduced.find((r) => r.selector.includes('*'));
    expect(all?.declarations.get('animation-duration')).toMatch(/^1ms/);
    expect(all?.declarations.get('transition-duration')).toMatch(/^1ms/);
  });

  it('resets box sizing and the default body margin', () => {
    expect(declared(css, '*', 'box-sizing')).toBe('border-box');
    expect(declared(css, 'body', 'margin')).toBe('0');
  });
});

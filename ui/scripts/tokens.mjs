// Renders ui/design/tokens.json, the design system's token source, into
// ui/design/tokens.css. tokens.json is the one place a colour, a type style, a
// space or a duration is written; tokens.css is derived from it and committed,
// and tests/design-tokens.test.ts fails when the two disagree. The theme
// mechanism this output implements is ui/ARCHITECTURE.md § The design system.

/**
 * @typedef {{ light: string, dark: string }} Themed
 * @typedef {{ name: string, value: string | Themed, usage?: string }} Token
 * @typedef {{ name: string, fontSize: string, lineHeight: string, fontWeight: number, letterSpacing?: string, usage?: string }} TypeStyle
 * @typedef {{
 *   color: { tokens: Token[] },
 *   type: { families: Record<string, string>, groups: { family: string, styles: TypeStyle[] }[] },
 *   spacing: { tokens: Token[] }, radius: { tokens: Token[] }, shadow: { tokens: Token[] },
 *   size: { tokens: Token[] }, duration: { tokens: Token[] }, easing: { tokens: Token[] },
 * }} Tokens
 */

const HEADER = `/* Generated from tokens.json by ui/scripts/gen-tokens.mjs (npm run tokens).
   Do not edit: change tokens.json and regenerate. Tokens are named by role,
   never by value, so a theme swaps values without touching a component. */
`;

/** `{accent}` in tokens.json is an alias; in CSS it is a reference. */
const cssValue = (/** @type {string} */ v) => v.replace(/^\{([a-z0-9-]+)\}$/, 'var(--$1)');

/** @param {Token} token */
const isThemed = (token) => typeof token.value === 'object';

/**
 * @param {Token} token
 * @param {'light' | 'dark'} theme
 */
function themedValue(token, theme) {
  const v = token.value;
  return cssValue(typeof v === 'string' ? v : v[theme]);
}

/**
 * @param {string[]} lines
 * @param {string} indent
 */
const block = (lines, indent) => lines.map((l) => `${indent}${l}`).join('\n');

/**
 * The declarations one theme sets: every colour token (an alias included, so
 * it resolves against the theme of the element it is declared on) and every
 * shadow.
 *
 * @param {Tokens} t
 * @param {'light' | 'dark'} theme
 * @param {boolean} withUsage
 */
function themeLines(t, theme, withUsage) {
  const lines = [`color-scheme: ${theme};`];
  for (const token of [...t.color.tokens, ...t.shadow.tokens]) {
    if (withUsage && token.usage) lines.push(`/* ${token.usage.replace(/\*\//g, '* /')} */`);
    lines.push(`--${token.name}: ${themedValue(token, theme)};`);
  }
  return lines;
}

/**
 * @param {Tokens} t
 * @returns {string}
 */
export function renderTokensCss(t) {
  /** @type {string[]} */
  const root = ['/* Type: three families, twelve styles. */'];
  for (const [family, stack] of Object.entries(t.type.families)) root.push(`--font-${family}: ${stack};`);
  for (const group of t.type.groups) {
    for (const s of group.styles) {
      root.push(`--${s.name}: ${s.fontWeight} ${s.fontSize}/${s.lineHeight} var(--font-${group.family});`);
      root.push(`--${s.name}-tracking: ${s.letterSpacing ?? 'normal'};`);
    }
  }
  /** @type {[string, Token[]][]} */
  const scales = [
    ['Space: the 4 px grid.', t.spacing.tokens],
    ['Radius: concentric.', t.radius.tokens],
    ['Size: controls and canvas geometry.', t.size.tokens],
    ['Motion: durations.', t.duration.tokens],
    ['Motion: easings.', t.easing.tokens],
  ];
  for (const [title, tokens] of scales) {
    root.push(`/* ${title} */`);
    for (const token of tokens) {
      if (isThemed(token)) throw new Error(`${token.name} is themed; only colours and shadows may be`);
      root.push(`--${token.name}: ${cssValue(/** @type {string} */ (token.value))};`);
    }
  }
  const reduced = t.duration.tokens.filter((d) => d.value !== '0ms').map((d) => `--${d.name}: 1ms;`);

  return [
    HEADER,
    '/* Theme-independent tokens. */',
    `:root {\n${block(root, '  ')}\n}`,
    '',
    '/* Light: the complete palette on bare :root, so no token exists only behind a theme block. */',
    `:root,\n[data-theme="light"] {\n${block(themeLines(t, 'light', true), '  ')}\n}`,
    '',
    '/* Dark, when the system asks for it and the page has not chosen light. */',
    `@media (prefers-color-scheme: dark) {\n  :root:not([data-theme="light"]) {\n${block(themeLines(t, 'dark', false), '    ')}\n  }\n}`,
    '',
    '/* Dark, when the page or a subtree chooses it. */',
    `:root[data-theme="dark"],\n[data-theme="dark"] {\n${block(themeLines(t, 'dark', false), '  ')}\n}`,
    '',
    '/* Motion answers an action; under reduced motion it becomes instant. */',
    `@media (prefers-reduced-motion: reduce) {\n  :root {\n${block(reduced, '    ')}\n  }\n}`,
    '',
  ].join('\n');
}

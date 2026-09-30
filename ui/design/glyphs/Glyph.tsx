import type { SVGProps } from 'react';
import './glyphs.css';

/**
 * The node families, in the design system's fixed slot order. Query and
 * control flow take the neutral pigment; the others take a slot each.
 */
export const FAMILIES = ['query', 'retriever', 'fusion', 'reranker', 'context', 'generator', 'judge', 'control'] as const;
export type Family = (typeof FAMILIES)[number];

/** A family as a person reads it: the name that sits beside its pigment. */
export const FAMILY_LABEL: Record<Family, string> = {
  query: 'query',
  retriever: 'retriever',
  fusion: 'fusion',
  reranker: 'reranker',
  context: 'context builder',
  generator: 'generator',
  judge: 'judge',
  control: 'control flow',
};

/** Every glyph, one SVG file each beside this module (the test holds the two lists equal). */
export const GLYPH_NAMES = [
  ...FAMILIES,
  'check', 'cross', 'alert', 'clock', 'up', 'down', 'chevron', 'close', 'undo', 'redo', 'fit', 'plus', 'minus',
  'copy', 'play', 'download', 'sun', 'star', 'split', 'prefix', 'link', 'more',
] as const;
export type GlyphName = (typeof GLYPH_NAMES)[number];

// The files are the drawings; this reads them at build time, so the bundle
// carries each once and nothing is fetched.
const files = import.meta.glob<string>('./*.svg', { query: '?raw', import: 'default', eager: true });

/** The markup inside a file's root `<svg>`: the drawing without its frame. */
function drawing(name: GlyphName): string {
  const raw = files[`./${name}.svg`] ?? '';
  return raw.slice(raw.indexOf('>') + 1, raw.lastIndexOf('</svg>'));
}

export type GlyphProps = {
  name: GlyphName;
  /** Give one when the glyph is the only content of a control; otherwise it is decorative. */
  label?: string;
} & Omit<SVGProps<SVGSVGElement>, 'name' | 'children' | 'dangerouslySetInnerHTML'>;

/**
 * One glyph, inline, 16px on a 16px grid, stroked in `currentColor` so it takes
 * the ink of its context.
 */
export function Glyph({ name, label, className, ...rest }: GlyphProps) {
  const a11y = label === undefined ? { 'aria-hidden': true } : { role: 'img', 'aria-label': label };
  return (
    <svg
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      className={className === undefined ? 'rg-glyph' : `rg-glyph ${className}`}
      {...a11y}
      {...rest}
      dangerouslySetInnerHTML={{ __html: drawing(name) }}
    />
  );
}

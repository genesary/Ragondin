/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { FAMILIES, FAMILY_LABEL, GLYPH_NAMES, Glyph } from './Glyph.tsx';

const files = Object.keys(import.meta.glob('./*.svg')).map((p) => p.replace(/^\.\/(.*)\.svg$/, '$1'));

describe('the glyph set', () => {
  it('has one SVG file per glyph name, and a name for every file', () => {
    expect([...GLYPH_NAMES].sort()).toEqual(files.sort());
  });

  it('includes a glyph for every node family, control flow and query among them', () => {
    for (const family of FAMILIES) expect(GLYPH_NAMES).toContain(family);
    expect(FAMILY_LABEL.context).toBe('context builder');
    expect(FAMILY_LABEL.control).toBe('control flow');
  });
});

describe('Glyph', () => {
  it('renders the named drawing inline, 16px, in currentColor', () => {
    const { container } = render(<Glyph name="retriever" />);
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('viewBox')).toBe('0 0 16 16');
    expect(svg?.getAttribute('stroke')).toBe('currentColor');
    expect(svg?.getAttribute('width')).toBe('16');
    expect(svg?.querySelector('path')?.getAttribute('d')).toBe('M3 4.5h10M3 8h7M3 11.5h4');
  });

  it('is decorative by default, hidden from assistive technology', () => {
    const { container } = render(<Glyph name="check" />);
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
    expect(svg?.getAttribute('role')).toBeNull();
  });

  it('is an image with a name when it is the only content of a control', () => {
    const { getByRole } = render(<Glyph name="close" label="Close inspector" />);
    expect(getByRole('img', { name: 'Close inspector' })).toBeTruthy();
  });

  it('keeps a drawing’s own fill, as the star does', () => {
    const { container } = render(<Glyph name="star" />);
    expect(container.querySelector('path')?.getAttribute('fill')).toBe('currentColor');
  });
});

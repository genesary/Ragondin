/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import css from './glyphs.css?raw';
import { FamilyTile } from './FamilyTile.tsx';
import { FAMILIES } from './Glyph.tsx';

describe('FamilyTile', () => {
  it.each(FAMILIES.filter((f) => f !== 'control' && f !== 'query'))('paints %s in its pigment and carries its glyph, never the pigment alone', (family) => {
    const { container } = render(<FamilyTile family={family} />);
    const tile = container.querySelector('.rg-tile');
    expect(tile?.getAttribute('data-family')).toBe(family);
    expect(declared(css, `.rg-tile[data-family="${family}"]`, '--fam')).toBe(`var(--family-${family})`);
    expect(tile?.querySelector('svg path')).toBeTruthy();
  });

  it('gives query and control flow the neutral pigment; control flow is told apart by its diamond', () => {
    expect(declared(css, '.rg-tile[data-family="query"]', '--fam')).toBe('var(--family-query)');
    expect(declared(css, '.rg-tile[data-family="control"]', '--fam')).toBe('var(--family-query)');
    expect(declared(css, '.rg-tile[data-family="control"]', 'transform')).toBe('rotate(45deg) scale(0.82)');
  });

  it('draws the glyph in on-family ink', () => {
    expect(declared(css, '.rg-tile', 'color')).toBe('var(--on-family)');
  });

  it('names the family for assistive technology when asked to', () => {
    const { getByRole } = render(<FamilyTile family="context" labelled />);
    expect(getByRole('img', { name: 'context builder' })).toBeTruthy();
  });
});

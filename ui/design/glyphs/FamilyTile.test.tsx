/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import css from './glyphs.css?raw';
import { FamilyTile, familyOfComponent } from './FamilyTile.tsx';
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

  it('draws the glyph in the ink made for its own family\'s pigment', () => {
    expect(declared(css, '.rg-tile', 'color')).toBe('var(--fam-ink)');
    for (const family of FAMILIES.filter((f) => f !== 'control')) {
      expect(declared(css, `.rg-tile[data-family="${family}"]`, '--fam-ink')).toBe(`var(--on-family-${family})`);
    }
    // Control flow takes the neutral pigment, and with it the neutral pigment's ink.
    expect(declared(css, '.rg-tile', '--fam-ink')).toBe('var(--on-family-query)');
    expect(declared(css, '.rg-tile[data-family="control"]', '--fam-ink')).toBe('var(--on-family-query)');
  });

  it('names the family for assistive technology when asked to', () => {
    const { getByRole } = render(<FamilyTile family="context" labelled />);
    expect(getByRole('img', { name: 'context builder' })).toBeTruthy();
  });
});

describe('familyOfComponent', () => {
  it.each([
    ['retriever', 'retriever'],
    ['fusion', 'fusion'],
    ['reranker', 'reranker'],
    ['context_builder', 'context'],
    ['generator', 'generator'],
  ] as const)('draws a configuration’s `%s` node with the %s tile', (component, family) => {
    expect(familyOfComponent(component)).toBe(family);
  });

  it('draws no tile for an extension node or a family it does not know, so the caller writes its word', () => {
    expect(familyOfComponent('extension')).toBeNull();
    expect(familyOfComponent('embedder')).toBeNull();
  });
});

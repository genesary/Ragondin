/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './PrefixLabel.css?raw';
import { PrefixLabel, prefixWords } from './PrefixLabel.tsx';

describe('PrefixLabel', () => {
  it('names the parent and the node the prefix stops at, in words, beside the prefix glyph', () => {
    const { container } = render(<PrefixLabel parents={['hybrid-rerank-gen']} upTo="rerank" />);
    const label = container.querySelector('.rg-prefix') as HTMLElement;
    expect(label.textContent).toBe('prefix of hybrid-rerank-gen, up to rerank');
    // The glyph is decoration: the words carry the relation.
    expect(label.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('says only the node when no parent is named', () => {
    expect(prefixWords([], 'rrf')).toBe('prefix up to rrf');
    const { container } = render(<PrefixLabel parents={[]} upTo="rrf" />);
    expect(container.textContent).toBe('prefix up to rrf');
  });

  it('reads several parents as a sentence: two by name, more as the first and a count', () => {
    expect(prefixWords(['a', 'b'], 'x')).toBe('prefix of a and b, up to x');
    expect(prefixWords(['a', 'b', 'c'], 'x')).toBe('prefix of a (and 2 others), up to x');
    expect(prefixWords(['a', 'b', 'c', 'd'], 'x')).toBe('prefix of a (and 3 others), up to x');
  });

  it('names every parent it counts, for a pointer and for assistive technology', () => {
    const { container } = render(<PrefixLabel parents={['a', 'b', 'c']} upTo="x" />);
    const label = container.querySelector('.rg-prefix') as HTMLElement;
    expect(label.getAttribute('title')).toBe('prefix of a, b and c, up to x');
    expect(label.querySelector('.rg-visually-hidden')?.textContent).toBe(': b and c');
  });

  it('is quiet caption text in the third ink, the glyph in line with it', () => {
    expect(declared(css, '.rg-prefix', 'font')).toBe('var(--type-caption)');
    expect(declared(css, '.rg-prefix', 'color')).toBe('var(--ink-3)');
    expect(declared(css, '.rg-prefix', 'display')).toBe('inline-flex');
  });
});

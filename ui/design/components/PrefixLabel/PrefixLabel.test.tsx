/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './PrefixLabel.css?raw';
import { PrefixLabel, prefixWords } from './PrefixLabel.tsx';

describe('PrefixLabel', () => {
  it('names the parent and the node the prefix stops at, in words, beside the prefix glyph', () => {
    const { container } = render(<PrefixLabel parent="hybrid-rerank-gen" upTo="rerank" />);
    const label = container.querySelector('.rg-prefix') as HTMLElement;
    expect(label.textContent).toBe('prefix of hybrid-rerank-gen, up to rerank');
    // The glyph is decoration: the words carry the relation.
    expect(label.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('says only the node when no parent is named', () => {
    expect(prefixWords(null, 'rrf')).toBe('prefix up to rrf');
    const { container } = render(<PrefixLabel parent={null} upTo="rrf" />);
    expect(container.textContent).toBe('prefix up to rrf');
  });

  it('is quiet caption text in the third ink, the glyph in line with it', () => {
    expect(declared(css, '.rg-prefix', 'font')).toBe('var(--type-caption)');
    expect(declared(css, '.rg-prefix', 'color')).toBe('var(--ink-3)');
    expect(declared(css, '.rg-prefix', 'display')).toBe('inline-flex');
  });
});

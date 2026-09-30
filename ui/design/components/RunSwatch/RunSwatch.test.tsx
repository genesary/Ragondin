/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './RunSwatch.css?raw';
import { RunSwatch, type RunSlot } from './RunSwatch.tsx';

const HASH = '9e2b7d41c0a3f5e6';

describe.each([
  ['a', 'A', 'var(--run-a)', 'var(--on-run)'],
  ['b', 'B', 'var(--run-b)', 'var(--on-run-b)'],
  ['c', 'C', 'var(--run-c)', 'var(--on-run)'],
  ['d', 'D', 'var(--run-d)', 'var(--on-run)'],
] as [RunSlot, string, string, string][])('RunSwatch run %s', (slot, letter, fill, ink) => {
  it(`carries the letter ${letter} on its ink, beside the name and hash`, () => {
    const { container } = render(<RunSwatch slot={slot} name="hybrid-rerank" hash={HASH} />);
    const swatch = container.querySelector('.rg-swatch') as HTMLElement;
    expect(swatch.dataset.run).toBe(slot);
    expect(swatch.textContent).toBe(letter);
    expect(screen.getByText('hybrid-rerank')).toBeTruthy();
    expect(declared(css, `.rg-swatch[data-run="${slot}"]`, '--run')).toBe(fill);
    expect(declared(css, `.rg-swatch[data-run="${slot}"]`, '--on') ?? declared(css, '.rg-swatch', '--on')).toBe(ink);
  });
});

describe('RunSwatch baseline', () => {
  it('is the dashed neutral outline, and says "baseline" rather than a letter', () => {
    const { container } = render(<RunSwatch slot="base" name="dense-only" hash={HASH} />);
    const swatch = container.querySelector('.rg-swatch') as HTMLElement;
    expect(swatch.dataset.run).toBe('base');
    expect(swatch.textContent).toBe('baseline');
    expect(declared(css, '.rg-swatch[data-run="base"]', 'border')).toBe('1.5px dashed var(--run-base)');
  });
});

describe('RunSwatch hash label', () => {
  it('shows six characters in mono, the whole hash on hover, and copies the whole hash', () => {
    const onCopyHash = vi.fn();
    render(<RunSwatch slot="a" name="hybrid-rerank" hash={HASH} onCopyHash={onCopyHash} />);
    const hash = screen.getByRole('button', { name: `Copy run hash ${HASH}` });
    expect(hash.textContent).toBe('9e2b7d');
    expect(hash.getAttribute('title')).toBe(HASH);
    fireEvent.click(hash);
    expect(onCopyHash).toHaveBeenCalledWith(HASH);
    expect(declared(css, '.rg-hash', 'font')).toBe('var(--type-hash)');
  });
});

describe('RunSwatch small', () => {
  it('drops the letter from sight at 10px but keeps it for assistive technology, with the name beside it', () => {
    const { container } = render(<RunSwatch slot="c" name="bm25" small />);
    const swatch = container.querySelector('.rg-swatch') as HTMLElement;
    expect(swatch.classList.contains('rg-swatch--s')).toBe(true);
    expect(swatch.querySelector('.rg-visually-hidden')?.textContent).toBe('C');
    expect(screen.getByText('bm25')).toBeTruthy();
  });
});

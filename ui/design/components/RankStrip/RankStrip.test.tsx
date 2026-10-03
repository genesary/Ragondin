/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './RankStrip.css?raw';
import { RankStrip, rankSentence } from './RankStrip.tsx';

const cells = (el: HTMLElement) => [...el.querySelectorAll('.rg-rankstrip > i')];

describe('RankStrip', () => {
  it('renders ten cells', () => {
    const { container } = render(<RankStrip hits={[]} />);
    expect(cells(container)).toHaveLength(10);
  });

  it('fills exactly the ranks it is given, in rank order', () => {
    const { container } = render(<RankStrip hits={[7, 1, 2]} />);
    const filled = cells(container).map((c, i) => (c.getAttribute('data-cell') === 'hit' ? i + 1 : null)).filter((r) => r !== null);
    expect(filled).toEqual([1, 2, 7]);
  });

  it('ignores ranks outside the top ten', () => {
    const { container } = render(<RankStrip hits={[0, 11, 3, 3]} />);
    expect(cells(container).filter((c) => c.getAttribute('data-cell') === 'hit')).toHaveLength(1);
  });

  it('is filled versus hollow, not a colour: the hit is solid accent, the miss an outline', () => {
    expect(declared(css, '.rg-rankstrip > i[data-cell="hit"]', 'background')).toBe('var(--accent)');
    expect(css).not.toMatch(/\.is-[a-z]/);
    expect(declared(css, '.rg-rankstrip > i', 'box-shadow')).toBe('inset 0 0 0 1px var(--line-control)');
  });

  it('fades the ranks past the cut-off', () => {
    const { container } = render(<RankStrip hits={[2]} cut={5} />);
    const cut = cells(container).map((c, i) => (c.getAttribute('data-cell') === 'cut' ? i + 1 : null)).filter((r) => r !== null);
    expect(cut).toEqual([6, 7, 8, 9, 10]);
  });

  it('says where the gold went, as its accessible name and its tooltip', () => {
    render(<RankStrip hits={[2, 1]} />);
    const strip = screen.getByRole('img', { name: '2 gold passages in the top 10, at rank 1, 2' });
    expect(strip.getAttribute('title')).toBe('2 gold passages in the top 10, at rank 1, 2');
  });

  it('says so when no gold passage reached the top ten', () => {
    render(<RankStrip hits={[]} />);
    expect(screen.getByRole('img', { name: '0 gold passages in the top 10' })).toBeTruthy();
  });

  it('grows to 12px cells in its large form', () => {
    const { container } = render(<RankStrip hits={[1]} large />);
    expect(container.querySelector('.rg-rankstrip--l')).toBeTruthy();
    expect(declared(css, '.rg-rankstrip--l > i', 'width')).toBe('12px');
  });

  it('says its sentence through one function, so a node described elsewhere reads the same words', () => {
    expect(rankSentence([3, 1, 1, 12])).toBe('2 gold passages in the top 10, at rank 1, 3');
    expect(rankSentence([2])).toBe('1 gold passage in the top 10, at rank 2');
    expect(rankSentence([])).toBe('0 gold passages in the top 10');
    render(<RankStrip hits={[3, 1]} />);
    expect(screen.getByRole('img').getAttribute('aria-label')).toBe(rankSentence([3, 1]));
  });
});

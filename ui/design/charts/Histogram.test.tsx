/** @vitest-environment happy-dom */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../testing/css.ts';
import { THEMES, tokenIn } from '../testing/themes.ts';
import css from './Charts.css?raw';
import { Histogram, type HistogramProps } from './Histogram.tsx';

const BINS: HistogramProps['bins'] = [
  { id: 'much_worse', label: 'much worse', range: 'below −0.3', count: 6, tone: 'worse' },
  { id: 'worse', label: 'worse', range: '−0.3 to −0.1', count: 20, tone: 'worse' },
  { id: 'slightly_worse', label: 'slightly worse', range: '−0.1 to 0', count: 35, tone: 'worse' },
  { id: 'unchanged', label: 'unchanged', range: '0', count: 108, tone: 'zero' },
  { id: 'slightly_better', label: 'slightly better', range: '0 to 0.1', count: 54, tone: 'better' },
  { id: 'better', label: 'better', range: '0.1 to 0.3', count: 0, tone: 'better' },
  { id: 'much_better', label: 'much better', range: 'above 0.3', count: 77, tone: 'better' },
];

const props = (over: Partial<HistogramProps> = {}): HistogramProps => ({
  label: 'Per-query change in ndcg@10, A against the baseline',
  bins: BINS,
  halves: { worse: 'worse', better: 'better' },
  active: null,
  onActivate: () => {},
  controls: 'list',
  ...over,
});

describe.each(THEMES)('Histogram, %s theme', (theme) => {
  it('draws one bar per bin, its height its count over the largest, with the count written on it', () => {
    const { container } = render(
      <div data-theme={theme}>
        <Histogram {...props()} />
      </div>,
    );
    const bars = [...container.querySelectorAll<HTMLElement>('.rg-hist__bar')];
    expect(bars).toHaveLength(7);
    const expected = [6, 20, 35, 108, 54, 0, 77].map((n) => (n / 108) * 100);
    bars.forEach((b, i) => {
      expect(b.style.height.endsWith('%')).toBe(true);
      expect(Number.parseFloat(b.style.height)).toBeCloseTo(expected[i] as number, 6);
    });
    expect([...container.querySelectorAll('.rg-hist__count')].map((c) => c.textContent)).toEqual(['6', '20', '35', '108', '54', '0', '77']);
    expect(bars.map((b) => b.dataset.tone)).toEqual(['worse', 'worse', 'worse', 'zero', 'better', 'better', 'better']);
  });

  it('colours the arms from the reserved better and worse tokens, the middle neutral', () => {
    expect(declared(css, '.rg-hist__bar[data-tone="worse"]', 'background')).toBe('var(--worse)');
    expect(declared(css, '.rg-hist__bar[data-tone="better"]', 'background')).toBe('var(--better)');
    expect(declared(css, '.rg-hist__bar[data-tone="zero"]', 'background')).toBe('var(--delta-zero)');
    for (const token of ['--worse', '--better', '--delta-zero']) expect(tokenIn(theme, token)).toMatch(/^#/);
  });
});

describe('Histogram', () => {
  it('makes each bar a button named by its bin, its range and its count — its hover target the whole column', () => {
    render(<Histogram {...props()} />);
    const group = screen.getByRole('group', { name: 'Per-query change in ndcg@10, A against the baseline' });
    const buttons = within(group).getAllByRole('button');
    expect(buttons.map((b) => b.getAttribute('aria-label'))).toEqual([
      'much worse, below −0.3: 6 queries',
      'worse, −0.3 to −0.1: 20 queries',
      'slightly worse, −0.1 to 0: 35 queries',
      'unchanged, 0: 108 queries',
      'slightly better, 0 to 0.1: 54 queries',
      'better, 0.1 to 0.3: 0 queries',
      'much better, above 0.3: 77 queries',
    ]);
    expect(buttons[0]?.querySelector('.rg-hist__bar')).toBeTruthy();
    expect(declared(css, '.rg-hist__col', 'height')).toBe('100%');
  });

  it('writes the two halves under the axis in words, never colour alone', () => {
    const { container } = render(<Histogram {...props()} />);
    expect([...container.querySelectorAll('.rg-hist__half')].map((h) => h.textContent)).toEqual(['worse', 'better']);
  });

  it('is one tab stop, moved along by the arrow keys, Home and End', () => {
    render(<Histogram {...props()} />);
    const buttons = screen.getAllByRole('button');
    expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1, -1, -1, -1, -1, -1]);
    (buttons[0] as HTMLElement).focus();
    fireEvent.keyDown(buttons[0] as HTMLElement, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(buttons[1]);
    fireEvent.keyDown(buttons[1] as HTMLElement, { key: 'End' });
    expect(document.activeElement).toBe(buttons[6]);
    fireEvent.keyDown(buttons[6] as HTMLElement, { key: 'Home' });
    expect(document.activeElement).toBe(buttons[0]);
  });

  it('opens a bin on click — Enter and Space are a button\'s click — and marks it expanded, controlling the list', () => {
    const onActivate = vi.fn();
    const { rerender } = render(<Histogram {...props({ onActivate })} />);
    fireEvent.click(screen.getByRole('button', { name: /^worse,/ }));
    expect(onActivate).toHaveBeenCalledWith('worse');
    rerender(<Histogram {...props({ onActivate, active: 'worse' })} />);
    const open = screen.getByRole('button', { name: /^worse,/ });
    expect(open.getAttribute('aria-expanded')).toBe('true');
    expect(open.getAttribute('aria-controls')).toBe('list');
    expect(open.tabIndex).toBe(0);
    expect(screen.getByRole('button', { name: /^much worse,/ }).getAttribute('aria-expanded')).toBe('false');
  });
});

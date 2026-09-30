/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './Progress.css?raw';
import { Progress } from './Progress.tsx';

const fill = (el: HTMLElement) => el.querySelector('.rg-progress__fill') as HTMLElement | null;

describe('Progress has no indeterminate variant', () => {
  it('refuses to render without a value', () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    // @ts-expect-error value is required: there is nothing to draw without one.
    expect(() => render(<Progress state="running" total={300} label="running" />)).toThrow(/value/);
    expect(() => render(<Progress state="running" value={Number.NaN} total={300} label="running" />)).toThrow(/value/);
    expect(() => render(<Progress state="running" value={3} total={0} label="running" />)).toThrow(/total/);
    quiet.mockRestore();
  });
});

describe('Progress queued', () => {
  it('draws a hatch and no fill, because nothing has happened, and says "queued"', () => {
    const { container } = render(<Progress state="queued" value={0} total={10570} label="Queued, 2 runs ahead" />);
    const bar = screen.getByRole('progressbar');
    expect(bar.getAttribute('aria-valuetext')).toBe('queued, 0 of 10,570');
    expect(fill(container)).toBeNull();
    expect(declared(css, '.rg-progress[data-state="queued"] .rg-progress__track', 'background')).toMatch(/^repeating-linear-gradient/);
  });
});

describe('Progress running', () => {
  it('fills to the counted fraction and reads the count', () => {
    const { container } = render(<Progress state="running" value={3982} total={10570} label="3,982 / 10,570 questions" detail="about 6 min left" />);
    const bar = screen.getByRole('progressbar');
    expect(bar.getAttribute('aria-valuenow')).toBe('3982');
    expect(bar.getAttribute('aria-valuemax')).toBe('10570');
    expect(bar.getAttribute('aria-valuetext')).toBe('running, 3,982 of 10,570');
    expect(fill(container)?.style.width).toBe('37.67%');
    expect(screen.getByText('about 6 min left')).toBeTruthy();
    expect(declared(css, '.rg-progress__fill', 'background')).toBe('var(--accent)');
  });
});

describe('Progress done', () => {
  it('ends on its result, fills in good, and says "done"', () => {
    const { container } = render(<Progress state="done" value={300} total={300} label="Done in 9 min 12 s. nDCG@10 0.7217" />);
    expect(screen.getByRole('progressbar').getAttribute('aria-valuetext')).toBe('done, 300 of 300');
    expect(fill(container)?.style.width).toBe('100%');
    expect(screen.getByText('Done in 9 min 12 s. nDCG@10 0.7217')).toBeTruthy();
    expect(declared(css, '.rg-progress[data-state="done"] .rg-progress__fill', 'background')).toBe('var(--good)');
  });
});

describe('Progress failed', () => {
  it('fills to where it stopped, in critical, and says "failed" and where', () => {
    const { container } = render(<Progress state="failed" value={1} total={300} label="Failed at rerank, query 1 of 300" />);
    expect(screen.getByRole('progressbar').getAttribute('aria-valuetext')).toBe('failed, 1 of 300');
    expect(fill(container)?.style.width).toBe('0.33%');
    expect(declared(css, '.rg-progress[data-state="failed"] .rg-progress__fill', 'background')).toBe('var(--critical)');
  });
});

describe('Progress accessibility', () => {
  it('names the bar by its count and keeps the action reachable outside it', () => {
    render(<Progress state="done" value={300} total={300} label="Done in 9 min 12 s" action={<button type="button">Compare</button>} />);
    const bar = screen.getByRole('progressbar', { name: 'Done in 9 min 12 s' });
    expect(bar.classList.contains('rg-progress__track')).toBe(true);
    const action = screen.getByRole('button', { name: 'Compare' });
    expect(bar.contains(action)).toBe(false);
    expect(action.closest('[role="progressbar"]')).toBeNull();
  });
});

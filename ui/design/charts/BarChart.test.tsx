/** @vitest-environment happy-dom */
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import { THEMES, tokenIn } from '../testing/themes.ts';
import { BarChart, type BarChartProps } from './BarChart.tsx';
import css from './Charts.css?raw';
import { PLOT } from './scale.ts';

const INNER = PLOT.height - PLOT.top - PLOT.bottom;

const PROPS: BarChartProps = {
  label: 'Each metric per run',
  groups: [
    { id: 'ndcg@10', label: 'ndcg@10' },
    { id: 'recall@100', label: 'recall@100' },
  ],
  series: [
    { id: 'r1', label: 'baseline', short: 'base', ink: 'base' },
    { id: 'r2', label: 'A', short: 'A', ink: 'a' },
  ],
  values: [
    [0.5, 0.75],
    [0.25, null],
  ],
  domain: [0, 1],
  format: (v) => v.toFixed(4),
  best: (g, s) => (g === 0 && s === 1) || (g === 1 && s === 0),
};

const bars = (root: ParentNode) => [...root.querySelectorAll<SVGRectElement>('rect.rg-chart__bar')];
const num = (el: Element, attr: string) => Number(el.getAttribute(attr));

describe.each(THEMES)('BarChart, %s theme', (theme) => {
  it('draws one bar per recorded value on one scale, its height the value over the domain', () => {
    const { container } = render(
      <div data-theme={theme}>
        <BarChart {...PROPS} />
      </div>,
    );
    const drawn = bars(container);
    // Three values recorded; the missing one draws no bar rather than a zero.
    expect(drawn.map((b) => b.dataset.ink)).toEqual(['base', 'a', 'base']);
    expect(num(drawn[0] as Element, 'height')).toBeCloseTo(INNER * 0.5);
    expect(num(drawn[1] as Element, 'height')).toBeCloseTo(INNER * 0.75);
    expect(num(drawn[2] as Element, 'height')).toBeCloseTo(INNER * 0.25);
    // Every bar stands on the one axis.
    for (const b of drawn) expect(num(b, 'y') + num(b, 'height')).toBeCloseTo(PLOT.height - PLOT.bottom);
  });

  it('fills each bar from its run ink, a token this theme defines', () => {
    expect(declared(css, '.rg-chart__bar[data-ink="a"]', 'fill')).toBe('var(--run-a)');
    expect(tokenIn(theme, '--run-a')).toMatch(/^#/);
    // The baseline is a dashed neutral outline, never a filled contender.
    expect(declared(css, '.rg-chart__bar[data-ink="base"]', 'fill')).toBe('none');
    expect(declared(css, '.rg-chart__bar[data-ink="base"]', 'stroke')).toBe('var(--run-base)');
    expect(declared(css, '.rg-chart__bar[data-ink="base"]', 'stroke-dasharray')).toBeDefined();
    expect(tokenIn(theme, '--run-base')).toMatch(/^#/);
  });
});

describe('BarChart', () => {
  it('has one value axis, ticked across its domain', () => {
    const { container } = render(<BarChart {...PROPS} />);
    expect(container.querySelectorAll('.rg-chart__axis')).toHaveLength(1);
    expect([...container.querySelectorAll('.rg-chart__tick')].map((t) => t.textContent)).toEqual(['0', '0.2', '0.4', '0.6', '0.8', '1']);
  });

  it('labels the best value of each group with a star and the number, in ink', () => {
    const { container } = render(<BarChart {...PROPS} />);
    const best = [...container.querySelectorAll('.rg-chart__best')].map((t) => t.textContent);
    expect(best).toEqual(['★ 0.7500', '★ 0.2500']);
  });

  it('writes each bar\'s letter under it, so a run is never told by colour alone', () => {
    const { container } = render(<BarChart {...PROPS} />);
    expect([...container.querySelectorAll('.rg-chart__letter')].map((t) => t.textContent)).toEqual(['base', 'A', 'base', 'A']);
    expect([...container.querySelectorAll('.rg-chart__group')].map((t) => t.textContent)).toEqual(['ndcg@10', 'recall@100']);
  });

  it('hovers a whole group, wider than its bars, and shows each run\'s value — "not recorded" for a gap', () => {
    const { container } = render(<BarChart {...PROPS} />);
    const hits = [...container.querySelectorAll<SVGRectElement>('rect.rg-chart__hit')];
    expect(hits).toHaveLength(2);
    const bar = bars(container)[0] as Element;
    expect(num(hits[0] as Element, 'width')).toBeGreaterThan(num(bar, 'width'));
    fireEvent.pointerEnter(hits[1] as Element);
    const tip = container.querySelector('.rg-chart__tip') as HTMLElement;
    expect(tip.textContent).toBe('recall@100baseline0.2500Anot recorded');
    fireEvent.pointerLeave(hits[1] as Element);
    expect(container.querySelector('.rg-chart__tip')).toBeNull();
  });

  it('hides the drawing from assistive technology: the table is its equivalent', () => {
    const { container } = render(<BarChart {...PROPS} />);
    expect(container.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });
});

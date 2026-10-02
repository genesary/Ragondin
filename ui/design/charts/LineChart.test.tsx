/** @vitest-environment happy-dom */
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import { THEMES, tokenIn } from '../testing/themes.ts';
import css from './Charts.css?raw';
import { LineChart, type LineChartProps } from './LineChart.tsx';
import { linear, PLOT } from './scale.ts';

const PROPS: LineChartProps = {
  label: 'ndcg@10 by stage',
  x: [
    { id: 'legs', label: 'retrieval legs' },
    { id: 'fusion', label: 'after fusion' },
    { id: 'rerank', label: 'after rerank' },
    { id: 'final', label: 'final ranking' },
  ],
  series: [
    { id: 'r1', label: 'baseline', short: 'baseline', ink: 'base' },
    { id: 'r2', label: 'A', short: 'A', ink: 'a' },
  ],
  // The baseline has no fusion and no rerank: its line breaks there.
  values: [
    [0.5, null, null, 0.5],
    [0.55, 0.6, 0.7, 0.7],
  ],
  dots: [
    { series: 1, x: 0, value: 0.4, label: 'bm25' },
    { series: 1, x: 0, value: 0.55, label: 'dense' },
  ],
  domain: [0, 1],
  format: (v) => v.toFixed(4),
  gapLabel: 'no stage here',
};

const step = (PLOT.width - PLOT.left - PLOT.right) / 4;
const cx = (i: number) => PLOT.left + step * (i + 0.5);
const cy = linear([0, 1], [PLOT.height - PLOT.bottom, PLOT.top]);

describe.each(THEMES)('LineChart, %s theme', (theme) => {
  it('draws a segment only between neighbouring stages both present: a gap, never a line across it', () => {
    const { container } = render(
      <div data-theme={theme}>
        <LineChart {...PROPS} />
      </div>,
    );
    const segments = [...container.querySelectorAll<SVGPathElement>('path.rg-chart__line')].map((p) => [p.dataset.ink, p.getAttribute('d')]);
    expect(segments).toEqual([
      ['a', `M${cx(0)},${cy(0.55)}L${cx(1)},${cy(0.6)}L${cx(2)},${cy(0.7)}L${cx(3)},${cy(0.7)}`],
    ]);
    // The baseline's two stages stand as points, unjoined.
    const points = [...container.querySelectorAll<SVGCircleElement>('circle.rg-chart__point[data-ink="base"]')];
    expect(points.map((p) => [Number(p.getAttribute('cx')), Number(p.getAttribute('cy'))])).toEqual([
      [cx(0), cy(0.5)],
      [cx(3), cy(0.5)],
    ]);
  });

  it('strokes each line in its run ink, the baseline dashed, from tokens this theme defines', () => {
    expect(declared(css, '.rg-chart__line[data-ink="a"]', 'stroke')).toBe('var(--run-a)');
    expect(declared(css, '.rg-chart__line[data-ink="base"]', 'stroke')).toBe('var(--run-base)');
    expect(declared(css, '.rg-chart__line[data-ink="base"]', 'stroke-dasharray')).toBeDefined();
    expect(tokenIn(theme, '--run-a')).toMatch(/^#/);
  });
});

describe('LineChart', () => {
  it('draws the extra marks at a stage — each leg — beside the line through the best', () => {
    const { container } = render(<LineChart {...PROPS} />);
    const dots = [...container.querySelectorAll<SVGCircleElement>('circle.rg-chart__dot')];
    expect(dots.map((d) => [d.dataset.ink, Number(d.getAttribute('cy'))])).toEqual([
      ['a', cy(0.4)],
      ['a', cy(0.55)],
    ]);
  });

  it('writes each run\'s letter at its last point', () => {
    const { container } = render(<LineChart {...PROPS} />);
    expect([...container.querySelectorAll('.rg-chart__end')].map((t) => t.textContent)).toEqual(['baseline', 'A']);
  });

  it('hovers a whole stage column and says "no stage here" for a run that lacks it', () => {
    const { container } = render(<LineChart {...PROPS} />);
    const hits = [...container.querySelectorAll('rect.rg-chart__hit')];
    expect(hits).toHaveLength(4);
    expect(Number(hits[1]?.getAttribute('width'))).toBeCloseTo(step);
    fireEvent.pointerEnter(hits[1] as Element);
    expect(container.querySelector('.rg-chart__tip')?.textContent).toBe('after fusionbaselineno stage hereA0.6000');
  });

  it('keeps two letters ending at nearly the same value apart, so neither hides the other', () => {
    const { container } = render(<LineChart {...PROPS} values={[[0.5, null, null, 0.7], [0.55, 0.6, 0.7, 0.701]]} />);
    const ys = [...container.querySelectorAll('.rg-chart__end')].map((t) => Number(t.getAttribute('y')));
    expect(Math.abs((ys[0] as number) - (ys[1] as number))).toBeGreaterThanOrEqual(12);
    // Spread both ways about the points, not only pushed down from the first.
    expect(((ys[0] as number) + (ys[1] as number)) / 2).toBeCloseTo((cy(0.7) + cy(0.701)) / 2);
  });

  it('asks the caller why a value is missing, when it says per point', () => {
    const { container } = render(<LineChart {...PROPS} gapLabel={(s, x) => `gap ${s}:${x}`} />);
    fireEvent.pointerEnter(container.querySelectorAll('rect.rg-chart__hit')[2] as Element);
    expect(container.querySelector('.rg-chart__tip')?.textContent).toBe('after rerankbaselinegap 0:2A0.7000');
  });

  it('has one value axis and the stages written along the other', () => {
    const { container } = render(<LineChart {...PROPS} />);
    expect(container.querySelectorAll('.rg-chart__axis')).toHaveLength(1);
    expect([...container.querySelectorAll('.rg-chart__group')].map((t) => t.textContent)).toEqual(['retrieval legs', 'after fusion', 'after rerank', 'final ranking']);
    expect(container.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });
});

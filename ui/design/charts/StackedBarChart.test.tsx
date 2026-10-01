/** @vitest-environment happy-dom */
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import { THEMES, tokenIn } from '../testing/themes.ts';
import css from './Charts.css?raw';
import { linear, PLOT } from './scale.ts';
import { StackedBarChart, type StackedBarChartProps } from './StackedBarChart.tsx';

const PROPS: StackedBarChartProps = {
  label: 'Median latency per node',
  bars: [
    { id: 'r1', label: 'baseline' },
    { id: 'r2', label: 'A' },
  ],
  segments: [
    [{ id: 'dense', label: 'dense', value: 10, family: 'retriever' }],
    [
      { id: 'bm25', label: 'bm25', value: 5, family: 'retriever' },
      { id: 'rrf', label: 'rrf', value: 1, family: 'fusion' },
      { id: 'rerank', label: 'rerank', value: 24, family: 'reranker' },
      { id: 'custom', label: 'custom', value: 0, family: null },
    ],
  ],
  format: (v) => `${v} ms`,
};

describe.each(THEMES)('StackedBarChart, %s theme', (theme) => {
  it('stacks each bar\'s segments end to end on one scale, from its start', () => {
    const { container } = render(
      <div data-theme={theme}>
        <StackedBarChart {...PROPS} />
      </div>,
    );
    const rows = [...container.querySelectorAll('g.rg-chart__stack')];
    expect(rows).toHaveLength(2);
    const segs = [...(rows[1] as Element).querySelectorAll<SVGRectElement>('rect.rg-chart__seg')];
    // The largest total, 30, sets the domain; ticks round it to 30.
    const x = linear([0, 30], [PLOT.left, PLOT.width - PLOT.right]);
    expect(segs.map((s) => [s.dataset.family, Number(s.getAttribute('x')), Number(s.getAttribute('width'))])).toEqual([
      ['retriever', x(0), x(5) - x(0)],
      ['fusion', x(5), x(6) - x(5)],
      ['reranker', x(6), x(30) - x(6)],
      ['none', x(30), 0],
    ]);
  });

  it('fills each segment from its family pigment — never a run ink — a token this theme defines', () => {
    expect(declared(css, '.rg-chart__seg[data-family="retriever"]', 'fill')).toBe('var(--family-retriever)');
    expect(declared(css, '.rg-chart__seg[data-family="reranker"]', 'fill')).toBe('var(--family-reranker)');
    // A family with no pigment of its own takes the neutral one.
    expect(declared(css, '.rg-chart__seg[data-family="none"]', 'fill')).toBe('var(--family-query)');
    expect(tokenIn(theme, '--family-retriever')).toMatch(/^#/);
    expect(css).not.toMatch(/\.rg-chart__seg[^{]*\{[^}]*--run-/);
  });
});

describe('StackedBarChart', () => {
  it('names each bar along its axis, and hovers the whole row to list its segments', () => {
    const { container } = render(<StackedBarChart {...PROPS} />);
    expect([...container.querySelectorAll('.rg-chart__group')].map((t) => t.textContent)).toEqual(['baseline', 'A']);
    const hits = [...container.querySelectorAll('rect.rg-chart__hit')];
    expect(hits).toHaveLength(2);
    expect(Number(hits[0]?.getAttribute('width'))).toBe(PLOT.width - PLOT.left - PLOT.right);
    fireEvent.pointerEnter(hits[0] as Element);
    expect(container.querySelector('.rg-chart__tip')?.textContent).toBe('baselinedense10 mstotal10 ms');
  });

  it('writes the total at the end of each bar', () => {
    const { container } = render(<StackedBarChart {...PROPS} />);
    expect([...container.querySelectorAll('.rg-chart__total')].map((t) => t.textContent)).toEqual(['10 ms', '30 ms']);
    expect(container.querySelectorAll('.rg-chart__axis')).toHaveLength(1);
  });
});

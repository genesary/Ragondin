/** @vitest-environment happy-dom */
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Glyph } from '../glyphs/Glyph.tsx';
import { declared, parseRules } from '../testing/css.ts';
import { THEMES, tokenIn } from '../testing/themes.ts';
import tokens from '../tokens.css?raw';
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

// One run with two legs of one family, a fusion too narrow for its name, and a
// node with no family narrower still.
const LEGS: StackedBarChartProps = {
  label: 'Median latency per node',
  bars: [{ id: 'r1', label: 'A' }],
  segments: [
    [
      { id: 'bm25', label: 'bm25', value: 40, family: 'retriever' },
      { id: 'dense', label: 'dense', value: 40, family: 'retriever' },
      { id: 'rrf', label: 'rrf', value: 3, family: 'fusion' },
      { id: 'ext', label: 'ext', value: 0.5, family: null },
      { id: 'rerank', label: 'rerank', value: 16.5, family: 'reranker' },
    ],
  ],
  format: (v) => `${v} ms`,
};

/** What each segment of the first stack is labelled with, in stacking order. */
function marks(container: Element): (string | null)[] {
  const stack = container.querySelector('g.rg-chart__stack') as Element;
  return [...stack.querySelectorAll('rect.rg-chart__seg')].map((seg) => {
    const label = seg.nextElementSibling;
    if (label === null || !label.classList.contains('rg-chart__seg-label')) return null;
    const name = label.querySelector('text.rg-chart__seg-name');
    return name === null ? `glyph:${label.getAttribute('data-family')}` : name.textContent;
  });
}

describe.each(THEMES)('StackedBarChart, %s theme', (theme) => {
  it('names each segment wide enough to hold its node name, so two legs of one family are told apart in the plot', () => {
    const { container } = render(
      <div data-theme={theme}>
        <StackedBarChart {...LEGS} />
      </div>,
    );
    expect(marks(container)).toEqual(['bm25', 'dense', 'glyph:fusion', null, 'rerank']);
  });

  it('counts a wide letter as wide: in segments of one width, a seven-letter lowercase name is written, and a name no longer in wide capitals is a glyph', () => {
    const { container } = render(
      <div data-theme={theme}>
        <StackedBarChart
          {...LEGS}
          segments={[
            [
              { id: 'n', label: 'dense_2', value: 8, family: 'retriever' },
              { id: 'w', label: 'MWMWMWM', value: 8, family: 'retriever' },
              // Round capitals are nearly as wide as M in the UI face.
              { id: 'q', label: 'QQQQQQ', value: 8, family: 'retriever' },
              { id: 'o', label: 'OOOOOO', value: 8, family: 'retriever' },
              { id: 'rest', label: 'rest', value: 68, family: 'generator' },
            ],
          ]}
        />
      </div>,
    );
    expect(marks(container)).toEqual(['dense_2', 'glyph:retriever', 'glyph:retriever', 'glyph:retriever', 'rest']);
  });

  it('draws the family glyph, in its own drawing, in a segment too narrow for its name', () => {
    const { container } = render(
      <div data-theme={theme}>
        <StackedBarChart {...LEGS} />
      </div>,
    );
    const glyph = container.querySelector('svg.rg-chart__seg-label[data-family="fusion"] .rg-glyph');
    const fusion = render(<Glyph name="fusion" />).container.querySelector('.rg-glyph');
    expect(glyph?.innerHTML).toBe(fusion?.innerHTML);
  });

  it('writes labels in the ink made for a pigment, never in a pigment or a run ink, a token this theme defines', () => {
    expect(declared(css, '.rg-chart__seg-name', 'fill')).toBe('var(--on-family)');
    expect(declared(css, '.rg-chart__seg-label', 'color')).toBe('var(--on-family)');
    expect(tokenIn(theme, '--on-family')).toMatch(/^#/);
    for (const rule of parseRules(css).filter((r) => r.selector.includes('rg-chart__seg-'))) {
      expect([...rule.declarations.values()].join(';')).not.toMatch(/--family-|--run-/);
    }
  });

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
  it('never lets two labels overlap: each is clipped to its own segment, and segments do not overlap', () => {
    const props: StackedBarChartProps = { ...LEGS, bars: [...LEGS.bars, { id: 'r2', label: 'B' }], segments: [...LEGS.segments, PROPS.segments[1] ?? []] };
    const { container } = render(<StackedBarChart {...props} />);
    const boxes = (el: Element) => ['x', 'y', 'width', 'height'].map((a) => Number(el.getAttribute(a)));
    for (const stack of container.querySelectorAll('g.rg-chart__stack')) {
      const labels = [...stack.querySelectorAll('svg.rg-chart__seg-label')];
      expect(labels.length).toBeGreaterThan(0);
      for (const label of labels) {
        // Browsers clip a nested svg's content to its own box by default
        // (overflow: hidden for any svg that is not the root, and Charts.css
        // makes only the root's visible), so a label confined to its
        // segment's box cannot be drawn over its neighbour's.
        expect(label.previousElementSibling?.classList.contains('rg-chart__seg')).toBe(true);
        expect(boxes(label)).toEqual(boxes(label.previousElementSibling as Element));
      }
      const segs = [...stack.querySelectorAll('rect.rg-chart__seg')].map(boxes);
      segs.slice(1).forEach(([x], i) => expect(x).toBeGreaterThanOrEqual((segs[i]?.[0] ?? 0) + (segs[i]?.[2] ?? 0) - 1e-9));
    }
  });

  it('estimates names in the type their labels are drawn in: --type-micro, 11px, the chart\'s LABEL_SIZE', () => {
    expect(declared(css, '.rg-chart__seg-name', 'font')).toBe('var(--type-micro)');
    const micro = parseRules(tokens).map((r) => r.declarations.get('--type-micro')).find((v) => v !== undefined);
    expect(micro).toMatch(/\b11px\//);
  });

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

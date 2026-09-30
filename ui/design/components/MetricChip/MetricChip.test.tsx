/** @vitest-environment happy-dom */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './MetricChip.css?raw';
import { Delta, MetricChip } from './MetricChip.tsx';

describe('MetricChip at rest', () => {
  it('shows the metric name and its value', () => {
    const { container } = render(<MetricChip name="nDCG@10" value="0.7217" />);
    expect(screen.getByText('nDCG@10').classList.contains('rg-metric__k')).toBe(true);
    expect(screen.getByText('0.7217').classList.contains('rg-metric__v')).toBe(true);
    expect(container.querySelector('.rg-metric.is-best')).toBeNull();
  });
});

describe('MetricChip best', () => {
  it('marks the one best value by weight and outline, and says so in words', () => {
    const { container } = render(<MetricChip name="nDCG@10" value="0.7217" best />);
    expect(container.querySelector('.rg-metric.is-best')).toBeTruthy();
    expect(declared(css, '.rg-metric.is-best .rg-metric__v', 'font-weight')).toBe('700');
    expect(screen.getByText('best').classList.contains('rg-visually-hidden')).toBe(true);
  });
});

describe('MetricChip with a delta', () => {
  it('reads a better delta by sign and up arrow, not by colour alone', () => {
    const { container } = render(<MetricChip name="nDCG@10" value="0.7217" delta={<Delta meaning="better" direction="up">+0.073</Delta>} />);
    const delta = container.querySelector('.rg-delta') as HTMLElement;
    expect(delta.dataset.meaning).toBe('better');
    expect(delta.textContent).toContain('+0.073');
    expect(delta.textContent).toContain('better');
    expect(delta.querySelector('svg')).toBeTruthy();
    expect(declared(css, '.rg-delta[data-meaning="better"]', 'color')).toBe('var(--better)');
  });

  it('reads a worse delta by sign and arrow; the arrow follows direction, the colour meaning', () => {
    const { container } = render(<Delta meaning="worse" direction="up">+351 ms</Delta>);
    const delta = container.querySelector('.rg-delta') as HTMLElement;
    expect(delta.dataset.meaning).toBe('worse');
    expect(delta.dataset.direction).toBe('up');
    expect(delta.textContent).toContain('worse');
    expect(declared(css, '.rg-delta[data-meaning="worse"]', 'color')).toBe('var(--worse)');
  });

  it('reads an unchanged value without an arrow', () => {
    const { container } = render(<Delta meaning="same" direction="none">0.000</Delta>);
    expect(container.querySelector('svg')).toBeNull();
    expect(container.textContent).toContain('unchanged');
  });
});

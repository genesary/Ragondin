/** @vitest-environment happy-dom */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../testing/css.ts';
import { ChartFrame, ChartTooltip } from './ChartFrame.tsx';
import css from './Charts.css?raw';
import { PLOT } from './scale.ts';

const legend = [
  { id: 'base', label: 'baseline · dense-only', mark: <span data-mark="base" /> },
  { id: 'a', label: 'A · hybrid', mark: <span data-mark="a" /> },
];

describe('ChartFrame', () => {
  it('is a figure named by its caption, with its legend always shown', () => {
    render(
      <ChartFrame caption="nDCG@10 per run" legend={legend} table={<table aria-label="nDCG@10 per run, as a table" />}>
        <svg />
      </ChartFrame>,
    );
    const figure = screen.getByRole('figure', { name: 'nDCG@10 per run' });
    const items = screen.getByRole('list', { name: 'Legend' }).querySelectorAll('li');
    expect(figure).toBeTruthy();
    expect([...items].map((li) => li.textContent)).toEqual(['baseline · dense-only', 'A · hybrid']);
  });

  it('refuses to draw without a legend: every hue on screen is data a legend explains', () => {
    expect(() =>
      render(
        <ChartFrame caption="x" legend={[]} table={<table />}>
          <svg />
        </ChartFrame>,
      ),
    ).toThrow(/legend/);
  });

  it('hides its table until a button — a keyboard stop — shows it, and says which state it is in', () => {
    render(
      <ChartFrame caption="nDCG@10 per run" legend={legend} table={<table aria-label="nDCG@10 per run, as a table" />}>
        <svg />
      </ChartFrame>,
    );
    const toggle = screen.getByRole('button', { name: 'Show as a table' });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    // Several charts share the words; each button is described by its own chart's caption.
    expect(document.getElementById(toggle.getAttribute('aria-describedby') ?? '')?.textContent).toBe('nDCG@10 per run');
    const region = document.getElementById(toggle.getAttribute('aria-controls') as string) as HTMLElement;
    expect(region.hidden).toBe(true);
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(region.hidden).toBe(false);
    expect(screen.getByRole('table', { name: 'nDCG@10 per run, as a table' })).toBeTruthy();
  });
});

describe('ChartFrame plot', () => {
  it('is never drawn wider than the drawing box, so a label in SVG units is never magnified', () => {
    expect(declared(css, '.rg-chart__plot', 'max-width')).toBe(`${PLOT.width}px`);
  });
});

describe('ChartTooltip', () => {
  it('floats at the point it is given, as a share of the plot, and stays out of the accessibility tree', () => {
    const { container } = render(
      <ChartTooltip x={0.25} y={0.5}>
        A: 0.6611
      </ChartTooltip>,
    );
    const tip = container.querySelector('.rg-chart__tip') as HTMLElement;
    expect(tip.getAttribute('aria-hidden')).toBe('true');
    expect(tip.style.left).toBe('25%');
    expect(tip.style.top).toBe('50%');
    expect(tip.textContent).toBe('A: 0.6611');
  });
});

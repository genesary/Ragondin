/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './StatusChip.css?raw';
import { StatusChip } from './StatusChip.tsx';

const chip = (el: HTMLElement) => el.querySelector('.rg-status') as HTMLElement;

describe('StatusChip queued', () => {
  it('reads "queued" beside a clock', () => {
    const { container } = render(<StatusChip state="queued">queued, 2 ahead</StatusChip>);
    expect(chip(container).textContent).toBe('queued, 2 ahead');
    expect(chip(container).querySelector('svg')).toBeTruthy();
    expect(chip(container).dataset.state).toBe('queued');
  });

  it('says the state word when given no specifics', () => {
    const { container } = render(<StatusChip state="queued" />);
    expect(chip(container).textContent).toBe('queued');
  });
});

describe('StatusChip running', () => {
  it('reads the real fraction in words and in a meter that never loops', () => {
    const { container } = render(<StatusChip state="running" fraction={0.38} />);
    expect(chip(container).textContent).toBe('running 38%');
    const meter = chip(container).querySelector('.rg-status__meter i') as HTMLElement;
    expect(meter.style.width).toBe('38%');
    expect(css).not.toMatch(/animation/);
  });
});

describe('StatusChip done', () => {
  it('reads "done" beside a check, on the good wash', () => {
    const { container } = render(<StatusChip state="done" />);
    expect(chip(container).textContent).toBe('done');
    expect(chip(container).querySelector('svg')).toBeTruthy();
    expect(declared(css, '.rg-status[data-state="done"]', '--st')).toBe('var(--good)');
  });
});

describe('StatusChip warning', () => {
  it('reads its specifics beside an alert glyph, in warning ink', () => {
    const { container } = render(<StatusChip state="warning">slow: p95 2.1 s</StatusChip>);
    expect(chip(container).textContent).toBe('slow: p95 2.1 s');
    expect(chip(container).querySelector('svg')).toBeTruthy();
    expect(declared(css, '.rg-status[data-state="warning"]', '--st')).toBe('var(--warning-ink)');
  });
});

describe('StatusChip failed', () => {
  it('names where it failed beside a cross, on the critical wash', () => {
    const { container } = render(
      <StatusChip state="failed">
        failed at <code>rerank</code>
      </StatusChip>,
    );
    expect(chip(container).textContent).toBe('failed at rerank');
    expect(chip(container).querySelector('code')?.textContent).toBe('rerank');
    expect(chip(container).querySelector('svg')).toBeTruthy();
    expect(declared(css, '.rg-status[data-state="failed"]', '--st')).toBe('var(--critical)');
  });
});

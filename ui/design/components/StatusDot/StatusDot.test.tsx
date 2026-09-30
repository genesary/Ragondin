/** @vitest-environment happy-dom */
import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { declared } from '../../testing/css.ts';
import css from './StatusDot.css?raw';
import { StatusDot } from './StatusDot.tsx';

describe('StatusDot reachable', () => {
  it('is a filled dot in the good ink, hidden from assistive technology: its word beside it carries the state', () => {
    const { container } = render(<StatusDot connected />);
    const dot = container.querySelector('.rg-dot') as HTMLElement;
    expect(dot.getAttribute('data-connected')).toBe('true');
    expect(dot.getAttribute('aria-hidden')).toBe('true');
    expect(declared(css, '.rg-dot', 'background')).toBe('var(--good)');
  });
});

describe('StatusDot unreachable', () => {
  it('is a hollow ring, so the shape differs and not the colour alone', () => {
    const { container } = render(<StatusDot connected={false} />);
    expect(container.querySelector('.rg-dot')?.getAttribute('data-connected')).toBe('false');
    expect(declared(css, '.rg-dot[data-connected="false"]', 'background')).toBe('transparent');
    expect(declared(css, '.rg-dot[data-connected="false"]', 'box-shadow')).toBe('inset 0 0 0 1.5px var(--ink-3)');
  });
});
